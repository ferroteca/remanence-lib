// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! MBR partition discovery: the four primary entries and the
//! extended-partition chain, partition types pinned value by value. The
//! report is complete (U4): an entry outside the pinned claim,
//! or a chain the walk cannot follow, stays in the report carrying a
//! structured issue instead of failing the whole disk or vanishing, so
//! the rows behind it never renumber. Blank is an answer, kept distinct
//! from an unreadable image.

use crate::error::{Error, Result};
use crate::io::device::Device;

const SECTOR: u64 = 512;
const BOOT_SIGNATURE: [u8; 2] = [0x55, 0xaa];

/// The block size this release's reading of the scheme is written
/// against, which is what its extents are numbered in.
///
/// It is stated as a geometry reading of its own rather than assumed
/// silently: every offset in this module is computed in these blocks, so
/// a medium whose load declared some other addressable unit disagrees
/// with the table it was read under, and the disagreement is reported
/// rather than resolved.
pub(crate) const TABLE_BLOCK_BYTES: u64 = SECTOR;

/// What the tuple's own fields can hold: six bits of sector number and
/// eight bits of head number, so a geometry solved out of one is bounded
/// by the form it was written in.
const SECTORS_PER_TRACK_CEILING: u32 = 0x3f;
const HEADS_CEILING: u64 = 256;

/// Where a partition row sits: an MBR slot, or the extended chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartitionKind {
    /// An MBR slot — the extended partition included.
    Primary,
    /// A row of the extended chain.
    Logical,
}

impl PartitionKind {
    /// The stable cross-language spelling of this kind.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Logical => "logical",
        }
    }
}

/// One discovered partition row. Every entry the table declares is
/// reported (U4): a row the library cannot read stays here
/// carrying its [`issue`](Self::issue) instead of vanishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartitionInfo {
    /// 1-based partition number in discovery order (primaries first, then
    /// logicals along the extended chain). A row carrying an issue keeps
    /// its number, so the rows behind it never renumber.
    pub(crate) number: u32,
    pub(crate) kind: PartitionKind,
    /// Whether the table flags this entry active — the boot flag, read as
    /// the schema records it and never derived from anything else.
    pub(crate) active: bool,
    pub(crate) type_byte: u8,
    /// The pinned type name; `None` when the type byte is outside the
    /// claim — the issue then names the refusal.
    pub(crate) type_name: Option<String>,
    pub(crate) start_bytes: u64,
    pub(crate) length_bytes: u64,
    /// The structured refusal — a stable category plus its diagnostic —
    /// that keeps this row in the report when its type is outside the
    /// claim or its volume cannot be read; `None` for a row read cleanly.
    pub(crate) issue: Option<Error>,
}

/// What sector 0 turned out to be (U4). Blank is an answer, and so is
/// content no adapter claims — each kept distinct from the other and from
/// an unreadable image, which is still a refusal from [`discover`].
#[derive(Debug)]
pub(crate) enum Discovery {
    /// An MBR partition table: the discovered rows.
    Partitioned(Vec<PartitionInfo>),
    /// A filesystem boot record: the whole device is one bare volume.
    BareVolume,
    /// Sector 0 is all zero: a blank disk with zero volumes.
    Blank,
    /// Sector 0 carries data that is none of the above. The layered
    /// report states this as an outcome and carries the evidence, while
    /// identification refuses it as it always has — which is why the
    /// reason travels with the arm rather than being reconstructed.
    UnknownNonblank { evidence: String },
}

/// Why nothing claimed a non-zero sector 0. One sentence, stated once, so
/// the layered report's evidence and identification's refusal say the
/// same thing about the same disk.
pub(crate) const UNKNOWN_NONBLANK: &str = "sector 0 carries data but no boot signature: neither a blank disk, a \
     supported filesystem boot record, nor a partition table — corruption, \
     or a format outside this release's claim";

pub(crate) fn unknown_nonblank(reason: &str) -> Error {
    invalid(reason.to_owned())
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::invalid_image("mbr", reason)
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::categorized_image(crate::ErrorCategory::Unsupported, "mbr", reason)
}

/// The pinned partition-type claim. Everything else is a named refusal.
fn pinned_type_name(type_byte: u8) -> Option<&'static str> {
    match type_byte {
        0x01 => Some("FAT12"),
        0x04 => Some("FAT16 (<32M)"),
        0x06 => Some("FAT16B"),
        0x0e => Some("FAT16B (LBA)"),
        0x05 => Some("extended"),
        0x0f => Some("extended (LBA)"),
        _ => None,
    }
}

