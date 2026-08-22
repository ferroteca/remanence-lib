// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The authored-to-recorded arc: recording a published DOS floppy layout
//! onto a blank article, after which the medium testifies for itself.
//!
//! A blank article is the manufactured disk in its sleeve — an article
//! and nothing else. Recording onto it is what `FORMAT` does to a new
//! disk: lay down the boot record with its parameter block, the FAT
//! copies with their media descriptor, and an empty root directory.
//! From then on the medium is a DOS floppy and is read as one: its
//! geometry is the layout's, its device is the drive the layout is
//! recorded for, and the FAT seam opens over it by the evidence of the
//! boot record just written.
//!
//! **The kinds are an enumerated claim** (P3), like every other creation
//! grammar: each names one published layout whole, and a layout this
//! release does not claim is refused by name. Nothing is chosen on the
//! author's behalf — there is no free-form parameter block, because a
//! partly stated layout is a classification rather than a declaration.
//!
//! **A kind declares the article it records onto.** The 1.44 MB layout
//! fits a 3.5-inch high-density disk and nothing else; recording it onto
//! a 5.25-inch article is refused by name, the check being the catalog's
//! rather than a guess at the author's intent.
//!
//! **What is laid down is exactly the layout's own state.** The boot
//! record's code bytes are zero — putting a system on the disk is `SYS`'s
//! job, not `FORMAT`'s — and the volume serial is zero, because no clock
//! is consulted and the same declaration lays down the same bytes every
//! time (P29). The OEM name is the one DOS 5 and later write.

use crate::error::{Error, Result};
use crate::io::device::Device;
use crate::model::device_type::{DeviceType, FloppyDrive};
use crate::model::geometry::RecordingGeometry;
use crate::model::media_profile::{FLEXIBLE_3_5_HD, FLEXIBLE_5_25_HD, MediaProfile};

/// One published DOS floppy layout, declared at
/// [`PartitionView::record_as`](crate::PartitionView::record_as).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Recording {
    /// The 1.2 MB layout: 80 cylinders of 2 heads at 15 sectors of 512
    /// bytes, media descriptor `0xF9`, recorded onto the 5.25-inch
    /// high-density article for the PC's 1.2 MB drive.
    Dos12,
    /// The 1.44 MB layout: 80 cylinders of 2 heads at 18 sectors of 512
    /// bytes, media descriptor `0xF0`, recorded onto the 3.5-inch
    /// high-density article for the PC's 1.44 MB drive.
    Dos144,
}

/// What one recording kind lays down: its identity, the article it
/// records onto, and the coordinates the layout records.
///
/// It is the enumerated claim read the other way round, in a shape the
/// text boundaries (C, Python) can enumerate and refuse against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingClaim {
    id: &'static str,
    name: &'static str,
    article: &'static str,
    geometry: RecordingGeometry,
}

impl RecordingClaim {
    /// The stable cross-language spelling.
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// The layout's name, fit to show a user.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The article this layout records onto, by the article catalog's
    /// stable spelling — and the only article it records onto.
    pub const fn article(&self) -> &'static str {
        self.article
    }

    /// The coordinates the layout records: what the boot record states
    /// and what the sector verbs address in afterwards.
    pub const fn geometry(&self) -> RecordingGeometry {
        self.geometry
    }
}

/// The parameter block one layout writes, in the fields DOS writes it
/// in. Every value is the published one for the layout it belongs to.
struct DosLayout {
    media_descriptor: u8,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fat_count: u8,
    root_entries: u16,
    sectors_per_fat: u16,
    geometry: RecordingGeometry,
}

impl DosLayout {
    const SECTOR: usize = 512;

    fn total_sectors(&self) -> u64 {
        self.geometry.total_sectors()
    }

