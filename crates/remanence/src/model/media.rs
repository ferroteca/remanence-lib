// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The media pool and the medium a caller holds.
//!
//! **The medium is the content handle.** A [`Session`](crate::Session)
//! owns two pools — machines, which are configuration, and media, which
//! are state — and everything the recording can answer answers here:
//! what the artifact is, what claim it holds, what the open established,
//! its layers, its spaces and its files, and the commit point beneath
//! them all. A device is the slot it may be linked to, not the thing it
//! is.
//!
//! **A medium is created by a declared reading.**
//! [`Session::load_media`](crate::Session::load_media) takes the
//! caller's own opened [`std::fs::File`] and one concrete [`Format`],
//! checked by that format's own adapter and refused by name where the
//! evidence cannot bear it. A classification could check nothing and is
//! not a declaration; a caller who does not yet know what an artifact is
//! asks [`discover_media`](crate::discover_media), which answers the
//! other question.
//!
//! **The pool outlives every machine.** A medium is loaded unlinked, may
//! be inserted into a device and ejected again — ejecting severs and
//! destroys nothing, so the claim and everything buffered survive — and
//! is destroyed only by
//! [`Session::release_media`](crate::Session::release_media) or by the
//! session going away. That is what lets a disk mastered out of an
//! archive outlive the archive it came from.

use std::fmt;
use std::path::Path;

use crate::archive::ArchiveMedium;
use crate::error::{Error, Result};
use crate::filesystem::Catalog;
use crate::filesystem::catalog::FilesystemAdapter;
use crate::filesystem::fat::FatEntry;

use crate::flux::presentation::{Bitstream, Bytestream};
use crate::image::adapters::{RAW_RECORDED_DEVICES, RECORDED_HARD_DRIVES};
use crate::io::device::AccessMode;
use crate::io::source::FileSource;
use crate::model::assurance::Assurance;
use crate::model::authored::NewMedia;
use crate::model::device_type::{Addressing, DeviceSlot, DeviceType, FloppyDrive, HardDrive};
use crate::model::discovery::Discovery;
use crate::model::disk::{DiskFormat, MediumRecognition, MediumState};
use crate::model::geometry::{self, Geometry, GeometryRule, RecordingGeometry};
use crate::model::report::DiskReport;
use crate::model::session::Identification;
use crate::model::storage_device::AttachmentId;
use crate::partition::{Partition, PartitionPool, PartitionScheme, PartitionView};

/// How a refusal names an artifact whose handle may have no name.
pub(crate) fn named(path: Option<&str>) -> String {
    match path {
        Some(path) => format!("'{path}'"),
        None => "this medium (its source handle has no recoverable name)".to_owned(),
    }
}

/// One concrete artifact format, declared at the load, carrying the
/// device its content was recorded by.
///
/// **A declaration names a concrete catalog entry, never a
/// classification** (P3): "an archive" or "some floppy image" could
/// check nothing, so the set below is exactly the formats this release
/// reads, each checked by its own adapter. A format this release does
/// not claim fails to compile rather than being spelled and refused at
/// run time.
///
/// **A format that admits one device type carries it bare; one that
/// records many declares it, the field typed by the class its adapter
/// records.** [`Format::H8d`] is a Heathkit H-17 recording and says so
/// by having nothing to say; a qcow2 records some hard drive and the
/// caller states which, so a flux capture of a hard drive fails to
/// compile and a pairing no adapter declares within the class — a
/// `gpt-hd` today — is a named refusal at the load
/// ([`Format::declared`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Bytes, and nothing else — the geometry-opaque reading. It records
    /// no ecosystem and declares no structure, so nothing about the
    /// artifact can contradict it; an artifact belonging to a family no
    /// device holds is still refused, because a raw reading of it would
    /// declare the block layer authoritative where its own adapter
    /// declares otherwise (P13).
    ///
    /// The block size is the caller's too: a raw image records no
    /// addressable unit, and bytes have no block meaning without one.
    Raw {
        device: DeviceType,
        block_bytes: u64,
    },
    /// QEMU copy-on-write, versions 2 and 3, backing chains composed.
    Qcow2 { device: HardDrive },
    /// VirtualBox disk image, differencing chains composed.
    Vdi { device: HardDrive },
    /// Heathkit H8/H17 disk image — a Heathkit H-17 recording, and the
    /// format admits no other.
    H8d,
    /// ImageDisk sector image. The recording states its own sector
    /// identities, and the adapter presents them in that order (D60), so
    /// what reads is the disk as its own numbering has it rather than as
    /// the artifact happens to store it. More than one drive records the
    /// format, so the load declares which.
    Imd { device: FloppyDrive },
    /// ZIP, whose content is a namespace. An archive was recorded by no
    /// device, so there is nothing to declare.
    Zip,
    /// 7z, whose content is a namespace.
    SevenZip,
    /// KryoFlux capture set — the collection-sourced format: one disk
    /// spread over a stream per head per drive-step position, read as a
    /// declared collection of the caller's opened files or of files
    /// from another medium's namespace. The field is typed by the class
    /// the adapter records, so a flux capture of a hard drive fails to
    /// compile; the pairing within the class is the catalog's check.
    ///
    /// The member grammar, the set's completeness, every stream's own
    /// grammar and the declared device's profile claim are checked
    /// whole, then the reduction runs under the profile's declared
    /// materialization defaults — and the result is a medium of the
    /// declared family with the verdicts, the policy and the
    /// declared-loss account as provenance (P29, P30).
    KryoFlux { device: FloppyDrive },
    /// MAME floppy image — cell transitions around a revolution, which
    /// is a flux artifact rather than a sector one. The container states
    /// no rate, encoding or drive, so the load declares which family
    /// recorded it and the artifact's geometry is checked against that
    /// family rather than overriding it.
    Mfi { device: FloppyDrive },
    /// P64 flux container — a Commodore 1541 recording, and the format
    /// admits no other. A P64 already holds a flux medium at rest, so
    /// the load takes the served form straight in (D31).
    P64,
}

/// What one declarable format admits: its identity, the device types its
/// adapter records, and whether its declaration carries a block size.
///
/// It is the enumerated claim read the other way round — the same
/// declarations the [`Format`] variants above spell, in a shape the text
/// boundaries (C, Python) can enumerate and refuse against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatClaim {
    id: &'static str,
    name: &'static str,
    devices: &'static [DeviceType],
    block_bytes: bool,
    /// Whether this format reads a collection of sources rather than
    /// one artifact — a capture set is one disk spread over many
    /// streams, and nothing else is.
    collection: bool,
}

