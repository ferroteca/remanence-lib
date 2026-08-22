// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The block medium's own state, and the two planes one P7 claim
//! serves (F43).
//!
//! [`MediaState`] is what a storage device homes while a space-native
//! medium occupies its slot: the **raw** plane — the image's own bytes,
//! streamed through the session cache and predictive reader — and the
//! **presented** plane, the disk a format adapter exposes above it.
//! They are different layers under P13, not duplicates.
//!
//! `Composed` is the presented plane as a [`Device`]: reads stream
//! through the cache and see buffered writes, and nothing reaches the
//! file until commit (P2). `Window` bounds a degraded session's reads
//! to the extent that is really there, checked once where the reads
//! are rather than at each verb that might cross it, and `assess` is
//! the narrow P28 gate that settles which of the two an open gets.
//!
//! Opening, loading and the plain facts of an opened medium live here.
//! What it *says about itself* is `super::inspect`-adjacent and lives
//! in this file too; the namespace verbs are in [`super::files`] and
//! the durable commit in [`super::commit`].

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::filesystem::Catalog;
use crate::filesystem::catalog::FilesystemAdapter;
use crate::filesystem::fat::{FatVolume, VolumeDeclaration};
use crate::image::adapters::{ActiveLayer, DeviceIdentity, ImageFormatDescriptor, OpenedImage};
use crate::io::cache::SessionCache;
#[cfg(test)]
use crate::io::device::AccessIntent;
use crate::io::device::{AccessMode, Claim, Device};
use crate::io::journal;
use crate::io::source::{ClaimedSource, ImageSource};
use crate::model::assurance::{self, Assurance, ReadBound, Shortfall};
use crate::model::device_type::DeviceType;
use crate::model::geometry::{self, Geometry, GeometrySources};
use crate::model::media::Format;
use crate::model::media_profile::MediaProfile;
use crate::model::session::{self, Identification, Layer};
use crate::partition::PartitionPool;
use crate::partition::mbr::{self, Discovery};

use super::DiskFormat;
use super::recognition::Recognition;

use crate::model::report::{
    DeclaredGeometry, DeviceInfo, DiskReport, FilesystemId, FilesystemInfo, PartitionSchemaInfo,
    RegionId, RegionInfo, VolumeId, VolumeInfo, VolumeOrigin,
};

