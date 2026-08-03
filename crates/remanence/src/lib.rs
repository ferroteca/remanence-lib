// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Self-contained disk image analysis library.
//!
//! A [`Session`] opens a disk image — optionally inside a `.zip` archive — and
//! [`Session::identify`] reports the container layers (archive, image,
//! physical media, filesystem) recognized by built-in executable adapters.

mod adapters;
mod archive;
mod cache;
mod device;
mod disk;
mod error;
mod fat;
mod filesystem;
mod hdos;
mod inflate;
mod journal;
mod mbr;
mod partition;
mod qcow2;
mod session;
mod volume;
mod zip;

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
