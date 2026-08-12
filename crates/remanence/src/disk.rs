// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The media state a storage device homes (U3 and U4): a raw, qcow2 or
//! VDI image open under the P7 claim, its partitions and volumes as they
//! actually are, and read/write access to the files in its FAT volumes
//! with a commit point (P2) — everything rolls back until `commit`. The commit is
//! durable (P9): a recovery journal is armed beneath the write-through,
//! so an interruption at any point leaves state the next open
//! reconciles — wholly the old image or wholly the committed new one —
//! before the disk is exposed. An image whose content lives partly
//! behind it — a qcow2 backing chain, a VDI differencing chain — opens
//! as one composed disk (U6), every member claimed for the session's
//! life and writes allocated copy-on-write into the top image only.
//!
//! Every open carries its assurance (P28). A source that satisfies its
//! interpretation is verified and keeps whatever authority the caller
//! declared; one that falls short of it — a raw image whose FAT boot
//! record declares more bytes than the file holds — is degraded: bounded
//! to the extent that is really there, read-only for the session's whole
//! life, and naming every operation it withholds.
//!
//! **This is not a handle.** A caller never holds a medium outside a
//! device, so the medium survives as a model node and as data on
//! [`crate::StorageDevice`] rather than as a type of its own. Every verb
//! below is reached through the device that homes it, and the
//! caller-facing contract each one answers for is documented there,
//! beside the slot-side facts it sits with.

use std::path::{Path, PathBuf};

use crate::adapters::{self, ActiveLayer, DeviceIdentity, ImageFormatDescriptor, OpenedImage};
use crate::archive::ArchiveMedium;
use crate::assurance::{self, Assurance, ReadBound, Shortfall};
use crate::cache::SessionCache;
use crate::device::{AccessIntent, AccessMode, Claim, Device};
use crate::device_family::DeviceFamily;
use crate::error::{Error, Result};
use crate::fat::{FatEntry, FatVolume, VolumeDeclaration};
use crate::filesystem::Catalog;
use crate::filesystem_catalog::FilesystemAdapter;
use crate::journal;
use crate::mbr::{self, Discovery};
use crate::media::Format;
use crate::media_profile::MediaProfile;
use crate::partition::{PartitionPool, PartitionScheme};
use crate::session::{self, Identification, Layer};
use crate::source::{self, ImageSource, ResolvedImage};

use crate::report::{
    DeclaredGeometry, DeviceInfo, DiskReport, FilesystemId, FilesystemInfo, PartitionSchemaInfo,
    RegionId, RegionInfo, VolumeId, VolumeInfo, VolumeOrigin,
};

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

/// The image container format a disk image turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFormat {
    Raw,
    Qcow2 { version: u32 },
    Vdi { major: u32, minor: u32 },
}

/// A composed view: the session cache over the virtual disk (P27).
/// Reads stream through the cache and see buffered writes; altered
/// extents stay in the cache — in memory or spilled to private session
/// storage — and nothing reaches the file until commit (P2).
struct Composed<'a> {
    base: &'a mut dyn Device,
    cache: &'a mut SessionCache,
    /// The readable extent of a degraded session (P28). Every read of the
    /// presented disk passes through here, so the bound is checked once,
    /// where the reads are, rather than at each verb that might cross it.
    bound: Option<ReadBound>,
}

