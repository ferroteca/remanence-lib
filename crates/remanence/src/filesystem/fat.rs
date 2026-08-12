// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! FAT12/FAT16/FAT16B volume access, read and write, over any byte range
//! of a [`Device`]. The width is decided by cluster count — the format's
//! own rule — and FAT32 (or anything else) is a named refusal. Semantics
//! follow the Python implementation whose behavior this absorbs.

use crate::error::{Error, ErrorCategory, Result};
use crate::filesystem::dos_name;
use crate::io::device::Device;
use crate::model::report::{LabelReading, VolumeLabel};

/// The FAT width of a recognized volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatKind {
    Fat12,
    Fat16,
}

impl FatKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fat12 => "FAT12",
            Self::Fat16 => "FAT16",
        }
    }
}

/// What a directory entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatEntryKind {
    File,
    Directory,
}

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatEntry {
    pub name: String,
    pub kind: FatEntryKind,
    pub size_bytes: u64,
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::invalid_image("fat", reason)
}

fn categorized(category: ErrorCategory, reason: impl Into<String>) -> Error {
    Error::categorized_image(category, "fat", reason)
}

fn unsupported(reason: impl Into<String>) -> Error {
    categorized(ErrorCategory::Unsupported, reason)
}

fn not_found(reason: impl Into<String>) -> Error {
    categorized(ErrorCategory::NotFound, reason)
}

fn not_directory(reason: impl Into<String>) -> Error {
    categorized(ErrorCategory::NotDirectory, reason)
}

fn is_directory(reason: impl Into<String>) -> Error {
    categorized(ErrorCategory::IsDirectory, reason)
}

fn no_space(reason: impl Into<String>) -> Error {
    categorized(ErrorCategory::NoSpace, reason)
}

fn io(reason: impl Into<String>) -> Error {
    categorized(ErrorCategory::Io, reason)
}

const RECORD: usize = 32;
const FREE: u8 = 0x00;
const DELETED: u8 = 0xe5;
const ATTR_LONG_NAME: u8 = 0x0f;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_VOLUME_ID: u8 = 0x08;

/// The extended boot record form that carries a label field. The claim is
/// this value alone (P3): 0x28 declares the shorter form, which stops at
/// the volume serial and has no label field at all.
const EXTENDED_BOOT_SIGNATURE_WITH_LABEL: u8 = 0x29;

/// The two places FAT records a label, in their stable cross-language
/// spellings.
const ROOT_DIRECTORY_ENTRY: &str = "root-directory-entry";
const BOOT_RECORD_FIELD: &str = "boot-record-field";

/// The format's own spelling of unlabeled, which DOS writes where a
/// volume has no name.
const UNLABELED: &str = "NO NAME";

/// A fixed-width name field's content: what it holds, with the format's
/// own space padding removed.
fn field_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_end().to_string()
}

struct Bpb {
    bytes_per_sector: u64,
    sectors_per_cluster: u64,
    reserved_sectors: u64,
    fat_count: u64,
    root_entries: u64,
    total_sectors: u64,
    sectors_per_fat: u64,
    // Geometry the BPB states, where it states one.
    sectors_per_track: u16,
    heads: u16,
    // The extended boot record's label field, where the format says there
    // is one at all. `None` means this volume has no such field.
    boot_label: Option<String>,
}

fn le16(bytes: &[u8], offset: usize) -> u64 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as u64
}

fn le32(bytes: &[u8], offset: usize) -> u64 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as u64
}

impl Bpb {
    fn parse(sector: &[u8]) -> Result<Self> {
        if sector.len() < 512 {
            return Err(invalid("boot sector too short"));
        }
        let bytes_per_sector = le16(sector, 11);
        if !(512..=4096).contains(&bytes_per_sector) || !bytes_per_sector.is_power_of_two() {
            return Err(invalid(format!(
                "implausible bytes-per-sector {bytes_per_sector}"
            )));
        }
        let sectors_per_cluster = sector[13] as u64;
        if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
            return Err(invalid("implausible sectors-per-cluster"));
        }
        let reserved_sectors = le16(sector, 14);
        if reserved_sectors == 0 {
            return Err(invalid("zero reserved sectors"));
        }
        let fat_count = sector[16] as u64;
        if !(1..=2).contains(&fat_count) {
            return Err(invalid(format!("implausible FAT count {fat_count}")));
        }
        let root_entries = le16(sector, 17);
        let total_sectors = match le16(sector, 19) {
            0 => le32(sector, 32),
            small => small,
        };
        let sectors_per_fat = le16(sector, 22);
        if sectors_per_fat == 0 {
            // FAT32 stores its FAT size at offset 36 instead; this is the
            // structural marker of a generation beyond the claim (P8).
            return Err(unsupported(
                "32-bit FAT size field in use: FAT32 or later, which is beyond \
                 this release's claim (FAT12/FAT16 only)",
            ));
        }
        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            fat_count,
            root_entries,
            total_sectors,
            sectors_per_fat,
            sectors_per_track: le16(sector, 24) as u16,
            heads: le16(sector, 26) as u16,
            // The label field is only a field where the format says it
            // is: it belongs to the extended boot record and exists only
            // under that structure's own signature. Where the signature
            // is absent this volume has no such field — a state distinct
            // from the field being present and blank — and reading the
            // offset regardless would manufacture a label out of whatever
            // bytes happen to sit there.
            boot_label: (sector[38] == EXTENDED_BOOT_SIGNATURE_WITH_LABEL)
                .then(|| field_text(&sector[43..54])),
        })
    }

    fn root_dir_bytes(&self) -> u64 {
        (self.root_entries * RECORD as u64).div_ceil(self.bytes_per_sector) * self.bytes_per_sector
    }

    fn fat_offset(&self) -> u64 {
        self.reserved_sectors * self.bytes_per_sector
    }

    fn root_dir_offset(&self) -> u64 {
        self.fat_offset() + self.fat_count * self.sectors_per_fat * self.bytes_per_sector
    }

    fn data_offset(&self) -> u64 {
        self.root_dir_offset() + self.root_dir_bytes()
    }

    fn cluster_bytes(&self) -> u64 {
        self.sectors_per_cluster * self.bytes_per_sector
    }

    fn cluster_count(&self) -> u64 {
        let data_sectors = self
            .total_sectors
            .saturating_sub(self.data_offset() / self.bytes_per_sector);
        data_sectors / self.sectors_per_cluster
    }

    /// Cylinders, only where the derivation is exact: the boot record
    /// states track geometry and it divides the total sector count with
    /// no remainder. Never invented (U4).
    fn cylinders(&self) -> Option<u64> {
        let track = self.sectors_per_track as u64 * self.heads as u64;
        (track != 0 && self.total_sectors % track == 0).then(|| self.total_sectors / track)
    }
}