impl FormatClaim {
    /// The stable cross-language spelling.
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// The format's name, fit to show a user.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Every device type this format's adapter records. Empty for an
    /// archive grammar, which records no device at all.
    pub const fn devices(&self) -> &'static [DeviceType] {
        self.devices
    }

    /// Whether a declaration of this format carries the block size —
    /// true for the raw reading alone, which records no addressable
    /// unit of its own.
    pub const fn takes_block_bytes(&self) -> bool {
        self.block_bytes
    }

    /// Which source shape this format reads: `true` for a collection
    /// of sources — a KryoFlux capture set is one disk spread over a
    /// stream per head per step position — and `false` for one
    /// artifact.
    pub const fn takes_collection(&self) -> bool {
        self.collection
    }

    /// The device type this format declares where it admits exactly one,
    /// and `None` where the caller states it.
    pub fn declared_device(&self) -> Option<DeviceType> {
        match self.devices {
            [only] => Some(*only),
            _ => None,
        }
    }
}

/// Every format a load may declare, and what each admits. The catalog is
/// the claim: a pairing absent from it is refused by name.
static CLAIMED: [FormatClaim; 10] = [
    FormatClaim {
        id: "raw",
        name: "Raw disk image",
        devices: &RAW_RECORDED_DEVICES,
        block_bytes: true,
        collection: false,
    },
    FormatClaim {
        id: "qcow2",
        name: "QEMU copy-on-write disk image",
        devices: &RECORDED_HARD_DRIVES,
        block_bytes: false,
        collection: false,
    },
    FormatClaim {
        id: "vdi",
        name: "VirtualBox disk image",
        devices: &RECORDED_HARD_DRIVES,
        block_bytes: false,
        collection: false,
    },
    FormatClaim {
        id: "h8d",
        name: "Heathkit H8 H17 disk image",
        devices: &[DeviceType::Floppy(FloppyDrive::HeathH17)],
        block_bytes: false,
        collection: false,
    },
    FormatClaim {
        id: "imd",
        name: "ImageDisk sector image",
        devices: &crate::image::adapters::IMD_RECORDED_DEVICES,
        block_bytes: false,
        collection: false,
    },
    FormatClaim {
        id: "zip",
        name: "ZIP archive",
        devices: &[],
        block_bytes: false,
        collection: false,
    },
    FormatClaim {
        id: "7z",
        name: "7z archive",
        devices: &[],
        block_bytes: false,
        collection: false,
    },
    FormatClaim {
        id: "kryoflux",
        name: "KryoFlux capture set",
        devices: &[DeviceType::Floppy(FloppyDrive::Commodore1541)],
        block_bytes: false,
        collection: true,
    },
    FormatClaim {
        id: "mfi",
        name: "MAME floppy image",
        devices: &crate::image::adapters::MFI_RECORDED_DEVICES,
        block_bytes: false,
        collection: false,
    },
    FormatClaim {
        id: "p64",
        name: "P64 flux image",
        devices: &[DeviceType::Floppy(FloppyDrive::Commodore1541)],
        block_bytes: false,
        collection: false,
    },
];

