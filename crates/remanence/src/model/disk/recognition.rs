// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! What a load recognized, before it becomes state.
//!
//! A recognition holds the claim and the work already done — the
//! adapter that claimed the artifact, what the assurance gate settled,
//! and the identification — so that a discovery and the load that
//! consumes it never run the same work twice and no window opens
//! between the question and the load (P27: it builds no cache, the
//! bound being the load's own declaration).
//!
//! [`Recognition`] is the block-family form and [`MediumRecognition`]
//! the one over every family, each answering the same facts and each
//! turning into state through `into_state`. The two refusals at the
//! foot of the file are the shape a namespace-native medium takes when
//! it is asked for a space it does not have.

use std::path::{Path, PathBuf};

use crate::archive::{ArchiveMedium, ArchiveRecognition};
use crate::error::{Error, Result};
use crate::image::adapters::{self, DeviceIdentity, ImageFormatDescriptor, OpenedImage};
use crate::io::cache::SessionCache;
use crate::io::device::{AccessIntent, AccessMode};
use crate::io::journal;
use crate::io::source::{self, ClaimedSource, Evidence};
use crate::model::assurance::{Assurance, ReadBound};
use crate::model::device_type::{DeviceSlot, DeviceType};
use crate::model::media::Format;
use crate::model::media_profile::MediaProfile;
use crate::model::session::{self, Identification, Layer};

use super::state::{MediaState, assess};
use super::{DiskFormat, MediumState};

/// The device types the flux-family formats record — both record the
/// Commodore 1541 and nothing else in this release.
pub(super) static FLUX_RECORDED_DEVICES: [DeviceType; 1] = [DeviceType::Floppy(
    crate::model::device_type::FloppyDrive::Commodore1541,
)];

/// One artifact **recognized and nothing more**: the claim, the format
/// adapter that claimed it, the layers it was reached through, and what
/// the assurance gate settled — with no session cache, no commit buffer
/// and nothing spilled.
///
/// This is the state a discovery holds (F67). Recognition is the work
/// that answers *what is this?*: the claim is taken once, the adapter
/// reads the header evidence it names, and the gate runs. What it
/// deliberately does not do is build the medium — the cache bound is
/// the load's own declaration, and a verb that creates nothing has
/// nothing to bound.
///
/// [`Recognition::into_state`] is where the load turns it into a
/// [`MediaState`], over the very claim already held: nothing is
/// re-opened, no adapter runs twice, and no window opens between the
/// question and the load (P7 continuity).
#[derive(Debug)]
pub(crate) struct Recognition {
    source: ClaimedSource,
    virtual_disk: Box<dyn OpenedImage>,
    layers: Vec<Layer>,
    format: DiskFormat,
    descriptor: &'static ImageFormatDescriptor,
    device: Option<DeviceType>,
    declared_sector_bytes: Option<u64>,
    mode: AccessMode,
    assurance: Assurance,
    bound: Option<ReadBound>,
    /// The artifact claimed, which for an archived image is the archive
    /// rather than the entry recognized out of it.
    path: Option<String>,
    journal_path: Option<PathBuf>,
}

impl Recognition {
    /// Claims the artifact at `path` (P7) and recognizes it.
    ///
    /// An interrupted commit left by an earlier session is reconciled
    /// here, before anything reads the image (P9): the artifact comes
    /// back wholly the old state or wholly the committed new one, never
    /// a partial third state. The image format is whichever adapter
    /// recognizes it — qcow2 or VDI today — and raw where none does. A
    /// qcow2 naming a backing file, and a VDI naming a parent identity,
    /// compose their whole chain here, every file behind the top one
    /// claimed immutable for the session's life (U6).
    pub(crate) fn at_path(path: &Path, intent: AccessIntent) -> Result<Self> {
        let recovery = journal::sidecar_path(path);
        let mut claimed = source::claim_image(path, intent)?;
        // The sidecar check runs under our claim, so no live commit can
        // be mid-flight — a sidecar here is an interrupted one (P9).
        if recovery.exists() {
            match intent {
                AccessIntent::Write => {
                    let mut host = claimed.medium_device(path.display().to_string());
                    journal::reconcile(&recovery, &mut host, path)?;
                }
                AccessIntent::Read => {
                    // Reconciling writes; trade the read claim for a
                    // moment of exclusive access, then take it back.
                    drop(claimed);
                    journal::reconcile_at(path)?;
                    claimed = source::claim_image(path, intent)?;
                }
            }
        }
        Self::establish(claimed, Some(recovery), None)
    }