/// What a boot record declares about the extent of the volume it
/// describes, read before anything is interpreted so the assurance gate
/// (P28) can compare the declaration with what the source holds.
pub(crate) enum VolumeDeclaration {
    /// The sector is not a FAT12/FAT16 boot record, so this seam declares
    /// nothing about the medium's size.
    Absent,
    /// The record declares `bytes`, and its own leading structures — boot
    /// sector, FAT copies and root directory — end at `metadata_end`,
    /// inside that declaration.
    Bounded {
        bytes: u64,
        metadata_end: u64,
        /// The declaration in the format's own terms, for the evidence.
        reading: String,
    },
    /// The record's declarations contradict one another, so no bound can be
    /// stated from it; `bytes` is the largest size any reading of it
    /// declares, and `detail` says what disagrees with what.
    Conflicted { bytes: u64, detail: String },
}

/// Reads what sector 0 declares about the size of the volume it describes.
///
/// This is deliberately a reading of the declaration alone, not a
/// recognition: it is asked before the medium is trusted, so it refuses to
/// answer for anything but a boot record the discovery seam would also
/// call one (the same signature and BPB plausibility `mbr` tests), and it
/// states a contradiction rather than resolving one.
pub(crate) fn declared_volume(sector: &[u8]) -> VolumeDeclaration {
    if sector.len() < 512
        || sector[510..512] != [0x55, 0xaa]
        || !crate::partition::mbr::looks_like_bpb(sector)
    {
        return VolumeDeclaration::Absent;
    }
    let Ok(bpb) = Bpb::parse(sector) else {
        return VolumeDeclaration::Absent;
    };

    // Exactly one of the two total-sector fields carries the count; a
    // record filling both with different numbers declares two sizes and
    // settles neither.
    let small = le16(sector, 19);
    let large = le32(sector, 32);
    if small != 0 && large != 0 && small != large {
        return VolumeDeclaration::Conflicted {
            bytes: small.max(large) * bpb.bytes_per_sector,
            detail: format!(
                "the boot record's two total-sector fields disagree: {small} in the \
                 16-bit field and {large} in the 32-bit one, where the format \
                 requires exactly one of them to be zero"
            ),
        };
    }

    let bytes = bpb.total_sectors * bpb.bytes_per_sector;
    let metadata_end = bpb.data_offset();
    if bytes == 0 || metadata_end > bytes {
        return VolumeDeclaration::Conflicted {
            bytes,
            detail: format!(
                "the boot record's own reserved, FAT and root-directory areas end \
                 at byte {metadata_end} of a volume it declares to be {bytes} \
                 bytes long"
            ),
        };
    }
    VolumeDeclaration::Bounded {
        bytes,
        metadata_end,
        reading: format!(
            "the boot record declares {} sectors of {} bytes: {bytes} bytes",
            bpb.total_sectors, bpb.bytes_per_sector
        ),
    }
}

/// What a boot record records about the geometry of the drive it was
/// written on, where it records anything.
///
/// The boot record is a *source* about the recording's coordinates and
/// not the answer: DOS wrote down the sectors-per-track and heads it was
/// formatting under, and both fields are zero on media formatted where
/// nothing knew them. What it never states is a cylinder count — its own
/// sector total describes the volume it sits at the front of, not the
/// drive that held it.
pub(crate) struct BootRecordGeometry {
    pub(crate) bytes_per_sector: u64,
    /// `None` where the field is zero, which is the format's own way of
    /// recording that nothing stated one.
    pub(crate) sectors_per_track: Option<u32>,
    pub(crate) heads: Option<u32>,
}

/// Reads what a boot record states about the drive's geometry, under the
/// same gate [`declared_volume`] uses: this is a reading of a
/// declaration, not a recognition, so it answers for nothing the
/// discovery seam would not also call a boot record.
pub(crate) fn declared_geometry(sector: &[u8]) -> Option<BootRecordGeometry> {
    if sector.len() < 512
        || sector[510..512] != [0x55, 0xaa]
        || !crate::partition::mbr::looks_like_bpb(sector)
    {
        return None;
    }
    let bpb = Bpb::parse(sector).ok()?;
    Some(BootRecordGeometry {
        bytes_per_sector: bpb.bytes_per_sector,
        sectors_per_track: (bpb.sectors_per_track != 0).then_some(bpb.sectors_per_track.into()),
        heads: (bpb.heads != 0).then_some(bpb.heads.into()),
    })
}

