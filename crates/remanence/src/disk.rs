// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The `Disk` surface (U3 and U4): open a raw or qcow2
//! image under the P7 claim, report its partitions and volumes as they
//! actually are, and read/write files in its FAT volumes with a commit
//! point (P2) — everything rolls back until `commit`. The commit is
//! durable (P9): a recovery journal is armed beneath the write-through,
//! so an interruption at any point leaves state the next open
//! reconciles — wholly the old image or wholly the committed new one —
//! before the disk is exposed. A qcow2 whose content lives partly in a
//! backing chain opens as one composed disk (U6), every member claimed
//! for the session's life and writes allocated copy-on-write into the
//! top image only.

use std::path::{Path, PathBuf};

use crate::cache::SessionCache;
use crate::device::{AccessIntent, AccessMode, Device, FileDevice};
use crate::error::{Error, Result};
use crate::fat::{FatEntry, FatVolume, VolumeInfo};
use crate::journal;
use crate::mbr::{self, Discovery, PartitionInfo, PartitionKind};
use crate::qcow2::{self, QCOW2_MAGIC, Qcow2};

#[cfg(test)]
fn crash_test_process_at(boundary: &str) {
    if std::env::var_os("REMANENCE_CRASH_TEST_BOUNDARY").as_deref()
        == Some(std::ffi::OsStr::new(boundary))
    {
        // Deliberately bypass destructors: the parent test must observe
        // exactly what a vanished process leaves on disk.
        std::process::exit(86);
    }
}

/// The container format a disk image turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFormat {
    Raw,
    Qcow2 { version: u32 },
}

/// The complete report of one disk, as it actually is (U4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskGeometry {
    /// Sector 0 is all zero: a blank disk with zero volumes — an answer,
    /// not an error.
    pub blank: bool,
    pub partitions: Vec<PartitionInfo>,
    pub volumes: Vec<VolumeInfo>,
}

#[derive(Debug)]
enum Virtual {
    Raw(FileDevice),
    Qcow2(Qcow2<FileDevice>),
}

impl Virtual {
    fn device(&mut self) -> &mut dyn Device {
        match self {
            Self::Raw(device) => device,
            Self::Qcow2(device) => device,
        }
    }

    /// The host file commits land in: the image itself, or a chain's
    /// top image — the only member writes ever reach (U6).
    fn host(&mut self) -> &mut FileDevice {
        match self {
            Self::Raw(device) => device,
            Self::Qcow2(device) => device.host_mut(),
        }
    }

    /// Driver cache state a staged-but-unlanded commit must put back.
    fn cache_snapshot(&self) -> Option<Vec<u64>> {
        match self {
            Self::Raw(_) => None,
            Self::Qcow2(device) => Some(device.l1_snapshot()),
        }
    }

    fn restore_cache(&mut self, snapshot: Option<Vec<u64>>) {
        if let (Self::Qcow2(device), Some(l1)) = (self, snapshot) {
            device.restore_l1(l1);
        }
    }
}

/// A composed view: the session cache over the virtual disk (P27).
/// Reads stream through the cache and see buffered writes; altered
/// extents stay in the cache — in memory or spilled to private session
/// storage — and nothing reaches the file until commit (P2).
struct Composed<'a> {
    base: &'a mut dyn Device,
    cache: &'a mut SessionCache,
}