    /// Recognizes the caller's own opened file as the format they
    /// **declared**, under their claim (P7 as amended).
    ///
    /// A journal is derived where the handle has a recoverable name and
    /// is absent where it does not; an interrupted commit is reconciled
    /// here as it is on the path journey, and only where there is a name
    /// to find one by.
    pub(crate) fn over_handle(file: std::fs::File, format: Format) -> Result<Self> {
        let claimed = source::claim_handle(file)?;
        let recovery = claimed
            .source_path
            .as_deref()
            .map(journal::sidecar_path)
            .filter(|sidecar| sidecar.exists());
        if let (Some(sidecar), Some(path)) = (&recovery, claimed.source_path.clone()) {
            // Reconciling writes, and only a handle that affords writing
            // can perform it — a read-only claim is told rather than
            // silently left standing on an unreconciled image (P9, P7).
            if claimed.mode() != AccessMode::ReadWrite {
                return Err(Error::read_only(format!(
                    "'{}' carries an interrupted commit's recovery journal \
                     '{}', and reconciling it is a write: hand over a handle \
                     opened for writing so the image can be put back to \
                     wholly the old state or wholly the committed new one",
                    path.display(),
                    sidecar.display()
                )));
            }
            let mut host = claimed.medium_device(path.display().to_string());
            journal::reconcile(sidecar, &mut host, &path)?;
        }
        let sidecar = claimed.source_path.as_deref().map(journal::sidecar_path);
        Self::establish(claimed, sidecar, Some(format))
    }

    /// Recognizes one artifact reached through a namespace rather than
    /// named by path — the nested journey.
    ///
    /// An archive entry is read-only and never commits, so it has no
    /// journal to reconcile and no sidecar to find; everything above the
    /// backing is the same recognition a path takes, which is what makes
    /// a nested artifact the same journey rather than a second one.
    pub(crate) fn over_entry(claimed: ClaimedSource) -> Result<Self> {
        let recovery = claimed.image_path.as_deref().map(journal::sidecar_path);
        Self::establish(claimed, recovery, None)
    }

    /// The recognition every journey shares: the format adapter over the
    /// claimed backing — the one the caller declared, or whichever the
    /// catalog recognizes — the assurance gate before anything is
    /// exposed, and the layers the artifact was reached through.
    pub(super) fn establish(
        source: ClaimedSource,
        recovery: Option<PathBuf>,
        declared: Option<Format>,
    ) -> Result<Self> {
        let path = source.source_path.clone();
        let named = crate::model::media::named(
            path.as_deref()
                .map(|path| path.to_string_lossy())
                .as_deref(),
        );
        let mode = source.mode();

        // One claim, two planes: the adapter opens the presented disk over
        // a medium device sharing the very claim the raw plane reads (F43).
        let host = source.medium_device(named.clone());
        let image_path = source.image_path.as_deref();
        let (mut virtual_disk, descriptor) = match declared {
            Some(declared) => adapters::open_declared(declared, host, image_path, &named)?,
            None => adapters::image_catalog().open_disk(host, image_path)?,
        };
        // The declaration's own device type where a caller made one, and
        // the format's where it admits exactly one — a discovery over a
        // format that records several asserts nothing, and says so by
        // carrying none.
        let device = match declared {
            Some(declared) => declared.device_type(),
            None => match descriptor.devices {
                [only] => Some(*only),
                _ => None,
            },
        };
        let format = virtual_disk.format();

        // The assurance gate (P28), settled before the medium is exposed:
        // a caller who is going to be told a disk is degraded is told
        // before it reads a byte of it.
        let assurance = assess(virtual_disk.as_mut(), format, mode, source.claim_class)?;
        let mode = assurance.access;
        let bound = assurance.condition.map(|condition| ReadBound {
            end: assurance.first_unavailable_byte.unwrap_or(0),
            declared: assurance.declared_bytes.unwrap_or(0),
            condition,
        });

        let layers = source
            .archive_layers
            .iter()
            .cloned()
            .map(session::layer_from_archive)
            .collect();

        Ok(Self {
            source,
            virtual_disk,
            layers,
            format,
            descriptor,
            device,
            declared_sector_bytes: declared.and_then(Format::block_bytes),
            mode,
            assurance,
            bound,
            path: path.map(|path| path.display().to_string()),
            journal_path: recovery,
        })
    }

