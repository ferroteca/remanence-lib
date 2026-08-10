// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Self-contained disk image analysis library.
//!
//! A [`Session`] is the claim and cache scope; the [`Machine`]s within it
//! are the device sets, and a [`StorageDevice`] is the one storage
//! handle — the slot, its family, and the state of whatever medium
//! occupies it. **Devices are added and media are loaded, as two acts**:
//! [`Machine::add_device`] takes a [`DeviceFamily`] as concrete as the
//! drive the machine actually had, and [`StorageDevice::load_media`]
//! loads a disk image into it, optionally naming an entry inside a
//! supported archive. That load takes one P7 claim — refusing a medium
//! the family is not served, naming both sides — and serves both of the
//! medium's planes through that handle.
//!
//! [`discover_media`] answers what an artifact is before any of that, on
//! no handle at all: the exact medium, the device families served it, and
//! the image format's declared default device. The [`Discovery`] it
//! returns holds that claim and the work already done, and
//! [`StorageDevice::load_discovery`] consumes it into a device so nothing
//! runs twice; [`Machine::add_device_for`] composes the two acts over it
//! where a format declares a default, and refuses by name where none
//! does. Through the storage handle:
//! [`StorageDevice::identify`] reports the layers of the artifact's
//! nesting (archive, image, physical media, filesystem) recognized by
//! built-in executable adapters, over the image's own bytes, while
//! [`StorageDevice::inspect`] works over the disk a format adapter
//! presents above them.
//!
//! **File access lives on one node.** [`StorageDevice::filesystem`]
//! resolves device → volume → filesystem where every seam has exactly
//! one supported answer and refuses naming the candidates where one does
//! not; [`StorageDevice::volume`] selects by the identity the inspection
//! report issued where several exist. The verbs — [`StorageSpace::entries`],
//! [`StorageSpace::stat`], [`StorageSpace::get_file`] and their kin — live
//! on the [`StorageSpace`] the resolver answers with, and the device
//! carries none of them.
//!
//! **An archive is a medium like any other.** It loads into an
//! archive-family device ([`DeviceFamily::ARCHIVE_DEVICE`]) and its
//! content is walked through the [`StorageSpace`] that device resolves
//! to — a namespace with no addressed extent beneath it, ZIP and 7z
//! being the claimed grammars. An entry recognized as an artifact of its
//! own is opened by [`File::discover`] and loaded into a device of its
//! own, which is the one recursion this model has.
//!
//! **The flux family's physical stratum is reached through its own
//! type.** [`RemanenceImage`] opens a `.remanence` artifact and answers
//! the physical facts of one disk, and the C64 renditions are mastered
//! off it: [`RemanenceImage::write_d64`], [`RemanenceImage::write_g64`]
//! and [`RemanenceImage::write_p64`], each paired with a `describe_`
//! verb that computes everything and writes nothing, and each stating
//! what its destination did not carry (P29).
//!
//! Every open also states what it established about the evidence beneath
//! it ([`StorageDevice::assurance`]): a source short of what its own
//! interpretation declares is read as far as it truthfully goes,
//! read-only, with the shortfall named rather than hidden or thrown away
//! whole (P28).

mod adapters;
mod archive;
mod assurance;
mod c1541_mastering;
mod c1541_presentation;
mod c64_renditions;
mod cache;
mod checksum;
mod deflate;
mod device;
mod device_family;
mod discovery;
mod disk;
mod dos_letters;
mod dos_name;
mod drive_profile;
mod encoded_bytestream;
mod error;
mod evidence;
mod fat;
mod filesystem;
mod filesystem_catalog;
mod flux_analysis;
mod flux_capture;
mod flux_medium;
mod hardware_bitstream;
mod hdos;
mod inflate;
mod journal;
mod kryoflux;
mod lzma;
mod machine;
mod mbr;
mod media_profile;
mod p64;
mod partition;
mod qcow2;
mod remanence_format;
mod remanence_image;
mod remanence_reconstruction;
mod report;
mod session;
mod sevenzip;
mod source;
mod storage_device;
mod vdi;
mod volume;
mod zip;

pub use assurance::{Assurance, AssuranceCondition, AssuranceOutcome, ByteRange};
pub use c1541_mastering::{
    DuplicatePolicy, MasteredLocation, MasteredMedium, MasteringPlan, MasteringPlanReport,
    MasteringPolicy, ObservationPolicy, OriginPolicy, ProjectionPolicy, PulseStrengthPolicy,
};
pub use c64_renditions::{D64Block, D64Report, G64HalfTrack, G64Report};
pub use c1541_presentation::{
    AlignmentPolicy, BitstreamLocation, BitstreamReport, BytestreamLocation, BytestreamReport,
    C1541Bitstream, C1541Bytestream, DensityPolicy, GcrCodecPolicy, ReadChannelPolicy,
    UnassignedSymbolPolicy, UnzonedPolicy, WeakPulsePolicy,
};
pub use cache::DEFAULT_CACHE_BYTES;
pub use device::{AccessIntent, AccessMode};
pub use device_family::DeviceFamily;
pub use discovery::{Discovery, discover_media, discover_media_with_cache};
pub use disk::DiskFormat;
pub use dos_letters::{
    DosAssignmentRule, DosMachine, DriveMap, DriveMapping, LetterOutcome, MachineDevice,
    ResidentCondition,
};
pub use dos_name::DosNameRule;
pub use drive_profile::{LocationVerdict, ProfileVerdict, Recognition, ZoneClaim};
pub use error::{Error, ErrorCategory, Result, RuleIdentity};
pub use evidence::DeclaredLoss;
pub use fat::FatKind;
pub use filesystem::{Entry, EntryFact, EntryKind, File, SpaceRule, StorageSpace};
pub use machine::{Machine, Session};
pub use kryoflux::{
    CaptureIssue, CaptureRunReport, CaptureSet, CaptureSetMember, CaptureSetReport,
    ObservationReport, StepPosition, TimeBaseReport,
};
pub use report::{
    DeclaredGeometry, DeviceInfo, DiskContent, DiskReport, FilesystemId, FilesystemInfo,
    LabelReading, PartitionSchemaInfo, RegionId, RegionInfo, RegionRole, VolumeId, VolumeInfo,
    VolumeLabel, VolumeOrigin,
};
pub use p64::{P64HalfTrack, P64Image, P64Report};
pub use remanence_format::RemanenceWriteReport;
pub use remanence_image::{RemanenceHole, RemanenceImage, RemanenceImageReport, RemanenceOrbit};
pub use storage_device::{AttachmentId, StorageDevice};
pub use session::{
    ArchiveLayout, DiskLayout, FilesystemLayout, Identification, ImageLayout, Layer, LayerKind,
    LayerLayout, PhysicalMediaLayout, SectorLayout, SizeInformation, TrackSectorLayout,
};