/// What recognizing FAT on one volume established (P18). These are the
/// filesystem's own declarations: the geometry here is what the boot
/// record states, and it manufactures no physical drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FatRecognition {
    pub(crate) kind: FatKind,
    pub(crate) label: VolumeLabel,
    pub(crate) cluster_bytes: u64,
    pub(crate) cluster_count: u64,
    pub(crate) sectors_per_track: Option<u16>,
    pub(crate) heads: Option<u16>,
    pub(crate) cylinders: Option<u64>,
}

/// A FAT volume over a byte range of a device.
pub(crate) struct FatVolume {
    offset: u64,
    bpb: Bpb,
    kind: FatKind,
}

/// One located directory record: its attributes, first cluster, size,
/// and where the record itself sits.
struct Located {
    attributes: u8,
    first_cluster: u64,
    size: u64,
    record_offset: u64,
}

impl FatVolume {
    /// Recognizes a FAT12/16 volume at `offset`; anything else is a named
    /// refusal.
    pub fn open(device: &mut dyn Device, offset: u64) -> Result<Self> {
        let mut sector = [0u8; 512];
        device.read_at(offset, &mut sector)?;
        let bpb = Bpb::parse(&sector)?;
        let count = bpb.cluster_count();
        // The format's own rule: the width is a function of cluster count.
        let kind = if count < 4085 {
            FatKind::Fat12
        } else if count < 65525 {
            FatKind::Fat16
        } else {
            return Err(unsupported(format!(
                "{count} clusters means FAT32 or later, which is beyond this \
                 release's claim (FAT12/FAT16 only)"
            )));
        };
        Ok(Self { offset, bpb, kind })
    }

    /// The facts recognition established, in the filesystem seam's own
    /// vocabulary — no volume identity, no partition number, no bounds
    /// belonging to the volume this was recognized on.
    pub(crate) fn recognized(&self, device: &mut dyn Device) -> Result<FatRecognition> {
        Ok(FatRecognition {
            kind: self.kind,
            label: self.label(device)?,
            cluster_bytes: self.bpb.cluster_bytes(),
            cluster_count: self.bpb.cluster_count(),
            sectors_per_track: (self.bpb.sectors_per_track != 0)
                .then_some(self.bpb.sectors_per_track),
            heads: (self.bpb.heads != 0).then_some(self.bpb.heads),
            cylinders: self.bpb.cylinders(),
        })
    }

    // FAT table access.

    fn fat_entry(&self, device: &mut dyn Device, cluster: u64) -> Result<u64> {
        let fat = self.offset + self.bpb.fat_offset();
        match self.kind {
            FatKind::Fat16 => {
                let mut raw = [0u8; 2];
                device.read_at(fat + cluster * 2, &mut raw)?;
                Ok(u16::from_le_bytes(raw) as u64)
            }
            FatKind::Fat12 => {
                let mut raw = [0u8; 2];
                device.read_at(fat + cluster * 3 / 2, &mut raw)?;
                let pair = u16::from_le_bytes(raw) as u64;
                Ok(if cluster % 2 == 0 {
                    pair & 0xfff
                } else {
                    pair >> 4
                })
            }
        }
    }

    fn set_fat_entry(&self, device: &mut dyn Device, cluster: u64, value: u64) -> Result<()> {
        // Every FAT copy is kept in step.
        for copy in 0..self.bpb.fat_count {
            let fat = self.offset
                + self.bpb.fat_offset()
                + copy * self.bpb.sectors_per_fat * self.bpb.bytes_per_sector;
            match self.kind {
                FatKind::Fat16 => {
                    device.write_at(fat + cluster * 2, &(value as u16).to_le_bytes())?;
                }
                FatKind::Fat12 => {
                    let at = fat + cluster * 3 / 2;
                    let mut raw = [0u8; 2];
                    device.read_at(at, &mut raw)?;
                    let mut pair = u16::from_le_bytes(raw) as u64;
                    if cluster % 2 == 0 {
                        pair = (pair & 0xf000) | (value & 0xfff);
                    } else {
                        pair = (pair & 0x000f) | ((value & 0xfff) << 4);
                    }
                    device.write_at(at, &(pair as u16).to_le_bytes())?;
                }
            }
        }
        Ok(())
    }

    fn is_end(&self, value: u64) -> bool {
        match self.kind {
            FatKind::Fat12 => value >= 0xff8,
            FatKind::Fat16 => value >= 0xfff8,
        }
    }

    fn end_marker(&self) -> u64 {
        match self.kind {
            FatKind::Fat12 => 0xfff,
            FatKind::Fat16 => 0xffff,
        }
    }

    fn chain(&self, device: &mut dyn Device, first: u64) -> Result<Vec<u64>> {
        let max = self.bpb.cluster_count() + 2;
        let mut clusters = Vec::new();
        let mut current = first;
        while !self.is_end(current) {
            if !(2..max).contains(&current) {
                return Err(invalid(format!("corrupt FAT chain at cluster {current}")));
            }
            clusters.push(current);
            if clusters.len() as u64 > max {
                return Err(invalid("FAT chain does not terminate"));
            }
            current = self.fat_entry(device, current)?;
        }
        Ok(clusters)
    }

