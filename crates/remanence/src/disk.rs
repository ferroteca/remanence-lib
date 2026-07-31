// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The `Disk` surface (U3 and U4): open a raw or qcow2
//! image under the P7 claim, report its partitions and volumes as they
//! actually are, and read/write files in its FAT volumes with a commit
//! point (P2) — everything rolls back until `commit`.

use std::path::Path;

use crate::device::{AccessIntent, AccessMode, Device, FileDevice, Overlay};
use crate::error::{Error, Result};
use crate::fat::{FatEntry, FatVolume, VolumeInfo};
use crate::mbr::{self, Discovery, PartitionInfo, PartitionKind};
use crate::qcow2::{QCOW2_MAGIC, Qcow2};

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
}

/// A composed view: the overlay patched over the virtual disk. Reads see
/// buffered writes; nothing reaches the file until commit.
struct Composed<'a> {
    base: &'a mut dyn Device,
    overlay: &'a mut Overlay,
}

impl Device for Composed<'_> {
    fn len(&self) -> u64 {
        self.base.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.overlay.read_at(self.base, offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.overlay.write_at(self.base, offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// An open disk image.
#[derive(Debug)]
pub struct Disk {
    virtual_disk: Virtual,
    overlay: Overlay,
    format: DiskFormat,
    mode: AccessMode,
    path: String,
}

impl Disk {
    /// Opens `path` with the caller's declared intent (P7). A `Write`
    /// open claims the image exclusively — no other reader or writer
    /// for the session's whole life — and an open that cannot secure
    /// that claim fails at the open, naming the reason, never by
    /// falling back to read-only (a running VM holding the image is
    /// the designed refusal). A `Read` open takes read access only,
    /// denies writes to every other process, and keeps admitting other
    /// readers. The container is detected by magic: qcow2, else raw.
    pub fn open(path: impl AsRef<Path>, intent: AccessIntent) -> Result<Self> {
        let path = path.as_ref();
        let mut file = FileDevice::open(path, intent)?;
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
                let qcow2 = Qcow2::open(file)?;
                let version = qcow2.header().version;
                (Virtual::Qcow2(qcow2), DiskFormat::Qcow2 { version })
            }
        };

        Ok(Self {
            virtual_disk,
            overlay: Overlay::new(),
            format,
            mode,
            path: path.display().to_string(),
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
        self.overlay.modified()
    }

    fn composed(&mut self) -> Composed<'_> {
        Composed {
            base: self.virtual_disk.device(),
            overlay: &mut self.overlay,
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
    pub fn commit(&mut self) -> Result<()> {
        self.require_writable()?;
        self.overlay.commit(self.virtual_disk.device())
    }

    /// Discards everything buffered; the image is untouched.
    pub fn rollback(&mut self) {
        self.overlay.rollback();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcow2::QCOW2_MAGIC;

    /// Builds a qcow2 file on disk whose virtual disk carries a FAT16
    /// volume, using the crate's own writer — then exercises the whole
    /// public path reliquary runs: open, geometry, write, commit, reopen,
    /// read back.
    #[test]
    fn fat16_inside_qcow2_end_to_end() {
        const CLUSTER_BITS: u32 = 12;
        const CLUSTER: u64 = 1 << CLUSTER_BITS;

        // A minimal empty v3 qcow2 (mirrors the qcow2 unit-test builder).
        let virtual_size = 4_096_000u64; // the synthetic FAT16 volume size
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

        let path =
            std::env::temp_dir().join(format!("remanence-qcow2-e2e-{}.qcow2", std::process::id()));
        std::fs::write(&path, image).expect("qcow2 writes");

        // Format the virtual disk: write a FAT16 volume into guest space
        // through the crate's own qcow2 writer.
        {
            let file = crate::device::FileDevice::open(&path, AccessIntent::Write).expect("opens");
            let mut qcow2 = crate::qcow2::Qcow2::open(file).expect("parses");
            let volume = fat16_volume_bytes();
            assert_eq!(volume.len() as u64, virtual_size);
            qcow2.write_at(0, &volume).expect("formats");
            qcow2.flush().expect("flushes");
        }

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

    /// The same synthetic FAT16 volume the integration tests build.
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