/// A composed view: the session cache over the virtual disk (P27).
/// Reads stream through the cache and see buffered writes; altered
/// extents stay in the cache — in memory or spilled to private session
/// storage — and nothing reaches the file until commit (P2).
pub(super) struct Composed<'a> {
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
pub(super) struct Window<'a> {
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
pub(super) fn assess(
    image: &mut dyn OpenedImage,
    format: DiskFormat,
    mode: AccessMode,
    claim: Claim,
) -> Result<Assurance> {
    let observed = image.presented_size();
    // What the open observed travels with the medium whatever else the
    // gate settles (P4): a decoding driver's account is as much a fact
    // about this session as the shortfall gate's own findings.
    let opened = image.open_evidence();
    let with_evidence = |mut assurance: Assurance| {
        assurance.evidence.splice(0..0, opened.iter().cloned());
        assurance
    };
    if format != DiskFormat::Raw || observed < 512 {
        return Ok(with_evidence(Assurance::verified(observed, mode, claim)));
    }
    let mut sector = [0u8; 512];
    image.read_at(0, &mut sector)?;
    Ok(match crate::filesystem::fat::declared_volume(&sector) {
        VolumeDeclaration::Bounded {
            bytes,
            metadata_end,
            reading,
        } if bytes > observed => with_evidence(assurance::degraded(
            Shortfall {
                declared: bytes,
                observed,
                metadata_end,
            },
            &reading,
            claim,
        )),
        // A contradiction is only this gate's business where the source is
        // also short: with the declared bytes all present there is nothing
        // to bound, and what the boot record says about itself is the
        // filesystem seam's to report as an issue, exactly as it does today.
        VolumeDeclaration::Conflicted { bytes, detail } if bytes > observed => {
            return Err(assurance::conflicted(&detail, bytes, observed));
        }
        _ => with_evidence(Assurance::verified(observed, mode, claim)),
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
    pub(super) virtual_disk: Box<dyn OpenedImage>,
    /// The raw plane over the shared claim: the session cache and the
    /// predictive reader (P27, P34).
    pub(super) source: ImageSource,
    /// The archive wrappers unwrapped on the way in, if any.
    pub(super) layers: Vec<Layer>,
    /// The resolved image — the entry name for an archived image, else
    /// the name recovered from the source. Absent where the caller's
    /// handle has none.
    pub(super) image_path: Option<PathBuf>,
    pub(super) cache: SessionCache,
    /// The declared session cache bound (P27), governing the session
    /// cache and each commit's capture alike.
    pub(super) cache_bytes: u64,
    pub(super) format: DiskFormat,
    pub(super) descriptor: &'static ImageFormatDescriptor,
    /// The article this medium is (P14). The adapter named it when it
    /// loaded the state — an image format loads and saves media state,
    /// so it is what establishes which medium the state belongs to — and
    /// the medium carries it from there, immutably, for as long as the
    /// session holds it.
    pub(super) media: &'static MediaProfile,
    /// The device this medium's content is assumed recorded by: the
    /// load's own declaration, or the one type the recognizing format
    /// admits where a discovery reached it.
    ///
    /// Absent where an artifact was reached without a declaration and
    /// its format records several device types — the discovery journey,
    /// which answers what an artifact *is* without asserting which drive
    /// wrote it. The media pool refuses to admit such a medium (P3).
    pub(super) device: Option<DeviceType>,
    /// The addressable unit the load declared, where the format records
    /// none of its own — the raw reading's block size, and nothing else.
    /// It is a declaration about the reading being made rather than one
    /// laid onto an existing medium, and it enters the geometry as one
    /// source among the others.
    pub(super) declared_sector_bytes: Option<u64>,
    pub(super) device_identity: DeviceIdentity,
    pub(super) active_layer: ActiveLayer,
    /// The session's **effective** access (P28): the declared intent's
    /// echo, or read-only where the evidence never established write
    /// authority.
    pub(super) mode: AccessMode,
    /// What this open established about the evidence beneath it (P28),
    /// settled once at the open and never revisited: a session never
    /// regains authority it did not open with.
    pub(super) assurance: Assurance,
    /// The readable extent a degraded session reads under, carried beside
    /// the assurance because every composed read consults it.
    pub(super) bound: Option<ReadBound>,
    /// The artifact claimed, where a name for it exists — recovered from
    /// the caller's handle, or the path the library opened.
    pub(super) path: Option<String>,
    /// The recovery sidecar's derived path (P9) — private transient
    /// state, never a user-owned file. Absent where the source handle
    /// has no recoverable name, and the commit refuses by name there
    /// rather than journalling somewhere nobody named.
    pub(super) journal_path: Option<PathBuf>,
    /// Set when a commit failed partway and its in-process undo failed
    /// too: the session's caches no longer describe the file, so every
    /// verb refuses until a fresh open reconciles the image.
    pub(super) failed: Option<String>,
}

impl MediaState {
    /// Recognizes `path` and materializes it at the stated default cache
    /// bound.
    ///
    /// Test-only. A medium reaches a caller through
    /// [`crate::Session::load_media`] and nothing else, so
    /// this exists for the unit tests in this module, which exercise the
    /// device stack below the device tier.
    #[cfg(test)]
    pub(crate) fn open(path: impl AsRef<Path>, intent: AccessIntent) -> Result<Self> {
        Self::open_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// The same, under a stated bound. Test-only for the same reason.
    #[cfg(test)]
    pub(crate) fn open_with_cache(
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<Self> {
        Ok(Recognition::at_path(path.as_ref(), intent)?.into_state(cache_bytes))
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
        Ok(Recognition::over_handle(file, format)?.into_state(cache_bytes))
    }

    /// The same entry journey under the caller's own declaration — a
    /// `File` from another medium's namespace, loaded as the format the
    /// caller names rather than probed for one (F59's source shape).
    pub(crate) fn load_claimed(
        claimed: ClaimedSource,
        format: Format,
        cache_bytes: u64,
    ) -> Result<Self> {
        let recovery = claimed.image_path.as_deref().map(journal::sidecar_path);
        Ok(Recognition::establish(claimed, recovery, Some(format))?.into_state(cache_bytes))
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

    /// What the leading content is, where the device type declares no
    /// scheme for a table to be checked against: blank, one bare
    /// volume, or content nothing claims — and never a layout, which
    /// nobody declared.
    pub(crate) fn classify_content(&mut self) -> Result<Discovery> {
        self.require_usable()?;
        let mut composed = self.composed();
        mbr::classify(&mut composed)
    }

    /// Reads every source that states part of the recording's
    /// coordinates and settles what they agree on.
    ///
    /// The positions the boot records could be at are the partition
    /// pool's own — the pool was established first, so nothing here
    /// hunts for a volume — and the table's end tuples are read only
    /// where a scheme was actually read. A medium whose state is
    /// unusable states nothing rather than refusing: the load is not
    /// conditional on establishing a geometry.
    pub(crate) fn establish_geometry(&mut self, partitions: &PartitionPool) -> Geometry {
        if self.require_usable().is_err() {
            return Geometry::unstated();
        }
        let boot_records: Vec<(u32, u64)> = partitions
            .partitions()
            .iter()
            .filter(|partition| partition.is_addressable())
            .filter_map(|partition| Some((partition.ordinal(), partition.start_bytes()?)))
            .collect();
        let sources = GeometrySources {
            format_id: self.descriptor.id,
            // The artifact's own reading first: a format declaring one
            // geometry for every image it claims states it in the
            // descriptor, and one whose recording carries its own
            // states it here (F68).
            format_disk: self
                .virtual_disk
                .declared_geometry()
                .or(self.descriptor.disk),
            declared_sector_bytes: self.declared_sector_bytes,
            reads_table: partitions.scheme().is_some(),
            boot_records: &boot_records,
            extent_bytes: self.size(),
        };
        let mut composed = self.composed();
        geometry::establish(&mut composed, sources)
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
    pub(crate) fn recognize_fat(
        &mut self,
        offset: u64,
    ) -> Result<crate::filesystem::fat::FatRecognition> {
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

    /// The article this medium is (P14) — named by the image-format
    /// adapter that loaded its state.
    pub(crate) fn media(&self) -> &'static MediaProfile {
        self.media
    }

    /// The device this medium's content is assumed recorded by, where a
    /// declaration or a single-device format named one.
    pub(crate) fn device_type(&self) -> Option<DeviceType> {
        self.device
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
    /// adapter declares flux, which in-force P13 forbids. It is loaded
    /// by its own declaration — `Format::P64`, which answers with a
    /// flux medium — never as a block reading.
    pub(crate) fn foreign_family(&self) -> Option<&'static str> {
        let mut prefix = [0u8; 8];
        if self.read_at(0, &mut prefix).is_err() {
            return None;
        }
        crate::flux::p64::has_signature(&prefix).then_some("flux")
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
    pub(super) fn named(&self) -> String {
        crate::model::media::named(self.path())
    }

    /// Whether uncommitted changes exist.
    pub(crate) fn is_modified(&self) -> bool {
        self.cache.modified()
    }

    pub(super) fn composed(&mut self) -> Composed<'_> {
        Composed {
            base: self.virtual_disk.device_mut(),
            cache: &mut self.cache,
            bound: self.bound,
        }
    }

    /// The presented disk beneath the session cache — the plane a
    /// commit writes into, and so the medium's committed state.
    pub(crate) fn committed_device(&mut self) -> &mut dyn Device {
        self.virtual_disk.device_mut()
    }

    /// How many cached extents hold uncommitted writes.
    pub(crate) fn uncommitted_extents(&self) -> u64 {
        self.cache.dirty_extents()
    }

    pub(super) fn split_path(path: &str) -> Result<Vec<&str>> {
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
            article: self.media.id.to_owned(),
            device_type: self.device.map(|device| device.id().to_owned()),
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
                declared_active: partition.active(),
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
}

impl MediaState {
    pub(super) fn require_writable(&self) -> Result<()> {
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
                // Unreachable: an authored medium is not block state and
                // never reaches this seam. Spelled as an answer rather
                // than an unreachable, because a refusal is one (P6).
                Claim::Authored => format!(
                    "{} was created by its author and holds no artifact to \
                     write through",
                    self.named()
                ),
            }));
        }
        Ok(())
    }

    pub(super) fn require_usable(&self) -> Result<()> {
        match &self.failed {
            Some(reason) => Err(Error::io(reason.clone())),
            None => Ok(()),
        }
    }
}
