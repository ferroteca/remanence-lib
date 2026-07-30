// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! MBR partition discovery: the four primary entries and the
//! extended-partition chain, partition types pinned value by value. An
//! entry outside the pinned set is refused rather than skipped, because
//! skipping renumbers every volume behind it (pledged U4).

use crate::device::Device;
use crate::error::{Error, Result};

const SECTOR: u64 = 512;
const BOOT_SIGNATURE: [u8; 2] = [0x55, 0xaa];

/// One discovered partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionInfo {
    /// 1-based partition number in discovery order (primaries first, then
    /// logicals along the extended chain).
    pub number: u32,
    pub type_byte: u8,
    pub type_name: String,
    pub start_bytes: u64,
    pub length_bytes: u64,
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::invalid_image("mbr", reason)
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

fn is_extended(type_byte: u8) -> bool {
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

/// Discovers the partitions of `device`. Returns an empty list when the
/// device is partitionless (its first sector is a bare volume).
pub(crate) fn discover(device: &mut dyn Device) -> Result<Vec<PartitionInfo>> {
    if device.len() < SECTOR {
        return Err(invalid("device too small for a boot sector"));
    }
    let mbr = read_sector(device, 0)?;
    if mbr[510..512] != BOOT_SIGNATURE {
        return Err(invalid("missing boot signature"));
    }
    if looks_like_bpb(&mbr) {
        return Ok(Vec::new());
    }

    let mut partitions = Vec::new();
    let mut number = 0u32;
    let mut extended_chain: Option<(u64, u64)> = None; // (base, current)

    for entry in parse_entries(&mbr) {
        if entry.type_byte == 0x00 {
            continue;
        }
        let Some(type_name) = pinned_type_name(entry.type_byte) else {
            return Err(invalid(format!(
                "partition type 0x{:02x} is outside this release's claim; \
                 refusing rather than skipping (skipping would renumber \
                 every volume behind it)",
                entry.type_byte
            )));
        };
        number += 1;
        if is_extended(entry.type_byte) {
            if extended_chain.is_some() {
                return Err(invalid("more than one extended partition"));
            }
            extended_chain = Some((entry.start_lba as u64, entry.start_lba as u64));
            partitions.push(PartitionInfo {
                number,
                type_byte: entry.type_byte,
                type_name: type_name.to_owned(),
                start_bytes: entry.start_lba as u64 * SECTOR,
                length_bytes: entry.sectors as u64 * SECTOR,
            });
        } else {
            partitions.push(PartitionInfo {
                number,
                type_byte: entry.type_byte,
                type_name: type_name.to_owned(),
                start_bytes: entry.start_lba as u64 * SECTOR,
                length_bytes: entry.sectors as u64 * SECTOR,
            });
        }
    }

    // Walk the extended chain: each EBR names one logical partition
    // (relative to the EBR) and optionally the next EBR (relative to the
    // extended base).
    let mut hops = 0;
    while let Some((base, current)) = extended_chain {
        hops += 1;
        if hops > 128 {
            return Err(invalid("extended partition chain does not terminate"));
        }
        let ebr = read_sector(device, current)?;
        if ebr[510..512] != BOOT_SIGNATURE {
            return Err(invalid("extended boot record missing its signature"));
        }
        let entries = parse_entries(&ebr);

        let logical = &entries[0];
        if logical.type_byte != 0x00 {
            let Some(type_name) = pinned_type_name(logical.type_byte) else {
                return Err(invalid(format!(
                    "logical partition type 0x{:02x} is outside this release's \
                     claim; refusing rather than skipping",
                    logical.type_byte
                )));
            };
            if is_extended(logical.type_byte) {
                return Err(invalid("extended partition nested inside the chain"));
            }
            number += 1;
            partitions.push(PartitionInfo {
                number,
                type_byte: logical.type_byte,
                type_name: type_name.to_owned(),
                start_bytes: (current + logical.start_lba as u64) * SECTOR,
                length_bytes: logical.sectors as u64 * SECTOR,
            });
        }

        let next = &entries[1];
        extended_chain = if next.type_byte == 0x00 {
            None
        } else if is_extended(next.type_byte) {
            Some((base, base + next.start_lba as u64))
        } else {
            return Err(invalid(format!(
                "unexpected type 0x{:02x} in the extended chain's link slot",
                next.type_byte
            )));
        };
    }

    Ok(partitions)
}