    /// The medium this recognition becomes, under the bound the load
    /// declared (P27): the session cache the reads stream through and
    /// the commit buffer the writes land in, over the claim already
    /// held.
    pub(crate) fn into_state(self, cache_bytes: u64) -> MediaState {
        let image_path = self.source.image_path.clone();
        MediaState {
            virtual_disk: self.virtual_disk,
            source: self.source.resolve(cache_bytes),
            layers: self.layers,
            image_path,
            cache: SessionCache::with_bytes_offloading(cache_bytes),
            cache_bytes,
            format: self.format,
            descriptor: self.descriptor,
            // The article is the **declared device's**, not the
            // format's: a raw image is bytes, and what article those
            // bytes were recorded on is the device's own declaration.
            // Where nothing declares a device the format's own article
            // stands, which is every archive grammar. For every existing
            // pairing the two agree — each claimed hard drive declares
            // the same logical-block article the block formats do — so
            // this changes only what a newly declarable device brings.
            media: self
                .device
                .map_or(self.descriptor.media, |device| device.article_profile()),
            device: self.device,
            declared_sector_bytes: self.declared_sector_bytes,
            device_identity: DeviceIdentity::first(),
            active_layer: self.descriptor.initial_active_layer,
            mode: self.mode,
            assurance: self.assurance,
            bound: self.bound,
            path: self.path,
            journal_path: self.journal_path,
            failed: None,
        }
    }

    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The resolved artifact — the entry name for an image recognized
    /// inside an archive, else the source's own name.
    pub(crate) fn image_path(&self) -> Option<&Path> {
        self.source.image_path.as_deref()
    }

    /// The artifact's own bytes — the raw plane, distinct from
    /// [`Recognition::size`], which is the disk the format presents.
    pub(crate) fn image_size_bytes(&self) -> u64 {
        self.source.len()
    }

    /// The virtual disk size (the guest-visible size for qcow2).
    pub(crate) fn size(&self) -> u64 {
        self.virtual_disk.presented_size()
    }

    pub(crate) fn format(&self) -> DiskFormat {
        self.format
    }

    pub(crate) fn descriptor(&self) -> &'static ImageFormatDescriptor {
        self.descriptor
    }

    pub(crate) fn media(&self) -> &'static MediaProfile {
        self.descriptor.media
    }

    pub(crate) fn device_type(&self) -> Option<DeviceType> {
        self.device
    }

    pub(crate) fn mode(&self) -> AccessMode {
        self.mode
    }

    pub(crate) fn assurance(&self) -> &Assurance {
        &self.assurance
    }

    /// Identifies the artifact's nesting layers and probable filesystem
    /// over the raw plane, probing bounded evidence alone (P27) — the
    /// same reading the medium gives once a load has built one.
    pub(crate) fn identify(&self) -> Identification {
        session::identify_medium(
            &self.source,
            self.source.image_path.as_deref(),
            &self.layers,
            DeviceIdentity::first(),
            false,
        )
    }

    /// The family of an artifact this release recognizes as belonging to
    /// another one — flux, today. See [`MediaState::foreign_family`],
    /// which asks the same question of a loaded medium.
    pub(crate) fn foreign_family(&self) -> Option<&'static str> {
        let mut prefix = [0u8; 8];
        if Evidence::read_at(&self.source, 0, &mut prefix).is_err() {
            return None;
        }
        crate::flux::p64::has_signature(&prefix).then_some("flux")
    }
}

/// One artifact recognized as whichever medium it is — the answer to
/// *what is this?*, holding the claim that established it and no medium
/// at all.
///
/// **The grammar that recognizes an artifact is what settles this**
/// (P12): an enrolled archive grammar claiming the artifact makes it an
/// archive, and everything else goes to the image catalog. Discovery
/// stops here; a load takes it the rest of the way through
/// [`MediumRecognition::into_state`], which is the only place a bound
/// is declared, so the two never disagree about what an artifact is and
/// neither runs the other's work.
#[derive(Debug)]
pub(crate) enum MediumRecognition {
    Space(Recognition),
    Archive(ArchiveRecognition),
}

impl MediumRecognition {
    /// Claims the artifact at `path` (P7) and recognizes it.
    pub(crate) fn at_path(path: &Path, intent: AccessIntent) -> Result<Self> {
        if crate::archive::is_archive(path) {
            return Ok(Self::Archive(ArchiveRecognition::open(path, intent)?));
        }
        Ok(Self::Space(Recognition::at_path(path, intent)?))
    }

    /// Recognizes one artifact reached through a namespace — the nested
    /// journey, under the claim the medium that named it already holds.
    pub(crate) fn over_entry(claimed: ClaimedSource) -> Result<Self> {
        Ok(Self::Space(Recognition::over_entry(claimed)?))
    }