impl Format {
    /// Every format a load may declare, with what each admits.
    pub fn claimed() -> &'static [FormatClaim] {
        &CLAIMED
    }

    /// The stable cross-language spelling, which is what the C and
    /// Python surfaces carry and what a refusal quotes back.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Raw { .. } => "raw",
            Self::Qcow2 { .. } => "qcow2",
            Self::Vdi { .. } => "vdi",
            Self::H8d => "h8d",
            Self::Imd { .. } => "imd",
            Self::Mfi { .. } => "mfi",
            Self::Zip => "zip",
            Self::SevenZip => "7z",
            Self::KryoFlux { .. } => "kryoflux",
            Self::P64 => "p64",
        }
    }

    /// The format's name, fit to show a user.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw { .. } => "Raw disk image",
            Self::Qcow2 { .. } => "QEMU copy-on-write disk image",
            Self::Vdi { .. } => "VirtualBox disk image",
            Self::H8d => "Heathkit H8 H17 disk image",
            Self::Imd { .. } => "ImageDisk sector image",
            Self::Mfi { .. } => "MAME floppy image",
            Self::Zip => "ZIP archive",
            Self::SevenZip => "7z archive",
            Self::KryoFlux { .. } => "KryoFlux capture set",
            Self::P64 => "P64 flux image",
        }
    }

    /// What this format admits — its own entry in the catalog above.
    pub fn claim(self) -> &'static FormatClaim {
        CLAIMED
            .iter()
            .find(|claim| claim.id == self.id())
            .expect("every variant has a catalog entry")
    }

    /// The device type this declaration names, or `None` for an archive
    /// grammar — an archive was recorded by no device.
    pub fn device_type(self) -> Option<DeviceType> {
        match self {
            Self::Raw { device, .. } => Some(device),
            Self::Qcow2 { device } | Self::Vdi { device } => Some(DeviceType::HardDrive(device)),
            Self::H8d => Some(DeviceType::Floppy(FloppyDrive::HeathH17)),
            Self::Imd { device } => Some(DeviceType::Floppy(device)),
            Self::Mfi { device } => Some(DeviceType::Floppy(device)),
            Self::KryoFlux { device } => Some(DeviceType::Floppy(device)),
            Self::P64 => Some(DeviceType::Floppy(FloppyDrive::Commodore1541)),
            Self::Zip | Self::SevenZip => None,
        }
    }

    /// The block size this declaration carries, where the format takes
    /// one.
    pub fn block_bytes(self) -> Option<u64> {
        match self {
            Self::Raw { block_bytes, .. } => Some(block_bytes),
            _ => None,
        }
    }

    /// Checks that the adapter this declaration names records the device
    /// type it names (P3), refusing the pairing by name where it does
    /// not.
    ///
    /// The class is checked by the compiler — a flux capture of a hard
    /// drive cannot be spelled — and this is the check within the class,
    /// where the catalog is narrower than the class is.
    pub(crate) fn check_pairing(self) -> Result<()> {
        let claim = self.claim();
        let Some(device) = self.device_type() else {
            return Ok(());
        };
        if claim.devices.contains(&device) {
            return Ok(());
        }
        let recorded: Vec<&str> = claim.devices.iter().map(|device| device.id()).collect();
        Err(Error::unsupported(format!(
            "no adapter in this release records a {} ({}) as a {}; the \
             {} records {}",
            device.name(),
            device.id(),
            claim.name,
            claim.id,
            match recorded.len() {
                0 => "no device at all".to_owned(),
                _ => recorded.join(" and "),
            }
        )))
    }

    /// Builds a declaration from the stable spellings, for the C and
    /// Python surfaces where one arrives as text (P5).
    ///
    /// Each half is refused by name on its own terms: a format spelling
    /// this release does not claim, a device type it does not claim, a
    /// device of the wrong class for the format, a missing device where
    /// the format needs one, and a block size where the format records
    /// its own.
    pub fn declared(
        format: &str,
        device: Option<DeviceType>,
        block_bytes: Option<u64>,
    ) -> Result<Self> {
        let claim = CLAIMED
            .iter()
            .find(|claim| claim.id == format)
            .ok_or_else(|| {
                let claimed: Vec<&str> = CLAIMED.iter().map(|claim| claim.id).collect();
                Error::unsupported(format!(
                    "'{format}' names no format this release loads; the \
                 declarations it claims are {}",
                    claimed.join(", ")
                ))
            })?;

        // A format admitting exactly one device type carries it bare, so
        // stating it is allowed and stating a different one is refused —
        // the same rule the pairing check makes, at the boundary where
        // the declaration is text rather than a variant.
        let device = match (device, claim.declared_device()) {
            (None, Some(bare)) => Some(bare),
            (given, _) => given,
        };
        // A raw reading records no ecosystem, so it takes any device the
        // catalog says it records — the class check the container formats
        // need does not apply, and the pairing check below is what
        // narrows it to the recorded set.
        let any_device = |device: Option<DeviceType>| -> Result<DeviceType> {
            device.ok_or_else(|| {
                Error::unsupported(format!(
                    "the {} records more than one device type, so a load \
                     declares which: it records {}",
                    claim.name,
                    claim
                        .devices
                        .iter()
                        .map(|device| device.id())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
        };
        let hard_drive = |device: Option<DeviceType>| -> Result<HardDrive> {
            match device {
                Some(DeviceType::HardDrive(drive)) => Ok(drive),
                Some(other) => Err(Error::unsupported(format!(
                    "the {} records a hard drive, and '{}' is a {} device",
                    claim.name,
                    other.id(),
                    other.class()
                ))),
                None => Err(Error::unsupported(format!(
                    "the {} records more than one device type, so a load \
                     declares which: it records {}",
                    claim.name,
                    claim
                        .devices
                        .iter()
                        .map(|device| device.id())
                        .collect::<Vec<_>>()
                        .join(" and ")
                ))),
            }
        };
        let floppy = |device: Option<DeviceType>| -> Result<FloppyDrive> {
            match device {
                Some(DeviceType::Floppy(drive)) => Ok(drive),
                Some(other) => Err(Error::unsupported(format!(
                    "the {} records a floppy drive, and '{}' is a {} device",
                    claim.name,
                    other.id(),
                    other.class()
                ))),
                None => Err(Error::unsupported(format!(
                    "the {} records more than one device type, so a load \
                     declares which: it records {}",
                    claim.name,
                    claim
                        .devices
                        .iter()
                        .map(|device| device.id())
                        .collect::<Vec<_>>()
                        .join(" and ")
                ))),
            }
        };

        if device.is_some() && claim.devices.is_empty() {
            return Err(Error::unsupported(format!(
                "the {} records no device at all — an archive was written by \
                 none — so a declaration naming one names a recording that \
                 never happened",
                claim.name
            )));
        }

        let declaration = match claim.id {
            "raw" => Self::Raw {
                device: any_device(device)?,
                block_bytes: block_bytes.ok_or_else(|| {
                    Error::unsupported(
                        "a raw image records no addressable unit, so its \
                         declaration carries the block size"
                            .to_owned(),
                    )
                })?,
            },
            "qcow2" => Self::Qcow2 {
                device: hard_drive(device)?,
            },
            "vdi" => Self::Vdi {
                device: hard_drive(device)?,
            },
            "h8d" => Self::H8d,
            "imd" => Self::Imd {
                device: floppy(device)?,
            },
            "mfi" => Self::Mfi {
                device: floppy(device)?,
            },
            "zip" => Self::Zip,
            "7z" => Self::SevenZip,
            "kryoflux" => Self::KryoFlux {
                device: floppy(device)?,
            },
            "p64" => Self::P64,
            other => unreachable!("'{other}' is not a claimed format id"),
        };

        if block_bytes.is_some() && !claim.block_bytes {
            return Err(Error::unsupported(format!(
                "the {} records its own addressable unit, so a declared \
                 block size would be a second answer about one disk",
                claim.name
            )));
        }
        if let (Some(stated), Some(bare)) = (device, claim.declared_device()) {
            if stated != bare {
                return Err(Error::unsupported(format!(
                    "the {} records a {} ({}) and nothing else, so a \
                     declaration naming '{}' names a recording it never made",
                    claim.name,
                    bare.name(),
                    bare.id(),
                    stated.id()
                )));
            }
        }
        declaration.check_pairing()?;
        Ok(declaration)
    }

    /// The archive grammar this format is, where it is one.
    ///
    /// An archive's native vantage is a namespace rather than a space
    /// (P14 as amended), so the two kinds of medium are built by
    /// different adapters and this is the fork between them.
    pub(crate) fn archive_grammar(self) -> Option<&'static str> {
        match self {
            Self::Zip | Self::SevenZip => Some(self.id()),
            Self::Raw { .. }
            | Self::Qcow2 { .. }
            | Self::Vdi { .. }
            | Self::H8d
            | Self::Imd { .. }
            | Self::KryoFlux { .. }
            | Self::Mfi { .. }
            | Self::P64 => None,
        }
    }

    /// The flux family's fork: what a flux-family declaration loads —
    /// `None` for every format whose medium is a block device or an
    /// archive (P13).
    pub(crate) fn flux_family(self) -> Option<FluxFormat> {
        match self {
            Self::KryoFlux { device } => Some(FluxFormat::KryoFlux { device }),
            Self::Mfi { device } => Some(FluxFormat::Mfi { device }),
            Self::P64 => Some(FluxFormat::P64),
            Self::Raw { .. }
            | Self::Qcow2 { .. }
            | Self::Vdi { .. }
            | Self::H8d
            | Self::Imd { .. }
            | Self::Zip
            | Self::SevenZip => None,
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// A flux-family declaration, forked out of [`Format`] for the load
/// that builds the medium.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FluxFormat {
    KryoFlux { device: FloppyDrive },
    Mfi { device: FloppyDrive },
    P64,
}

/// What a load reads — the source shapes of
/// [`Session::load_media`](crate::Session::load_media).
///
/// Four shapes, each arriving by plain conversion so a caller passes
/// the thing itself: one opened [`std::fs::File`] (the portable file,
/// the caller's own open and the caller's own lock — P7 as amended), a
/// collection of them, one [`FileSource`] taken from another medium's
/// namespace, or a collection of those. **A format declares which
/// shape it reads**, as it declares everything else: a KryoFlux
/// capture set is a collection, and every other claimed format is one
/// artifact — a shape the format does not read is refused by name at
/// the load.
#[derive(Debug)]
pub struct MediaSource(pub(crate) SourceShape);

#[derive(Debug)]
pub(crate) enum SourceShape {
    Handle(std::fs::File),
    Handles(Vec<std::fs::File>),
    Entry(FileSource),
    Entries(Vec<FileSource>),
}

impl SourceShape {
    /// How a refusal names the shape that arrived.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Handle(_) => "one opened file".to_owned(),
            Self::Handles(handles) => {
                format!("a collection of {} opened files", handles.len())
            }
            Self::Entry(entry) => {
                format!("the file '{}' from a medium's namespace", entry.name())
            }
            Self::Entries(entries) => format!(
                "a collection of {} files from a medium's namespace",
                entries.len()
            ),
        }
    }
}

impl From<std::fs::File> for MediaSource {
    fn from(file: std::fs::File) -> Self {
        Self(SourceShape::Handle(file))
    }
}

impl From<Vec<std::fs::File>> for MediaSource {
    fn from(files: Vec<std::fs::File>) -> Self {
        Self(SourceShape::Handles(files))
    }
}

impl From<FileSource> for MediaSource {
    fn from(file: FileSource) -> Self {
        Self(SourceShape::Entry(file))
    }
}

impl From<Vec<FileSource>> for MediaSource {
    fn from(files: Vec<FileSource>) -> Self {
        Self(SourceShape::Entries(files))
    }
}

/// A medium's identity within its session's pool.
///
/// It is issued by the pool, opaque, and never reused for the session's
/// life — a released medium's identity does not come back on the next
/// load, so an identity kept past a release resolves to absence rather
/// than to a stranger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MediaId(u64);

impl MediaId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The identity a value names, for the C and Python surfaces, where
    /// one arrives as a number. It is not a lookup: an identity the pool
    /// never issued simply resolves to absence.
    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    /// The identity's value, for the C and Python surfaces.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for MediaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "media{}", self.0)
    }
}