pub(crate) fn is_extended(type_byte: u8) -> bool {
    matches!(type_byte, 0x05 | 0x0f)
}

/// What a declared type value *declares*, as a phrase that completes
/// "partition type 0xNN declares …" in a refusal a user reads.
///
/// It is present for every value, whether or not this release reads the
/// type, because the region a caller most needs explained is the one the
/// library declines to read: without it, every consumer keeps a second
/// partition-type table in order to say what was refused, which is the
/// duplication P16 puts inside the schema adapter to prevent.
///
/// It describes the declaration and never the content. An unread `0x07`
/// region is not thereby asserted to hold NTFS — only to say it does.
pub(crate) fn declared_type_reading(type_byte: u8) -> &'static str {
    match type_byte {
        0x00 => "an unused entry",
        0x01 => "FAT12",
        0x04 => "FAT16, under 32 MB",
        0x05 => "an extended partition, CHS-addressed",
        0x06 => "FAT16B",
        0x07 => "NTFS or exFAT",
        0x0b => "FAT32, CHS-addressed",
        0x0c => "FAT32, LBA-addressed",
        0x0e => "FAT16B, LBA-addressed",
        0x0f => "an extended partition, LBA-addressed",
        0x11 => "a hidden FAT12",
        0x14 => "a hidden FAT16, under 32 MB",
        0x16 => "a hidden FAT16B",
        0x17 => "a hidden NTFS or exFAT",
        0x1b => "a hidden FAT32, CHS-addressed",
        0x1c => "a hidden FAT32, LBA-addressed",
        0x1e => "a hidden FAT16B, LBA-addressed",
        0x82 => "Linux swap, or a Solaris slice",
        0x83 => "a Linux filesystem",
        0x8e => "a Linux LVM physical volume",
        0xa5 => "a FreeBSD slice",
        0xa6 => "an OpenBSD slice",
        0xa8 => "a macOS UFS volume",
        0xaf => "an HFS or HFS+ volume",
        0xee => {
            "that the whole disk is GPT rather than MBR, this entry \
                 being the protective placeholder GPT writes"
        }
        0xef => "an EFI system partition",
        0xfd => "a Linux RAID autodetect member",
        _ => "no type this release has a reading for",
    }
}

struct RawEntry {
    /// The boot flag exactly as the slot records it: `0x80` is active and
    /// every other value is not.
    active: bool,
    type_byte: u8,
    /// The last block of this entry in cylinder-head-sector form, exactly
    /// as the slot records it. A table written by a machine that
    /// addressed the drive by CHS states here what geometry it used, and
    /// that is the only place the drive's own coordinates survive.
    end_chs: [u8; 3],
    start_lba: u32,
    sectors: u32,
}

/// One entry's end tuple read as a track geometry, where the tuple and
/// the extent the same entry declares agree about which block it names.
pub(crate) struct ImpliedGeometry {
    /// The entry the tuple was read from, in the table's own numbering.
    pub(crate) entry: u32,
    pub(crate) heads: u32,
    pub(crate) sectors_per_track: u32,
    /// What was read and what it was checked against (P4).
    pub(crate) detail: String,
}

impl RawEntry {
    /// The cylinder, head and sector the end tuple names, in the packed
    /// form the slot records: the cylinder's top two bits ride in the
    /// sector byte.
    fn end_tuple(&self) -> (u32, u32, u32) {
        let cylinder = (u32::from(self.end_chs[1] & 0xc0) << 2) | u32::from(self.end_chs[2]);
        (
            cylinder,
            u32::from(self.end_chs[0]),
            u32::from(self.end_chs[1] & 0x3f),
        )
    }