    fn cluster_offset(&self, cluster: u64) -> u64 {
        self.offset + self.bpb.data_offset() + (cluster - 2) * self.bpb.cluster_bytes()
    }

    // Directory records.

    fn root_records(&self) -> (u64, u64) {
        (
            self.offset + self.bpb.root_dir_offset(),
            self.bpb.root_entries * RECORD as u64,
        )
    }

    fn read_records(
        &self,
        device: &mut dyn Device,
        first_cluster: u64,
    ) -> Result<Vec<(u64, [u8; RECORD])>> {
        let mut records = Vec::new();
        let mut push_area = |device: &mut dyn Device, offset: u64, len: u64| -> Result<()> {
            let mut area = vec![0u8; len as usize];
            device.read_at(offset, &mut area)?;
            for (i, chunk) in area.chunks_exact(RECORD).enumerate() {
                records.push((
                    offset + (i * RECORD) as u64,
                    chunk.try_into().expect("record size"),
                ));
            }
            Ok(())
        };
        if first_cluster == 0 {
            let (offset, len) = self.root_records();
            push_area(device, offset, len)?;
        } else {
            for cluster in self.chain(device, first_cluster)? {
                push_area(
                    device,
                    self.cluster_offset(cluster),
                    self.bpb.cluster_bytes(),
                )?;
            }
        }
        Ok(records)
    }

    fn entries_of(
        &self,
        device: &mut dyn Device,
        first_cluster: u64,
    ) -> Result<Vec<(FatEntry, Located)>> {
        let mut out = Vec::new();
        for (record_offset, record) in self.read_records(device, first_cluster)? {
            if record[0] == FREE {
                break;
            }
            if record[0] == DELETED {
                continue;
            }
            let attributes = record[11];
            if attributes & ATTR_LONG_NAME == ATTR_LONG_NAME || attributes & ATTR_VOLUME_ID != 0 {
                continue;
            }
            let name = dos_name::read(&record);
            if name == "." || name == ".." {
                continue;
            }
            let first = le16(&record, 26);
            let size = le32(&record, 28);
            out.push((
                FatEntry {
                    name,
                    kind: if attributes & ATTR_DIRECTORY != 0 {
                        FatEntryKind::Directory
                    } else {
                        FatEntryKind::File
                    },
                    size_bytes: size,
                },
                Located {
                    attributes,
                    first_cluster: first,
                    size,
                    record_offset,
                },
            ));
        }
        Ok(out)
    }

    /// The root directory's volume-ID entry as it stands, or `None` where
    /// the directory holds no such entry. An entry that exists and is
    /// blank reads as `Some("")`: it is present, and being present is
    /// what decides which source answers.
    fn root_directory_label(&self, device: &mut dyn Device) -> Result<Option<String>> {
        for (_, record) in self.read_records(device, 0)? {
            if record[0] == FREE {
                break;
            }
            if record[0] == DELETED {
                continue;
            }
            let attributes = record[11];
            if attributes & ATTR_LONG_NAME != ATTR_LONG_NAME && attributes & ATTR_VOLUME_ID != 0 {
                return Ok(Some(field_text(&record[..11])));
            }
        }
        Ok(None)
    }

    /// This volume's label, answered whole: the label, or the fact that
    /// the volume has none.
    ///
    /// FAT records a label in two places and a volume may carry either,
    /// both, or disagreeing values, so choosing between them is a policy
    /// about FAT and this seam owns it (P18). The root directory's
    /// volume-ID entry is the label DOS itself displays and answers
    /// wherever it exists; the boot record's field answers where it does
    /// not; and `NO NAME` at either source is the format's own spelling
    /// of unlabeled, decided here rather than by a string comparison in
    /// every consumer that displays a drive. Both readings come back
    /// beside the answer as evidence (P4).
    pub(crate) fn label(&self, device: &mut dyn Device) -> Result<VolumeLabel> {
        let root = self.root_directory_label(device)?;
        let boot = self.bpb.boot_label.clone();
        let named =
            |stored: &str| (!stored.is_empty() && stored != UNLABELED).then(|| stored.to_owned());
        let (name, answered_by) = match (&root, &boot) {
            (Some(stored), _) => (named(stored), Some(ROOT_DIRECTORY_ENTRY.to_owned())),
            (None, Some(stored)) => (named(stored), Some(BOOT_RECORD_FIELD.to_owned())),
            (None, None) => (None, None),
        };
        Ok(VolumeLabel {
            name,
            answered_by,
            readings: vec![
                LabelReading {
                    source: ROOT_DIRECTORY_ENTRY.to_owned(),
                    stored: root,
                },
                LabelReading {
                    source: BOOT_RECORD_FIELD.to_owned(),
                    stored: boot,
                },
            ],
        })
    }