/// Where a medium is currently linked: the slot of the device holding
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaLink {
    pub(crate) attachment: AttachmentId,
}

impl fmt::Display for MediaLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the device at {}", self.attachment)
    }
}

/// One loaded medium: the content handle, pool-owned and holdable.
///
/// Every content verb lives here. A medium answers whether or not a
/// device links it — a disk mastered out of an archive answers before
/// any drive exists to seat it — and the verbs a namespace-native
/// medium has no space for refuse by name rather than by failing further
/// in.
#[derive(Debug)]
pub struct Medium {
    id: MediaId,
    state: MediumState,
    link: Option<MediaLink>,
    /// The evidence pool, established at the load and immutable for as
    /// long as the session holds the medium (F56).
    partitions: PartitionPool,
    /// The recording's coordinates as the evidence left them,
    /// established in the same act and immutable for the same reason:
    /// geometry is discovered, and nothing is declared onto a medium
    /// that already exists.
    geometry: Geometry,
}

impl Medium {
    pub(crate) fn new(
        id: MediaId,
        state: MediumState,
        partitions: PartitionPool,
        geometry: Geometry,
    ) -> Self {
        Self {
            id,
            state,
            link: None,
            partitions,
            geometry,
        }
    }

    /// This medium's identity in its session's pool — what a device is
    /// told to insert, and what a lookup answers by.
    pub fn id(&self) -> MediaId {
        self.id
    }

    /// The device this medium is currently linked to, where one links it.
    pub(crate) fn link(&self) -> Option<&MediaLink> {
        self.link.as_ref()
    }

    pub(crate) fn set_link(&mut self, link: Option<MediaLink>) {
        self.link = link;
    }

    /// Whether a device currently links this medium. An unlinked medium
    /// is ordinary rather than idle: it is loaded, claimed, and answering.
    pub fn is_linked(&self) -> bool {
        self.link.is_some()
    }

    /// The artifact claimed — the archive itself for an image loaded out
    /// of one — or `None` where the caller's handle has no recoverable
    /// name.
    pub fn path(&self) -> Option<&str> {
        self.state.path()
    }

    /// The resolved artifact — the entry name for an image loaded out of
    /// an archive, else the source's own name.
    pub fn image_path(&self) -> Option<&Path> {
        self.state.image_path()
    }

    /// The resolved image's own size in bytes — the raw plane.
    ///
    /// Distinct from [`Medium::size`], which is the size of the disk the
    /// format adapter presents. For a raw image they agree; for a qcow2
    /// they do not.
    pub fn image_size_bytes(&self) -> u64 {
        self.state.image_size_bytes()
    }