    /// The track geometry this entry's end tuple implies, **solved
    /// against the extent the same entry declares**.
    ///
    /// A tuple on its own states no geometry: it names one block in
    /// coordinates, and how many heads and sectors those coordinates run
    /// to is exactly what is missing. What makes it a reading is that
    /// the same entry declares the same block a second way — as the last
    /// block of its own LBA extent — so the geometry is whatever puts
    /// the one where the other says it is. Where the two ways agree on
    /// exactly one geometry within the field widths, that is the
    /// reading; where they agree on several, or on none, the tuple
    /// states nothing and nothing is inferred from it. A drive past what
    /// CHS can address writes a saturated tuple, and this is what makes
    /// its numbers state nothing rather than a geometry nobody used.
    fn implied_geometry(&self, entry: u32) -> Option<ImpliedGeometry> {
        let (cylinder, head, sector) = self.end_tuple();
        // A cylinder of zero leaves the head count out of the arithmetic
        // altogether: every head count above the one named puts the
        // block in the same place, so the tuple determines none of them.
        if sector == 0 || self.sectors == 0 || cylinder == 0 {
            return None;
        }
        let last = u64::from(self.start_lba) + u64::from(self.sectors) - 1;
        let base = last.checked_sub(u64::from(sector - 1))?;

        let mut solved: Option<(u32, u32)> = None;
        for sectors_per_track in sector..=SECTORS_PER_TRACK_CEILING {
            if base % u64::from(sectors_per_track) != 0 {
                continue;
            }
            let track = base / u64::from(sectors_per_track);
            let Some(cylinders_worth) = track.checked_sub(u64::from(head)) else {
                continue;
            };
            if cylinders_worth % u64::from(cylinder) != 0 {
                continue;
            }
            let heads = cylinders_worth / u64::from(cylinder);
            if heads <= u64::from(head) || heads > HEADS_CEILING {
                continue;
            }
            if solved.is_some() {
                // Two geometries put the block where the extent says it
                // is, and the tuple says nothing about which was used.
                return None;
            }
            solved = Some((heads as u32, sectors_per_track));
        }

        let (heads, sectors_per_track) = solved?;
        Some(ImpliedGeometry {
            entry,
            heads,
            sectors_per_track,
            detail: format!(
                "the entry's end tuple names cylinder {cylinder}, head {head}, \
                 sector {sector}, and the extent the same entry declares ends \
                 at block {last}: {heads} heads of {sectors_per_track} sectors \
                 is the one geometry within the field widths that puts the \
                 first where the second is"
            ),
        })
    }
}

/// What the partition table's own end tuples state about the recording's
/// coordinates.
///
/// Every primary entry that carries a checkable tuple contributes one
/// reading. Two entries written under different geometries therefore
/// disagree here and settle nothing, which is the honest answer about a
/// table two machines wrote.
pub(crate) fn implied_geometry(device: &mut dyn Device) -> Vec<ImpliedGeometry> {
    if device.len() < SECTOR {
        return Vec::new();
    }
    let Ok(sector) = read_sector(device, 0) else {
        return Vec::new();
    };
    if sector[510..512] != BOOT_SIGNATURE || looks_like_bpb(&sector) {
        return Vec::new();
    }
    let mut readings = Vec::new();
    let mut number = 0;
    for entry in parse_entries(&sector) {
        if entry.type_byte == 0x00 {
            continue;
        }
        number += 1;
        if let Some(implied) = entry.implied_geometry(number) {
            readings.push(implied);
        }
    }
    readings
}

fn read_sector(device: &mut dyn Device, lba: u64) -> Result<[u8; 512]> {
    let mut sector = [0u8; 512];
    device.read_at(lba * SECTOR, &mut sector)?;
    Ok(sector)
}

fn parse_entries(sector: &[u8; 512]) -> [RawEntry; 4] {
    core::array::from_fn(|i| {
        let at = 446 + i * 16;
        RawEntry {
            active: sector[at] == 0x80,
            type_byte: sector[at + 4],
            end_chs: [sector[at + 5], sector[at + 6], sector[at + 7]],
            start_lba: u32::from_le_bytes(sector[at + 8..at + 12].try_into().unwrap()),
            sectors: u32::from_le_bytes(sector[at + 12..at + 16].try_into().unwrap()),
        }
    })
}

/// Heuristic: does this sector look like a FAT BPB rather than an MBR?
/// (A partitionless FAT image's sector 0 carries the same 55AA signature.)
pub(crate) fn looks_like_bpb(sector: &[u8]) -> bool {
    if sector.len() < 512 {
        return false;
    }
    // A plausible x86 jump at offset 0.
    let jump_ok = matches!(sector[0], 0xeb | 0xe9);
    // Bytes per sector: a power of two between 512 and 4096.
    let bytes_per_sector = u16::from_le_bytes([sector[11], sector[12]]);
    let bps_ok = bytes_per_sector.is_power_of_two() && (512..=4096).contains(&bytes_per_sector);
    // Sectors per cluster: a power of two up to 128.
    let spc_ok = sector[13].is_power_of_two();
    // At least one FAT.
    let fats_ok = (1..=2).contains(&sector[16]);
    jump_ok && bps_ok && spc_ok && fats_ok
}