    /// The boot sector, exactly as `FORMAT` lays it down on a disk it
    /// has not been asked to make bootable: a jump over a parameter
    /// block that describes the layout, zero code bytes, the signature.
    fn boot_sector(&self) -> [u8; Self::SECTOR] {
        let mut sector = [0u8; Self::SECTOR];
        // The jump DOS writes ahead of the parameter block. The bytes it
        // jumps to are zero here, which is a disk nothing boots, and
        // the jump is what lets a reader know a parameter block follows.
        sector[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
        sector[3..11].copy_from_slice(b"MSDOS5.0");
        put16(&mut sector, 11, self.geometry.sector_bytes as u16);
        sector[13] = self.sectors_per_cluster;
        put16(&mut sector, 14, self.reserved_sectors);
        sector[16] = self.fat_count;
        put16(&mut sector, 17, self.root_entries);
        put16(&mut sector, 19, self.total_sectors() as u16);
        sector[21] = self.media_descriptor;
        put16(&mut sector, 22, self.sectors_per_fat);
        put16(&mut sector, 24, self.geometry.sectors_per_track as u16);
        put16(&mut sector, 26, self.geometry.heads as u16);
        // Hidden sectors and the 32-bit total, both zero: a floppy is
        // not behind a partition table, and the 16-bit total holds the
        // count.
        // The extended boot record: drive 0, the signature that says
        // the serial, label and type fields are fields, a serial of
        // zero because no clock was consulted, the label DOS writes for
        // an unlabelled disk, and the type string it writes for
        // readers that look at it.
        sector[36] = 0x00;
        sector[38] = 0x29;
        sector[43..54].copy_from_slice(b"NO NAME    ");
        sector[54..62].copy_from_slice(b"FAT12   ");
        sector[510] = 0x55;
        sector[511] = 0xaa;
        sector
    }

    /// The first sector of a FAT copy: the media descriptor in the
    /// first entry, the end-of-chain mark in the second, and nothing
    /// allocated.
    fn fat_head(&self) -> [u8; Self::SECTOR] {
        let mut sector = [0u8; Self::SECTOR];
        sector[0] = self.media_descriptor;
        sector[1] = 0xff;
        sector[2] = 0xff;
        sector
    }

    /// Writes the layout onto `device`: the boot record, every FAT copy
    /// and the root directory. The device is the blank article's own
    /// content, which reads as zeros where nothing is written, so the
    /// zero tails of the FATs and the empty root directory are already
    /// what they should be.
    fn lay_down(&self, device: &mut dyn Device) -> Result<()> {
        device.write_at(0, &self.boot_sector())?;
        let fat_bytes = u64::from(self.sectors_per_fat) * Self::SECTOR as u64;
        let first_fat = u64::from(self.reserved_sectors) * Self::SECTOR as u64;
        for copy in 0..u64::from(self.fat_count) {
            device.write_at(first_fat + copy * fat_bytes, &self.fat_head())?;
        }
        Ok(())
    }
}

fn put16(sector: &mut [u8], at: usize, value: u16) {
    sector[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

static DOS12_LAYOUT: DosLayout = DosLayout {
    media_descriptor: 0xf9,
    sectors_per_cluster: 1,
    reserved_sectors: 1,
    fat_count: 2,
    root_entries: 224,
    sectors_per_fat: 7,
    geometry: RecordingGeometry {
        cylinders: 80,
        heads: 2,
        sectors_per_track: 15,
        sector_bytes: 512,
    },
};

static DOS144_LAYOUT: DosLayout = DosLayout {
    media_descriptor: 0xf0,
    sectors_per_cluster: 1,
    reserved_sectors: 1,
    fat_count: 2,
    root_entries: 224,
    sectors_per_fat: 9,
    geometry: RecordingGeometry {
        cylinders: 80,
        heads: 2,
        sectors_per_track: 18,
        sector_bytes: 512,
    },
};

/// Every layout this release records, and what each lays down. The
/// catalog is the claim: a kind absent from it is refused by name.
static CLAIMED: [RecordingClaim; 2] = [
    RecordingClaim {
        id: "dos-1.2",
        name: "DOS 1.2 MB floppy layout (FAT12, 80 cylinders, 2 heads, 15 sectors)",
        article: "flexible-5.25-hd",
        geometry: DOS12_LAYOUT.geometry,
    },
    RecordingClaim {
        id: "dos-1.44",
        name: "DOS 1.44 MB floppy layout (FAT12, 80 cylinders, 2 heads, 18 sectors)",
        article: "flexible-3.5-hd",
        geometry: DOS144_LAYOUT.geometry,
    },
];

impl Recording {
    /// Every layout a recording may declare, with what each lays down.
    pub fn claimed() -> &'static [RecordingClaim] {
        &CLAIMED
    }

    /// The stable cross-language spelling, which is what the C and Python
    /// surfaces carry and what a refusal quotes back.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Dos12 => "dos-1.2",
            Self::Dos144 => "dos-1.44",
        }
    }