impl Device for Composed<'_> {
    fn len(&self) -> u64 {
        self.base.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.cache.read_at(self.base, offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.cache.write_at(self.base, offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// An open disk image.
#[derive(Debug)]
pub struct Disk {
    virtual_disk: Virtual,
    cache: SessionCache,
    /// The declared session cache bound (P27), governing the session
    /// cache and each commit's capture alike.
    cache_bytes: u64,
    format: DiskFormat,
    mode: AccessMode,
    path: String,
    /// The recovery sidecar's derived path (P9) — private transient
    /// state, never a user-owned file.
    journal_path: PathBuf,
    /// Set when a commit failed partway and its in-process undo failed
    /// too: the session's caches no longer describe the file, so every
    /// verb refuses until a fresh open reconciles the image.
    failed: Option<String>,
}

impl Disk {
    /// Opens `path` with the caller's declared intent (P7). A `Write`
    /// open claims the image exclusively — no other reader or writer
    /// for the session's whole life — and an open that cannot secure
    /// that claim fails at the open, naming the reason, never by
    /// falling back to read-only (a running VM holding the image is
    /// the designed refusal). A `Read` open takes read access only,
    /// denies writes to every other process, and keeps admitting other
    /// readers. An interrupted commit left by an earlier session is
    /// reconciled here, before the disk is exposed (P9): the image
    /// comes back wholly the old state or wholly the committed new
    /// one, never a partial third state. The container is detected by
    /// magic: qcow2, else raw.
    /// A qcow2 naming a backing file opens with its whole chain
    /// composed, every backing file claimed immutable for the session's
    /// life (U6). Writes allocate copy-on-write into the top image only;
    /// commit preserves the backing relationship.
    pub fn open(path: impl AsRef<Path>, intent: AccessIntent) -> Result<Self> {
        Self::open_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// Opens `path` as [`Disk::open`] does, under a caller-declared
    /// session cache bound (P27): at most `cache_bytes` of session
    /// state stays resident — reads, uncommitted writes, and a
    /// commit's staging alike — rounded up to whole 64 KiB extents,
    /// with one extent as the floor. Altered state past the bound
    /// spills to private session storage, never the image.
    pub fn open_with_cache(
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<Self> {
        let path = path.as_ref();
        let recovery = journal::sidecar_path(path);
        let mut file = FileDevice::open(path, intent)?;
        // The sidecar check runs under our claim, so no live commit can
        // be mid-flight — a sidecar here is an interrupted one (P9).
        if recovery.exists() {
            match intent {
                AccessIntent::Write => journal::reconcile(&recovery, &mut file, path)?,
                AccessIntent::Read => {
                    // Reconciling writes; trade the read claim for a
                    // moment of exclusive access, then take it back.
                    drop(file);
                    journal::reconcile_at(path)?;
                    file = FileDevice::open(path, intent)?;
                }
            }
        }
        let mode = file.mode();

        let mut magic = [0u8; 4];
        let format = if file.len() >= 4 {
            file.read_at(0, &mut magic)?;
            if magic == QCOW2_MAGIC {
                None
            } else {
                Some(DiskFormat::Raw)
            }
        } else {
            Some(DiskFormat::Raw)
        };

        let (virtual_disk, format) = match format {
            Some(raw) => (Virtual::Raw(file), raw),
            None => {
                let qcow2 = qcow2::open_chain(file, path)?;
                let version = qcow2.header().version;
                (Virtual::Qcow2(qcow2), DiskFormat::Qcow2 { version })
            }
        };

        Ok(Self {
            virtual_disk,
            cache: SessionCache::with_bytes_offloading(cache_bytes),
            cache_bytes,
            format,
            mode,
            path: path.display().to_string(),
            journal_path: recovery,
            failed: None,
        })
    }

    /// The session's access mode — an echo of the declared intent.
    pub fn mode(&self) -> AccessMode {
        self.mode
    }

    pub fn format(&self) -> DiskFormat {
        self.format
    }

    /// The virtual disk size (the guest-visible size for qcow2).
    pub fn size(&self) -> u64 {
        match &self.virtual_disk {
            Virtual::Raw(device) => device.len(),
            Virtual::Qcow2(device) => device.header().virtual_size,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether uncommitted changes exist.
    pub fn is_modified(&self) -> bool {
        self.cache.modified()
    }

    fn composed(&mut self) -> Composed<'_> {
        Composed {
            base: self.virtual_disk.device(),
            cache: &mut self.cache,
        }
    }

    fn split_path(path: &str) -> Result<Vec<&str>> {
        let segments: Vec<&str> = path
            .split(['/', '\\'])
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .collect();
        if segments.iter().any(|segment| *segment == "..") {
            return Err(Error::io(format!("path '{path}' may not contain '..'")));
        }
        Ok(segments)
    }

    fn volume_at(&mut self, id: &str) -> Result<(u64, FatVolume)> {
        let geometry = self.geometry()?;
        let offset = geometry
            .volumes
            .iter()
            .find(|volume| volume.id == id)
            .map(|volume| volume.offset_bytes)
            .ok_or_else(|| Error::not_found(format!("volume identifier '{id}' not found")))?;
        let mut composed = self.composed();
        let volume = FatVolume::open(&mut composed, offset)?;
        Ok((offset, volume))
    }

    fn partition_volume_id(partition: &PartitionInfo) -> String {
        match partition.kind {
            PartitionKind::Primary => format!("partition:{}", partition.number),
            PartitionKind::Logical => format!("logical:{}", partition.number),
        }
    }

    /// The complete report (U4): the disk's partitions and
    /// volumes as they actually are. Blank is an answer — an all-zero
    /// sector 0 reports a blank disk with zero volumes — and non-zero
    /// data that is neither a supported filesystem nor a partition
    /// table is a named refusal kept distinct from blank. A partition
    /// row outside the pinned claim, or one whose volume cannot be
    /// read, stays in the report carrying its structured issue instead
    /// of failing the whole disk or vanishing, and the volumes behind
    /// it never renumber.
    pub fn geometry(&mut self) -> Result<DiskGeometry> {
        self.require_usable()?;
        let mut composed = self.composed();
        match mbr::discover(&mut composed)? {
            Discovery::Blank => Ok(DiskGeometry {
                blank: true,
                partitions: Vec::new(),
                volumes: Vec::new(),
            }),
            Discovery::BareVolume => {
                let volume = FatVolume::open(&mut composed, 0)?;
                let length = composed.len();
                let info = volume.info(&mut composed, "superfloppy:0".to_owned(), None, length)?;
                Ok(DiskGeometry {
                    blank: false,
                    partitions: Vec::new(),
                    volumes: vec![info],
                })
            }
            Discovery::Partitioned(mut partitions) => {
                let mut volumes = Vec::new();
                for partition in &mut partitions {
                    if partition.issue.is_some() || mbr::is_extended(partition.type_byte) {
                        continue;
                    }
                    match FatVolume::open(&mut composed, partition.start_bytes).and_then(|volume| {
                        volume.info(
                            &mut composed,
                            Self::partition_volume_id(partition),
                            Some(partition.number),
                            partition.length_bytes,
                        )
                    }) {
                        Ok(info) => volumes.push(info),
                        // The row stays, carrying why (U4); the
                        // volumes behind it never renumber.
                        Err(error) => partition.issue = Some(error),
                    }
                }
                Ok(DiskGeometry {
                    blank: false,
                    partitions,
                    volumes,
                })
            }
        }
    }

    /// Lists a directory in the volume identified by `volume_id`
    /// ("" = root; "A/B" descends).
    pub fn entries(&mut self, volume_id: &str, path: &str) -> Result<Vec<FatEntry>> {
        let segments = Self::split_path(path)?;
        let (_, fat) = self.volume_at(volume_id)?;
        let mut composed = self.composed();
        fat.entries(&mut composed, &segments)
    }

    /// Answers one path in the volume identified by `volume_id` with its
    /// entry, or `None` when nothing exists at that path — a missing
    /// leaf, a missing parent, or a parent that is a file alike. Absence
    /// is an answer, distinguished from failure to read the volume (U3).
    pub fn stat(&mut self, volume_id: &str, path: &str) -> Result<Option<FatEntry>> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a path is required".to_owned()));
        }
        let (_, fat) = self.volume_at(volume_id)?;
        let mut composed = self.composed();
        fat.stat(&mut composed, &segments)
    }

    /// Copies a file's bytes out of the volume identified by `volume_id`.
    pub fn read_file(&mut self, volume_id: &str, path: &str) -> Result<Vec<u8>> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let (_, fat) = self.volume_at(volume_id)?;
        let mut composed = self.composed();
        fat.read_file(&mut composed, &segments)
    }

    fn require_writable(&self) -> Result<()> {
        if self.mode == AccessMode::ReadOnly {
            return Err(Error::read_only(format!(
                "'{}' was opened for reading; write actions are denied",
                self.path
            )));
        }
        Ok(())
    }

    fn require_usable(&self) -> Result<()> {
        match &self.failed {
            Some(reason) => Err(Error::io(reason.clone())),
            None => Ok(()),
        }
    }

    /// Writes a file into the volume identified by `volume_id`. An
    /// existing file is overwritten — shorter or longer, its old
    /// clusters released and reclaimed, every FAT copy kept in step —
    /// while an existing directory is refused. Buffered until
    /// [`Disk::commit`].
    pub fn write_file(&mut self, volume_id: &str, path: &str, contents: &[u8]) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let (_, fat) = self.volume_at(volume_id)?;
        let mut composed = self.composed();
        fat.write_file(&mut composed, &segments, contents)
    }

    /// Ensures a directory exists in the volume identified by
    /// `volume_id`: missing parents are created, and a path that already
    /// leads to a directory — the root included — succeeds unchanged.
    /// Buffered until commit.
    pub fn make_directory(&mut self, volume_id: &str, path: &str) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        let (_, fat) = self.volume_at(volume_id)?;
        let mut composed = self.composed();
        fat.make_directory(&mut composed, &segments)
    }

    /// The commit point (P2): writes everything buffered since open (or
    /// the last commit/rollback) through to the image, then flushes.
    /// The commit is durable (P9): every host write is staged in memory
    /// first, the bytes it will overwrite are made durable in a private
    /// recovery journal, and only then does the file change — so an
    /// interruption at any point leaves state the next open reconciles
    /// to wholly the old image or wholly the committed new one. A
    /// write-through refusal (P6) likewise surfaces before a single
    /// byte of the file has moved.
    pub fn commit(&mut self) -> Result<()> {
        self.require_usable()?;
        self.require_writable()?;
        if !self.cache.modified() {
            return self.virtual_disk.device().flush();
        }

        // Stage: the write-through runs against a capture of the host
        // file — itself a bounded cache spilling to session storage
        // (P27) — so the complete set of host writes is known while the
        // file is still untouched. A refusal discards the staging —
        // the driver's caches put back alongside it — and keeps the
        // buffered state for the caller.
        let cache_snapshot = self.virtual_disk.cache_snapshot();
        let cache_bytes = self.cache_bytes;
        // Consuming the altered set joins the offloads in flight first.
        self.cache.join_offloads();
        self.virtual_disk.host().begin_capture(cache_bytes);
        let staged = self.cache.write_through(self.virtual_disk.device());
        let capture = self.virtual_disk.host().take_capture();
        if let Err(error) = staged {
            self.virtual_disk.restore_cache(cache_snapshot);
            return Err(error);
        }
        if capture.is_clean() {
            self.cache.mark_committed();
            return self.virtual_disk.device().flush();
        }

        // The durability boundary (P9): the bytes the apply will
        // overwrite are durable in the recovery journal — streamed
        // there, never held whole — before the first of them changes.
        if let Err(error) =
            journal::record(&self.journal_path, self.virtual_disk.host(), &capture)
        {
            let _ = journal::retire(&self.journal_path);
            self.virtual_disk.restore_cache(cache_snapshot);
            return Err(error);
        }
        #[cfg(test)]
        crash_test_process_at("journal-armed");

        // Apply, then retire the journal. Should either fail, the
        // in-process undo reconciles from the armed journal, putting
        // the image back to wholly the old state; should even that
        // fail, the journal remains for the next open to reconcile.
        let applied = self
            .virtual_disk
            .host()
            .apply(&capture)
            .and_then(|()| {
                #[cfg(test)]
                crash_test_process_at("image-applied");
                journal::retire(&self.journal_path).map_err(|error| {
                    Error::io(format!(
                        "cannot retire the commit's recovery journal '{}': {error}",
                        self.journal_path.display()
                    ))
                })
            })
            .map(|()| {
                #[cfg(test)]
                crash_test_process_at("journal-retired");
            });
        if let Err(error) = applied {
            let image_path = PathBuf::from(&self.path);
            match journal::reconcile(
                &self.journal_path,
                self.virtual_disk.host(),
                &image_path,
            ) {
                Ok(()) => {
                    self.virtual_disk.restore_cache(cache_snapshot);
                }
                Err(_) => {
                    self.failed = Some(format!(
                        "a commit on '{}' failed partway and could not be undone \
                         in this session; reopen the disk to reconcile it",
                        self.path
                    ));
                }
            }
            return Err(error);
        }

        self.cache.mark_committed();
        Ok(())
    }

    /// Discards everything buffered; the image is untouched. Unaltered
    /// cached extents stay resident — they still mirror the image.
    pub fn rollback(&mut self) {
        self.cache.discard_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcow2::QCOW2_MAGIC;
    use std::process::Command;

    /// A minimal empty v3 qcow2 sized for the synthetic FAT16 volume
    /// (mirrors the qcow2 unit-test builder).
    fn empty_qcow2_bytes(virtual_size: u64) -> Vec<u8> {
        const CLUSTER_BITS: u32 = 12;
        const CLUSTER: u64 = 1 << CLUSTER_BITS;

        let l2_entries = CLUSTER / 8;
        let l1_size = virtual_size.div_ceil(CLUSTER * l2_entries) as u32;
        let mut image = vec![0u8; 4 * CLUSTER as usize];
        image[..4].copy_from_slice(&QCOW2_MAGIC);
        image[4..8].copy_from_slice(&3u32.to_be_bytes());
        image[20..24].copy_from_slice(&CLUSTER_BITS.to_be_bytes());
        image[24..32].copy_from_slice(&virtual_size.to_be_bytes());
        image[36..40].copy_from_slice(&l1_size.to_be_bytes());
        image[40..48].copy_from_slice(&(3 * CLUSTER).to_be_bytes());
        image[48..56].copy_from_slice(&CLUSTER.to_be_bytes());
        image[56..60].copy_from_slice(&1u32.to_be_bytes());
        image[96..100].copy_from_slice(&4u32.to_be_bytes());
        image[100..104].copy_from_slice(&112u32.to_be_bytes());
        image[CLUSTER as usize..CLUSTER as usize + 8].copy_from_slice(&(2 * CLUSTER).to_be_bytes());
        for cluster in 0..4usize {
            let at = 2 * CLUSTER as usize + cluster * 2;
            image[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
        }
        image
    }

    /// Builds a qcow2 file at `path` whose virtual disk carries the
    /// synthetic FAT16 volume, using the crate's own writer.
    fn build_fat16_qcow2(path: &std::path::Path) -> u64 {
        let virtual_size = 4_096_000u64; // the synthetic FAT16 volume size
        std::fs::write(path, empty_qcow2_bytes(virtual_size)).expect("qcow2 writes");

        // Format the virtual disk: write a FAT16 volume into guest space
        // through the crate's own qcow2 writer.
        let file = crate::device::FileDevice::open(path, AccessIntent::Write).expect("opens");
        let mut qcow2 = crate::qcow2::Qcow2::open(file).expect("parses");
        let volume = fat16_volume_bytes();
        assert_eq!(volume.len() as u64, virtual_size);
        qcow2.write_at(0, &volume).expect("formats");
        qcow2.flush().expect("flushes");
        virtual_size
    }

    /// Exercises the whole public qcow2 path reliquary runs: open,
    /// geometry, write, commit, reopen, read back.
    #[test]
    fn fat16_inside_qcow2_end_to_end() {
        let path =
            std::env::temp_dir().join(format!("remanence-qcow2-e2e-{}.qcow2", std::process::id()));
        let virtual_size = build_fat16_qcow2(&path);

        // Now the public path.
        let mut disk = Disk::open(&path, AccessIntent::Write).expect("disk opens");
        assert!(matches!(disk.format(), DiskFormat::Qcow2 { version: 3 }));
        assert_eq!(disk.size(), virtual_size);

        let geometry = disk.geometry().expect("geometry");
        assert_eq!(geometry.volumes.len(), 1);
        assert_eq!(geometry.volumes[0].label.as_deref(), Some("REMANENCE"));

        disk.make_directory("superfloppy:0", "GUEST")
            .expect("mkdir");
        disk.write_file("superfloppy:0", "GUEST/PAYLOAD.BIN", b"through the mapping")
            .expect("write");
        assert_eq!(
            disk.stat("superfloppy:0", "GUEST/PAYLOAD.BIN")
                .expect("stat")
                .map(|entry| entry.size_bytes),
            Some(b"through the mapping".len() as u64)
        );
        assert_eq!(
            disk.stat("superfloppy:0", "GUEST/ABSENT.BIN").expect("stat"),
            None
        );
        disk.commit().expect("commit");
        drop(disk);

        let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened
                .read_file("superfloppy:0", "GUEST/PAYLOAD.BIN")
                .expect("read"),
            b"through the mapping"
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    fn temp_image(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "remanence-durable-{tag}-{}.img",
            std::process::id()
        ))
    }

    /// A raw FAT16 image at `path` holding `OLD.BIN` = `old_content()`,
    /// committed and closed — the wholly-old state the durability tests
    /// expect an interrupted commit to come back to.
    fn build_committed_raw(path: &std::path::Path) {
        std::fs::write(path, fat16_volume_bytes()).expect("image writes");
        let mut disk = Disk::open(path, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &old_content())
            .expect("writes");
        disk.commit().expect("commits");
    }

    fn old_content() -> Vec<u8> {
        (0..48 * 1024u32).map(|n| (n % 240) as u8).collect()
    }

    fn new_content() -> Vec<u8> {
        (0..64 * 1024u32).map(|n| (n % 251) as u8).collect()
    }

    const CRASH_IMAGE: &str = "REMANENCE_CRASH_TEST_IMAGE";

    /// The subprocess half of the crash harness. It is ignored during an
    /// ordinary test walk and selected explicitly by the parent below.
    /// `commit` terminates this process at the requested boundary; if
    /// it returns, the harness was not attached to that boundary.
    #[test]
    #[ignore]
    fn crash_commit_child() {
        let path = std::env::var_os(CRASH_IMAGE).expect("the parent supplies an image path");
        let mut disk =
            Disk::open(std::path::PathBuf::from(path), AccessIntent::Write).expect("child opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &new_content())
            .expect("child overwrites");
        disk.commit().expect("child commits");
        panic!("the requested crash boundary was not reached");
    }

    fn run_crashing_commit(path: &std::path::Path, boundary: &str) {
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--ignored")
            .arg("--exact")
            .arg("disk::tests::crash_commit_child")
            .arg("--nocapture")
            .env(CRASH_IMAGE, path)
            .env("REMANENCE_CRASH_TEST_BOUNDARY", boundary)
            .status()
            .expect("crash child starts");
        assert_eq!(
            status.code(),
            Some(86),
            "the child terminates at the {boundary} durability boundary"
        );
    }

    fn build_committed_qcow2(path: &std::path::Path) {
        build_fat16_qcow2(path);
        let mut disk = Disk::open(path, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &old_content())
            .expect("writes old state");
        disk.commit().expect("commits old state");
    }

    fn build_committed_chain(directory: &std::path::Path) -> (PathBuf, PathBuf) {
        std::fs::create_dir_all(directory).expect("chain directory");
        let base = directory.join("base.qcow2");
        let top = directory.join("top.qcow2");
        let virtual_size = build_fat16_qcow2(&base);
        let mut base_disk = Disk::open(&base, AccessIntent::Write).expect("base opens");
        base_disk
            .write_file("superfloppy:0", "OLD.BIN", &old_content())
            .expect("writes old state");
        base_disk.commit().expect("commits old state");
        drop(base_disk);

        let mut image = empty_qcow2_bytes(virtual_size);
        let name = b"base.qcow2";
        image[0x200..0x200 + name.len()].copy_from_slice(name);
        image[8..16].copy_from_slice(&0x200u64.to_be_bytes());
        image[16..20].copy_from_slice(&(name.len() as u32).to_be_bytes());
        std::fs::write(&top, image).expect("top writes");
        (top, base)
    }

    #[test]
    fn crash_harness_covers_every_commit_boundary_and_image_shape() {
        let boundaries = [
            ("journal-armed", false),
            ("image-applied", false),
            ("journal-retired", true),
        ];
        for (boundary, expect_new) in boundaries {
            for shape in ["raw", "qcow2", "chain"] {
                let stem = format!("remanence-crash-{shape}-{boundary}-{}", std::process::id());
                let (path, backing, directory) = match shape {
                    "raw" => {
                        let path = std::env::temp_dir().join(format!("{stem}.img"));
                        build_committed_raw(&path);
                        (path, None, None)
                    }
                    "qcow2" => {
                        let path = std::env::temp_dir().join(format!("{stem}.qcow2"));
                        build_committed_qcow2(&path);
                        (path, None, None)
                    }
                    "chain" => {
                        let directory = std::env::temp_dir().join(stem);
                        let (path, backing) = build_committed_chain(&directory);
                        (path, Some(backing), Some(directory))
                    }
                    _ => unreachable!(),
                };
                let backing_before = backing
                    .as_ref()
                    .map(|path| std::fs::read(path).expect("backing reads"));

                run_crashing_commit(&path, boundary);

                let mut reopened = Disk::open(&path, AccessIntent::Read)
                    .unwrap_or_else(|error| panic!("{shape}/{boundary} reopens: {error}"));
                let content = reopened
                    .read_file("superfloppy:0", "OLD.BIN")
                    .unwrap_or_else(|error| panic!("{shape}/{boundary} reads: {error}"));
                assert_eq!(
                    content,
                    if expect_new {
                        new_content()
                    } else {
                        old_content()
                    },
                    "{shape}/{boundary} reconciles to a whole state"
                );
                assert!(
                    !crate::journal::sidecar_path(&path).exists(),
                    "{shape}/{boundary} leaves no recovery artifact after reopen"
                );
                drop(reopened);

                if let (Some(backing), Some(before)) = (&backing, backing_before) {
                    assert_eq!(
                        std::fs::read(backing).expect("backing reads"),
                        before,
                        "{shape}/{boundary} never modifies the backing file"
                    );
                }
                std::fs::remove_file(&path).ok();
                if let Some(backing) = backing {
                    std::fs::remove_file(backing).ok();
                }
                if let Some(directory) = directory {
                    std::fs::remove_dir(directory).ok();
                }
            }
        }
    }

    /// Runs a commit's staging and journal phases exactly as
    /// [`Disk::commit`] does, stopping at the durability boundary: the
    /// journal is armed, the file untouched. Returns the staged host
    /// writes so a test can apply any prefix of them before "crashing".
    fn stage_and_arm(disk: &mut Disk) -> (Vec<(u64, Vec<u8>)>, u64) {
        let cache_bytes = disk.cache_bytes;
        disk.cache.join_offloads();
        disk.virtual_disk.host().begin_capture(cache_bytes);
        disk.cache
            .write_through(disk.virtual_disk.device())
            .expect("stages");
        let capture = disk.virtual_disk.host().take_capture();
        crate::journal::record(&disk.journal_path, disk.virtual_disk.host(), &capture)
            .expect("journals");
        let mut blocks = Vec::new();
        capture
            .for_each_dirty(&mut |offset, data| {
                blocks.push((offset, data.to_vec()));
                Ok(())
            })
            .expect("collects");
        (blocks, capture.len())
    }

    #[test]
    fn a_tiny_declared_cache_bound_still_commits_correctly() {
        let path = temp_image("tiny-bound");
        build_committed_raw(&path);

        // A one-extent working set: reads, uncommitted writes, and the
        // commit's capture all evict and spill constantly (P27), and
        // the result is byte-identical to an unbounded run.
        let mut disk =
            Disk::open_with_cache(&path, AccessIntent::Write, 1).expect("opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &new_content())
            .expect("overwrites");
        assert!(disk.is_modified());
        disk.commit().expect("commits");
        drop(disk);

        let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened.read_file("superfloppy:0", "OLD.BIN").expect("reads"),
            new_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_commit_retires_its_recovery_journal() {
        let path = temp_image("retires");
        build_committed_raw(&path);

        let mut disk = Disk::open(&path, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "NEW.BIN", &new_content())
            .expect("writes");
        disk.commit().expect("commits");
        assert!(
            !crate::journal::sidecar_path(&path).exists(),
            "a completed commit leaves no recovery sidecar behind"
        );
        drop(disk);

        let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened.read_file("superfloppy:0", "NEW.BIN").expect("reads"),
            new_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_torn_journal_proves_the_image_was_never_touched() {
        let path = temp_image("torn");
        build_committed_raw(&path);

        // A crash before the durability boundary leaves a torn sidecar
        // and an untouched image; the next open discards the one and
        // exposes the other unchanged — through the read-intent route,
        // which must trade its claim for the reconciliation.
        let sidecar = crate::journal::sidecar_path(&path);
        std::fs::write(&sidecar, b"torn mid-write, never sealed").expect("sidecar writes");

        let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!sidecar.exists(), "the torn sidecar is discarded");
        assert_eq!(
            reopened.read_file("superfloppy:0", "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interruption_mid_apply_reconciles_to_the_old_image() {
        let path = temp_image("mid-apply");
        build_committed_raw(&path);

        let mut disk = Disk::open(&path, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &new_content())
            .expect("overwrites");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        assert!(blocks.len() >= 2, "the staged write set spans blocks");

        // The crash: half the staged writes land, the rest never do,
        // and the process vanishes without retiring the journal.
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        let mut reopened = Disk::open(&path, AccessIntent::Write).expect("reconciles and opens");
        assert!(!crate::journal::sidecar_path(&path).exists());
        assert_eq!(
            reopened.read_file("superfloppy:0", "OLD.BIN").expect("reads"),
            old_content(),
            "the image reconciles to wholly the old state"
        );

        // The reconciled disk is fully usable: the same overwrite
        // commits durably this time.
        reopened
            .write_file("superfloppy:0", "OLD.BIN", &new_content())
            .expect("overwrites");
        reopened.commit().expect("commits");
        drop(reopened);
        let mut committed = Disk::open(&path, AccessIntent::Read).expect("opens");
        assert_eq!(
            committed.read_file("superfloppy:0", "OLD.BIN").expect("reads"),
            new_content()
        );
        drop(committed);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interruption_before_retirement_reconciles_to_the_old_image() {
        let path = temp_image("unretired");
        build_committed_raw(&path);

        let mut disk = Disk::open(&path, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "NEW.BIN", &new_content())
            .expect("writes");
        let (blocks, new_len) = stage_and_arm(&mut disk);

        // The crash: the apply completes, but the journal is never
        // retired — the commit never returned, so the armed journal
        // governs and the next open rolls the image back.
        for &(offset, ref block) in &blocks {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!crate::journal::sidecar_path(&path).exists());
        assert_eq!(
            reopened.stat("superfloppy:0", "NEW.BIN").expect("stats"),
            None,
            "the interrupted commit's file never existed"
        );
        assert_eq!(
            reopened.read_file("superfloppy:0", "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interrupted_qcow2_commit_reconciles_before_the_disk_is_exposed() {
        let path = std::env::temp_dir().join(format!(
            "remanence-durable-qcow2-{}.qcow2",
            std::process::id()
        ));
        build_fat16_qcow2(&path);

        // The wholly-old state: one committed file inside the qcow2.
        let mut disk = Disk::open(&path, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &old_content())
            .expect("writes");
        disk.commit().expect("commits");
        assert!(!crate::journal::sidecar_path(&path).exists());
        drop(disk);

        // An interrupted commit: cluster allocations and metadata
        // updates land partially, then the process vanishes.
        let mut disk = Disk::open(&path, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "NEW.BIN", &new_content())
            .expect("writes");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        assert!(blocks.len() >= 2, "the staged write set spans blocks");
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        // The next open reconciles to wholly the old image: metadata
        // consistent, the old file intact, the interrupted one absent.
        let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!crate::journal::sidecar_path(&path).exists());
        let geometry = reopened.geometry().expect("geometry");
        assert_eq!(geometry.volumes.len(), 1);
        assert_eq!(
            reopened.stat("superfloppy:0", "NEW.BIN").expect("stats"),
            None
        );
        assert_eq!(
            reopened.read_file("superfloppy:0", "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_backing_member_reconciles_before_the_chain_composes() {
        let base = std::env::temp_dir().join(format!(
            "remanence-durable-chain-base-{}.qcow2",
            std::process::id()
        ));
        let top = std::env::temp_dir().join(format!(
            "remanence-durable-chain-top-{}.qcow2",
            std::process::id()
        ));
        let virtual_size = build_fat16_qcow2(&base);
        let mut disk = Disk::open(&base, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &old_content())
            .expect("writes");
        disk.commit().expect("commits");
        drop(disk);

        // An interrupted commit on the base, left behind before it
        // became a backing file.
        let mut disk = Disk::open(&base, AccessIntent::Write).expect("opens");
        disk.write_file("superfloppy:0", "OLD.BIN", &new_content())
            .expect("overwrites");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        // A fresh top image naming the base as its backing file.
        let mut image = empty_qcow2_bytes(virtual_size);
        let name = base.to_str().expect("utf-8 temp path").as_bytes();
        image[0x200..0x200 + name.len()].copy_from_slice(name);
        image[8..16].copy_from_slice(&0x200u64.to_be_bytes());
        image[16..20].copy_from_slice(&(name.len() as u32).to_be_bytes());
        std::fs::write(&top, image).expect("top writes");

        // Composing the chain reconciles the base first (P9): its
        // sidecar is gone and wholly the old bytes show through.
        let mut chained = Disk::open(&top, AccessIntent::Read).expect("reconciles and composes");
        assert!(!crate::journal::sidecar_path(&base).exists());
        assert_eq!(
            chained.read_file("superfloppy:0", "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(chained);
        std::fs::remove_file(&top).ok();
        std::fs::remove_file(&base).ok();
    }

    /// The same synthetic FAT16 volume the unit tests build.
    fn fat16_volume_bytes() -> Vec<u8> {
        const TOTAL_SECTORS: usize = 8000;
        let mut image = vec![0u8; TOTAL_SECTORS * 512];
        image[0] = 0xeb;
        image[1] = 0x3c;
        image[2] = 0x90;
        image[3..11].copy_from_slice(b"REMANENC");
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1u16.to_le_bytes());
        image[16] = 2;
        image[17..19].copy_from_slice(&512u16.to_le_bytes());
        image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
        image[21] = 0xf8;
        image[22..24].copy_from_slice(&32u16.to_le_bytes());
        image[510] = 0x55;
        image[511] = 0xaa;
        for fat in 0..2usize {
            let base = (1 + fat * 32) * 512;
            image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
            image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
        }
        let root = (1 + 2 * 32) * 512;
        image[root..root + 11].copy_from_slice(b"REMANENCE  ");
        image[root + 11] = 0x08;
        image
    }
}