impl Device for Composed<'_> {
    fn len(&self) -> u64 {
        self.base.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if let Some(bound) = &self.bound {
            bound.check(offset, buf.len() as u64)?;
        }
        self.cache.read_at(self.base, offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.cache.write_at(self.base, offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// One partition's extent, presented as a device of its own.
///
/// A filesystem adapter receives an extent and does not know whether it
/// came from one partition or from the whole content (P18), so a
/// declared namespace is read through this rather than through the
/// presented disk with an offset the adapter would have to be told
/// about.
struct Window<'a> {
    base: &'a mut dyn Device,
    offset: u64,
    length: u64,
}

impl Device for Window<'_> {
    fn len(&self) -> u64 {
        self.length
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.within(offset, buf.len() as u64, "read")?;
        self.base.read_at(self.offset + offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.within(offset, data.len() as u64, "write")?;
        self.base.write_at(self.offset + offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        self.base.flush()
    }
}

impl Window<'_> {
    /// The bound is the partition's, not the medium's, which is the point
    /// of reading a namespace within one.
    fn within(&self, offset: u64, length: u64, act: &str) -> Result<()> {
        offset
            .checked_add(length)
            .filter(|end| *end <= self.length)
            .map(drop)
            .ok_or_else(|| {
                Error::io(format!(
                    "this partition runs {} bytes and the {act} at {offset} \
                     reaches past it",
                    self.length
                ))
            })
    }
}

/// The assurance gate (P28) over one opened image, run before the medium
/// is exposed to anything.
///
/// The gate is narrow, and this is where its narrowness lives: it applies
/// to a raw image whose leading sector is a FAT boot record — the one
/// composition where a filesystem's own declaration bounds the whole disk,
/// because the caller selected the image's bytes as the disk. An image
/// container format declares its own virtual size and answers for it at
/// its version gate (P8), so no automatic degradation rule is claimed for
/// qcow2, VDI, an archive, or a partition schema; each is its own feature
/// if it is ever wanted.
fn assess(
    image: &mut dyn OpenedImage,
    format: DiskFormat,
    mode: AccessMode,
    claim: Claim,
) -> Result<Assurance> {
    let observed = image.presented_size();
    if format != DiskFormat::Raw || observed < 512 {
        return Ok(Assurance::verified(observed, mode, claim));
    }
    let mut sector = [0u8; 512];
    image.read_at(0, &mut sector)?;
    Ok(match crate::fat::declared_volume(&sector) {
        VolumeDeclaration::Bounded {
            bytes,
            metadata_end,
            reading,
        } if bytes > observed => assurance::degraded(
            Shortfall {
                declared: bytes,
                observed,
                metadata_end,
            },
            &reading,
            claim,
        ),
        // A contradiction is only this gate's business where the source is
        // also short: with the declared bytes all present there is nothing
        // to bound, and what the boot record says about itself is the
        // filesystem seam's to report as an issue, exactly as it does today.
        VolumeDeclaration::Conflicted { bytes, detail } if bytes > observed => {
            return Err(assurance::conflicted(&detail, bytes, observed));
        }
        _ => Assurance::verified(observed, mode, claim),
    })
}

/// The state of one open medium — what a storage device homes while that
/// medium occupies its slot.
///
/// One medium, one P7 claim, and the two planes that claim serves (F43):
/// the **raw** plane — the image's own bytes, which identification and the
/// HDOS reader work over, streamed through `source`'s cache and predictive
/// reader — and the **presented** plane, the disk a format adapter exposes
/// above it, which `virtual_disk` owns and the file verbs work over. They
/// are different layers under P13, not duplicates, and before F43 they were
/// two separate top-level types that could not both be opened on one image
/// because each took its own claim.
#[derive(Debug)]
pub(crate) struct MediaState {
    virtual_disk: Box<dyn OpenedImage>,
    /// The raw plane over the shared claim: the session cache and the
    /// predictive reader (P27, P34).
    source: ImageSource,
    /// The archive wrappers unwrapped on the way in, if any.
    layers: Vec<Layer>,
    /// The resolved image — the entry name for an archived image, else
    /// the name recovered from the source. Absent where the caller's
    /// handle has none.
    image_path: Option<PathBuf>,
    cache: SessionCache,
    /// The declared session cache bound (P27), governing the session
    /// cache and each commit's capture alike.
    cache_bytes: u64,
    format: DiskFormat,
    descriptor: &'static ImageFormatDescriptor,
    /// The media type this medium is (P14). The adapter named it when it
    /// loaded the state — an image format loads and saves media state,
    /// so it is what establishes which medium the state belongs to — and
    /// the medium carries it from there, immutably, for as long as the
    /// session holds it.
    media: &'static MediaProfile,
    device_identity: DeviceIdentity,
    active_layer: ActiveLayer,
    /// The session's **effective** access (P28): the declared intent's
    /// echo, or read-only where the evidence never established write
    /// authority.
    mode: AccessMode,
    /// What this open established about the evidence beneath it (P28),
    /// settled once at the open and never revisited: a session never
    /// regains authority it did not open with.
    assurance: Assurance,
    /// The readable extent a degraded session reads under, carried beside
    /// the assurance because every composed read consults it.
    bound: Option<ReadBound>,
    /// The artifact claimed, where a name for it exists — recovered from
    /// the caller's handle, or the path the library opened.
    path: Option<String>,
    /// The recovery sidecar's derived path (P9) — private transient
    /// state, never a user-owned file. Absent where the source handle
    /// has no recoverable name, and the commit refuses by name there
    /// rather than journalling somewhere nobody named.
    journal_path: Option<PathBuf>,
    /// Set when a commit failed partway and its in-process undo failed
    /// too: the session's caches no longer describe the file, so every
    /// verb refuses until a fresh open reconciles the image.
    failed: Option<String>,
}

/// What occupies a device's slot: a medium of one of the two vantages
/// the model claims.
///
/// **Families own their representation** (P14), and this is that rule at
/// the state tier. A space-native medium holds an addressed disk with
/// structure above it; a namespace-native one holds named entries and no
/// space at all. Rather than one state with most of its fields empty,
/// each kind is its own, and the verbs that need a space say so —
/// asking an archive to inspect partitions is a category error answered
/// by name, not a hole to fall into.
#[derive(Debug)]
pub(crate) enum MediumState {
    /// A medium whose native vantage is a space: the block state an
    /// image format loaded, with partitions, volumes and namespaces
    /// above it.
    Space(MediaState),
    /// A medium whose native vantage is a namespace: an archive, whose
    /// content is names and whose bytes are its encoding (P13).
    Archive(ArchiveMedium),
}

impl MediumState {
    /// Opens the artifact at `path` as whichever medium it is.
    ///
    /// **The grammar that recognizes an artifact is what settles this**
    /// (P12): an enrolled archive grammar claiming the artifact makes it
    /// an archive medium, and everything else goes to the image catalog
    /// as before. Both loads and discovery come through here, so the two
    /// never disagree about what an artifact is.
    pub(crate) fn open(path: &Path, intent: AccessIntent, cache_bytes: u64) -> Result<Self> {
        if crate::archive::is_archive(path) {
            return Ok(Self::Archive(ArchiveMedium::open(
                path,
                intent,
                cache_bytes,
            )?));
        }
        Ok(Self::Space(MediaState::open_with_cache(
            path,
            intent,
            cache_bytes,
        )?))
    }

    /// Loads the caller's own opened artifact as the format they
    /// **declared** it to be.
    ///
    /// This is the declared reading, and the declaration is checked
    /// rather than trusted: the one adapter the format names is asked
    /// whether the evidence bears it, and a refusal names both what was
    /// declared and what was found. Nothing here probes for a second
    /// answer — a caller who does not know what an artifact is asks
    /// [`discover_media`](crate::discover_media) instead.
    pub(crate) fn load(file: std::fs::File, format: Format, cache_bytes: u64) -> Result<Self> {
        if let Some(grammar) = format.archive_grammar() {
            return Ok(Self::Archive(ArchiveMedium::load(
                file,
                grammar,
                cache_bytes,
            )?));
        }
        Ok(Self::Space(MediaState::load(file, format, cache_bytes)?))
    }

    /// Opens one archive entry as a medium of its own — the nested
    /// journey, reached from the file view that names the entry.
    pub(crate) fn open_entry(resolved: ResolvedImage, cache_bytes: u64) -> Result<Self> {
        Ok(Self::Space(MediaState::open_resolved(
            resolved,
            cache_bytes,
        )?))
    }

    /// The artifact claimed — the archive itself for an image loaded out
    /// of one — where a name for it exists.
    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Self::Space(space) => space.path(),
            Self::Archive(archive) => archive.path(),
        }
    }

    /// The artifact as a refusal names it: the recovered name, or the
    /// stated fact that the caller's handle has none.
    pub(crate) fn named(&self) -> String {
        crate::media::named(self.path())
    }

    /// The resolved artifact — the entry name for an image loaded out of
    /// an archive, else the source's own name.
    pub(crate) fn image_path(&self) -> Option<&Path> {
        match self {
            Self::Space(space) => space.image_path(),
            Self::Archive(archive) => archive.path().map(Path::new),
        }
    }

    /// The artifact's own bytes, which every medium has: the evidence
    /// plane, distinct from any vantage above it.
    pub(crate) fn image_size_bytes(&self) -> u64 {
        match self {
            Self::Space(space) => space.image_size_bytes(),
            Self::Archive(archive) => archive.size_bytes(),
        }
    }

    /// Reads the artifact's own bytes, streamed (P27).
    pub(crate) fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Space(space) => space.read_at(offset, buf),
            Self::Archive(archive) => archive.read_at(offset, buf),
        }
    }

    pub(crate) fn media(&self) -> &'static MediaProfile {
        match self {
            Self::Space(space) => space.media(),
            Self::Archive(archive) => archive.media(),
        }
    }

    pub(crate) fn mode(&self) -> AccessMode {
        match self {
            Self::Space(space) => space.mode(),
            Self::Archive(archive) => archive.mode(),
        }
    }

    pub(crate) fn assurance(&self) -> &Assurance {
        match self {
            Self::Space(space) => space.assurance(),
            Self::Archive(archive) => archive.assurance(),
        }
    }

    pub(crate) fn identify(&self) -> Identification {
        match self {
            Self::Space(space) => space.identify(),
            Self::Archive(archive) => archive.identify(),
        }
    }

    pub(crate) fn is_modified(&self) -> bool {
        match self {
            Self::Space(space) => space.is_modified(),
            // Read-only, so there is never anything buffered to lose.
            Self::Archive(_) => false,
        }
    }

    /// The recognized format's stable spelling — an image format for a
    /// space, an archive grammar for a namespace.
    pub(crate) fn format_id(&self) -> &'static str {
        match self {
            Self::Space(space) => space.descriptor().id,
            Self::Archive(archive) => archive.format_id(),
        }
    }

    /// That format's name, fit to show a user.
    pub(crate) fn format_name(&self) -> &'static str {
        match self {
            Self::Space(space) => space.descriptor().name,
            Self::Archive(archive) => archive.format_name(),
        }
    }

    /// The device family the recognizing format declares for what it
    /// records (P12), or `None` where it declares none.
    ///
    /// An archive grammar declares the archive slot, and does so with
    /// certainty no image format has about a disk: a zip belongs in
    /// nothing else.
    pub(crate) fn default_device(&self) -> Option<DeviceFamily> {
        match self {
            Self::Space(space) => space.descriptor().default_device,
            Self::Archive(_) => Some(DeviceFamily::ARCHIVE_DEVICE),
        }
    }

    /// The family of an artifact this release recognizes and holds in no
    /// device — flux, today.
    pub(crate) fn foreign_family(&self) -> Option<&'static str> {
        match self {
            Self::Space(space) => space.foreign_family(),
            Self::Archive(_) => None,
        }
    }

    /// The space this medium presents, or the refusal naming the vantage
    /// it has instead.
    ///
    /// Every verb that addresses a space passes through here, so a
    /// namespace-native medium answers by name rather than by a verb
    /// failing further in — the same discipline an empty slot already
    /// answers a content verb with.
    pub(crate) fn space(&self, verb: &str) -> Result<&MediaState> {
        match self {
            Self::Space(space) => Ok(space),
            Self::Archive(archive) => Err(no_space(verb, archive)),
        }
    }

    pub(crate) fn space_mut(&mut self, verb: &str) -> Result<&mut MediaState> {
        match self {
            Self::Space(space) => Ok(space),
            Self::Archive(archive) => Err(no_space(verb, archive)),
        }
    }

    /// The archive this medium is, where it is one.
    pub(crate) fn archive(&self) -> Option<&ArchiveMedium> {
        match self {
            Self::Space(_) => None,
            Self::Archive(archive) => Some(archive),
        }
    }

    /// The partition pool this medium bears, established here and
    /// nowhere else — at the load, before the medium is handed to
    /// anyone (F56's "checked at load").
    ///
    /// **The pool populates under the medium's own kind, never by
    /// probing for a layout.** A medium whose native vantage is a space
    /// has the scheme its family is laid out under checked against its
    /// content, and bears the direct partition where the content does
    /// not carry that scheme; a medium whose native vantage is a
    /// namespace bears the direct partition with no extent at all,
    /// because there is no position for one to be within.
    pub(crate) fn establish_partitions(&mut self) -> Result<PartitionPool> {
        match self {
            Self::Archive(_) => Ok(PartitionPool::native_namespace()),
            Self::Space(space) => {
                let length = space.size();
                let discovery = space.check_scheme()?;
                Ok(PartitionPool::over_space(
                    PartitionScheme::Mbr,
                    &discovery,
                    length,
                ))
            }
        }
    }
}