    /// Walks `segments` to the directory holding the last segment,
    /// returning that directory's first cluster (0 = root) and the leaf
    /// name.
    fn walk_to_parent<'s>(
        &self,
        device: &mut dyn Device,
        segments: &'s [&str],
    ) -> Result<(u64, &'s str)> {
        let Some((leaf, dirs)) = segments.split_last() else {
            return Err(io("empty path"));
        };
        let mut current = 0u64;
        for dir in dirs {
            let entries = self.entries_of(device, current)?;
            let found = entries
                .iter()
                .find(|(entry, _)| dos_name::matches(&entry.name, dir))
                .ok_or_else(|| not_found(format!("directory '{dir}' not found")))?;
            if found.0.kind != FatEntryKind::Directory {
                return Err(not_directory(format!("'{dir}' is not a directory")));
            }
            current = found.1.first_cluster;
        }
        Ok((current, leaf))
    }

    fn locate(&self, device: &mut dyn Device, segments: &[&str]) -> Result<Located> {
        let (parent, leaf) = self.walk_to_parent(device, segments)?;
        self.entries_of(device, parent)?
            .into_iter()
            .find(|(entry, _)| dos_name::matches(&entry.name, leaf))
            .map(|(_, located)| located)
            .ok_or_else(|| not_found(format!("'{leaf}' not found")))
    }

    /// Lists a directory ("" or "A/B" style path segments; empty = root).
    pub fn entries(&self, device: &mut dyn Device, segments: &[&str]) -> Result<Vec<FatEntry>> {
        let first_cluster = if segments.is_empty() {
            0
        } else {
            let located = self.locate(device, segments)?;
            if located.attributes & ATTR_DIRECTORY == 0 {
                return Err(not_directory(format!(
                    "'{}' is not a directory",
                    segments.join("/")
                )));
            }
            located.first_cluster
        };
        Ok(self
            .entries_of(device, first_cluster)?
            .into_iter()
            .map(|(entry, _)| entry)
            .collect())
    }

    /// Answers one path with its entry, or `None` when nothing exists at
    /// that path — a missing leaf, a missing parent, or a parent that is
    /// a file all mean the path names nothing, an answer distinguished
    /// from failure to read the volume.
    pub fn stat(&self, device: &mut dyn Device, segments: &[&str]) -> Result<Option<FatEntry>> {
        let Some((leaf, dirs)) = segments.split_last() else {
            return Err(io("a path is required"));
        };
        let mut current = 0u64;
        for dir in dirs {
            let found = self
                .entries_of(device, current)?
                .into_iter()
                .find(|(entry, _)| dos_name::matches(&entry.name, dir));
            match found {
                Some((entry, located)) if entry.kind == FatEntryKind::Directory => {
                    current = located.first_cluster;
                }
                _ => return Ok(None),
            }
        }
        Ok(self
            .entries_of(device, current)?
            .into_iter()
            .find(|(entry, _)| dos_name::matches(&entry.name, leaf))
            .map(|(entry, _)| entry))
    }

    /// The last byte, in the device's own addressing, that reading this
    /// entry whole would touch: the directory record it was located
    /// through, and every cluster its chain visits up to its recorded
    /// size.
    ///
    /// A degraded session (P28) compares this with its readable extent, so
    /// an entry whose chain leaves that extent is refused whole rather than
    /// answered in part. It walks exactly what a read walks, so the two can
    /// never disagree about what a file needs.
    pub fn extent_end(&self, device: &mut dyn Device, segments: &[&str]) -> Result<u64> {
        let located = self.locate(device, segments)?;
        let mut end = located.record_offset + RECORD as u64;
        if located.size > 0 && located.first_cluster >= 2 {
            let cluster_bytes = self.bpb.cluster_bytes();
            let mut remaining = located.size;
            for cluster in self.chain(device, located.first_cluster)? {
                if remaining == 0 {
                    break;
                }
                end = end.max(self.cluster_offset(cluster) + cluster_bytes.min(remaining));
                remaining = remaining.saturating_sub(cluster_bytes);
            }
        }
        Ok(end)
    }

    /// Reads a file's bytes out.
    pub fn read_file(&self, device: &mut dyn Device, segments: &[&str]) -> Result<Vec<u8>> {
        let located = self.locate(device, segments)?;
        if located.attributes & ATTR_DIRECTORY != 0 {
            return Err(is_directory(format!(
                "'{}' is a directory",
                segments.join("/")
            )));
        }
        let mut out = Vec::with_capacity(located.size as usize);
        if located.size > 0 && located.first_cluster >= 2 {
            for cluster in self.chain(device, located.first_cluster)? {
                let take =
                    (self.bpb.cluster_bytes() as usize).min(located.size as usize - out.len());
                let mut chunk = vec![0u8; take];
                device.read_at(self.cluster_offset(cluster), &mut chunk)?;
                out.extend_from_slice(&chunk);
                if out.len() >= located.size as usize {
                    break;
                }
            }
        }
        if out.len() < located.size as usize {
            return Err(invalid(format!(
                "FAT chain shorter than the recorded size of '{}'",
                segments.join("/")
            )));
        }
        Ok(out)
    }

    /// Reads part of a file — the streamed form (P27): exactly `buf`
    /// bytes at `offset`, which must lie wholly within the file's
    /// recorded size.
    pub fn read_file_at(
        &self,
        device: &mut dyn Device,
        segments: &[&str],
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        let located = self.locate(device, segments)?;
        if located.attributes & ATTR_DIRECTORY != 0 {
            return Err(is_directory(format!(
                "'{}' is a directory",
                segments.join("/")
            )));
        }
        if offset + buf.len() as u64 > located.size {
            return Err(io(format!(
                "read past end of '{}' (offset {offset}, length {}, size {})",
                segments.join("/"),
                buf.len(),
                located.size
            )));
        }
        if buf.is_empty() {
            return Ok(());
        }
        let cluster_bytes = self.bpb.cluster_bytes();
        let chain = self.chain(device, located.first_cluster)?;
        let end = offset + buf.len() as u64;
        let first_index = (offset / cluster_bytes) as usize;
        let last_index = ((end - 1) / cluster_bytes) as usize;
        if last_index >= chain.len() {
            return Err(invalid(format!(
                "FAT chain shorter than the recorded size of '{}'",
                segments.join("/")
            )));
        }
        for (index, &cluster) in chain[first_index..=last_index].iter().enumerate() {
            let cluster_start = (first_index + index) as u64 * cluster_bytes;
            let from = cluster_start.max(offset);
            let to = (cluster_start + cluster_bytes).min(end);
            device.read_at(
                self.cluster_offset(cluster) + (from - cluster_start),
                &mut buf[(from - offset) as usize..(to - offset) as usize],
            )?;
        }
        Ok(())
    }

    /// Writes part of a file in place — the streamed form (P27): the
    /// span must lie wholly within the file's recorded size (resize
    /// first to change it). The record's timestamp advances; nothing
    /// else about the entry moves.
    pub fn write_file_at(
        &self,
        device: &mut dyn Device,
        segments: &[&str],
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        let located = self.locate(device, segments)?;
        if located.attributes & ATTR_DIRECTORY != 0 {
            return Err(is_directory(format!(
                "'{}' is a directory",
                segments.join("/")
            )));
        }
        if offset + data.len() as u64 > located.size {
            return Err(io(format!(
                "write past end of '{}' (offset {offset}, length {}, size {}); \
                 resize the file first",
                segments.join("/"),
                data.len(),
                located.size
            )));
        }
        if data.is_empty() {
            return Ok(());
        }
        let cluster_bytes = self.bpb.cluster_bytes();
        let chain = self.chain(device, located.first_cluster)?;
        let end = offset + data.len() as u64;
        let first_index = (offset / cluster_bytes) as usize;
        let last_index = ((end - 1) / cluster_bytes) as usize;
        if last_index >= chain.len() {
            return Err(invalid(format!(
                "FAT chain shorter than the recorded size of '{}'",
                segments.join("/")
            )));
        }
        for (index, &cluster) in chain[first_index..=last_index].iter().enumerate() {
            let cluster_start = (first_index + index) as u64 * cluster_bytes;
            let from = cluster_start.max(offset);
            let to = (cluster_start + cluster_bytes).min(end);
            device.write_at(
                self.cluster_offset(cluster) + (from - cluster_start),
                &data[(from - offset) as usize..(to - offset) as usize],
            )?;
        }
        let (date, time) = Self::timestamp();
        device.write_at(located.record_offset + 22, &time.to_le_bytes())?;
        device.write_at(located.record_offset + 24, &date.to_le_bytes())
    }

    /// Sets a file's size, creating it when absent: bytes within the
    /// kept prefix are preserved in place, a grown region reads as
    /// zeros, and a shrunk chain's surplus clusters are released with
    /// every FAT copy kept in step. P6: everything validated before the
    /// first mutation.
    pub fn resize_file(&self, device: &mut dyn Device, segments: &[&str], size: u64) -> Result<()> {
        let (parent, leaf) = self.walk_to_parent(device, segments)?;
        let raw_name = dos_name::store(leaf)?;
        let existing = self
            .entries_of(device, parent)?
            .into_iter()
            .find(|(entry, _)| dos_name::matches(&entry.name, leaf));
        let (old_chain, old_size, slot) = match &existing {
            Some((entry, located)) => {
                if entry.kind == FatEntryKind::Directory {
                    return Err(is_directory(format!("'{leaf}' is a directory")));
                }
                let chain = if located.first_cluster >= 2 {
                    self.chain(device, located.first_cluster)?
                } else {
                    Vec::new()
                };
                (chain, located.size, Some(located.record_offset))
            }
            None => (Vec::new(), 0, self.find_free_record(device, parent)?),
        };
        let growth = u64::from(slot.is_none());
        if growth != 0 && parent == 0 {
            return Err(no_space("root directory is full"));
        }
        let cluster_bytes = self.bpb.cluster_bytes();
        let needed = size.div_ceil(cluster_bytes);
        let kept = (old_chain.len() as u64).min(needed) as usize;
        if needed > kept as u64 {
            let more = needed - kept as u64 + growth;
            let free = self.free_cluster_count(device, more)?;
            if free < more {
                return Err(no_space(format!(
                    "volume full: {more} clusters needed, {free} free"
                )));
            }
        }

        // All checks passed; now mutate (into the session cache above).
        for &cluster in &old_chain[kept..] {
            self.set_fat_entry(device, cluster, 0)?;
        }
        let slot = match slot {
            Some(offset) => offset,
            None => self.grow_directory(device, parent)?,
        };
        let mut chain: Vec<u64> = old_chain[..kept].to_vec();
        if (chain.len() as u64) < needed {
            let new = self.claim_clusters(device, (needed - chain.len() as u64) as usize)?;
            let zeroes = vec![0u8; cluster_bytes as usize];
            for &cluster in &new {
                device.write_at(self.cluster_offset(cluster), &zeroes)?;
            }
            chain.extend_from_slice(&new);
            // A grown file must not resurrect stale bytes between the
            // old size and its cluster's end.
            if old_size % cluster_bytes != 0 && kept > 0 {
                let tail = old_size % cluster_bytes;
                let scrub = vec![0u8; (cluster_bytes - tail) as usize];
                device.write_at(self.cluster_offset(chain[kept - 1]) + tail, &scrub)?;
            }
        }
        for (i, &cluster) in chain.iter().enumerate() {
            let next = chain
                .get(i + 1)
                .copied()
                .unwrap_or_else(|| self.end_marker());
            self.set_fat_entry(device, cluster, next)?;
        }
        let first = chain.first().copied().unwrap_or(0);
        self.write_record(device, slot, raw_name, 0x20, first, size)
    }

    // Write half (P6: validation precedes the first mutating write; the
    // commit-point overlay above this makes it atomic besides).

    fn claim_clusters(&self, device: &mut dyn Device, count: usize) -> Result<Vec<u64>> {
        let mut free = Vec::with_capacity(count);
        let max = self.bpb.cluster_count() + 2;
        let mut cluster = 2;
        while free.len() < count && cluster < max {
            if self.fat_entry(device, cluster)? == 0 {
                free.push(cluster);
            }
            cluster += 1;
        }
        if free.len() < count {
            return Err(no_space(format!(
                "volume full: {count} clusters needed, {} free",
                free.len()
            )));
        }
        Ok(free)
    }

    /// Counts free clusters, stopping once `cap` are found.
    fn free_cluster_count(&self, device: &mut dyn Device, cap: u64) -> Result<u64> {
        let max = self.bpb.cluster_count() + 2;
        let mut found = 0u64;
        let mut cluster = 2;
        while found < cap && cluster < max {
            if self.fat_entry(device, cluster)? == 0 {
                found += 1;
            }
            cluster += 1;
        }
        Ok(found)
    }

    fn timestamp() -> (u16, u16) {
        // Seconds since the epoch, folded into FAT date/time fields
        // without a timezone database: days since 1980-01-01 rendered
        // through a simple civil-date conversion, UTC.
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let days_since_epoch = seconds / 86_400;
        let day_seconds = seconds % 86_400;
        // Civil-from-days (Howard Hinnant's algorithm), era-based.
        let z = days_since_epoch as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let year = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if month <= 2 { year + 1 } else { year };
        let fat_year = (year - 1980).clamp(0, 127) as u16;
        let date = (fat_year << 9) | ((month as u16) << 5) | day as u16;
        let time = ((day_seconds / 3600) as u16) << 11
            | (((day_seconds % 3600) / 60) as u16) << 5
            | ((day_seconds % 60) / 2) as u16;
        (date, time)
    }

    /// Finds a free directory record slot without mutating; `None` means
    /// the directory is full and must grow to take another record.
    fn find_free_record(
        &self,
        device: &mut dyn Device,
        parent_cluster: u64,
    ) -> Result<Option<u64>> {
        for (record_offset, record) in self.read_records(device, parent_cluster)? {
            if record[0] == FREE || record[0] == DELETED {
                return Ok(Some(record_offset));
            }
        }
        Ok(None)
    }

    /// Grows a (non-root) directory by one cluster and returns the offset
    /// of the new cluster's first record. Mutating: claim its cluster
    /// only after every validation has passed (P6).
    fn grow_directory(&self, device: &mut dyn Device, parent_cluster: u64) -> Result<u64> {
        let new = self.claim_clusters(device, 1)?[0];
        let chain = self.chain(device, parent_cluster)?;
        let last = *chain
            .last()
            .ok_or_else(|| invalid("empty directory chain"))?;
        self.set_fat_entry(device, last, new)?;
        self.set_fat_entry(device, new, self.end_marker())?;
        let zeroes = vec![0u8; self.bpb.cluster_bytes() as usize];
        device.write_at(self.cluster_offset(new), &zeroes)?;
        Ok(self.cluster_offset(new))
    }

    fn write_record(
        &self,
        device: &mut dyn Device,
        record_offset: u64,
        raw_name: [u8; 11],
        attributes: u8,
        first_cluster: u64,
        size: u64,
    ) -> Result<()> {
        let (date, time) = Self::timestamp();
        let mut record = [0u8; RECORD];
        record[..11].copy_from_slice(&raw_name);
        record[11] = attributes;
        record[22..24].copy_from_slice(&time.to_le_bytes());
        record[24..26].copy_from_slice(&date.to_le_bytes());
        record[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
        record[28..32].copy_from_slice(&(size as u32).to_le_bytes());
        device.write_at(record_offset, &record)
    }

    /// Writes a file. An existing file is overwritten — shorter or
    /// longer — its old clusters released and the new contents' claimed,
    /// every FAT copy kept in step; an existing directory is refused.
    /// P6: everything validated before the first mutation, the released
    /// clusters counted toward the space the new contents need.
    pub fn write_file(
        &self,
        device: &mut dyn Device,
        segments: &[&str],
        contents: &[u8],
    ) -> Result<()> {
        let (parent, leaf) = self.walk_to_parent(device, segments)?;
        let raw_name = dos_name::store(leaf)?;
        let existing = self
            .entries_of(device, parent)?
            .into_iter()
            .find(|(entry, _)| dos_name::matches(&entry.name, leaf));
        let old_chain = match &existing {
            Some((entry, located)) => {
                if entry.kind == FatEntryKind::Directory {
                    return Err(is_directory(format!("'{leaf}' is a directory")));
                }
                if located.first_cluster >= 2 {
                    self.chain(device, located.first_cluster)?
                } else {
                    Vec::new()
                }
            }
            None => Vec::new(),
        };
        // The record slot: the overwritten file's own, or a free one in
        // the parent — absent both, the parent must grow by one cluster.
        let slot = match &existing {
            Some((_, located)) => Some(located.record_offset),
            None => self.find_free_record(device, parent)?,
        };
        let growth = u64::from(slot.is_none());
        if growth != 0 && parent == 0 {
            return Err(no_space("root directory is full"));
        }
        let cluster_bytes = self.bpb.cluster_bytes() as usize;
        let needed = contents.len().div_ceil(cluster_bytes) as u64;
        let available = self.free_cluster_count(device, needed + growth)? + old_chain.len() as u64;
        if available < needed + growth {
            return Err(no_space(format!(
                "volume full: {} clusters needed, {available} available",
                needed + growth
            )));
        }

        // All checks passed; now mutate (into the overlay above us).
        for &cluster in &old_chain {
            self.set_fat_entry(device, cluster, 0)?;
        }
        let slot = match slot {
            Some(offset) => offset,
            None => self.grow_directory(device, parent)?,
        };
        let clusters = self.claim_clusters(device, needed as usize)?;
        for (i, &cluster) in clusters.iter().enumerate() {
            let next = clusters
                .get(i + 1)
                .copied()
                .unwrap_or_else(|| self.end_marker());
            self.set_fat_entry(device, cluster, next)?;
            let start = i * cluster_bytes;
            let chunk = &contents[start..(start + cluster_bytes).min(contents.len())];
            let mut padded = vec![0u8; cluster_bytes];
            padded[..chunk.len()].copy_from_slice(chunk);
            device.write_at(self.cluster_offset(cluster), &padded)?;
        }
        let first = clusters.first().copied().unwrap_or(0);
        self.write_record(device, slot, raw_name, 0x20, first, contents.len() as u64)
    }

    /// Creates one directory: claims its cluster, writes its "." and
    /// ".." records, and records it at `slot` in the parent. Returns the
    /// new directory's first cluster. Mutating.
    fn create_directory(
        &self,
        device: &mut dyn Device,
        parent: u64,
        slot: u64,
        raw_name: [u8; 11],
    ) -> Result<u64> {
        let cluster = self.claim_clusters(device, 1)?[0];
        self.set_fat_entry(device, cluster, self.end_marker())?;
        let zeroes = vec![0u8; self.bpb.cluster_bytes() as usize];
        let offset = self.cluster_offset(cluster);
        device.write_at(offset, &zeroes)?;
        let mut dot = [b' '; 11];
        dot[0] = b'.';
        self.write_record(device, offset, dot, ATTR_DIRECTORY, cluster, 0)?;
        let mut dotdot = [b' '; 11];
        dotdot[0] = b'.';
        dotdot[1] = b'.';
        self.write_record(
            device,
            offset + RECORD as u64,
            dotdot,
            ATTR_DIRECTORY,
            parent,
            0,
        )?;
        self.write_record(device, slot, raw_name, ATTR_DIRECTORY, cluster, 0)?;
        Ok(cluster)
    }

    /// Ensures a directory path exists: missing parents are created, and
    /// a path that already leads to a directory — the root included —
    /// succeeds unchanged. A segment that exists as a file is refused.
    /// P6-ordered like `write_file`: every name to create, the record
    /// slot, and the cluster budget are validated before the first
    /// mutation.
    pub fn make_directory(&self, device: &mut dyn Device, segments: &[&str]) -> Result<()> {
        // Walk the prefix that already exists.
        let mut current = 0u64;
        let mut index = 0;
        while index < segments.len() {
            let name = segments[index];
            let found = self
                .entries_of(device, current)?
                .into_iter()
                .find(|(entry, _)| dos_name::matches(&entry.name, name));
            match found {
                Some((entry, located)) => {
                    if entry.kind != FatEntryKind::Directory {
                        return Err(not_directory(format!(
                            "'{name}' exists and is not a directory"
                        )));
                    }
                    current = located.first_cluster;
                    index += 1;
                }
                None => break,
            }
        }
        let missing = &segments[index..];
        if missing.is_empty() {
            return Ok(());
        }
        let raw_names = missing
            .iter()
            .map(|name| dos_name::store(name))
            .collect::<Result<Vec<_>>>()?;
        let slot = self.find_free_record(device, current)?;
        let growth = u64::from(slot.is_none());
        if growth != 0 && current == 0 {
            return Err(no_space("root directory is full"));
        }
        // One cluster per directory created, plus one if the existing
        // parent must grow to take the first record; a freshly created
        // directory always has room for its child's record.
        let needed = missing.len() as u64 + growth;
        let free = self.free_cluster_count(device, needed)?;
        if free < needed {
            return Err(no_space(format!(
                "volume full: {needed} clusters needed, {free} free"
            )));
        }

        // All checks passed; now mutate (into the overlay above us).
        for (i, raw_name) in raw_names.iter().enumerate() {
            let slot = if i == 0 {
                match slot {
                    Some(offset) => offset,
                    None => self.grow_directory(device, current)?,
                }
            } else {
                // A freshly created directory: "." and ".." fill its
                // first two records, and the third is free.
                self.cluster_offset(current) + 2 * RECORD as u64
            };
            current = self.create_directory(device, current, slot, *raw_name)?;
        }
        Ok(())
    }
}
