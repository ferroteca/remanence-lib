// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The rig disk, built rather than downloaded (D49).
//!
//! One shape is wanted of it: **two DOS primaries and an extended chain
//! of two logicals** on a single disk, every volume FAT16 and labeled.
//! That shape exercises the partition and volume seams over a table
//! richer than any single-partition image reaches, and it is wholly
//! specified — so it is written here rather than downloaded.
//!
//! [`rig_layout`](../rig_layout.rs) is what says the built disk is the
//! shape claimed. The builder lives apart from that suite deliberately:
//! a fixture and the assertions that vouch for it should not be able to
//! drift together.

use std::path::PathBuf;

/// Sectors in each volume this layout carries.
///
/// FAT16 begins above 4084 clusters, and this leaves 4403 after the
/// reserved sector, both FATs and the root directory — comfortably
/// inside FAT16 while keeping the whole disk around twelve megabytes.
const VOLUME_SECTORS: usize = 4500;
const SECTORS_PER_FAT: usize = 32;
const ROOT_ENTRIES: usize = 512;
const ROOT_SECTORS: usize = ROOT_ENTRIES * 32 / 512;
const FIRST_DATA_SECTOR: usize = 1 + 2 * SECTORS_PER_FAT + ROOT_SECTORS;

pub fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-rig-{tag}-{}-{nonce}.img",
        std::process::id()
    ))
}

pub fn write_image(tag: &str, bytes: Vec<u8>) -> PathBuf {
    let path = temp_path(tag);
    std::fs::write(&path, bytes).expect("image writes");
    path
}

