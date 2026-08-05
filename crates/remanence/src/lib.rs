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
//! medium's planes through that handle:
//! [`StorageDevice::identify`] reports the container layers (archive,
//! image, physical media, filesystem) recognized by built-in executable
//! adapters, over the image's own bytes, while
//! [`StorageDevice::inspect`] and the volume-scoped file verbs work over
//! the disk a format adapter presents above them. Every open also states
//! what it established about the evidence beneath it
//! ([`StorageDevice::assurance`]): a source short of what its own
//! interpretation declares is read as far as it truthfully goes,
//! read-only, with the shortfall named rather than hidden or thrown away
//! whole (P28). An [`Archive`] lists what a supported archive holds — ZIP
//! and 7z.

mod adapters;
mod archive;
mod assurance;
mod c1541_mastering;
mod c1541_presentation;
mod cache;
mod checksum;
mod device;
mod device_family;
mod disk;
mod dos_letters;
mod dos_name;
mod drive_profile;
mod encoded_bytestream;
mod error;
mod evidence;
mod fat;
mod file_container;
mod filesystem;
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
mod report;
mod session;
mod sevenzip;
mod source;
mod storage_device;
mod vdi;
mod volume;
mod zip;

pub use archive::{Archive, ArchiveEntry};
pub use assurance::{Assurance, AssuranceCondition, AssuranceOutcome, ByteRange};
pub use c1541_mastering::{
    DuplicatePolicy, MasteredLocation, MasteredMedium, MasteringPlan, MasteringPlanReport,
    MasteringPolicy, ObservationPolicy, OriginPolicy, ProjectionPolicy, PulseStrengthPolicy,
};
pub use c1541_presentation::{
    AlignmentPolicy, BitstreamLocation, BitstreamReport, BytestreamLocation, BytestreamReport,
    C1541Bitstream, C1541Bytestream, DensityPolicy, GcrCodecPolicy, ReadChannelPolicy,
    UnassignedSymbolPolicy, UnzonedPolicy, WeakPulsePolicy,
};
pub use cache::DEFAULT_CACHE_BYTES;
pub use device::{AccessIntent, AccessMode};
pub use device_family::DeviceFamily;
pub use disk::DiskFormat;
pub use dos_letters::{
    DosAssignmentRule, DosMachine, DriveMap, DriveMapping, LetterOutcome, MachineDevice,
    ResidentCondition,
};
pub use dos_name::DosNameRule;
pub use drive_profile::{LocationVerdict, ProfileVerdict, Recognition, ZoneClaim};
pub use error::{Error, ErrorCategory, Result, RuleIdentity};
pub use evidence::DeclaredLoss;
pub use fat::{FatEntry, FatEntryKind, FatKind};
pub use hdos::{HdosFile, list_hdos_files, read_hdos_file};
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
pub use storage_device::{AttachmentId, StorageDevice};
pub use session::{
    ArchiveLayout, Container, ContainerKind, ContainerLayout, DiskLayout, FilesystemLayout,
    Identification, ImageLayout, PhysicalMediaLayout, SectorLayout, SizeInformation,
    TrackSectorLayout,
};
