// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! What the drive-letter suites ask of a machine's report.
//!
//! `dos_drive_letters.rs` builds its disks and runs by default;
//! `machine_report.rs` walks the whole journey. The questions they ask of
//! a `MachineReport` are the same, so they are asked from one place.
//!
//! **The letters come from a machine, never from an assertion.** A disk
//! that is to be lettered carries the DOS that letters it, because that
//! is where the rule is read from — [`dos_volume`] is what puts one
//! there.

// Each suite uses a different part of this module, and an unused
// re-export is not a finding.
#![allow(dead_code, unused_imports)]

use std::path::PathBuf;

use remanence::{
    DiskReport, Format, HardDrive, LetterOutcome, MachineReport, MediaId, Session, VolumeId,
};

#[path = "../common/mod.rs"]
mod common;
pub use common::{ensure_fixture, open_read};

pub fn attach(path: impl AsRef<std::path::Path>, format: Format) -> (Session, MediaId) {
    let mut session = Session::new();
    let id = session
        .load_media(open_read(path), format)
        .expect("the image loads")
        .id();
    (session, id)
}

pub fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-letters-{tag}-{}-{nonce}.img",
        std::process::id()
    ))
}

/// Inspects an image and hands back the report, keeping the session alive
/// for as long as the caller holds it.
pub fn inspect(path: &PathBuf, format: Format) -> (Session, DiskReport) {
    let (mut session, attachment) = attach(path, format);
    let report = session
        .medium_mut(attachment)
        .expect("the medium is pooled")
        .inspect()
        .expect("inspection reads");
    (session, report)
}

pub fn volume_at(report: &MachineReport, letter: char) -> VolumeId {
    match &report
        .letter(letter)
        .unwrap_or_else(|| panic!("{letter}: is mapped"))
        .outcome
    {
        LetterOutcome::Volume { volume, .. } => *volume,
        other => panic!("{letter}: names a volume, not {}", other.name()),
    }
}

/// Which drive the volume at `letter` sits on, by its attachment identity.
pub fn attachment_at(report: &MachineReport, letter: char) -> String {
    match &report
        .letter(letter)
        .unwrap_or_else(|| panic!("{letter}: is mapped"))
        .outcome
    {
        LetterOutcome::Volume { attachment, .. } => attachment.clone(),
        other => panic!("{letter}: names a drive, not {}", other.name()),
    }
}

pub fn reason_at(report: &MachineReport, letter: char) -> String {
    match &report
        .letter(letter)
        .unwrap_or_else(|| panic!("{letter}: is mapped"))
        .outcome
    {
        LetterOutcome::Undetermined { reason } => reason.clone(),
        other => panic!("{letter}: is undetermined, not {}", other.name()),
    }
}

/// A COMMAND.COM whose banner states `version`, which is the source the
/// installed DOS's assignment rule is settled from.
pub fn command_com(version: &str) -> Vec<u8> {
    format!("\x00\x00MS-DOS Version {version}\r\n$\x00\x00").into_bytes()
}

/// A FAT16 volume with MS-DOS installed on it — the kernel files that
/// recognize it, a shell whose banner states `version`, and whatever
/// `CONFIG.SYS` the test wants the machine to have declared.
pub fn dos_volume(label: &str, version: &str, config_sys: &str) -> Vec<u8> {
    let banner = command_com(version);
    let mut files: Vec<(&str, &str, &[u8])> = vec![
        ("IO", "SYS", b"kernel"),
        ("MSDOS", "SYS", b"kernel"),
        ("COMMAND", "COM", &banner),
    ];
    if !config_sys.is_empty() {
        files.push(("CONFIG", "SYS", config_sys.as_bytes()));
    }
    fat16_volume(label, &files)
}

/// Builds a machine holding `disks` in order, and reads it. Every disk is
/// a hard disk, in the order given, which is the attachment order the
/// letter rules reason over.
pub fn machine_of(disks: &[PathBuf]) -> (Session, MachineReport) {
    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    for path in disks {
        seat(&mut session, Some("pc"), path);
    }
    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");
    (session, report)
}

