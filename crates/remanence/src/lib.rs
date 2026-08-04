// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Self-contained disk image analysis library.
//!
//! An [`Archive`] lists what a supported archive holds — ZIP and 7z —
//! and a [`Disk`] opens a disk image, optionally naming an entry inside
//! one of those archives. One open takes one P7 claim and serves both of
//! the medium's planes: [`Disk::identify`] reports the container layers
//! (archive, image, physical media, filesystem) recognized by built-in
//! executable adapters, over the image's own bytes, while
//! [`Disk::inspect`] and the volume-scoped file verbs work over the disk
//! a format adapter presents above them.

mod adapters;
mod archive;
mod c1541_mastering;
mod cache;
mod checksum;
mod device;
mod disk;
mod drive_profile;
mod error;
mod evidence;
mod fat;
mod file_container;
mod filesystem;
mod flux_capture;
mod flux_medium;
mod hdos;
mod inflate;
mod journal;
mod kryoflux;
mod lzma;
mod machine;
mod mbr;
mod p64;
mod partition;
mod qcow2;
mod report;
mod session;
mod sevenzip;
mod source;
mod storage_device;
mod volume;
mod zip;

pub use archive::{Archive, ArchiveEntry};
pub use c1541_mastering::{
    DuplicatePolicy, MasteredLocation, MasteredMedium, MasteringPlan, MasteringPlanReport,
    MasteringPolicy, ObservationPolicy, OriginPolicy, ProjectionPolicy, PulseStrengthPolicy,
};
pub use cache::DEFAULT_CACHE_BYTES;
pub use device::{AccessIntent, AccessMode};
pub use disk::{Disk, DiskFormat};
pub use drive_profile::{LocationVerdict, ProfileVerdict, Recognition, ZoneClaim};
pub use error::{Error, ErrorCategory, Result};
pub use evidence::DeclaredLoss;
pub use fat::{FatEntry, FatEntryKind, FatKind};
pub use hdos::{HdosFile, list_hdos_files, read_hdos_file};
pub use machine::Session;
pub use kryoflux::{
    CaptureIssue, CaptureRunReport, CaptureSet, CaptureSetMember, CaptureSetReport,
    ObservationReport, StepPosition, TimeBaseReport,
};
pub use report::{
    DeclaredGeometry, DeviceInfo, DiskContent, DiskReport, FilesystemId, FilesystemInfo,
    PartitionSchemaInfo, RegionId, RegionInfo, RegionRole, VolumeId, VolumeInfo, VolumeOrigin,
};
pub use p64::{P64HalfTrack, P64Image, P64Report};
pub use storage_device::{AttachmentId, DeviceFamily, StorageDevice};
pub use session::{
    ArchiveLayout, Container, ContainerKind, ContainerLayout, DiskLayout, FilesystemLayout,
    Identification, ImageLayout, PhysicalMediaLayout, SectorLayout, SizeInformation,
    TrackSectorLayout,
};
