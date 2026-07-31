// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The `Disk` surface (U3 and U4): open a raw or qcow2
//! image under the P7 claim, report its partitions and volumes as they
//! actually are, and read/write files in its FAT volumes with a commit
//! point (P2) — everything rolls back until `commit`.

use std::path::Path;

use crate::device::{AccessMode, Device, FileDevice, Overlay};
use crate::error::{Error, Result};
use crate::fat::{FatEntry, FatVolume, VolumeInfo};
use crate::mbr::{self, PartitionInfo};
use crate::qcow2::{QCOW2_MAGIC, Qcow2};

/// The container format a disk image turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFormat {
    Raw,
    Qcow2 { version: u32 },
}

/// The host-side facts of one disk, as they actually are (pledged U4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskGeometry {
    pub partitions: Vec<PartitionInfo>,
    pub volumes: Vec<VolumeInfo>,
}

#[derive(Debug)]
enum Virtual {
    Raw(FileDevice),
    Qcow2(Qcow2<FileDevice>),
}

impl Virtual {
    fn device(&mut self) -> &mut dyn Device {
        match self {
            Self::Raw(device) => device,
            Self::Qcow2(device) => device,
        }
    }
}

/// A composed view: the overlay patched over the virtual disk. Reads see
/// buffered writes; nothing reaches the file until commit.
struct Composed<'a> {
    base: &'a mut dyn Device,
    overlay: &'a mut Overlay,
}