/// Pools an image and seats it in a fresh hard disk of `machine` — the
/// device set a composer reads its facts from.
pub fn seat(session: &mut Session, machine: Option<&str>, path: &PathBuf) {
    let media = session
        .load_media(
            open_read(path),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("the image loads")
        .id();
    let mut view = match machine {
        Some(identity) => session.machine_mut(identity).expect("is there"),
        None => session.anonymous_mut(),
    };
    view.add_device(HardDrive::MbrSector)
        .expect("a hard disk is added")
        .insert(media)
        .expect("the disk goes in");
}

/// Pools a floppy image and seats it in a fresh floppy drive of
/// `machine`. The slot is the order floppy drives were added, which is
/// what tells `A:` from `B:`.
///
/// A raw reading records no ecosystem, so declaring the drive is how the
/// caller says these bytes were a floppy — the same declaration a hard
/// disk's raw reading makes, and the only thing either says.
pub fn seat_floppy(session: &mut Session, machine: &str, path: &PathBuf) {
    let media = session
        .load_media(
            open_read(path),
            Format::Raw {
                device: remanence::FloppyDrive::Sector.into(),
                block_bytes: 512,
            },
        )
        .expect("the image loads")
        .id();
    session
        .machine_mut(machine)
        .expect("is there")
        .add_device(remanence::FloppyDrive::Sector)
        .expect("a floppy drive is added")
        .insert(media)
        .expect("the disk goes in");
}

/// A minimal FAT16 volume: 512-byte sectors, 1 sector/cluster, 2 FATs of
/// 32 sectors, 512 root entries, 8000 total sectors.
pub fn synthetic_fat16() -> Vec<u8> {
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
    image[24..26].copy_from_slice(&18u16.to_le_bytes());
    image[26..28].copy_from_slice(&2u16.to_le_bytes());
    image[510] = 0x55;
    image[511] = 0xaa;

    for fat in 0..2usize {
        let base = (1 + fat * 32) * 512;
        image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
        image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
    }

    image
}

/// A 1.44M-floppy-shaped FAT12 volume, bare on the medium as a floppy is.
pub fn synthetic_fat12_floppy() -> Vec<u8> {
    const TOTAL_SECTORS: usize = 2880;
    let mut image = vec![0u8; TOTAL_SECTORS * 512];

    image[0] = 0xeb;
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC");
    image[11..13].copy_from_slice(&512u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1u16.to_le_bytes());
    image[16] = 2;
    image[17..19].copy_from_slice(&224u16.to_le_bytes());
    image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
    image[21] = 0xf0;
    image[22..24].copy_from_slice(&9u16.to_le_bytes());
    image[24..26].copy_from_slice(&18u16.to_le_bytes());
    image[26..28].copy_from_slice(&2u16.to_le_bytes());
    image[510] = 0x55;
    image[511] = 0xaa;

    for fat in 0..2usize {
        let base = (1 + fat * 9) * 512;
        image[base] = 0xf0;
        image[base + 1] = 0xff;
        image[base + 2] = 0xff;
    }

    image
}

/// An MBR disk with one primary slot per entry, placed consecutively.
/// The same disk, with the primary in `active_slot` carrying the table's
/// boot flag. The claimed variants letter a disk's *bootable* primary
/// first, so a disk whose active partition is not its first one is what
/// tells that rule apart from "the first row wins".
pub fn synthetic_multi_mbr_active(entries: &[(u8, &[u8])], active_slot: usize) -> Vec<u8> {
    let mut disk = synthetic_multi_mbr(entries);
    disk[446 + active_slot * 16] = 0x80;
    disk
}

pub fn synthetic_multi_mbr(entries: &[(u8, &[u8])]) -> Vec<u8> {
    let mut start_lba = 2048usize;
    let mut layout = Vec::new();
    for (type_byte, volume) in entries {
        let sectors = volume.len() / 512;
        layout.push((*type_byte, start_lba, sectors, *volume));
        start_lba += sectors;
    }

    let mut disk = vec![0u8; start_lba * 512];
    for (slot, (type_byte, start, sectors, volume)) in layout.iter().enumerate() {
        let at = 446 + slot * 16;
        disk[at + 4] = *type_byte;
        disk[at + 8..at + 12].copy_from_slice(&(*start as u32).to_le_bytes());
        disk[at + 12..at + 16].copy_from_slice(&(*sectors as u32).to_le_bytes());
        disk[start * 512..start * 512 + volume.len()].copy_from_slice(volume);
    }
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk
}

/// A disk with one primary of `primary_type` and an extended chain of one
/// logical volume, the extended partition declared as `extended_type`.
pub fn synthetic_extended_disk(volume: &[u8], extended_type: u8, primary_type: u8) -> Vec<u8> {
    let sectors = volume.len() / 512;
    let primary_start = 2048usize;
    let ext_base = primary_start + sectors;
    let link_span = 2048 + sectors;
    let mut disk = vec![0u8; (ext_base + link_span) * 512];

    disk[446 + 4] = primary_type;
    disk[446 + 8..446 + 12].copy_from_slice(&(primary_start as u32).to_le_bytes());
    disk[446 + 12..446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[462 + 4] = extended_type;
    disk[462 + 8..462 + 12].copy_from_slice(&(ext_base as u32).to_le_bytes());
    disk[462 + 12..462 + 16].copy_from_slice(&(link_span as u32).to_le_bytes());
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk[primary_start * 512..primary_start * 512 + volume.len()].copy_from_slice(volume);

    let ebr = ext_base * 512;
    disk[ebr + 446 + 4] = 0x06;
    disk[ebr + 446 + 8..ebr + 446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    disk[ebr + 446 + 12..ebr + 446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[ebr + 510] = 0x55;
    disk[ebr + 511] = 0xaa;
    let logical = (ext_base + 2048) * 512;
    disk[logical..logical + volume.len()].copy_from_slice(volume);

    disk
}

pub fn write_image(tag: &str, bytes: Vec<u8>) -> PathBuf {
    let path = temp_path(tag);
    std::fs::write(&path, bytes).expect("image writes");
    path
}

// ---------------------------------------------------------------------------
// The rig layout, built rather than downloaded.
//
// The drive-letter variants differ over one shape: two DOS primaries and
// an extended chain of two logicals on a single disk. That shape is what
// the FreeDOS artifact was for, and it is wholly specified — so it can be
// written here, and the suite stops needing a downloaded qcow2 to decide
// what a rule assigns.

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
/// volume through the letter a rule assigned can prove it reached *that*
/// volume by reading it.
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
