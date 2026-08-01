// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Core disk image analysis library for Remanence Workbench.
//!
//! A [`Session`] opens a disk image — optionally inside a `.zip` archive — and
//! [`Session::identify`] reports the container layers (archive, image,
//! physical media, filesystem) it can detect, driven by the plain-text format
//! definitions in a [`FormatRegistry`].

mod archive;
mod cache;
mod container;
mod device;
mod disk;
mod error;
mod fat;
mod filesystem;
mod hdos;
mod image;
mod inflate;
mod journal;
mod mbr;
mod qcow2;
mod registry;
mod session;
mod zip;

pub use cache::DEFAULT_CACHE_BYTES;
pub use device::{AccessIntent, AccessMode};
pub use disk::{Disk, DiskFormat, DiskGeometry};
pub use error::{Error, ErrorCategory, Result};
pub use fat::{FatEntry, FatEntryKind, FatKind, VolumeInfo};
pub use hdos::{HdosFile, list_hdos_files, read_hdos_file};
pub use image::DiskImage;
pub use mbr::{PartitionInfo, PartitionKind};
pub use registry::{ContainerFormat, FilesystemFormat, FormatRegistry};
pub use session::{
    ArchiveLayout, Container, ContainerKind, ContainerLayout, DiskLayout,
    FilesystemLayout, Identification, ImageLayout, PhysicalMediaLayout, SectorLayout,
    Session, SizeInformation, TrackSectorLayout,
};

/// Built-in starter container definition text.
pub const DEFAULT_CONTAINER_FORMATS: &str =
    include_str!("../formats/container-formats.txt");

/// Built-in starter filesystem definition text.
pub const DEFAULT_FILESYSTEM_FORMATS: &str =
    include_str!("../formats/filesystem-formats.txt");

/// Parses the built-in starter format definitions.
pub fn default_format_registry() -> Result<FormatRegistry> {
    FormatRegistry::parse(DEFAULT_CONTAINER_FORMATS, DEFAULT_FILESYSTEM_FORMATS)
}