/// The refusal a space verb makes on a namespace-native medium.
fn no_space(verb: &str, archive: &ArchiveMedium) -> Error {
    Error::unsupported(format!(
        "'{verb}' addresses a space and {} holds an archive medium, whose \
         vantage is a namespace: an archive records no scheme, has no volume \
         and no sector to address, and its content is reached through the \
         namespace door of the direct partition it bears",
        archive.named()
    ))
}

impl MediaState {
    /// Opens `path` at the stated default cache bound.
    ///
    /// Test-only. A medium reaches a caller through
    /// [`crate::Session::load_media`] and nothing else, so
    /// this exists for the unit tests in this module, which exercise the
    /// device stack below the device tier.
    #[cfg(test)]
    pub(crate) fn open(path: impl AsRef<Path>, intent: AccessIntent) -> Result<Self> {
        Self::open_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// Opens `path` with the caller's declared intent (P7). A `Write`
    /// open claims the image exclusively — no other reader or writer
    /// for as long as the medium stays attached — and an open that
    /// cannot secure that claim fails here, naming the reason, never by
    /// falling back to read-only (a running VM holding the image is
    /// the designed refusal). A `Read` open takes read access only,
    /// denies writes to every other process, and keeps admitting other
    /// readers. An interrupted commit left by an earlier session is
    /// reconciled here, before the disk is exposed (P9): the image
    /// comes back wholly the old state or wholly the committed new
    /// one, never a partial third state. The image format is whichever
    /// adapter recognizes it — qcow2 or VDI today — and raw where none
    /// does. A qcow2 naming a backing file, and a VDI naming a parent
    /// identity, open with their whole chain composed, every file behind
    /// the top one claimed immutable for the session's life (U6). Writes
    /// allocate copy-on-write into the top image only; commit preserves
    /// the relationship.
    ///
    /// `cache_bytes` is the caller-declared session cache bound (P27):
    /// at most that much session state stays resident — reads,
    /// uncommitted writes, and a commit's staging alike — rounded up to
    /// whole 64 KiB extents, with one extent as the floor. Altered state
    /// past the bound spills to private session storage, never the
    /// image.
    pub(crate) fn open_with_cache(
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<Self> {
        let path = path.as_ref();
        let recovery = journal::sidecar_path(path);

        let mut resolved = source::resolve_image(path, intent, cache_bytes)?;
        // The sidecar check runs under our claim, so no live commit can
        // be mid-flight — a sidecar here is an interrupted one (P9).
        if recovery.exists() {
            match intent {
                AccessIntent::Write => {
                    let mut host = resolved.source.medium_device(path.display().to_string());
                    journal::reconcile(&recovery, &mut host, path)?;
                }
                AccessIntent::Read => {
                    // Reconciling writes; trade the read claim for a
                    // moment of exclusive access, then take it back.
                    drop(resolved);
                    journal::reconcile_at(path)?;
                    resolved = source::resolve_image(path, intent, cache_bytes)?;
                }
            }
        }
        Self::over(resolved, Some(recovery), None, cache_bytes)
    }

    /// Loads the caller's own opened file as the format they declared,
    /// under **their** claim (P7 as amended).
    ///
    /// The declaration is checked by the one adapter it names, so a
    /// qcow2 declared `h8d` is refused naming both sides rather than
    /// read as whatever a probe would have picked. A journal is derived
    /// where the handle has a recoverable name and is absent where it
    /// does not; an interrupted commit is reconciled here as it is on
    /// the path journey, and only where there is a name to find one by.
    pub(crate) fn load(file: std::fs::File, format: Format, cache_bytes: u64) -> Result<Self> {
        let resolved = source::resolve_handle(file, cache_bytes)?;
        let recovery = resolved
            .source_path
            .as_deref()
            .map(journal::sidecar_path)
            .filter(|sidecar| sidecar.exists());
        if let (Some(sidecar), Some(path)) = (&recovery, resolved.source_path.clone()) {
            // Reconciling writes, and only a handle that affords writing
            // can perform it — a read-only claim is told rather than
            // silently left standing on an unreconciled image (P9, P7).
            if resolved.source.mode() != AccessMode::ReadWrite {
                return Err(Error::read_only(format!(
                    "'{}' carries an interrupted commit's recovery journal \
                     '{}', and reconciling it is a write: hand over a handle \
                     opened for writing so the image can be put back to \
                     wholly the old state or wholly the committed new one",
                    path.display(),
                    sidecar.display()
                )));
            }
            let mut host = resolved.source.medium_device(path.display().to_string());
            journal::reconcile(sidecar, &mut host, &path)?;
        }
        let sidecar = resolved.source_path.as_deref().map(journal::sidecar_path);
        Self::over(resolved, sidecar, Some(format), cache_bytes)
    }

    /// Opens the medium an already-resolved source holds — the entry
    /// journey, where the artifact was reached through a namespace
    /// rather than named by path.
    ///
    /// An archive entry is read-only and never commits, so it has no
    /// journal to reconcile and no sidecar to find; everything above the
    /// backing is the same open a path takes, which is what makes a
    /// nested artifact the same journey rather than a second one.
    pub(crate) fn open_resolved(resolved: ResolvedImage, cache_bytes: u64) -> Result<Self> {
        let recovery = resolved.image_path.as_deref().map(journal::sidecar_path);
        Self::over(resolved, recovery, None, cache_bytes)
    }

    /// The open every journey shares: the format adapter over the
    /// resolved backing — the one the caller declared, or whichever the
    /// catalog recognizes — the assurance gate before anything is
    /// exposed, and the layers the artifact was reached through.
    fn over(
        resolved: ResolvedImage,
        recovery: Option<PathBuf>,
        declared: Option<Format>,
        cache_bytes: u64,
    ) -> Result<Self> {
        let path = resolved.source_path.clone();
        let named = crate::media::named(
            path.as_deref()
                .map(|path| path.to_string_lossy())
                .as_deref(),
        );
        let mode = resolved.source.mode();

        // One claim, two planes: the adapter opens the presented disk over
        // a medium device sharing the very claim the raw plane reads (F43).
        let host = resolved.source.medium_device(named.clone());
        let image_path = resolved.image_path.as_deref();
        let (mut virtual_disk, descriptor) = match declared {
            Some(declared) => adapters::open_declared(declared, host, image_path, &named)?,
            None => adapters::image_catalog().open_disk(host, image_path)?,
        };
        let format = virtual_disk.format();

        // The assurance gate (P28), settled before the medium is exposed:
        // a caller who is going to be told a disk is degraded is told
        // before it reads a byte of it.
        let assurance = assess(virtual_disk.as_mut(), format, mode, resolved.claim)?;
        let mode = assurance.access;
        let bound = assurance.condition.map(|condition| ReadBound {
            end: assurance.first_unavailable_byte.unwrap_or(0),
            declared: assurance.declared_bytes.unwrap_or(0),
            condition,
        });

        let layers = resolved
            .archive_layers
            .into_iter()
            .map(session::layer_from_archive)
            .collect();

        Ok(Self {
            virtual_disk,
            source: resolved.source,
            layers,
            image_path: resolved.image_path,
            cache: SessionCache::with_bytes_offloading(cache_bytes),
            cache_bytes,
            format,
            descriptor,
            media: descriptor.media,
            device_identity: DeviceIdentity::first(),
            active_layer: descriptor.initial_active_layer,
            mode,
            assurance,
            bound,
            // The artifact claimed, which for an archived image is the
            // archive rather than the entry loaded out of it.
            path: path.map(|path| path.display().to_string()),
            journal_path: recovery,
            failed: None,
        })
    }

    /// The resolved image — the entry name for an image opened from
    /// inside an archive, else the source's own recovered name.
    pub(crate) fn image_path(&self) -> Option<&Path> {
        self.image_path.as_deref()
    }

    /// The resolved image's own size in bytes — the raw plane, distinct
    /// from [`MediaState::size`], which is the presented one.
    pub(crate) fn image_size_bytes(&self) -> u64 {
        self.source.len()
    }

    /// Reads `buf` from the resolved image at `offset` — the medium's own
    /// bytes, not the presented disk, and bounded where the assurance
    /// narrowed the readable extent (P27, P28).
    pub(crate) fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if let Some(bound) = &self.bound {
            bound.check(offset, buf.len() as u64)?;
        }
        self.source.read_at(offset, buf)
    }

    /// Identifies the artifact's nesting layers and probable filesystem
    /// over the raw plane, probing bounded evidence alone (P27).
    pub(crate) fn identify(&self) -> Identification {
        session::identify_medium(
            &self.source,
            self.image_path.as_deref(),
            &self.layers,
            self.device_identity,
            self.is_modified(),
        )
    }

    /// Checks the scheme this medium's family is laid out under against
    /// the content, once, at the load.
    ///
    /// It reads the *presented* disk, not the raw plane: a partition
    /// table sits on the disk a format adapter exposes. The answer is
    /// the scheme adapter's own — the table it parsed, or which of the
    /// three ways the content does not carry one (P16).
    pub(crate) fn check_scheme(&mut self) -> Result<Discovery> {
        self.require_usable()?;
        let mut composed = self.composed();
        mbr::discover(&mut composed)
    }

    /// Opens the namespace `adapter` reads, over one partition's extent
    /// and nothing wider.
    ///
    /// The adapter is the one the caller's declaration named, and it
    /// reads the evidence to verify that declaration rather than to pick
    /// one (P18). Its own bound is what bounds the reading (P27).
    pub(crate) fn open_namespace_at(
        &mut self,
        adapter: &'static dyn FilesystemAdapter,
        offset: u64,
        length: u64,
    ) -> Result<Box<dyn Catalog>> {
        self.require_usable()?;
        let mut composed = self.composed();
        let mut window = Window {
            base: &mut composed,
            offset,
            length,
        };
        adapter.open(&mut window)
    }

    /// Recognizes FAT on one partition's extent, answering the facts the
    /// filesystem seam established (P18).
    pub(crate) fn recognize_fat(&mut self, offset: u64) -> Result<crate::fat::FatRecognition> {
        self.require_usable()?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.recognized(&mut composed)
    }

    /// The **effective** access mode this open settled on (P28): the
    /// declared intent's echo where the evidence supports it, read-only
    /// where it does not.
    pub(crate) fn mode(&self) -> AccessMode {
        self.mode
    }

    /// What this open established about the evidence beneath it (P28),
    /// settled before anything was read.
    pub(crate) fn assurance(&self) -> &Assurance {
        &self.assurance
    }

    /// The media type this medium is (P14) — named by the image-format
    /// adapter that loaded its state, and what a device's family is
    /// checked against when the medium is loaded into it.
    pub(crate) fn media(&self) -> &'static MediaProfile {
        self.media
    }

