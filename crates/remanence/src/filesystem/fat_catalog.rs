// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! FAT read through a plain device rather than through a pooled medium.
//!
//! **Why this exists.** A FAT volume on a partitioned image is reached
//! through the medium that composed the partition, and the medium serves
//! every read. A FAT volume on a flux recording has no medium beneath it
//! — the extent is composed by the recording's own sector layer (D62) —
//! so there is nothing for that path to read through. HDOS and CP/M
//! already have the shape that works here: an adapter opens a
//! [`Catalog`] against a device and the catalog answers from then on.
//! This gives FAT the same shape, which is what lets a FAT floppy on an
//! MFM recording open through the seam a hard-disk image opens through
//! without either side learning about the other.
//!
//! **Nothing about the reading changes.** The same [`FatVolume`] parses
//! the same boot record and walks the same chains; only what it reads
//! through differs. A refusal from beneath — a sector whose CRC
//! disagrees, say — travels out unchanged, because it is truer than
//! anything this layer could say about it.

use std::cell::RefCell;

use crate::Result;
use crate::filesystem::fat::{FatEntry, FatVolume};
use crate::filesystem::{Catalog, Entry, VolumeLabel};
use crate::io::device::Device;

/// A FAT volume read through a borrowed device.
///
/// The device is held behind a [`RefCell`] because [`Catalog`] reads by
/// shared reference and [`Device`] reads by exclusive one. The borrow is
/// sound rather than merely convenient: this catalog holds the only
/// reference to the device for its whole life, so no read can overlap
/// another.
pub(crate) struct FatDeviceCatalog<'a> {
    volume: FatVolume,
    device: RefCell<&'a mut dyn Device>,
    /// Where the volume begins in the device. It is kept because
    /// `FatVolume` states offsets against the device rather than against
    /// the volume.
    offset: u64,
    label: VolumeLabel,
    kind: String,
}

impl std::fmt::Debug for FatDeviceCatalog<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FatDeviceCatalog")
            .field("kind", &self.kind)
            .field("offset", &self.offset)
            .finish()
    }
}

impl<'a> FatDeviceCatalog<'a> {
    /// Opens the FAT volume at `offset`, or passes out the refusal the
    /// recognition made.
    pub(crate) fn open(device: &'a mut dyn Device, offset: u64) -> Result<Self> {
        let volume = FatVolume::open(device, offset)?;
        let recognition = volume.recognized(device)?;
        Ok(Self {
            volume,
            device: RefCell::new(device),
            offset,
            label: recognition.label,
            kind: recognition.kind.name().to_owned(),
        })
    }

    /// What the boot record declares this volume to be.
    pub(crate) fn kind(&self) -> &str {
        &self.kind
    }

    fn segments(path: &str) -> Result<Vec<&str>> {
        let segments: Vec<&str> = path
            .split(['/', '\\'])
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .collect();
        if segments.iter().any(|segment| *segment == "..") {
            return Err(crate::error::Error::io(format!(
                "path '{path}' may not contain '..'"
            )));
        }
        Ok(segments)
    }

    fn with_device<T>(&self, action: impl FnOnce(&mut dyn Device) -> Result<T>) -> Result<T> {
        let mut device = self.device.borrow_mut();
        action(&mut **device)
    }
}

impl Catalog for FatDeviceCatalog<'_> {
    fn entries(&self, path: &str) -> Result<Vec<Entry>> {
        let segments = Self::segments(path)?;
        let found: Vec<FatEntry> =
            self.with_device(|device| self.volume.entries(device, &segments))?;
        Ok(found.iter().map(Entry::from_fat).collect())
    }

    fn stat(&self, path: &str) -> Result<Option<Entry>> {
        let segments = Self::segments(path)?;
        if segments.is_empty() {
            return Err(crate::error::Error::io("a path is required".to_owned()));
        }
        let found = self.with_device(|device| self.volume.stat(device, &segments))?;
        Ok(found.as_ref().map(Entry::from_fat))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let segments = Self::segments(path)?;
        self.with_device(|device| self.volume.read_file(device, &segments))
    }

    fn label(&self) -> Option<VolumeLabel> {
        Some(self.label.clone())
    }

    fn evidence(&self) -> Vec<String> {
        vec![format!(
            "a {} boot record recognized at the extent's first sector, read \
             through the layer that composed the extent rather than through a \
             medium",
            self.kind
        )]
    }
}
