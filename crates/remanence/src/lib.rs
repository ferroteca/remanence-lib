// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Self-contained disk image analysis library.
//!
//! An [`Archive`] lists what a supported archive holds — ZIP and 7z —
//! and a [`Session`] opens a disk image, optionally naming an entry
//! inside one of those archives. [`Session::identify`] reports the
//! container layers (archive, image, physical media, filesystem)
//! recognized by built-in executable adapters.

mod adapters;
mod archive;
mod cache;
mod checksum;
mod device;
mod disk;
mod error;
mod evidence;
mod fat;
mod file_container;
mod filesystem;
mod flux_capture;
mod hdos;
mod inflate;
mod journal;
mod lzma;
mod mbr;
mod partition;
mod qcow2;
mod session;
mod sevenzip;
mod source;
mod volume;
mod zip;

pub use archive::{Archive, ArchiveEntry};
pub use cache::DEFAULT_CACHE_BYTES;
pub use device::{AccessIntent, AccessMode};
pub use disk::{Disk, DiskFormat, DiskGeometry};
pub use error::{Error, ErrorCategory, Result};
pub use fat::{FatEntry, FatEntryKind, FatKind, VolumeInfo};
pub use hdos::{HdosFile, list_hdos_files, read_hdos_file};
pub use mbr::{PartitionInfo, PartitionKind};
pub use session::{
    ArchiveLayout, Container, ContainerKind, ContainerLayout, DiskLayout, FilesystemLayout,
    Identification, ImageLayout, PhysicalMediaLayout, SectorLayout, Session, SizeInformation,
    TrackSectorLayout,
};