/// What the leading content is where **no scheme was declared** — the
/// reading a medium recorded by a schemeless device type gets.
///
/// It answers the three no-scheme answers and never the fourth: a table
/// nobody declared is not read, because reading it would be the probe
/// the declared tier exists to keep out. Sector 0 that looks like a
/// table is therefore content nothing claims, and says so.
pub(crate) fn classify(device: &mut dyn Device) -> Result<Discovery> {
    if device.len() < SECTOR {
        return Err(invalid("device too small for a boot sector"));
    }
    let leading = read_sector(device, 0)?;
    if leading.iter().all(|&byte| byte == 0) {
        return Ok(Discovery::Blank);
    }
    if looks_like_bpb(&leading) {
        return Ok(Discovery::BareVolume);
    }
    Ok(Discovery::UnknownNonblank {
        evidence: match leading[510..512] == BOOT_SIGNATURE {
            true => UNDECLARED_TABLE.to_owned(),
            false => UNKNOWN_NONBLANK.to_owned(),
        },
    })
}

/// Why a boot signature is not a partition table here. Stated once, for
/// the medium whose device type declares no scheme: what sector 0 holds
/// is not read as a layout nobody declared.
pub(crate) const UNDECLARED_TABLE: &str = "sector 0 carries a boot signature and no filesystem boot record, and the \
     device that recorded this medium declares no partition scheme: whatever \
     the sector is, it is not read as a table nobody declared";