/// A FAT16 volume carrying a label and whatever files are named.
///
/// `files` are `(name, extension, content)`; both name parts are
/// upper-cased into the 8.3 fields as DOS stores them.
pub fn fat16_volume(label: &str, files: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let mut image = vec![0u8; VOLUME_SECTORS * 512];

    image[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
    image[3..11].copy_from_slice(b"REMANENC");
    image[11..13].copy_from_slice(&512u16.to_le_bytes());
    image[13] = 1; // sectors per cluster
    image[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved
    image[16] = 2; // FAT count
    image[17..19].copy_from_slice(&(ROOT_ENTRIES as u16).to_le_bytes());
    image[19..21].copy_from_slice(&(VOLUME_SECTORS as u16).to_le_bytes());
    image[21] = 0xf8; // media descriptor
    image[22..24].copy_from_slice(&(SECTORS_PER_FAT as u16).to_le_bytes());
    image[24..26].copy_from_slice(&32u16.to_le_bytes()); // sectors per track
    image[26..28].copy_from_slice(&2u16.to_le_bytes()); // heads
    image[38] = 0x29; // extended boot signature: the three fields follow
    image[39..43].copy_from_slice(&0x1234_abcdu32.to_le_bytes());

    let mut stamped = [b' '; 11];
    for (at, byte) in label.bytes().take(11).enumerate() {
        stamped[at] = byte.to_ascii_uppercase();
    }
    image[43..54].copy_from_slice(&stamped);
    image[54..62].copy_from_slice(b"FAT16   ");
    image[510] = 0x55;
    image[511] = 0xaa;

    // Clusters 0 and 1 are the two reserved entries every FAT carries.
    let mut next_free = 2u16;
    let mut chains: Vec<(u16, usize)> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();

    // The volume label is a directory entry whose attribute says so.
    let mut label_entry = [0u8; 32];
    label_entry[0..11].copy_from_slice(&stamped);
    label_entry[11] = 0x08;
    directory.extend_from_slice(&label_entry);

    for (name, extension, content) in files {
        let clusters = content.len().div_ceil(512).max(1);
        let first = next_free;
        next_free += clusters as u16;
        chains.push((first, clusters));

        let mut entry = [0u8; 32];
        entry[0..11].fill(b' ');
        for (at, byte) in name.bytes().take(8).enumerate() {
            entry[at] = byte.to_ascii_uppercase();
        }
        for (at, byte) in extension.bytes().take(3).enumerate() {
            entry[8 + at] = byte.to_ascii_uppercase();
        }
        entry[11] = 0x20; // archive
        entry[22..24].copy_from_slice(&0x6000u16.to_le_bytes()); // time
        entry[24..26].copy_from_slice(&0x5a21u16.to_le_bytes()); // 2025-01-01
        entry[26..28].copy_from_slice(&first.to_le_bytes());
        entry[28..32].copy_from_slice(&(content.len() as u32).to_le_bytes());
        directory.extend_from_slice(&entry);

        for step in 0..clusters {
            let sector = FIRST_DATA_SECTOR + (first as usize - 2) + step;
            let start = sector * 512;
            let end = ((step + 1) * 512).min(content.len());
            let piece = &content[step * 512..end];
            image[start..start + piece.len()].copy_from_slice(piece);
        }
    }

    // Both FAT copies, sixteen bits per entry.
    for copy in 0..2usize {
        let base = (1 + copy * SECTORS_PER_FAT) * 512;
        image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
        image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
        for (first, clusters) in &chains {
            for step in 0..*clusters {
                let cluster = *first as usize + step;
                let last = step == clusters - 1;
                let value: u16 = if last { 0xffff } else { (cluster + 1) as u16 };
                let at = base + cluster * 2;
                image[at..at + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
    }

    let root = (1 + 2 * SECTORS_PER_FAT) * 512;
    image[root..root + directory.len()].copy_from_slice(&directory);
    image
}

/// Writes one MBR or EBR table entry in place.
fn table_entry(disk: &mut [u8], table: usize, slot: usize, kind: u8, start: u32, sectors: u32) {
    let at = table + 446 + slot * 16;
    disk[at + 4] = kind;
    disk[at + 8..at + 12].copy_from_slice(&start.to_le_bytes());
    disk[at + 12..at + 16].copy_from_slice(&sectors.to_le_bytes());
}

/// Two DOS primaries and an extended chain of two logicals.
///
/// The first primary carries `RMNMARK.TXT`, so a test that reaches a
/// volume through the partition pool can prove it reached *that* volume
/// by reading it.
pub fn synthetic_rig_disk() -> Vec<u8> {
    const GAP: usize = 2048;
    let span = VOLUME_SECTORS;
    let link_span = GAP + span;

    let primary_one = GAP;
    let primary_two = primary_one + span;
    let extended = primary_two + span;
    let second_ebr = extended + link_span;
    let total = extended + 2 * link_span;

    let mut disk = vec![0u8; total * 512];

    // The master table: two data primaries and the extended container.
    table_entry(&mut disk, 0, 0, 0x06, primary_one as u32, span as u32);
    table_entry(&mut disk, 0, 1, 0x06, primary_two as u32, span as u32);
    table_entry(
        &mut disk,
        0,
        2,
        0x05,
        extended as u32,
        (2 * link_span) as u32,
    );
    disk[510] = 0x55;
    disk[511] = 0xaa;

    // The first EBR: its logical volume, and the link to the next EBR.
    // A data entry's start is relative to its own EBR; a link entry's is
    // relative to the extended partition's base. Getting that backwards
    // is the classic way to build a chain nothing can walk.
    let ebr_one = extended * 512;
    table_entry(&mut disk, ebr_one, 0, 0x06, GAP as u32, span as u32);
    table_entry(
        &mut disk,
        ebr_one,
        1,
        0x05,
        link_span as u32,
        link_span as u32,
    );
    disk[ebr_one + 510] = 0x55;
    disk[ebr_one + 511] = 0xaa;

    // The second EBR ends the chain: a volume, and no link after it.
    let ebr_two = second_ebr * 512;
    table_entry(&mut disk, ebr_two, 0, 0x06, GAP as u32, span as u32);
    disk[ebr_two + 510] = 0x55;
    disk[ebr_two + 511] = 0xaa;

    let marker: &[u8] = b"remanence marker: the first primary\n";
    let volumes = [
        (
            primary_one,
            fat16_volume("RMNPRI1", &[("RMNMARK", "TXT", marker)]),
        ),
        (primary_two, fat16_volume("RMNPRI2", &[])),
        (extended + GAP, fat16_volume("RMNLOG1", &[])),
        (second_ebr + GAP, fat16_volume("RMNLOG2", &[])),
    ];
    for (start, bytes) in volumes {
        let at = start * 512;
        disk[at..at + bytes.len()].copy_from_slice(&bytes);
    }

    disk
}