    /// The layout's name, fit to show a user.
    pub fn name(self) -> &'static str {
        self.claim().name
    }

    /// What this kind lays down — its own entry in the catalog above.
    pub fn claim(self) -> &'static RecordingClaim {
        CLAIMED
            .iter()
            .find(|claim| claim.id == self.id())
            .expect("every variant has a catalog entry")
    }

    /// The article this layout records onto, by the article catalog's
    /// stable spelling.
    pub fn article(self) -> &'static str {
        self.article_profile().id
    }

    pub(crate) fn article_profile(self) -> &'static MediaProfile {
        match self {
            Self::Dos12 => &FLEXIBLE_5_25_HD,
            Self::Dos144 => &FLEXIBLE_3_5_HD,
        }
    }

    /// The coordinates the layout records.
    pub fn geometry(self) -> RecordingGeometry {
        self.layout().geometry
    }

    /// The drive this layout is recorded for — the device type the
    /// medium binds once the layout is on it, so that a drive takes it.
    pub fn device_type(self) -> DeviceType {
        match self {
            Self::Dos12 => DeviceType::Floppy(FloppyDrive::Pc525Hd),
            Self::Dos144 => DeviceType::Floppy(FloppyDrive::Pc35Hd),
        }
    }

    /// The media descriptor byte the layout writes into the boot record
    /// and the first FAT entry.
    pub fn media_descriptor(self) -> u8 {
        self.layout().media_descriptor
    }

    fn layout(self) -> &'static DosLayout {
        match self {
            Self::Dos12 => &DOS12_LAYOUT,
            Self::Dos144 => &DOS144_LAYOUT,
        }
    }

    /// Builds a declaration from its stable spelling, for the C and
    /// Python surfaces where one arrives as text (P5). A kind this
    /// release does not record is refused naming what it does.
    pub fn declared(kind: &str) -> Result<Self> {
        match kind {
            "dos-1.2" => Ok(Self::Dos12),
            "dos-1.44" => Ok(Self::Dos144),
            other => {
                let claimed: Vec<&str> = CLAIMED.iter().map(|claim| claim.id).collect();
                Err(Error::unsupported(format!(
                    "'{other}' names no layout this release records; the layouts it \
                     records are {}",
                    claimed.join(", ")
                )))
            }
        }
    }

    /// Lays the layout down on `device`, which is the blank article's
    /// whole content at the layout's own size.
    pub(crate) fn lay_down(self, device: &mut dyn Device) -> Result<()> {
        self.layout().lay_down(device)
    }

    /// What the layout laid down, for the medium's evidence (P4).
    pub(crate) fn describe(self) -> String {
        let layout = self.layout();
        format!(
            "{} ({}): {} — media descriptor {:#04x}, {} reserved sector(s), {} FAT \
             copies of {} sectors, a root directory of {} entries, {} sector(s) to a \
             cluster; the boot record carries the parameter block and the signature \
             with zero code bytes, and the volume serial is zero because no clock was \
             consulted",
            self.id(),
            self.name(),
            layout.geometry,
            layout.media_descriptor,
            layout.reserved_sectors,
            layout.fat_count,
            layout.sectors_per_fat,
            layout.root_entries,
            layout.sectors_per_cluster,
        )
    }
}