    /// Reads `buf` from the resolved image at `offset` — the medium's own
    /// bytes, not the presented disk. This is the bounded access form
    /// (P27): the image streams from its backing through the session
    /// cache, and no operation requires it resident whole.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.state.read_at(offset, buf)
    }

    /// Identifies the artifact's nesting layers and probable filesystem.
    /// Probes read bounded evidence — a leading prefix, the length, and
    /// the name — never the whole image (P27).
    pub fn identify(&self) -> Identification {
        self.state.identify()
    }

    /// The session's **effective** access mode for this medium: what the
    /// claim afforded where the evidence supports it, and read-only where
    /// it does not.
    ///
    /// A medium loaded on a writable handle whose evidence came up short
    /// reports read-only here and says why in [`Medium::assurance`]
    /// (P28).
    pub fn mode(&self) -> AccessMode {
        self.state.mode()
    }

    /// What the open established about the evidence beneath this medium
    /// (P28): the outcome, the condition where one narrowed the session,
    /// the ordered evidence, the exact extents that read, the access the
    /// evidence permits, and whose open the claim is.
    ///
    /// It is available immediately, before anything is read, so a caller
    /// meets a deficiency by being told rather than by an operation
    /// failing halfway.
    pub fn assurance(&self) -> &Assurance {
        self.state.assurance()
    }

    /// The image container format the medium's image turned out to be,
    /// or the refusal naming a medium that is no disk image.
    pub fn format(&self) -> Result<DiskFormat> {
        Ok(self.state.space("format")?.format())
    }

    /// The presented content's size — the guest-visible size for a
    /// qcow2, and what the author's own coordinates address for an
    /// authored disk — or the refusal naming a medium that presents
    /// none.
    pub fn size(&self) -> Result<u64> {
        self.state.presented_size("size")
    }

    /// Whether uncommitted changes exist.
    pub fn is_modified(&self) -> bool {
        self.state.is_modified()
    }

    /// The device this medium's content is assumed recorded by — the
    /// declaration the load carried — or `None` where no device
    /// recorded it.
    ///
    /// `None` is the honest answer rather than a gap: an archive was
    /// recorded by no device, and neither was an authored blank — an
    /// author assumes none, and only the reserved authored-to-recorded
    /// arc would bind one.
    pub fn device_type(&self) -> Option<DeviceType> {
        self.state.device_type()
    }

    /// The authored kind this medium was created as, or `None` where it
    /// was loaded from an artifact instead.
    ///
    /// It is the third fact class showing on the surface: a medium
    /// answers what it was *made* as exactly where an author made it, and
    /// answers nothing where evidence is what it has.
    pub fn authored_as(&self) -> Option<NewMedia> {
        self.state
            .authored_medium()
            .map(crate::model::authored::AuthoredMedium::kind)
    }

    /// The article this medium is (P14), by the catalog's stable
    /// spelling — the physical substrate, which is a different fact from
    /// the recording above.
    pub fn article(&self) -> &'static str {
        self.state.media().id
    }

    /// The article's name, fit to show a user beside the drive it goes
    /// in.
    pub fn article_name(&self) -> &'static str {
        self.state.media().name
    }

    /// What slot this medium goes in — the recording side of the insert
    /// check, and the whole of it — or `None` where it goes in no slot
    /// at all, which is what an authored blank answers.
    pub(crate) fn slot(&self) -> Option<DeviceSlot> {
        self.state.slot()
    }

    /// The layered inspection of this medium: the block-active device,
    /// what its leading structure turned out to be, any recognized
    /// partition schema, every region that schema declares, every volume
    /// actually composed, and every filesystem recognition attempted on
    /// one.
    ///
    /// Each fact stays at the seam that owns it. A region whose type this
    /// release declines to read is still reported, with a reading of what
    /// the type declares and the refusal beside it; a volume whose
    /// filesystem could not be recognized is still a volume, with the
    /// refusal at the filesystem seam; and neither renumbers what follows.
    ///
    /// Content no adapter claims is an outcome here rather than a
    /// refusal — a disk in no format this release knows is a fact about
    /// the disk. An image that cannot be *read* still fails.
    pub fn inspect(&mut self) -> Result<DiskReport> {
        let pool = self.partitions.clone();
        self.state.space_mut("inspect")?.inspect(&pool)
    }

    // -------------------------------------- geometry and its coordinates

    /// The recording's coordinates as the evidence beneath this medium
    /// left them: what was settled, what the sources contradict each
    /// other about, and every reading taken (P4).
    ///
    /// It was established when the medium was loaded and is evidence
    /// from then on — nothing re-reads a boot record behind a caller,
    /// and **nothing is ever declared onto it**. Where the readings
    /// disagree the answer is
    /// [`Undetermined`](crate::GeometryState::Undetermined), reported
    /// with both readings and settled by neither; where nothing states
    /// one it is [`Unstated`](crate::GeometryState::Unstated), which is
    /// a different fact and kept as one.
    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// Reads one whole sector in the recording's own coordinates.
    ///
    /// Cylinders and heads number from zero and **sectors from one**,
    /// which is the recording's convention rather than this library's,
    /// and `buf` is exactly one sector of this recording — a sector is
    /// answered whole or not at all.
    ///
    /// **It answers on a sector-addressed recording whose geometry the
    /// evidence established, and refuses by name otherwise**, its own
    /// [`GeometryRule`](crate::GeometryRule) set naming which: a
    /// block-addressed drive or a medium no device recorded has no such
    /// coordinates at all; a medium whose sources state no geometry, or
    /// state contradicting ones, has none to address in and the refusal
    /// points at [`geometry`](Self::geometry), where the readings are.
    /// A coordinate the geometry does not cover — or one it covers and
    /// the content does not hold — is refused rather than answered with
    /// zeros.
    pub fn read_sector(
        &mut self,
        cylinder: u32,
        head: u32,
        sector: u32,
        buf: &mut [u8],
    ) -> Result<()> {
        let offset = self.sector_offset("read_sector", cylinder, head, sector, buf.len())?;
        self.read_space_at(offset, buf)
    }

    /// Writes one whole sector in the recording's own coordinates,
    /// **buffered until [`commit`](Self::commit)** like every other
    /// write (P2), under the same rules
    /// [`read_sector`](Self::read_sector) answers by.
    pub fn write_sector(
        &mut self,
        cylinder: u32,
        head: u32,
        sector: u32,
        data: &[u8],
    ) -> Result<()> {
        let offset = self.sector_offset("write_sector", cylinder, head, sector, data.len())?;
        self.write_space_at(offset, data)
    }

    /// Where one coordinate sits in the presented content, or the
    /// refusal naming why this medium has no such coordinate.
    ///
    /// The checks run outermost-first, so a caller is told the largest
    /// true thing: that the recording is not addressed this way at all,
    /// before that its geometry is unsettled, before that the coordinate
    /// is outside it.
    fn sector_offset(
        &self,
        verb: &str,
        cylinder: u32,
        head: u32,
        sector: u32,
        length: usize,
    ) -> Result<u64> {
        let geometry = self.sector_coordinates()?;
        if length as u64 != geometry.sector_bytes {
            return Err(geometry::refuse(
                GeometryRule::PartialSector,
                format!(
                    "one sector of this recording is {} bytes and {length} \
                     were offered: a sector is read and written whole or not \
                     at all",
                    geometry.sector_bytes
                ),
            ));
        }
        let offset = geometry.offset_of(cylinder, head, sector)?;
        let held = self.state.presented_size(verb)?;
        if offset + geometry.sector_bytes > held {
            return Err(geometry::refuse(
                GeometryRule::OutsideGeometry,
                format!(
                    "cylinder {cylinder}, head {head}, sector {sector} is \
                     inside this recording's coordinates ({geometry}) and past \
                     the content, which holds {held} bytes: the last cylinder \
                     is not wholly present",
                ),
            ));
        }
        Ok(offset)
    }

    /// The coordinates this medium's sector verbs address in, or the
    /// refusal naming what it has instead.
    ///
    /// **What declares the addressing is whichever fact class the medium
    /// has.** A recorded medium's device type declares it (P14's
    /// granularity rule), and an authored one's kind does — the author
    /// stating coordinates is exactly what makes their blank addressed
    /// by them. A medium neither recorded nor authored with coordinates
    /// has none, and says which of the two it is.
    fn sector_coordinates(&self) -> Result<RecordingGeometry> {
        if let Some(authored) = self.state.authored_medium() {
            if !authored.is_sector_addressed() {
                return Err(geometry::refuse(
                    GeometryRule::NotSectorAddressed,
                    format!(
                        "{} is a blank article with nothing recorded on it, so \
                         there is no cylinder, head or sector of it to address: \
                         an authored medium addressed by coordinates states \
                         them at creation",
                        authored.named()
                    ),
                ));
            }
            return self.geometry.require(&self.state.named());
        }
        let Some(device) = self.device_type() else {
            return Err(geometry::refuse(
                GeometryRule::NotSectorAddressed,
                format!(
                    "{} was recorded by no device — an archive's content is \
                     reached by the names it holds — so there is no cylinder, \
                     head or sector of it to address",
                    self.state.named()
                ),
            ));
        };
        // The declaration is the type's own attribute, matched rather
        // than compared as text: a string-named rule in orchestration is
        // exactly what P12 keeps out of the middle of the library.
        if device.addressing_kind() != Addressing::Sector {
            return Err(geometry::refuse(
                GeometryRule::NotSectorAddressed,
                format!(
                    "a {} ({}) addresses its recording by {}, so it has no \
                     cylinder, head and sector to be told: read it through the \
                     partition that composes it",
                    device.name(),
                    device.id(),
                    device.addressing()
                ),
            ));
        }
        self.geometry.require(&self.state.named())
    }

    // -------------------------------------------- the flux presentation

    /// The family's hardware bitstream over this medium's recording,
    /// materialized once under the profile's declared mechanics and
    /// read-channel rules and answered from then on.
    ///
    /// **It takes no arguments because the type carries the rules**
    /// (P30 reached through the type): being a `Commodore1541` medium
    /// *means* clocking through the c1541 channel, and what was used
    /// travels into the result's account. It answers where the device
    /// type's profile bears flux, and refuses by name everywhere else —
    /// a block medium's recording is presented by its format adapter,
    /// and the two families are disjoint (P13).
    pub fn bitstream(&mut self) -> Result<&Bitstream> {
        self.state.flux_mut("bitstream")?.bitstream()
    }

    /// The family's encoded bytestream — the byte sequence the declared
    /// group code makes of the bitstream — materialized once under the
    /// same rule and answered from then on.
    ///
    /// The framed bytes of one location are read through
    /// [`Bytestream::location`]; no byte here is a header, a
    /// sector or a file, and the layers that assign those sit above.
    pub fn bytestream(&mut self) -> Result<&Bytestream> {
        self.state.flux_mut("bytestream")?.bytestream()
    }

    /// The recording's own record layer, for the namespace door: the
    /// sector layer the CBM DOS reading opens over, recognized once
    /// under the profile's declared grammar.
    pub(crate) fn c1541_sectors(&mut self) -> Result<&crate::flux::c1541::sectors::C1541Sectors> {
        let sectors = self
            .state
            .flux_mut("filesystem_as(\"cbmdos\")")?
            .sectors()?;
        // The rung is one; the reading it holds is the family's. A
        // medium of another family answers `None` here rather than
        // being read for records its recording never states.
        sectors.c1541().ok_or_else(|| {
            Error::unsupported(format!(
                "this medium's records were recognized by the '{}' family, whose claims                  are not CBM DOS sectors; the CBM DOS namespace opens over a 1541                  recording and nothing else",
                sectors.family()
            ))
        })
    }

    // ------------------------------------------------ the partition pool

    /// The scheme this medium's content is laid out under, or `None`
    /// where it records none and the direct partition stands.
    ///
    /// It is the evidence answer, and the direct partition leaves it
    /// unchanged: a medium that recorded no table still says so here.
    pub fn partition_scheme(&self) -> Option<PartitionScheme> {
        self.partitions.scheme()
    }

    /// Every partition in the pool, in the scheme's own order.
    ///
    /// A medium recording no scheme answers with exactly one — the
    /// direct partition, which is the library's own composition of the
    /// whole content and says so in its provenance.
    pub fn partitions(&self) -> Vec<Partition> {
        self.partitions.partitions().to_vec()
    }

    /// The partition the scheme's own ordinal names, ready to be worked —
    /// or `None`, absence being an answer and nothing manufactured to
    /// report it.
    ///
    /// The ordinals are the scheme's own: MBR entry 1 is `1`, and the
    /// direct partition is `0`. **The content of a medium is reached
    /// through here**: the vantage doors on the view hand out the one
    /// [`StorageSpace`](crate::StorageSpace) the partition composes, and
    /// the file verbs live on that node and nowhere else (P19).
    pub fn partition(&mut self, ordinal: u32) -> Option<PartitionView<'_>> {
        let partition = self.partitions.get(ordinal)?.clone();
        Some(PartitionView::over_medium(partition, self))
    }

    /// The commit point (P2): writes everything buffered since the medium
    /// was loaded (or the last commit/rollback) through to the image,
    /// then flushes. The commit is durable (P9): every host write is
    /// staged in memory first, the bytes it will overwrite are made
    /// durable in a private recovery journal, and only then does the file
    /// change — so an interruption at any point leaves state the next
    /// open reconciles to wholly the old image or wholly the committed
    /// new one. A write-through refusal (P6) likewise surfaces before a
    /// single byte of the file has moved.
    ///
    /// The journal lands **beside the artifact**, so a medium whose
    /// source handle has no recoverable name refuses here by name rather
    /// than committing without it.
    ///
    /// **An authored medium commits too, and there is nowhere for it to
    /// commit to but itself**: it holds no artifact, so the commit point
    /// makes the buffered writes the medium's own state — session-backed
    /// until an explicit encode gives it an artifact — and there is no
    /// journal, because no file changes for an interruption to leave
    /// half-written.
    pub fn commit(&mut self) -> Result<()> {
        self.state.commit()
    }

    /// Discards everything buffered; what the medium already holds is
    /// untouched. Unaltered cached extents stay resident — they still
    /// mirror it.
    pub fn rollback(&mut self) -> Result<()> {
        self.state.rollback()
    }

    // ------------------------------------- the plumbing the spaces read

    /// Reads within a space's extent, the offset already resolved against
    /// the presented disk by the space that owns the bound.
    pub(crate) fn read_space_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.state.read_space_at(offset, buf)
    }

    /// Writes within a space's extent, buffered until commit like every
    /// other write (P2).
    pub(crate) fn write_space_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.state.write_space_at(offset, data)
    }

    /// The namespace an archive medium bears, where this is one.
    ///
    /// An archive's content *is* its namespace, so nothing is probed for
    /// and nothing is refused: the grammar that recognized the artifact
    /// already read the index this presents.
    pub(crate) fn archive_namespace(&mut self) -> Option<(&'static str, Box<dyn Catalog>)> {
        self.state.archive().map(ArchiveMedium::namespace)
    }

    /// One entry of the archive this medium is, taken as a load's
    /// source — reached from the file view that names it.
    pub(crate) fn entry_source(&mut self, name: &str) -> Result<FileSource> {
        let archive = self.state.archive().ok_or_else(|| {
            Error::unsupported(format!(
                "{} is no archive medium, and this release takes a load's \
                 source from an archive's namespace alone",
                self.state.named()
            ))
        })?;
        archive.entry_file_source(name)
    }

    /// Every file of this archive under `path`, taken as a load's
    /// sources in one pass — reached from the space that bears the
    /// namespace.
    pub(crate) fn entry_group_sources(&mut self, path: &str) -> Result<Vec<FileSource>> {
        let archive = self.state.archive().ok_or_else(|| {
            Error::unsupported(format!(
                "{} is no archive medium, and this release gathers a load's \
                 collection from an archive's namespace alone",
                self.state.named()
            ))
        })?;
        archive.entry_group_sources(path)
    }

    /// A discovery over one entry of the archive this medium is — the
    /// nested journey, reached from the file view that names the entry.
    ///
    /// The entry is read through the archive's own claim, so the child is
    /// opened without re-opening anything and the artifact cannot change
    /// between the naming and the load (P7).
    pub(crate) fn discover_entry(&mut self, name: &str) -> Result<Discovery> {
        let archive = self.state.archive().ok_or_else(|| {
            Error::unsupported(format!(
                "{} is no archive medium, and this release mints a discovery \
                 from an archive entry alone",
                self.state.named()
            ))
        })?;
        let claimed = archive.claim_entry(name)?;
        Ok(Discovery::over(MediumRecognition::over_entry(claimed)?))
    }

    /// Opens the namespace a declaration named, over one partition's
    /// extent — the adapter the declaration names is the one that reads
    /// it, so nothing here branches on a filesystem identifier.
    pub(crate) fn open_namespace_at(
        &mut self,
        adapter: &'static dyn FilesystemAdapter,
        offset: u64,
        length: u64,
    ) -> Result<Box<dyn Catalog>> {
        self.state
            .space_mut("filesystem_as")?
            .open_namespace_at(adapter, offset, length)
    }

    /// Recognizes FAT on one partition's extent — the verification under
    /// a declared partition type, and under a declared `"fat"` reading
    /// where no type declared one (P18).
    pub(crate) fn recognize_fat(
        &mut self,
        offset: u64,
    ) -> Result<crate::filesystem::fat::FatRecognition> {
        self.state.space_mut("filesystem")?.recognize_fat(offset)
    }

    pub(crate) fn entries(&mut self, at: u64, path: &str) -> Result<Vec<FatEntry>> {
        self.state.space_mut("entries")?.entries(at, path)
    }

    pub(crate) fn stat(&mut self, at: u64, path: &str) -> Result<Option<FatEntry>> {
        self.state.space_mut("stat")?.stat(at, path)
    }

    pub(crate) fn read_file(&mut self, at: u64, path: &str) -> Result<Vec<u8>> {
        self.state.space_mut("read_file")?.read_file(at, path)
    }

    pub(crate) fn read_file_at(
        &mut self,
        at: u64,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        self.state
            .space_mut("read_file_at")?
            .read_file_at(at, path, offset, buf)
    }

    pub(crate) fn resize_file(&mut self, at: u64, path: &str, size: u64) -> Result<()> {
        self.state
            .space_mut("resize_file")?
            .resize_file(at, path, size)
    }

    pub(crate) fn write_file_at(
        &mut self,
        at: u64,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        self.state
            .space_mut("write_file_at")?
            .write_file_at(at, path, offset, data)
    }

    pub(crate) fn write_file(&mut self, at: u64, path: &str, contents: &[u8]) -> Result<()> {
        self.state
            .space_mut("write_file")?
            .write_file(at, path, contents)
    }

    pub(crate) fn make_directory(&mut self, at: u64, path: &str) -> Result<()> {
        self.state
            .space_mut("make_directory")?
            .make_directory(at, path)
    }
}

