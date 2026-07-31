// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! MBR partition discovery: the four primary entries and the
//! extended-partition chain, partition types pinned value by value. The
//! report is complete (pledged U4): an entry outside the pinned claim,
//! or a chain the walk cannot follow, stays in the report carrying a
//! structured issue instead of failing the whole disk or vanishing, so
//! the rows behind it never renumber. Blank is an answer, kept distinct
//! from an unreadable image.

use crate::device::Device;
use crate::error::{Error, Result};

const SECTOR: u64 = 512;
const BOOT_SIGNATURE: [u8; 2] = [0x55, 0xaa];

/// Where a partition row sits: an MBR slot, or the extended chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionKind {
    /// An MBR slot — the extended container included.
    Primary,
    /// A row of the extended chain.
    Logical,
}

impl PartitionKind {
    /// The stable cross-language spelling of this kind.
    pub fn name(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Logical => "logical",
        }
    }
}

/// One discovered partition row. Every entry the table declares is
/// reported (pledged U4): a row the library cannot read stays here
/// carrying its [`issue`](Self::issue) instead of vanishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionInfo {
    /// 1-based partition number in discovery order (primaries first, then
    /// logicals along the extended chain). A row carrying an issue keeps
    /// its number, so the rows behind it never renumber.
    pub number: u32,
    pub kind: PartitionKind,
    pub type_byte: u8,
    /// The pinned type name; `None` when the type byte is outside the
    /// claim — the issue then names the refusal.
    pub type_name: Option<String>,
    pub start_bytes: u64,
    pub length_bytes: u64,
    /// The structured refusal — a stable category plus its diagnostic —
    /// that keeps this row in the report when its type is outside the
    /// claim or its volume cannot be read; `None` for a row read cleanly.
    pub issue: Option<Error>,
}

/// What sector 0 turned out to be (pledged U4). Blank is an answer;
/// non-zero data that is none of these is a named refusal from
/// [`discover`], kept distinct from blank.
#[derive(Debug)]
pub(crate) enum Discovery {
    /// An MBR partition table: the discovered rows.
    Partitioned(Vec<PartitionInfo>),
    /// A filesystem boot record: the whole device is one bare volume.
    BareVolume,
    /// Sector 0 is all zero: a blank disk with zero volumes.
    Blank,
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

struct RawEntry {
    type_byte: u8,
    start_lba: u32,
    sectors: u32,
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
            type_byte: sector[at + 4],
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
    let bps_ok = bytes_per_sector.is_power_of_two()
        && (512..=4096).contains(&bytes_per_sector);
    // Sectors per cluster: a power of two up to 128.
    let spc_ok = sector[13].is_power_of_two();
    // At least one FAT.
    let fats_ok = (1..=2).contains(&sector[16]);
    jump_ok && bps_ok && spc_ok && fats_ok
}

/// Reads sector 0 and answers what the device is (pledged U4): a blank
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
        return Err(invalid(
            "sector 0 carries data but no boot signature: neither a blank \
             disk, a supported filesystem boot record, nor a partition \
             table — corruption, or a format outside this release's claim",
        ));
    }
    if looks_like_bpb(&mbr) {
        return Ok(Discovery::BareVolume);
    }

    let mut partitions = Vec::new();
    let mut number = 0u32;
    // (extended base, next EBR, container row index)
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
    // to the container row and stops: the logicals already found stay,
    // and nothing renumbers.
    let mut hops = 0;
    while let Some((base, current, container)) = chain {
        hops += 1;
        if hops > 128 {
            partitions[container].issue = Some(invalid(
                "extended partition chain does not terminate within 128 \
                 links; its remaining logical partitions are not read",
            ));
            break;
        }
        let ebr = match read_sector(device, current) {
            Ok(sector) => sector,
            Err(error) => {
                partitions[container].issue = Some(invalid(format!(
                    "extended boot record at sector {current} could not be \
                     read ({error}); the chain's remaining logical \
                     partitions are not read"
                )));
                break;
            }
        };
        if ebr[510..512] != BOOT_SIGNATURE {
            partitions[container].issue = Some(invalid(format!(
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
            Some((base, base + next.start_lba as u64, container))
        } else {
            partitions[container].issue = Some(invalid(format!(
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