impl std::fmt::Display for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::fat::{FatKind, FatVolume, declared_geometry};

    /// A device that is a vector of bytes — the blank article's content
    /// at the layout's own size, reading as zeros where nothing was
    /// written.
    struct Bytes(Vec<u8>);

    impl Device for Bytes {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
            let start = offset as usize;
            buf.copy_from_slice(&self.0[start..start + buf.len()]);
            Ok(())
        }
        fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
            let start = offset as usize;
            self.0[start..start + data.len()].copy_from_slice(data);
            Ok(())
        }
        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    fn laid_down(kind: Recording) -> Bytes {
        let mut device = Bytes(vec![0u8; kind.geometry().total_bytes() as usize]);
        kind.lay_down(&mut device).expect("lays down");
        device
    }

    #[test]
    fn every_claimed_kind_round_trips_through_its_spelling() {
        for claim in Recording::claimed() {
            let kind = Recording::declared(claim.id()).expect("claimed");
            assert_eq!(kind.id(), claim.id());
            assert_eq!(kind.name(), claim.name());
            assert_eq!(kind.article(), claim.article());
            assert_eq!(kind.geometry(), claim.geometry());
            assert_eq!(kind.claim(), claim);
            assert_eq!(
                kind.device_type().article(),
                claim.article(),
                "the drive a layout is recorded for is served the article it records onto"
            );
        }
        let error = Recording::declared("dos-720k").expect_err("refused");
        let message = error.to_string();
        assert!(
            message.contains("dos-720k"),
            "names what was asked: {message}"
        );
        assert!(
            message.contains("dos-1.44"),
            "names what is recorded: {message}"
        );
    }

    #[test]
    fn each_layout_is_the_published_disk_and_reads_back_as_fat12() {
        for (kind, bytes, media, sectors_per_fat) in [
            (Recording::Dos12, 1_228_800u64, 0xf9, 7u16),
            (Recording::Dos144, 1_474_560, 0xf0, 9),
        ] {
            assert_eq!(kind.geometry().total_bytes(), bytes, "{kind}");
            assert_eq!(kind.media_descriptor(), media, "{kind}");
            let mut device = laid_down(kind);

            // The boot record is one the discovery seam calls a boot
            // record, and it states the layout's own geometry.
            let sector = device.0[..512].to_vec();
            assert!(crate::partition::mbr::looks_like_bpb(&sector), "{kind}");
            let stated = declared_geometry(&sector).expect("a parameter block");
            assert_eq!(
                stated.sectors_per_track,
                Some(kind.geometry().sectors_per_track)
            );
            assert_eq!(stated.heads, Some(kind.geometry().heads));
            assert_eq!(&sector[43..54], b"NO NAME    ");
            assert_eq!(&sector[54..62], b"FAT12   ");
            assert_eq!(&sector[62..510], &[0u8; 448][..], "the code bytes are zero");

            // Each FAT copy opens with the descriptor and the end mark.
            let fat = u64::from(sectors_per_fat) * 512;
            for copy in 0..2u64 {
                let at = (512 + copy * fat) as usize;
                assert_eq!(
                    &device.0[at..at + 3],
                    &[media, 0xff, 0xff],
                    "{kind} copy {copy}"
                );
            }

            // And the FAT seam reads it as the FAT12 volume it is, with
            // an empty root and every cluster free.
            let volume = FatVolume::open(&mut device, 0).expect("a FAT volume");
            let recognized = volume.recognized(&mut device).expect("recognized");
            assert_eq!(recognized.kind, FatKind::Fat12, "{kind}");
            assert_eq!(recognized.cylinders, Some(80), "{kind}");
            assert!(
                recognized.cluster_count < 4085,
                "{kind}: {} clusters is FAT16's range",
                recognized.cluster_count
            );
            // FAT12 needs a byte and a half per cluster, and the FAT
            // copies the layout declares have to hold every entry.
            assert!(
                (recognized.cluster_count + 2) * 3 / 2 <= fat,
                "{kind}: the FAT copies cannot hold every cluster's entry"
            );
            assert!(
                volume.entries(&mut device, &[]).expect("lists").is_empty(),
                "{kind}: the root directory is empty"
            );
        }
    }
}