    /// The image format that loaded this state, as the catalog declares
    /// it — the format's own identity, and the device family it declares
    /// for the disks it records.
    pub(crate) fn descriptor(&self) -> &'static ImageFormatDescriptor {
        self.descriptor
    }

    /// The family of an artifact this release recognizes as belonging to
    /// another one — `None` where the medium is the block family this
    /// state serves.
    ///
    /// The library can only refuse what it can recognize. An artifact it
    /// cannot place at all still opens at the block catalog's raw
    /// fallback, which is the honest limit of this check rather than a
    /// hole in it: NIB and NBZ, for instance, have no recognizer until
    /// the principle that places them at the flux rung is delivered.
    ///
    /// A P64 records timed pulses, and the block catalog opens anything
    /// it cannot identify at the raw adapter — so without this the block
    /// layer would be declared authoritative where the artifact's own
    /// adapter declares flux, which in-force P13 forbids. It is reached
    /// through its own type, as the capture-set adapter is.
    pub(crate) fn foreign_family(&self) -> Option<&'static str> {
        let mut prefix = [0u8; 8];
        if self.read_at(0, &mut prefix).is_err() {
            return None;
        }
        crate::p64::has_signature(&prefix).then_some("flux")
    }

    pub(crate) fn format(&self) -> DiskFormat {
        debug_assert_eq!(self.active_layer, self.descriptor.initial_active_layer);
        let _composition_identity = self.device_identity.value();
        let _authoritative_layer = self.descriptor.authoritative_layer;
        self.format
    }

    /// The virtual disk size (the guest-visible size for qcow2).
    pub(crate) fn size(&self) -> u64 {
        self.virtual_disk.presented_size()
    }

    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The artifact as a refusal names it.
    fn named(&self) -> String {
        crate::media::named(self.path())
    }

    /// Whether uncommitted changes exist.
    pub(crate) fn is_modified(&self) -> bool {
        self.cache.modified()
    }

    fn composed(&mut self) -> Composed<'_> {
        Composed {
            base: self.virtual_disk.device_mut(),
            cache: &mut self.cache,
            bound: self.bound,
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

    /// The layered inspection of this disk — **a view derived from the
    /// partition pool**, not the walk that finds it.
    ///
    /// The pool is the evidence and this is a reading of it: each fact
    /// still stays with the seam that owns it, a partition whose type is
    /// unread or whose chain broke is still reported with its refusal
    /// beside it, and neither renumbers what follows. The direct
    /// partition never appears here — it is the library's own
    /// composition, carried as the pool's provenance and never offered as
    /// something the medium declared — so a medium recording no scheme
    /// reports no region, exactly as it always has.
    pub(crate) fn inspect(&mut self, pool: &PartitionPool) -> Result<DiskReport> {
        self.require_usable()?;
        let device_identity = self.device_identity;
        let device = DeviceInfo {
            id: device_identity.value(),
            image_format: self.descriptor.id.to_owned(),
            media_type: self.media.id.to_owned(),
            length_bytes: 0,
            authoritative_layer: self.descriptor.authoritative_layer.name().to_owned(),
            active_layer: self.active_layer.name().to_owned(),
        };
        let mut composed = self.composed();
        let device = DeviceInfo {
            length_bytes: composed.len(),
            ..device
        };

        let mut report = DiskReport {
            device,
            content: pool.content(),
            partition_schema: pool.scheme().map(|scheme| PartitionSchemaInfo {
                kind: scheme.id().to_owned(),
                evidence: pool.schema_evidence().to_vec(),
                issues: Vec::new(),
            }),
            regions: Vec::new(),
            volumes: Vec::new(),
            filesystems: Vec::new(),
        };

        for partition in pool.partitions() {
            if partition.is_direct() {
                // The one member the report does not carry: a composition
                // act is provenance, and the evidence answer is unchanged.
                if let Some(id) = partition.volume_id() {
                    report.volumes.push(VolumeInfo {
                        id,
                        origin: VolumeOrigin::WholeDevice,
                        start_bytes: partition.start_bytes().unwrap_or(0),
                        length_bytes: partition.length_bytes().unwrap_or(0),
                        evidence: vec![
                            "sector 0 is a filesystem boot record, so the whole \
                             device composes as one volume"
                                .to_owned(),
                        ],
                        issues: Vec::new(),
                    });
                }
                continue;
            }
            let id = RegionId::declared(partition.ordinal());
            report.regions.push(RegionInfo {
                id,
                declared_number: partition.ordinal(),
                declared_placement: partition.placement().to_owned(),
                role: partition.role(),
                declared_type: partition.type_byte().unwrap_or(0),
                declared_type_reading: partition.type_reading().unwrap_or_default().to_owned(),
                claimed: partition.is_claimed(),
                start_bytes: partition.start_bytes().unwrap_or(0),
                length_bytes: partition.length_bytes().unwrap_or(0),
                issue: partition.issue().cloned(),
            });
            let Some(volume) = partition.volume_id() else {
                continue;
            };
            report.volumes.push(VolumeInfo {
                id: volume,
                origin: VolumeOrigin::Regions(vec![id]),
                start_bytes: partition.start_bytes().unwrap_or(0),
                length_bytes: partition.length_bytes().unwrap_or(0),
                evidence: vec![format!(
                    "direct composition of one data region declared at \
                     partition {}",
                    partition.ordinal()
                )],
                issues: Vec::new(),
            });
        }

        // Filesystem recognition is its own seam: it runs over volumes that
        // already exist, and neither creates one nor removes one.
        let volumes: Vec<(VolumeId, u64)> = report
            .volumes
            .iter()
            .map(|volume| (volume.id, volume.start_bytes))
            .collect();
        for (volume, offset) in volumes {
            let recognition = FatVolume::open(&mut composed, offset)
                .and_then(|fat| fat.recognized(&mut composed));
            report.filesystems.push(match recognition {
                Ok(facts) => FilesystemInfo {
                    id: FilesystemId::on_volume(volume),
                    volume,
                    kind: Some(facts.kind.name().to_owned()),
                    label: Some(facts.label),
                    cluster_bytes: Some(facts.cluster_bytes),
                    cluster_count: Some(facts.cluster_count),
                    declared_geometry: DeclaredGeometry {
                        sectors_per_track: facts.sectors_per_track,
                        heads: facts.heads,
                        cylinders: facts.cylinders,
                    },
                    evidence: vec![
                        "a FAT boot record recognized at the volume's first \
                         sector"
                            .to_owned(),
                    ],
                    issues: Vec::new(),
                },
                Err(issue) => FilesystemInfo {
                    id: FilesystemId::on_volume(volume),
                    volume,
                    kind: None,
                    label: None,
                    cluster_bytes: None,
                    cluster_count: None,
                    declared_geometry: DeclaredGeometry::default(),
                    evidence: Vec::new(),
                    issues: vec![issue],
                },
            });
        }

        Ok(report)
    }

    /// Lists a directory in the extent starting at `offset`
    /// ("" = root; "A/B" descends).
    pub(crate) fn entries(&mut self, offset: u64, path: &str) -> Result<Vec<FatEntry>> {
        let segments = Self::split_path(path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.entries(&mut composed, &segments)
    }

    /// Answers one path with its entry, or `None` where nothing exists
    /// at it — absence being an answer rather than a failure (U3).
    pub(crate) fn stat(&mut self, offset: u64, path: &str) -> Result<Option<FatEntry>> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.stat(&mut composed, &segments)
    }

    /// The degraded session's extraction gate (P28): an entry is answered
    /// whole or not at all, so its directory record and its complete
    /// cluster chain must lie inside the readable extent before a byte of
    /// it is served. A verified session has no gate to pass.
    ///
    /// This is what keeps a crossing file from being clipped, zero-filled,
    /// or served in the part that happens to be present — including
    /// through the ranged form, where the requested span alone might sit
    /// inside the extent while the file does not.
    fn require_whole(&mut self, offset: u64, segments: &[&str], path: &str) -> Result<()> {
        let Some(bound) = self.bound else {
            return Ok(());
        };
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        let end = fat.extent_end(&mut composed, segments)?;
        if end > bound.end {
            return Err(bound.withheld(&format!("'{path}'"), end));
        }
        Ok(())
    }

    /// Copies a file's bytes out of the extent starting at `offset`.
    pub(crate) fn read_file(&mut self, offset: u64, path: &str) -> Result<Vec<u8>> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        self.require_whole(offset, &segments, path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.read_file(&mut composed, &segments)
    }

    /// Reads part of a file — the streamed form (P27): exactly `buf`
    /// bytes at `offset`, which must lie within the file.
    pub(crate) fn read_file_at(
        &mut self,
        at: u64,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        self.require_whole(at, &segments, path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, at)?;
        fat.read_file_at(&mut composed, &segments, offset, buf)
    }

    /// Sets a file's size, creating it when absent: kept bytes preserved
    /// in place, a grown region reading as zeros. Buffered until commit.
    pub(crate) fn resize_file(&mut self, offset: u64, path: &str, size: u64) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.resize_file(&mut composed, &segments, size)
    }

    /// Writes part of a file in place — the streamed form (P27): the
    /// span must lie within the file's current size. Buffered until
    /// commit.
    pub(crate) fn write_file_at(
        &mut self,
        at: u64,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, at)?;
        fat.write_file_at(&mut composed, &segments, offset, data)
    }

    /// Reads at an absolute offset in the presented disk, for a space that
    /// resolved the position against its own extent.
    ///
    /// It reads through the session cache rather than off the source, so
    /// a caller sees the state its own buffered writes produced — the
    /// same truth every other verb in the session reads (P2, P27).
    pub(crate) fn read_space_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if let Some(bound) = &self.bound {
            bound.check(offset, buf.len() as u64)?;
        }
        let mut composed = self.composed();
        composed.read_at(offset, buf)
    }

    /// Writes at an absolute offset in the presented disk, for a space
    /// that resolved the position against its own extent. Buffered until
    /// commit (P2), landing in the active layer (P23).
    pub(crate) fn write_space_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.require_writable()?;
        let mut composed = self.composed();
        composed.write_at(offset, data)
    }

    fn require_writable(&self) -> Result<()> {
        // A degraded session answers first and by name: its read-only mode
        // is evidence-driven, and a caller that declared write intent is
        // owed the condition rather than the generic refusal (P28).
        if self.assurance.is_degraded() {
            return Err(assurance::read_only(&self.assurance, &self.named()));
        }
        if self.mode == AccessMode::ReadOnly {
            return Err(Error::read_only(match self.assurance.claim {
                // The caller's own open is the claim, so the refusal
                // says whose it is: the library never escalates a handle
                // it was handed (P7 as amended).
                Claim::CallerOpened => format!(
                    "{} was handed over on a handle that affords no write;                      write actions are denied",
                    self.named()
                ),
                Claim::LibraryOpened => format!(
                    "{} was opened for reading; write actions are denied",
                    self.named()
                ),
            }));
        }
        Ok(())
    }

    fn require_usable(&self) -> Result<()> {
        match &self.failed {
            Some(reason) => Err(Error::io(reason.clone())),
            None => Ok(()),
        }
    }

    /// Writes a file into the extent starting at `offset`, an
    /// existing one overwritten and an existing directory refused.
    /// Buffered until [`MediaState::commit`].
    pub(crate) fn write_file(&mut self, offset: u64, path: &str, contents: &[u8]) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.write_file(&mut composed, &segments, contents)
    }

    /// Ensures a directory exists, missing parents created and an
    /// existing directory succeeding unchanged. Buffered until commit.
    pub(crate) fn make_directory(&mut self, offset: u64, path: &str) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.make_directory(&mut composed, &segments)
    }

    /// The commit point (P2), staged and journalled so that it is also
    /// durable (P9): the write-through runs against a capture while the
    /// file is untouched, the bytes it will overwrite reach the recovery
    /// journal before the first of them changes, and the journal retires
    /// only once the apply is through. The three phases below are that
    /// sequence, and each one's failure path is what makes an
    /// interruption reconcilable.
    pub(crate) fn commit(&mut self) -> Result<()> {
        self.require_usable()?;
        self.require_writable()?;
        if !self.cache.modified() {
            return self.virtual_disk.device_mut().flush();
        }
        // The durable commit journals beside the artifact (P9), so it
        // needs to know where the artifact sits. A handle whose name
        // could not be recovered refuses here rather than committing
        // without the journal that makes an interruption reconcilable.
        let (journal_path, image_path) = match (&self.journal_path, &self.path) {
            (Some(journal_path), Some(path)) => (journal_path.clone(), PathBuf::from(path)),
            _ => {
                return Err(Error::unsupported(
                    "this medium's source handle has no recoverable name, and a \
                     durable commit lands its recovery journal beside the \
                     artifact: commit through a handle whose file this host can \
                     name, or keep the changes in the session"
                        .to_owned(),
                ));
            }
        };

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
        self.virtual_disk.host_mut().begin_capture(cache_bytes);
        let staged = self.cache.write_through(self.virtual_disk.device_mut());
        let capture = self.virtual_disk.host_mut().take_capture();
        if let Err(error) = staged {
            self.virtual_disk.restore_cache(cache_snapshot);
            return Err(error);
        }
        if capture.is_clean() {
            self.cache.mark_committed();
            return self.virtual_disk.device_mut().flush();
        }

        // The durability boundary (P9): the bytes the apply will
        // overwrite are durable in the recovery journal — streamed
        // there, never held whole — before the first of them changes.
        if let Err(error) = journal::record(&journal_path, self.virtual_disk.host_mut(), &capture) {
            let _ = journal::retire(&journal_path);
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
            .host_mut()
            .apply(&capture)
            .and_then(|()| {
                #[cfg(test)]
                crash_test_process_at("image-applied");
                journal::retire(&journal_path).map_err(|error| {
                    Error::io(format!(
                        "cannot retire the commit's recovery journal '{}': {error}",
                        journal_path.display()
                    ))
                })
            })
            .map(|()| {
                #[cfg(test)]
                crash_test_process_at("journal-retired");
            });
        if let Err(error) = applied {
            match journal::reconcile(&journal_path, self.virtual_disk.host_mut(), &image_path) {
                Ok(()) => {
                    self.virtual_disk.restore_cache(cache_snapshot);
                }
                Err(_) => {
                    self.failed = Some(format!(
                        "a commit on '{}' failed partway and could not be undone \
                         in this session; reopen the disk to reconcile it",
                        image_path.display()
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
    pub(crate) fn rollback(&mut self) {
        self.cache.discard_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pool a partitionless test image bears, established the way a
    /// load establishes one. These tests drive the state below the medium
    /// tier, so they run the same act `Session::admit` runs.
    fn pool_of(disk: &mut MediaState) -> PartitionPool {
        let discovery = disk.check_scheme().expect("the scheme is checked");
        PartitionPool::over_space(PartitionScheme::Mbr, &discovery, disk.size())
    }

    /// The extent the one partition of a partitionless test image
    /// composes — the position the file verbs work within.
    fn only_extent(disk: &mut MediaState) -> u64 {
        let pool = pool_of(disk);
        let report = disk.inspect(&pool).expect("inspection reads");
        assert_eq!(report.volumes.len(), 1, "these images compose one volume");
        report.volumes[0].start_bytes
    }
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
        let file = crate::device::MediumDevice::open(path, AccessIntent::Write).expect("opens");
        let mut qcow2 = crate::qcow2::Qcow2::open(file).expect("parses");
        let volume = fat16_volume_bytes();
        assert_eq!(volume.len() as u64, virtual_size);
        qcow2.write_at(0, &volume).expect("formats");
        qcow2.flush().expect("flushes");
        virtual_size
    }

    /// Exercises the whole public qcow2 path a caller runs: open,
    /// geometry, write, commit, reopen, read back.
    #[test]
    fn fat16_inside_qcow2_end_to_end() {
        let path =
            std::env::temp_dir().join(format!("remanence-qcow2-e2e-{}.qcow2", std::process::id()));
        let virtual_size = build_fat16_qcow2(&path);

        // Now the public path.
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("disk opens");
        assert!(matches!(disk.format(), DiskFormat::Qcow2 { version: 3 }));
        assert_eq!(disk.size(), virtual_size);

        let pool = pool_of(&mut disk);
        let report = disk.inspect(&pool).expect("inspection reads");
        assert_eq!(report.volumes.len(), 1);
        let composed = report.volumes[0].id;
        let volume = report.volumes[0].start_bytes;
        assert_eq!(
            report
                .filesystem_on(composed)
                .and_then(|fs| fs.label.as_ref())
                .and_then(|label| label.name.clone()),
            Some("REMANENCE".to_owned())
        );

        disk.make_directory(volume, "GUEST").expect("mkdir");
        disk.write_file(volume, "GUEST/PAYLOAD.BIN", b"through the mapping")
            .expect("write");
        assert_eq!(
            disk.stat(volume, "GUEST/PAYLOAD.BIN")
                .expect("stat")
                .map(|entry| entry.size_bytes),
            Some(b"through the mapping".len() as u64)
        );
        assert_eq!(disk.stat(volume, "GUEST/ABSENT.BIN").expect("stat"), None);
        disk.commit().expect("commit");
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened
                .read_file(volume, "GUEST/PAYLOAD.BIN")
                .expect("read"),
            b"through the mapping"
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    /// The block size the synthetic VDI images use — small enough that a
    /// modest test volume spans several blocks, which is what makes the
    /// allocated/free distinction visible.
    const VDI_BLOCK: u64 = 64 * 1024;

    /// A dynamically allocated VDI whose virtual disk holds `content`.
    /// Only the blocks the content actually fills are allocated; the rest
    /// stay free, which is both the shape a real dynamic image has and
    /// the one a later write must allocate into.
    fn dynamic_vdi_bytes(content: &[u8]) -> Vec<u8> {
        let disk_size = content.len() as u64;
        let block_count = disk_size.div_ceil(VDI_BLOCK) as u32;
        let map_at = 0x200usize;
        let data_at = (map_at + block_count as usize * 4).div_ceil(512) * 512;

        let mut image = vec![0u8; data_at];
        image[..37].copy_from_slice(b"<<< remanence synthetic VDI image >>>");
        image[0x40..0x44].copy_from_slice(&0xbeda_107fu32.to_le_bytes());
        image[0x44..0x48].copy_from_slice(&0x0001_0001u32.to_le_bytes()); // version 1.1
        image[0x48..0x4c].copy_from_slice(&0x190u32.to_le_bytes()); // header size
        image[0x4c..0x50].copy_from_slice(&1u32.to_le_bytes()); // dynamically allocated
        image[0x154..0x158].copy_from_slice(&(map_at as u32).to_le_bytes());
        image[0x158..0x15c].copy_from_slice(&(data_at as u32).to_le_bytes());
        image[0x170..0x178].copy_from_slice(&disk_size.to_le_bytes());
        image[0x178..0x17c].copy_from_slice(&(VDI_BLOCK as u32).to_le_bytes());
        image[0x180..0x184].copy_from_slice(&block_count.to_le_bytes());

        let mut allocated = 0u32;
        for block in 0..block_count as usize {
            let start = block * VDI_BLOCK as usize;
            let end = (start + VDI_BLOCK as usize).min(content.len());
            let slice = &content[start..end];
            let entry = if slice.iter().all(|&byte| byte == 0) {
                0xffff_ffffu32 // free: it reads as zeroes
            } else {
                let index = allocated;
                allocated += 1;
                let at = data_at + index as usize * VDI_BLOCK as usize;
                image.resize(at + VDI_BLOCK as usize, 0);
                image[at..at + slice.len()].copy_from_slice(slice);
                index
            };
            let at = map_at + block * 4;
            image[at..at + 4].copy_from_slice(&entry.to_le_bytes());
        }
        image[0x184..0x188].copy_from_slice(&allocated.to_le_bytes());
        image
    }

    /// Builds a VDI file at `path` whose virtual disk carries the
    /// synthetic FAT16 volume, and returns that virtual size.
    fn build_fat16_vdi(path: &std::path::Path) -> u64 {
        let volume = fat16_volume_bytes();
        std::fs::write(path, dynamic_vdi_bytes(&volume)).expect("vdi writes");
        volume.len() as u64
    }

    /// Exercises the whole public VDI path a caller runs: open, geometry,
    /// write into a block the image never allocated, commit, reopen, read
    /// back.
    #[test]
    fn fat16_inside_vdi_end_to_end() {
        let path =
            std::env::temp_dir().join(format!("remanence-vdi-e2e-{}.vdi", std::process::id()));
        let virtual_size = build_fat16_vdi(&path);
        let before = std::fs::metadata(&path).expect("metadata").len();

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("disk opens");
        assert_eq!(disk.format(), DiskFormat::Vdi { major: 1, minor: 1 });
        assert_eq!(disk.size(), virtual_size);
        assert!(
            disk.image_size_bytes() < virtual_size,
            "a dynamic image is smaller than the disk it presents"
        );

        let pool = pool_of(&mut disk);
        let report = disk.inspect(&pool).expect("inspection reads");
        assert_eq!(report.volumes.len(), 1);
        let composed = report.volumes[0].id;
        let volume = report.volumes[0].start_bytes;
        assert_eq!(
            report
                .filesystem_on(composed)
                .and_then(|fs| fs.label.as_ref())
                .and_then(|label| label.name.clone()),
            Some("REMANENCE".to_owned())
        );

        // The file data lands in the volume's data area, which the
        // builder left unallocated: this write allocates.
        disk.make_directory(volume, "GUEST").expect("mkdir");
        disk.write_file(volume, "GUEST/PAYLOAD.BIN", &new_content())
            .expect("write");
        assert_eq!(
            std::fs::metadata(&path).expect("metadata").len(),
            before,
            "nothing reaches the file before the commit"
        );
        disk.commit().expect("commit");
        assert!(
            std::fs::metadata(&path).expect("metadata").len() > before,
            "the commit allocated new blocks into the image"
        );
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened
                .read_file(volume, "GUEST/PAYLOAD.BIN")
                .expect("read"),
            new_content()
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
        let mut disk = MediaState::open(path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &old_content())
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
        let mut disk = MediaState::open(std::path::PathBuf::from(path), AccessIntent::Write)
            .expect("child opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &new_content())
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
        let mut disk = MediaState::open(path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &old_content())
            .expect("writes old state");
        disk.commit().expect("commits old state");
    }

    fn build_committed_vdi(path: &std::path::Path) {
        build_fat16_vdi(path);
        let mut disk = MediaState::open(path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &old_content())
            .expect("writes old state");
        disk.commit().expect("commits old state");
    }

    /// The identity a synthetic base VDI is stamped with, and the one the
    /// differencing image over it names as its parent.
    const VDI_BASE_ID: [u8; 16] = [
        0x51, 0x42, 0x33, 0x24, 0x15, 0x06, 0x47, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f,
        0x90,
    ];

    /// A differencing image over [`VDI_BASE_ID`] presenting `disk_size`
    /// bytes, with every block free: the whole disk is the parent's until
    /// something is written into it.
    fn differencing_vdi_bytes(disk_size: u64) -> Vec<u8> {
        let mut image = dynamic_vdi_bytes(&vec![0u8; disk_size as usize]);
        image[0x4c..0x50].copy_from_slice(&4u32.to_le_bytes()); // differencing
        image[0x188..0x198].copy_from_slice(&[0xa5; 16]); // its own identity
        image[0x1a8..0x1b8].copy_from_slice(&VDI_BASE_ID); // its parent's
        image
    }

    /// A committed base VDI and an empty differencing image over it, in
    /// their own directory. The base is named as a person names it, not
    /// after its identity, so the open has to search the directory for
    /// the file declaring the identity the child asked for.
    fn build_committed_vdi_chain(directory: &std::path::Path) -> (PathBuf, PathBuf) {
        std::fs::create_dir_all(directory).expect("chain directory");
        let base = directory.join("base.vdi");
        let top = directory.join("top.vdi");

        let volume_bytes = fat16_volume_bytes();
        let mut base_image = dynamic_vdi_bytes(&volume_bytes);
        base_image[0x188..0x198].copy_from_slice(&VDI_BASE_ID);
        std::fs::write(&base, base_image).expect("base writes");

        let mut base_disk = MediaState::open(&base, AccessIntent::Write).expect("base opens");
        let volume = only_extent(&mut base_disk);
        base_disk
            .write_file(volume, "OLD.BIN", &old_content())
            .expect("writes old state");
        base_disk.commit().expect("commits old state");
        drop(base_disk);

        std::fs::write(&top, differencing_vdi_bytes(volume_bytes.len() as u64))
            .expect("top writes");
        (top, base)
    }

    fn build_committed_chain(directory: &std::path::Path) -> (PathBuf, PathBuf) {
        std::fs::create_dir_all(directory).expect("chain directory");
        let base = directory.join("base.qcow2");
        let top = directory.join("top.qcow2");
        let virtual_size = build_fat16_qcow2(&base);
        let mut base_disk = MediaState::open(&base, AccessIntent::Write).expect("base opens");
        let volume = only_extent(&mut base_disk);
        base_disk
            .write_file(volume, "OLD.BIN", &old_content())
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
            for shape in ["raw", "qcow2", "vdi", "chain", "vdi-chain"] {
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
                    "vdi" => {
                        let path = std::env::temp_dir().join(format!("{stem}.vdi"));
                        build_committed_vdi(&path);
                        (path, None, None)
                    }
                    "chain" => {
                        let directory = std::env::temp_dir().join(stem);
                        let (path, backing) = build_committed_chain(&directory);
                        (path, Some(backing), Some(directory))
                    }
                    "vdi-chain" => {
                        let directory = std::env::temp_dir().join(stem);
                        let (path, backing) = build_committed_vdi_chain(&directory);
                        (path, Some(backing), Some(directory))
                    }
                    _ => unreachable!(),
                };
                let backing_before = backing
                    .as_ref()
                    .map(|path| std::fs::read(path).expect("backing reads"));

                run_crashing_commit(&path, boundary);

                let mut reopened = MediaState::open(&path, AccessIntent::Read)
                    .unwrap_or_else(|error| panic!("{shape}/{boundary} reopens: {error}"));
                let volume = only_extent(&mut reopened);
                let content = reopened
                    .read_file(volume, "OLD.BIN")
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
    /// [`MediaState::commit`] does, stopping at the durability boundary: the
    /// journal is armed, the file untouched. Returns the staged host
    /// writes so a test can apply any prefix of them before "crashing".
    fn stage_and_arm(disk: &mut MediaState) -> (Vec<(u64, Vec<u8>)>, u64) {
        let cache_bytes = disk.cache_bytes;
        disk.cache.join_offloads();
        disk.virtual_disk.host_mut().begin_capture(cache_bytes);
        disk.cache
            .write_through(disk.virtual_disk.device_mut())
            .expect("stages");
        let capture = disk.virtual_disk.host_mut().take_capture();
        let journal_path = disk
            .journal_path
            .clone()
            .expect("a test image is opened by a path this host names");
        crate::journal::record(&journal_path, disk.virtual_disk.host_mut(), &capture)
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
    fn streamed_file_verbs_round_trip_beside_the_whole_file_forms() {
        let path = temp_image("streamed-verbs");
        std::fs::write(&path, fat16_volume_bytes()).expect("image writes");
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);

        // Streamed replace: size the file, then write it in chunks.
        let contents = new_content();
        disk.resize_file(volume, "BIG.BIN", contents.len() as u64)
            .expect("sizes");
        for (n, chunk) in contents.chunks(10_000).enumerate() {
            disk.write_file_at(volume, "BIG.BIN", (n * 10_000) as u64, chunk)
                .expect("writes a chunk");
        }
        assert_eq!(
            disk.read_file(volume, "BIG.BIN").expect("whole read"),
            contents,
            "the streamed write equals a whole-file write"
        );

        // Streamed read: ranged reads reassemble the whole.
        let mut ranged = vec![0u8; contents.len()];
        for start in (0..contents.len()).step_by(7_777) {
            let end = (start + 7_777).min(contents.len());
            disk.read_file_at(volume, "BIG.BIN", start as u64, &mut ranged[start..end])
                .expect("reads a range");
        }
        assert_eq!(ranged, contents);

        // Shrink keeps the prefix; growth reads as zeros, never stale bytes.
        disk.resize_file(volume, "BIG.BIN", 100).expect("shrinks");
        disk.resize_file(volume, "BIG.BIN", 20_000)
            .expect("regrows");
        let back = disk.read_file(volume, "BIG.BIN").expect("reads");
        assert_eq!(&back[..100], &contents[..100], "the kept prefix survives");
        assert!(back[100..].iter().all(|&byte| byte == 0), "growth is zeros");

        // The bounds are refusals, not clamps.
        let mut probe = [0u8; 8];
        assert!(
            disk.read_file_at(volume, "BIG.BIN", 19_996, &mut probe)
                .is_err(),
            "a read past the size is refused"
        );
        assert!(
            disk.write_file_at(volume, "BIG.BIN", 19_996, &probe)
                .is_err(),
            "a write past the size is refused"
        );

        // Everything above survives the commit.
        disk.commit().expect("commits");
        drop(disk);
        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        let back = reopened.read_file(volume, "BIG.BIN").expect("reads");
        assert_eq!(back.len(), 20_000);
        assert_eq!(&back[..100], &contents[..100]);
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_tiny_declared_cache_bound_still_commits_correctly() {
        let path = temp_image("tiny-bound");
        build_committed_raw(&path);

        // A one-extent working set: reads, uncommitted writes, and the
        // commit's capture all evict and spill constantly (P27), and
        // the result is byte-identical to an unbounded run.
        let mut disk = MediaState::open_with_cache(&path, AccessIntent::Write, 1).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        assert!(disk.is_modified());
        disk.commit().expect("commits");
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            new_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_commit_retires_its_recovery_journal() {
        let path = temp_image("retires");
        build_committed_raw(&path);

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");

        let volume = only_extent(&mut disk);
        disk.write_file(volume, "NEW.BIN", &new_content())
            .expect("writes");
        disk.commit().expect("commits");
        assert!(
            !crate::journal::sidecar_path(&path).exists(),
            "a completed commit leaves no recovery sidecar behind"
        );
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened.read_file(volume, "NEW.BIN").expect("reads"),
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

        let mut reopened =
            MediaState::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!sidecar.exists(), "the torn sidecar is discarded");
        let volume = only_extent(&mut reopened);
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interruption_mid_apply_reconciles_to_the_old_image() {
        let path = temp_image("mid-apply");
        build_committed_raw(&path);

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");

        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        assert!(blocks.len() >= 2, "the staged write set spans blocks");

        // The crash: half the staged writes land, the rest never do,
        // and the process vanishes without retiring the journal.
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        let mut reopened =
            MediaState::open(&path, AccessIntent::Write).expect("reconciles and opens");
        assert!(!crate::journal::sidecar_path(&path).exists());
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            old_content(),
            "the image reconciles to wholly the old state"
        );

        // The reconciled disk is fully usable: the same overwrite
        // commits durably this time.
        reopened
            .write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        reopened.commit().expect("commits");
        drop(reopened);
        let mut committed = MediaState::open(&path, AccessIntent::Read).expect("opens");
        assert_eq!(
            committed.read_file(volume, "OLD.BIN").expect("reads"),
            new_content()
        );
        drop(committed);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interruption_before_retirement_reconciles_to_the_old_image() {
        let path = temp_image("unretired");
        build_committed_raw(&path);

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");

        let volume = only_extent(&mut disk);
        disk.write_file(volume, "NEW.BIN", &new_content())
            .expect("writes");
        let (blocks, new_len) = stage_and_arm(&mut disk);

        // The crash: the apply completes, but the journal is never
        // retired — the commit never returned, so the armed journal
        // governs and the next open rolls the image back.
        for &(offset, ref block) in &blocks {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        let mut reopened =
            MediaState::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!crate::journal::sidecar_path(&path).exists());
        assert_eq!(
            reopened.stat(volume, "NEW.BIN").expect("stats"),
            None,
            "the interrupted commit's file never existed"
        );
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
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
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &old_content())
            .expect("writes");
        disk.commit().expect("commits");
        assert!(!crate::journal::sidecar_path(&path).exists());
        drop(disk);

        // An interrupted commit: cluster allocations and metadata
        // updates land partially, then the process vanishes.
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");
        disk.write_file(volume, "NEW.BIN", &new_content())
            .expect("writes");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        assert!(blocks.len() >= 2, "the staged write set spans blocks");
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        // The next open reconciles to wholly the old image: metadata
        // consistent, the old file intact, the interrupted one absent.
        let mut reopened =
            MediaState::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!crate::journal::sidecar_path(&path).exists());
        let volume = only_extent(&mut reopened);
        assert_eq!(reopened.stat(volume, "NEW.BIN").expect("stats"), None);
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
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
        let mut disk = MediaState::open(&base, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &old_content())
            .expect("writes");
        disk.commit().expect("commits");
        drop(disk);

        // An interrupted commit on the base, left behind before it
        // became a backing file.
        let mut disk = MediaState::open(&base, AccessIntent::Write).expect("opens");
        disk.write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
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
        let mut chained =
            MediaState::open(&top, AccessIntent::Read).expect("reconciles and composes");
        assert!(!crate::journal::sidecar_path(&base).exists());
        assert_eq!(
            chained.read_file(volume, "OLD.BIN").expect("reads"),
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