/// The session's media pool: every medium it holds, and the identities
/// they answer to.
///
/// It is not a cache and not a registry of artifacts — it is where media
/// *live*. Nothing here indexes by path, because a medium may have no
/// name and two media may have the same one.
#[derive(Debug, Default)]
pub(crate) struct MediaPool {
    media: Vec<Medium>,
    /// The next identity to issue. It only ever counts up, so a released
    /// medium's identity is never handed to another.
    next: u64,
}

impl MediaPool {
    /// Takes an opened medium into the pool, unlinked, and answers with
    /// the identity it was issued.
    pub(crate) fn admit(
        &mut self,
        state: MediumState,
        partitions: PartitionPool,
        geometry: Geometry,
    ) -> MediaId {
        let id = MediaId::new(self.next);
        self.next += 1;
        self.media
            .push(Medium::new(id, state, partitions, geometry));
        id
    }

    /// Every medium the pool holds, in the order they were loaded.
    pub(crate) fn ids(&self) -> Vec<MediaId> {
        self.media.iter().map(Medium::id).collect()
    }

    pub(crate) fn get(&self, id: MediaId) -> Option<&Medium> {
        self.media.iter().find(|medium| medium.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: MediaId) -> Option<&mut Medium> {
        self.media.iter_mut().find(|medium| medium.id == id)
    }

    /// Takes a medium out of the pool — the one state-destroying act.
    pub(crate) fn take(&mut self, id: MediaId) -> Result<Medium> {
        let at = self
            .media
            .iter()
            .position(|medium| medium.id == id)
            .ok_or_else(|| Error::not_found(format!("this session holds no {id}")))?;
        Ok(self.media.remove(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hard-drive declaration, spelled once for the tests below.
    fn hd() -> HardDrive {
        HardDrive::MbrBlock
    }

    #[test]
    fn every_claimed_format_round_trips_through_its_spelling() {
        for claim in Format::claimed() {
            let device = claim.devices().first().copied();
            let block_bytes = claim.takes_block_bytes().then_some(512);
            let format = Format::declared(claim.id(), device, block_bytes).expect("claimed");
            assert_eq!(format.id(), claim.id());
            assert_eq!(format.name(), claim.name());
            assert_eq!(format.device_type(), device);
            assert_eq!(format.block_bytes(), block_bytes);
        }
    }

    #[test]
    fn a_classification_is_not_a_declaration() {
        // P3: the set is enumerated, so a word that names a kind rather
        // than one catalog entry is refused naming what is claimed.
        let error = Format::declared("archive", None, None).expect_err("refused");
        let message = error.to_string();
        assert!(
            message.contains("archive"),
            "names what was asked: {message}"
        );
        assert!(message.contains("zip"), "names what is claimed: {message}");
    }

    #[test]
    fn a_format_recording_many_devices_is_declared_with_one() {
        // The class is the compiler's check; which type within it is the
        // caller's declaration, and leaving it out is a refusal naming
        // the types the adapter records.
        let error = Format::declared("qcow2", None, None).expect_err("refused");
        let message = error.to_string();
        assert!(
            message.contains("mbr-sector-hd") && message.contains("mbr-block-hd"),
            "names what it records: {message}"
        );

        // A device of the wrong class is refused at the text boundary,
        // where the compiler cannot make the check.
        let error = Format::declared(
            "qcow2",
            Some(DeviceType::Floppy(FloppyDrive::Commodore1541)),
            None,
        )
        .expect_err("refused");
        assert!(
            error.to_string().contains("floppy"),
            "names the class found: {error}"
        );
    }

    #[test]
    fn a_bare_format_carries_its_one_device_and_admits_no_other() {
        assert_eq!(
            Format::H8d.device_type(),
            Some(DeviceType::Floppy(FloppyDrive::HeathH17))
        );
        assert_eq!(
            Format::declared("h8d", None, None).expect("bare"),
            Format::H8d,
            "the format admits one type, so the declaration needs no second word"
        );
        let error = Format::declared("h8d", Some(DeviceType::Floppy(FloppyDrive::HeathH37)), None)
            .expect_err("refused");
        assert!(
            error.to_string().contains("h17"),
            "names the recording it does make: {error}"
        );
    }

    #[test]
    fn a_pairing_no_adapter_declares_is_refused_by_name() {
        // GPT is in the device catalog and read by no adapter, so
        // declaring it is refused rather than read as the wrong table.
        let error = Format::Qcow2 {
            device: HardDrive::Gpt,
        }
        .check_pairing()
        .expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("gpt-hd"), "names the pairing: {message}");
        assert!(
            message.contains("mbr-block-hd"),
            "names what it does record: {message}"
        );
        assert!(Format::Qcow2 { device: hd() }.check_pairing().is_ok());
    }

    #[test]
    fn the_block_size_is_declared_where_and_only_where_it_is_recorded() {
        assert_eq!(
            Format::Raw {
                device: hd().into(),
                block_bytes: 512
            }
            .block_bytes(),
            Some(512)
        );
        assert!(
            Format::declared("raw", Some(DeviceType::HardDrive(hd())), None).is_err(),
            "a raw image records no addressable unit"
        );
        assert!(
            Format::declared("qcow2", Some(DeviceType::HardDrive(hd())), Some(512)).is_err(),
            "a qcow2 records its own, and two answers about one disk is one too many"
        );
    }

    #[test]
    fn an_archive_grammar_takes_no_device_at_all() {
        assert_eq!(
            Format::declared("zip", None, None).expect("claimed"),
            Format::Zip
        );
        let error =
            Format::declared("zip", Some(DeviceType::HardDrive(hd())), None).expect_err("refused");
        assert!(
            error.to_string().contains("no device at all"),
            "names why an archive takes none: {error}"
        );
    }

    #[test]
    fn only_the_archive_grammars_declare_one() {
        assert_eq!(Format::Zip.archive_grammar(), Some("zip"));
        assert_eq!(Format::SevenZip.archive_grammar(), Some("7z"));
        assert_eq!(
            Format::Zip.device_type(),
            None,
            "an archive was recorded by no device"
        );
        for format in [
            Format::Raw {
                device: hd().into(),
                block_bytes: 512,
            },
            Format::Qcow2 { device: hd() },
            Format::Vdi { device: hd() },
            Format::H8d,
        ] {
            assert_eq!(format.archive_grammar(), None);
        }
    }

    #[test]
    fn an_identity_is_never_reused_within_a_session() {
        // A released medium's identity resolving to a stranger would make
        // every handle a caller kept a liability.
        let mut pool = MediaPool::default();
        assert_eq!(pool.ids(), Vec::new());
        assert_eq!(MediaId::new(3).to_string(), "media3");
        assert_eq!(MediaId::new(3).value(), 3);
        assert!(pool.get(MediaId::new(0)).is_none());
        assert!(pool.take(MediaId::new(0)).is_err());
    }
}