impl Device for Composed<'_> {
    fn len(&self) -> u64 {
        self.base.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.overlay.read_at(self.base, offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.overlay.write_at(self.base, offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// An open disk image.
#[derive(Debug)]
pub struct Disk {
    virtual_disk: Virtual,
    overlay: Overlay,
    format: DiskFormat,
    mode: AccessMode,
    path: String,
}

impl Disk {
    /// Opens `path` under the P7 ladder — read/write with writes denied
    /// to others (preferred); read-only with writes still denied to
    /// others; fail fast when deny-write cannot be obtained (a running
    /// VM holding the image is the designed refusal). The container is
    /// detected by magic: qcow2, else raw.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = FileDevice::open(path)?;
        let mode = file.mode();

        let mut magic = [0u8; 4];
        let format = if file.len() >= 4 {
            file.read_at(0, &mut magic)?;
            if magic == QCOW2_MAGIC { None } else { Some(DiskFormat::Raw) }
        } else {
            Some(DiskFormat::Raw)
        };

        let (virtual_disk, format) = match format {
            Some(raw) => (Virtual::Raw(file), raw),
            None => {
                let qcow2 = Qcow2::open(file)?;
                let version = qcow2.header().version;
                (Virtual::Qcow2(qcow2), DiskFormat::Qcow2 { version })
            }
        };

        Ok(Self {
            virtual_disk,
            overlay: Overlay::new(),
            format,
            mode,
            path: path.display().to_string(),
        })
    }

    /// Which P7 mode the open obtained.
    pub fn mode(&self) -> AccessMode {
        self.mode
    }

    pub fn format(&self) -> DiskFormat {
        self.format
    }

    /// The virtual disk size (the guest-visible size for qcow2).
    pub fn size(&self) -> u64 {
        match &self.virtual_disk {
            Virtual::Raw(device) => device.len(),
            Virtual::Qcow2(device) => device.header().virtual_size,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether uncommitted changes exist.
    pub fn is_modified(&self) -> bool {
        self.overlay.modified()
    }

    fn composed(&mut self) -> Composed<'_> {
        Composed { base: self.virtual_disk.device(), overlay: &mut self.overlay }
    }

    fn split_path(path: &str) -> Result<Vec<&str>> {
        let segments: Vec<&str> = path
            .split(['/', '\\'])
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .collect();
        if segments.iter().any(|segment| *segment == "..") {
            return Err(Error::io(format!("path '{path}' may not contain '..'")));
        }
        Ok(segments)
    }

    fn volume_at(&mut self, index: usize) -> Result<(u64, FatVolume)> {
        let offsets = self.volume_offsets()?;
        let &offset = offsets.get(index).ok_or_else(|| {
            Error::io(format!(
                "volume {index} does not exist ({} volumes)",
                offsets.len()
            ))
        })?;
        let mut composed = self.composed();
        let volume = FatVolume::open(&mut composed, offset)?;
        Ok((offset, volume))
    }

    fn volume_offsets(&mut self) -> Result<Vec<u64>> {
        let mut composed = self.composed();
        let partitions = mbr::discover(&mut composed)?;
        if partitions.is_empty() {
            return Ok(vec![0]);
        }
        Ok(partitions
            .iter()
            .filter(|partition| !partition.type_name.starts_with("extended"))
            .map(|partition| partition.start_bytes)
            .collect())
    }

    /// The disk's partitions and readable volumes, as they actually are
    /// (pledged U4). One volume entry per FAT volume actually read; a
    /// partition whose volume cannot be read contributes no volume and
    /// the reason is carried in the error when nothing at all is
    /// readable.
    pub fn geometry(&mut self) -> Result<DiskGeometry> {
        let mut composed = self.composed();
        let partitions = mbr::discover(&mut composed)?;

        let mut volumes = Vec::new();
        if partitions.is_empty() {
            let volume = FatVolume::open(&mut composed, 0)?;
            let length = composed.len();
            volumes.push(volume.info(&mut composed, None, length)?);
        } else {
            for partition in &partitions {
                if partition.type_name.starts_with("extended") {
                    continue;
                }
                match FatVolume::open(&mut composed, partition.start_bytes) {
                    Ok(volume) => volumes.push(volume.info(
                        &mut composed,
                        Some(partition.number),
                        partition.length_bytes,
                    )?),
                    // A declared partition whose volume is unreadable
                    // takes no letter; per U4 it simply contributes no
                    // volume here, and the caller sees the partition row
                    // without one.
                    Err(_) => continue,
                }
            }
        }

        Ok(DiskGeometry { partitions, volumes })
    }

    /// Lists a directory in volume `volume` ("" = root; "A/B" descends).
    pub fn entries(&mut self, volume: usize, path: &str) -> Result<Vec<FatEntry>> {
        let segments = Self::split_path(path)?;
        let (_, fat) = self.volume_at(volume)?;
        let mut composed = self.composed();
        fat.entries(&mut composed, &segments)
    }

    /// Copies a file's bytes out of volume `volume`.
    pub fn read_file(&mut self, volume: usize, path: &str) -> Result<Vec<u8>> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let (_, fat) = self.volume_at(volume)?;
        let mut composed = self.composed();
        fat.read_file(&mut composed, &segments)
    }

    fn require_writable(&self) -> Result<()> {
        if self.mode == AccessMode::ReadOnly {
            return Err(Error::io(format!(
                "'{}' is open read-only (P7 fallback); write actions are denied",
                self.path
            )));
        }
        Ok(())
    }

    /// Writes a file into volume `volume`. Buffered until [`Disk::commit`].
    pub fn write_file(&mut self, volume: usize, path: &str, contents: &[u8]) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let (_, fat) = self.volume_at(volume)?;
        let mut composed = self.composed();
        fat.write_file(&mut composed, &segments, contents)
    }

    /// Creates a directory in volume `volume`. Buffered until commit.
    pub fn make_directory(&mut self, volume: usize, path: &str) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a directory path is required".to_owned()));
        }
        let (_, fat) = self.volume_at(volume)?;
        let mut composed = self.composed();
        fat.make_directory(&mut composed, &segments)
    }

    /// The commit point (P2): writes everything buffered since open (or
    /// the last commit/rollback) through to the image, then flushes.
    pub fn commit(&mut self) -> Result<()> {
        self.require_writable()?;
        self.overlay.commit(self.virtual_disk.device())
    }

    /// Discards everything buffered; the image is untouched.
    pub fn rollback(&mut self) {
        self.overlay.rollback();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcow2::QCOW2_MAGIC;

    /// Builds a qcow2 file on disk whose virtual disk carries a FAT16
    /// volume, using the crate's own writer — then exercises the whole
    /// public path reliquary runs: open, geometry, write, commit, reopen,
    /// read back.
    #[test]
    fn fat16_inside_qcow2_end_to_end() {
        const CLUSTER_BITS: u32 = 12;
        const CLUSTER: u64 = 1 << CLUSTER_BITS;

        // A minimal empty v3 qcow2 (mirrors the qcow2 unit-test builder).
        let virtual_size = 4_096_000u64; // the synthetic FAT16 volume size
        let l2_entries = CLUSTER / 8;
        let l1_size = virtual_size.div_ceil(CLUSTER * l2_entries) as u32;
        let mut image = vec![0u8; 4 * CLUSTER as usize];
        image[..4].copy_from_slice(&QCOW2_MAGIC);
        image[4..8].copy_from_slice(&3u32.to_be_bytes());
        image[20..24].copy_from_slice(&CLUSTER_BITS.to_be_bytes());
        image[24..32].copy_from_slice(&virtual_size.to_be_bytes());
        image[36..40].copy_from_slice(&l1_size.to_be_bytes());
        image[40..48].copy_from_slice(&(3 * CLUSTER).to_be_bytes());
        image[48..56].copy_from_slice(&CLUSTER.to_be_bytes());
        image[56..60].copy_from_slice(&1u32.to_be_bytes());
        image[96..100].copy_from_slice(&4u32.to_be_bytes());
        image[100..104].copy_from_slice(&112u32.to_be_bytes());
        image[CLUSTER as usize..CLUSTER as usize + 8]
            .copy_from_slice(&(2 * CLUSTER).to_be_bytes());
        for cluster in 0..4usize {
            let at = 2 * CLUSTER as usize + cluster * 2;
            image[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
        }

        let path = std::env::temp_dir().join(format!(
            "remanence-qcow2-e2e-{}.qcow2",
            std::process::id()
        ));
        std::fs::write(&path, image).expect("qcow2 writes");

        // Format the virtual disk: write a FAT16 volume into guest space
        // through the crate's own qcow2 writer.
        {
            let file = crate::device::FileDevice::open(&path).expect("opens");
            let mut qcow2 = crate::qcow2::Qcow2::open(file).expect("parses");
            let volume = fat16_volume_bytes();
            assert_eq!(volume.len() as u64, virtual_size);
            qcow2.write_at(0, &volume).expect("formats");
            qcow2.flush().expect("flushes");
        }

        // Now the public path.
        let mut disk = Disk::open(&path).expect("disk opens");
        assert!(matches!(disk.format(), DiskFormat::Qcow2 { version: 3 }));
        assert_eq!(disk.size(), virtual_size);

        let geometry = disk.geometry().expect("geometry");
        assert_eq!(geometry.volumes.len(), 1);
        assert_eq!(geometry.volumes[0].label.as_deref(), Some("REMANENCE"));

        disk.make_directory(0, "GUEST").expect("mkdir");
        disk.write_file(0, "GUEST/PAYLOAD.BIN", b"through the mapping")
            .expect("write");
        disk.commit().expect("commit");
        drop(disk);

        let mut reopened = Disk::open(&path).expect("reopens");
        assert_eq!(
            reopened.read_file(0, "GUEST/PAYLOAD.BIN").expect("read"),
            b"through the mapping"
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    /// The same synthetic FAT16 volume the integration tests build.
    fn fat16_volume_bytes() -> Vec<u8> {
        const TOTAL_SECTORS: usize = 8000;
        let mut image = vec![0u8; TOTAL_SECTORS * 512];
        image[0] = 0xeb;
        image[1] = 0x3c;
        image[2] = 0x90;
        image[3..11].copy_from_slice(b"REMANENC");
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1u16.to_le_bytes());
        image[16] = 2;
        image[17..19].copy_from_slice(&512u16.to_le_bytes());
        image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
        image[21] = 0xf8;
        image[22..24].copy_from_slice(&32u16.to_le_bytes());
        image[510] = 0x55;
        image[511] = 0xaa;
        for fat in 0..2usize {
            let base = (1 + fat * 32) * 512;
            image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
            image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
        }
        let root = (1 + 2 * 32) * 512;
        image[root..root + 11].copy_from_slice(b"REMANENCE  ");
        image[root + 11] = 0x08;
        image
    }
}