/// Reads sector 0 and answers what the device is (U4): a blank
/// disk, one bare volume, or an MBR with every declared row reported —
/// rows outside the pinned claim included, each carrying its issue.
/// Non-zero data that is none of these is a named refusal, kept
/// distinct from blank.
pub(crate) fn discover(device: &mut dyn Device) -> Result<Discovery> {
    if device.len() < SECTOR {
        return Err(invalid("device too small for a boot sector"));
    }
    let mbr = read_sector(device, 0)?;
    if mbr.iter().all(|&byte| byte == 0) {
        return Ok(Discovery::Blank);
    }
    if mbr[510..512] != BOOT_SIGNATURE {
        return Ok(Discovery::UnknownNonblank {
            evidence: UNKNOWN_NONBLANK.to_owned(),
        });
    }
    if looks_like_bpb(&mbr) {
        return Ok(Discovery::BareVolume);
    }

    let mut partitions = Vec::new();
    let mut number = 0u32;
    // (extended base, next EBR, extended row index)
    let mut chain: Option<(u64, u64, usize)> = None;

    for entry in parse_entries(&mbr) {
        if entry.type_byte == 0x00 {
            continue;
        }
        number += 1;
        let type_name = pinned_type_name(entry.type_byte);
        let mut issue = type_name.is_none().then(|| {
            unsupported(format!(
                "partition type 0x{:02x} is outside this release's claim; \
                 the row is reported and no volume is read from it",
                entry.type_byte
            ))
        });
        if is_extended(entry.type_byte) {
            if chain.is_some() {
                issue = Some(invalid(
                    "a second extended partition; an MBR holds at most one, \
                     and only the first chain's logical partitions are read",
                ));
            } else {
                chain = Some((
                    entry.start_lba as u64,
                    entry.start_lba as u64,
                    partitions.len(),
                ));
            }
        }
        partitions.push(PartitionInfo {
            number,
            kind: PartitionKind::Primary,
            active: entry.active,
            type_byte: entry.type_byte,
            type_name: type_name.map(str::to_owned),
            start_bytes: entry.start_lba as u64 * SECTOR,
            length_bytes: entry.sectors as u64 * SECTOR,
            issue,
        });
    }

    // Walk the extended chain: each EBR names one logical partition
    // (relative to the EBR) and optionally the next EBR (relative to the
    // extended base). A chain the walk cannot follow attaches its issue
    // to the extended row and stops: the logicals already found stay,
    // and nothing renumbers.
    let mut hops = 0;
    while let Some((base, current, extended)) = chain {
        hops += 1;
        if hops > 128 {
            partitions[extended].issue = Some(invalid(
                "extended partition chain does not terminate within 128 \
                 links; its remaining logical partitions are not read",
            ));
            break;
        }
        let ebr = match read_sector(device, current) {
            Ok(sector) => sector,
            Err(error) => {
                partitions[extended].issue = Some(invalid(format!(
                    "extended boot record at sector {current} could not be \
                     read ({error}); the chain's remaining logical \
                     partitions are not read"
                )));
                break;
            }
        };
        if ebr[510..512] != BOOT_SIGNATURE {
            partitions[extended].issue = Some(invalid(format!(
                "extended boot record at sector {current} is missing its \
                 signature; the chain's remaining logical partitions are \
                 not read"
            )));
            break;
        }
        let entries = parse_entries(&ebr);

        let logical = &entries[0];
        if logical.type_byte != 0x00 {
            number += 1;
            let type_name = pinned_type_name(logical.type_byte);
            let issue = if is_extended(logical.type_byte) {
                Some(invalid(
                    "an extended partition nested in the chain's logical \
                     slot; the row is reported and no volume is read from it",
                ))
            } else {
                type_name.is_none().then(|| {
                    unsupported(format!(
                        "logical partition type 0x{:02x} is outside this \
                         release's claim; the row is reported and no volume \
                         is read from it",
                        logical.type_byte
                    ))
                })
            };
            partitions.push(PartitionInfo {
                number,
                kind: PartitionKind::Logical,
                active: logical.active,
                type_byte: logical.type_byte,
                type_name: type_name.map(str::to_owned),
                start_bytes: (current + logical.start_lba as u64) * SECTOR,
                length_bytes: logical.sectors as u64 * SECTOR,
                issue,
            });
        }

        let next = &entries[1];
        chain = if next.type_byte == 0x00 {
            None
        } else if is_extended(next.type_byte) {
            Some((base, base + next.start_lba as u64, extended))
        } else {
            partitions[extended].issue = Some(invalid(format!(
                "type 0x{:02x} in the extended chain's link slot where an \
                 extended type or an empty entry belongs; the chain's \
                 remaining logical partitions are not read",
                next.type_byte
            )));
            None
        };
    }

    Ok(Discovery::Partitioned(partitions))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Bytes(Vec<u8>);

    impl Device for Bytes {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            let end = at + buf.len();
            if end > self.0.len() {
                return Err(Error::io("past the end"));
            }
            buf.copy_from_slice(&self.0[at..end]);
            Ok(())
        }

        fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
            unreachable!("these tests only read")
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    /// A block in the packed CHS form a slot records, under a stated
    /// geometry — what the machine that formatted the disk wrote down.
    fn chs(block: u64, heads: u32, sectors_per_track: u32) -> [u8; 3] {
        let per_cylinder = u64::from(heads) * u64::from(sectors_per_track);
        let cylinder = block / per_cylinder;
        let head = (block % per_cylinder) / u64::from(sectors_per_track);
        let sector = block % u64::from(sectors_per_track) + 1;
        [
            head as u8,
            (sector as u8 & 0x3f) | (((cylinder >> 2) as u8) & 0xc0),
            (cylinder & 0xff) as u8,
        ]
    }

    /// One primary entry, its end tuple written under `heads` and
    /// `sectors_per_track` unless `end_chs` overrides it.
    fn table(entries: &[(u8, u32, u32, [u8; 3])]) -> Bytes {
        let mut sector = vec![0u8; 512];
        for (slot, &(type_byte, start_lba, sectors, end_chs)) in entries.iter().enumerate() {
            let at = 446 + slot * 16;
            sector[at + 4] = type_byte;
            sector[at + 5..at + 8].copy_from_slice(&end_chs);
            sector[at + 8..at + 12].copy_from_slice(&start_lba.to_le_bytes());
            sector[at + 12..at + 16].copy_from_slice(&sectors.to_le_bytes());
        }
        sector[510..512].copy_from_slice(&BOOT_SIGNATURE);
        Bytes(sector)
    }

    #[test]
    fn an_end_tuple_states_a_geometry_where_it_solves_against_its_own_extent() {
        // A partition of 8,001 blocks starting at 63, written by a
        // machine addressing 16 heads of 63 sectors.
        let last = 63 + 8_001 - 1;
        let mut device = table(&[(0x06, 63, 8_001, chs(last, 16, 63))]);
        let readings = implied_geometry(&mut device);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].entry, 1);
        assert_eq!(readings[0].heads, 16);
        assert_eq!(readings[0].sectors_per_track, 63);
        assert!(
            readings[0].detail.contains("ends at block 8063"),
            "the reading states what it was solved against: {}",
            readings[0].detail
        );
    }

    #[test]
    fn an_extent_ending_mid_cylinder_still_states_the_whole_head_count() {
        // The head the last block falls on is a floor and not the count:
        // this extent ends on head 11 of 15, and taking the tuple at face
        // value would state twelve heads for a disk that had fifteen.
        // Solving it against the extent recovers what was actually used.
        let last = 8_064 + 4_032 - 1;
        let mut device = table(&[(0x06, 8_064, 4_032, chs(last, 15, 63))]);
        let readings = implied_geometry(&mut device);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].heads, 15);
        assert_eq!(readings[0].sectors_per_track, 63);
    }

    #[test]
    fn a_saturated_tuple_states_nothing_rather_than_a_geometry_nobody_wrote() {
        // The tuple a machine writes when the extent is past what CHS can
        // address: 1023/254/63, which implies 255 heads of 63 and names a
        // block nowhere near the extent's own last one.
        let mut device = table(&[(0x06, 63, 20_000_000, [0xfe, 0xff, 0xff])]);
        assert!(
            implied_geometry(&mut device).is_empty(),
            "the arithmetic does not check out, so nothing is read from it"
        );
    }

    #[test]
    fn two_entries_written_under_different_geometries_each_state_their_own() {
        // Both check out against their own extents, so both are read —
        // and the disagreement is settled by nobody, which is the
        // geometry seam's business rather than this one's.
        let first_last = 63 + 8_001 - 1;
        let second_start = 8_064u64;
        let second_last = second_start + 4_032 - 1;
        let mut device = table(&[
            (0x06, 63, 8_001, chs(first_last, 16, 63)),
            (0x06, second_start as u32, 4_032, chs(second_last, 15, 63)),
        ]);
        let readings = implied_geometry(&mut device);
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0].heads, 16);
        assert_eq!(readings[1].heads, 15);
        assert_eq!(readings[1].entry, 2, "the table's own numbering");
    }

    #[test]
    fn a_sector_that_is_no_table_states_no_geometry() {
        let mut blank = Bytes(vec![0u8; 512]);
        assert!(implied_geometry(&mut blank).is_empty());
        let mut short = Bytes(vec![0u8; 16]);
        assert!(implied_geometry(&mut short).is_empty());
    }

    /// The type-byte table is a machine-input surface where one byte flips
    /// the meaning of a partition and the library acts on that meaning, so
    /// it is pinned value by value — mapped values and unmapped
    /// neighbours alike — and grows one row at a time.
    #[test]
    fn every_declared_type_reads_as_something_quotable() {
        let pinned: &[(u8, &str)] = &[
            (0x00, "an unused entry"),
            (0x01, "FAT12"),
            (0x04, "FAT16, under 32 MB"),
            (0x05, "an extended partition, CHS-addressed"),
            (0x06, "FAT16B"),
            (0x07, "NTFS or exFAT"),
            (0x0b, "FAT32, CHS-addressed"),
            (0x0c, "FAT32, LBA-addressed"),
            (0x0e, "FAT16B, LBA-addressed"),
            (0x0f, "an extended partition, LBA-addressed"),
            (0x83, "a Linux filesystem"),
            (0xef, "an EFI system partition"),
        ];
        for &(byte, reading) in pinned {
            assert_eq!(declared_type_reading(byte), reading, "type 0x{byte:02x}");
        }
        assert!(
            declared_type_reading(0xee).contains("GPT rather than MBR"),
            "0xee must say the disk is GPT, which is the sentence that \
             turns a confusing empty result into an answer"
        );
    }

    /// Unmapped bytes still read as something a refusal can quote: the
    /// reading is unconditional, which is the whole point of it.
    #[test]
    fn an_unmapped_type_still_reads_rather_than_reading_as_nothing() {
        for byte in [0x02u8, 0x3c, 0x77, 0xda, 0xff] {
            let reading = declared_type_reading(byte);
            assert!(!reading.is_empty(), "type 0x{byte:02x} reads as nothing");
            assert_eq!(reading, "no type this release has a reading for");
        }
    }

    /// A reading is present whether or not this release reads the type,
    /// and the two questions stay separate.
    #[test]
    fn a_reading_is_not_a_claim_that_the_type_is_read() {
        assert!(
            pinned_type_name(0x07).is_none(),
            "0x07 is outside the claim"
        );
        assert_eq!(declared_type_reading(0x07), "NTFS or exFAT");
        assert!(pinned_type_name(0x06).is_some(), "0x06 is inside the claim");
        assert_eq!(declared_type_reading(0x06), "FAT16B");
    }
}