    /// The medium this recognition becomes, under the bound the load
    /// declared (P27).
    pub(crate) fn into_state(self, cache_bytes: u64) -> MediumState {
        match self {
            Self::Space(space) => MediumState::Space(space.into_state(cache_bytes)),
            Self::Archive(archive) => MediumState::Archive(archive.into_medium(cache_bytes)),
        }
    }

    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Self::Space(space) => space.path(),
            Self::Archive(archive) => archive.path(),
        }
    }

    /// The artifact as a refusal names it.
    pub(crate) fn named(&self) -> String {
        match self {
            Self::Space(space) => crate::model::media::named(space.path()),
            Self::Archive(archive) => archive.named(),
        }
    }

    pub(crate) fn image_path(&self) -> Option<&Path> {
        match self {
            Self::Space(space) => space.image_path(),
            Self::Archive(archive) => archive.path().map(Path::new),
        }
    }

    pub(crate) fn image_size_bytes(&self) -> u64 {
        match self {
            Self::Space(space) => space.image_size_bytes(),
            Self::Archive(archive) => archive.size_bytes(),
        }
    }

    /// The presented disk's size, or the refusal naming a medium that
    /// presents no disk.
    pub(crate) fn size(&self, verb: &str) -> Result<u64> {
        match self {
            Self::Space(space) => Ok(space.size()),
            Self::Archive(archive) => Err(no_archive_space(verb, archive.named())),
        }
    }

    /// The image container format, or the refusal naming a medium that
    /// is no disk image.
    pub(crate) fn format(&self, verb: &str) -> Result<DiskFormat> {
        match self {
            Self::Space(space) => Ok(space.format()),
            Self::Archive(archive) => Err(no_archive_space(verb, archive.named())),
        }
    }

    pub(crate) fn format_id(&self) -> &'static str {
        match self {
            Self::Space(space) => space.descriptor().id,
            Self::Archive(archive) => archive.format_id(),
        }
    }

    pub(crate) fn format_name(&self) -> &'static str {
        match self {
            Self::Space(space) => space.descriptor().name,
            Self::Archive(archive) => archive.format_name(),
        }
    }

    pub(crate) fn media(&self) -> &'static MediaProfile {
        match self {
            Self::Space(space) => space.media(),
            Self::Archive(archive) => archive.media(),
        }
    }

    /// The device this artifact's content was recorded by, where the
    /// recognizing format admits exactly one — `None` for an archive,
    /// which no device recorded, and for a format that records several
    /// and says nothing about which.
    pub(crate) fn device_type(&self) -> Option<DeviceType> {
        match self {
            Self::Space(space) => space.device_type(),
            Self::Archive(_) => None,
        }
    }

    pub(crate) fn recorded_devices(&self) -> &'static [DeviceType] {
        match self {
            Self::Space(space) => space.descriptor().devices,
            Self::Archive(_) => &[],
        }
    }

    /// What slot a load of this artifact would go into — `None` where
    /// the format records several types and the load must declare which.
    pub(crate) fn slot(&self) -> Option<DeviceSlot> {
        match self {
            Self::Space(space) => space.device_type().map(DeviceSlot::Recorded),
            Self::Archive(_) => Some(DeviceSlot::Archive),
        }
    }

    /// Takes the caller's declaration of what recorded this artifact,
    /// for a format that records several and asserted none. An archive
    /// ignores it, having been recorded by nothing.
    pub(crate) fn declare_device(&mut self, device: DeviceType) {
        if let Self::Space(space) = self {
            space.device = Some(device);
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

    /// The family of an artifact this release recognizes and holds in no
    /// device — flux, today.
    pub(crate) fn foreign_family(&self) -> Option<&'static str> {
        match self {
            Self::Space(space) => space.foreign_family(),
            Self::Archive(_) => None,
        }
    }
}

/// The refusal a space verb makes on a namespace-native medium, spelled
/// where the medium behind it may be a recognition rather than a load.
pub(super) fn no_archive_space(verb: &str, named: String) -> Error {
    Error::unsupported(format!(
        "'{verb}' addresses a space and {named} holds an archive medium, whose \
         vantage is a namespace: an archive records no scheme, has no volume \
         and no sector to address, and its content is reached through the \
         namespace door of the direct partition it bears"
    ))
}

/// The refusal a space verb makes on a namespace-native medium.
pub(super) fn no_space(verb: &str, archive: &ArchiveMedium) -> Error {
    no_archive_space(verb, archive.named())
}
