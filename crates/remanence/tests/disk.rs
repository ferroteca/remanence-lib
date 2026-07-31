// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! `Disk`-surface integration tests over synthetic images the project
//! owns outright: a hand-built FAT16 volume, bare and behind an MBR, on
//! raw disks. (qcow2 round-trips are unit-tested inside the crate, where the
//! writer can build the image.)

use std::path::PathBuf;

use remanence::{
    AccessIntent, AccessMode, Disk, DiskFormat, ErrorCategory, FatEntryKind, FatKind,
    PartitionKind,
};

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-disk-{tag}-{}-{nonce}.img",
        std::process::id()
    ))
}

/// Builds a minimal FAT16 volume: 512-byte sectors, 1 sector/cluster,
/// 1 reserved sector, 2 FATs of 32 sectors, 512 root entries, 8000
/// total sectors (7903 data clusters — comfortably FAT16).
fn synthetic_fat16() -> Vec<u8> {
    const TOTAL_SECTORS: usize = 8000;
    let mut image = vec![0u8; TOTAL_SECTORS * 512];

    // BPB.
    image[0] = 0xeb; // jump
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC"); // OEM name
    image[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
    image[13] = 1; // sectors/cluster
    image[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved
    image[16] = 2; // FATs
    image[17..19].copy_from_slice(&512u16.to_le_bytes()); // root entries
    image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
    image[21] = 0xf8; // media descriptor
    image[22..24].copy_from_slice(&32u16.to_le_bytes()); // sectors/FAT
    image[24..26].copy_from_slice(&18u16.to_le_bytes()); // sectors/track
    image[26..28].copy_from_slice(&2u16.to_le_bytes()); // heads
    image[510] = 0x55;
    image[511] = 0xaa;

    // FAT[0..2] reserved entries in both FAT copies.
    for fat in 0..2usize {
        let base = (1 + fat * 32) * 512;
        image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
        image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
    }

    // Volume label in the root directory.
    let root = (1 + 2 * 32) * 512;
    image[root..root + 11].copy_from_slice(b"REMANENCE  ");
    image[root + 11] = 0x08; // volume-id attribute

    image
}

/// A 1.44M-floppy-shaped FAT12 volume: 2880 sectors, 18 sectors/track,
/// 2 heads — track geometry dividing the total exactly (80 cylinders).
fn synthetic_fat12_floppy() -> Vec<u8> {
    const TOTAL_SECTORS: usize = 2880;
    let mut image = vec![0u8; TOTAL_SECTORS * 512];

    image[0] = 0xeb; // jump
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC"); // OEM name
    image[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
    image[13] = 1; // sectors/cluster
    image[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved
    image[16] = 2; // FATs
    image[17..19].copy_from_slice(&224u16.to_le_bytes()); // root entries
    image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
    image[21] = 0xf0; // media descriptor
    image[22..24].copy_from_slice(&9u16.to_le_bytes()); // sectors/FAT
    image[24..26].copy_from_slice(&18u16.to_le_bytes()); // sectors/track
    image[26..28].copy_from_slice(&2u16.to_le_bytes()); // heads
    image[510] = 0x55;
    image[511] = 0xaa;

    // FAT[0..2] reserved entries in both FAT copies.
    for fat in 0..2usize {
        let base = (1 + fat * 9) * 512;
        image[base] = 0xf0;
        image[base + 1] = 0xff;
        image[base + 2] = 0xff;
    }

    image
}

/// Wraps volumes in an MBR disk, one primary slot each (up to four),
/// placed consecutively from LBA 2048.
fn synthetic_multi_mbr(entries: &[(u8, &[u8])]) -> Vec<u8> {
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

/// Wraps a volume image in a one-partition MBR disk (partition 1 starts
/// at LBA 2048, type 0x06).
fn synthetic_mbr_disk(volume: &[u8]) -> Vec<u8> {
    synthetic_multi_mbr(&[(0x06, volume)])
}

/// A disk with one FAT16B primary and an extended chain of two logical
/// FAT volumes; `corrupt_second_ebr` drops the second link's signature.
fn synthetic_extended_disk(volume: &[u8], corrupt_second_ebr: bool) -> Vec<u8> {
    let sectors = volume.len() / 512;
    let primary_start = 2048usize;
    let ext_base = primary_start + sectors;
    // Each link: the EBR sector, then its logical volume at +2048.
    let link_span = 2048 + sectors;
    let ext_len = 2 * link_span;
    let mut disk = vec![0u8; (ext_base + ext_len) * 512];

    // MBR: slot 0 = FAT16B primary, slot 1 = the extended container.
    disk[446 + 4] = 0x06;
    disk[446 + 8..446 + 12].copy_from_slice(&(primary_start as u32).to_le_bytes());
    disk[446 + 12..446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[462 + 4] = 0x05;
    disk[462 + 8..462 + 12].copy_from_slice(&(ext_base as u32).to_le_bytes());
    disk[462 + 12..462 + 16].copy_from_slice(&(ext_len as u32).to_le_bytes());
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk[primary_start * 512..primary_start * 512 + volume.len()]
        .copy_from_slice(volume);

    // EBR 1: logical 1 at +2048 (relative to this EBR), link to EBR 2
    // (relative to the extended base).
    let ebr1 = ext_base * 512;
    disk[ebr1 + 446 + 4] = 0x06;
    disk[ebr1 + 446 + 8..ebr1 + 446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    disk[ebr1 + 446 + 12..ebr1 + 446 + 16]
        .copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[ebr1 + 462 + 4] = 0x05;
    disk[ebr1 + 462 + 8..ebr1 + 462 + 12]
        .copy_from_slice(&(link_span as u32).to_le_bytes());
    disk[ebr1 + 462 + 12..ebr1 + 462 + 16]
        .copy_from_slice(&(link_span as u32).to_le_bytes());
    disk[ebr1 + 510] = 0x55;
    disk[ebr1 + 511] = 0xaa;
    let logical1 = (ext_base + 2048) * 512;
    disk[logical1..logical1 + volume.len()].copy_from_slice(volume);

    // EBR 2: logical 2, end of the chain.
    let ebr2_lba = ext_base + link_span;
    let ebr2 = ebr2_lba * 512;
    disk[ebr2 + 446 + 4] = 0x06;
    disk[ebr2 + 446 + 8..ebr2 + 446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    disk[ebr2 + 446 + 12..ebr2 + 446 + 16]
        .copy_from_slice(&(sectors as u32).to_le_bytes());
    if !corrupt_second_ebr {
        disk[ebr2 + 510] = 0x55;
        disk[ebr2 + 511] = 0xaa;
    }
    let logical2 = (ebr2_lba + 2048) * 512;
    disk[logical2..logical2 + volume.len()].copy_from_slice(volume);

    disk
}

#[test]
fn fat16_roundtrip_on_a_bare_raw_image() {
    let path = temp_path("bare");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Write).expect("disk opens");
    assert_eq!(disk.mode(), AccessMode::ReadWrite, "the mode echoes the intent");
    assert_eq!(disk.format(), DiskFormat::Raw);

    let geometry = disk.geometry().expect("geometry reads");
    assert!(!geometry.blank);
    assert!(geometry.partitions.is_empty());
    assert_eq!(geometry.volumes.len(), 1);
    let volume = &geometry.volumes[0];
    assert_eq!(volume.kind, FatKind::Fat16);
    assert_eq!(volume.label.as_deref(), Some("REMANENCE"));
    assert_eq!(volume.sectors_per_track, Some(18));
    assert_eq!(volume.heads, Some(2));
    // 8000 sectors do not divide into 18x2 tracks: omitted, not invented.
    assert_eq!(volume.cylinders, None);

    // Write a directory and a file; read them back through the overlay.
    disk.make_directory(0, "SUB").expect("mkdir");
    let payload: Vec<u8> = (0..2000u32).flat_map(|n| n.to_le_bytes()).collect();
    disk.write_file(0, "SUB/HELLO.BIN", &payload).expect("write");
    assert!(disk.is_modified());

    let entries = disk.entries(0, "SUB").expect("list");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "HELLO.BIN");
    assert_eq!(entries[0].kind, FatEntryKind::File);
    assert_eq!(entries[0].size_bytes, payload.len() as u64);
    assert_eq!(disk.read_file(0, "SUB/HELLO.BIN").expect("read"), payload);
    assert_eq!(
        disk.read_file(0, "SUB").expect_err("directory is not a file").category(),
        ErrorCategory::IsDirectory
    );
    assert_eq!(
        disk.entries(0, "SUB/HELLO.BIN")
            .expect_err("file is not a directory")
            .category(),
        ErrorCategory::NotDirectory
    );
    assert_eq!(
        disk.read_file(0, "SUB/MISSING.BIN")
            .expect_err("missing file is refused")
            .category(),
        ErrorCategory::NotFound
    );
    assert_eq!(
        disk.write_file(0, "TOO-BIG.BIN", &vec![0u8; 5_000_000])
            .expect_err("allocation exhaustion is refused")
            .category(),
        ErrorCategory::NoSpace
    );

    // Rollback: the image is untouched.
    disk.rollback();
    assert!(!disk.is_modified());
    assert!(disk.entries(0, "SUB").is_err());

    // Write again and commit this time.
    disk.write_file(0, "KEPT.TXT", b"kept bytes").expect("write");
    assert_eq!(
        disk.write_file(0, "KEPT.TXT", b"replacement")
            .expect_err("overwrite is outside the current claim")
            .category(),
        ErrorCategory::Unsupported
    );
    disk.commit().expect("commit");
    assert!(!disk.is_modified());
    drop(disk);

    let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reopens");
    assert_eq!(reopened.mode(), AccessMode::ReadOnly, "the mode echoes the intent");
    assert_eq!(reopened.read_file(0, "KEPT.TXT").expect("read"), b"kept bytes");
    drop(reopened);

    std::fs::remove_file(&path).ok();
}

#[test]
fn fat16_behind_an_mbr_partition() {
    let path = temp_path("mbr");
    std::fs::write(&path, synthetic_mbr_disk(&synthetic_fat16())).expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Write).expect("disk opens");
    let geometry = disk.geometry().expect("geometry reads");
    assert!(!geometry.blank);
    assert_eq!(geometry.partitions.len(), 1);
    assert_eq!(geometry.partitions[0].kind, PartitionKind::Primary);
    assert_eq!(geometry.partitions[0].type_byte, 0x06);
    assert_eq!(geometry.partitions[0].type_name.as_deref(), Some("FAT16B"));
    assert_eq!(geometry.partitions[0].start_bytes, 2048 * 512);
    assert_eq!(geometry.partitions[0].issue, None);
    assert_eq!(geometry.volumes.len(), 1);
    assert_eq!(geometry.volumes[0].partition_number, Some(1));
    assert_eq!(geometry.volumes[0].label.as_deref(), Some("REMANENCE"));

    disk.write_file(0, "ROOT.TXT", b"in the partition").expect("write");
    disk.commit().expect("commit");
    drop(disk);

    let mut reopened = Disk::open(&path, AccessIntent::Read).expect("reopens");
    assert_eq!(
        reopened.read_file(0, "ROOT.TXT").expect("read"),
        b"in the partition"
    );
    drop(reopened);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_blank_disk_is_an_answer_with_zero_volumes() {
    let path = temp_path("blank");
    std::fs::write(&path, vec![0u8; 8000 * 512]).expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("blank is an answer, not an error");
    assert!(geometry.blank);
    assert!(geometry.partitions.is_empty());
    assert!(geometry.volumes.is_empty());

    // Zero volumes means every volume address is a named refusal.
    assert!(disk.entries(0, "").is_err());
    drop(disk);

    std::fs::remove_file(&path).ok();
}

#[test]
fn an_empty_partition_table_is_not_blank() {
    let path = temp_path("empty-mbr");
    // A valid boot signature, no BPB shape, and four empty slots.
    let mut image = vec![0u8; 8000 * 512];
    image[510] = 0x55;
    image[511] = 0xaa;
    std::fs::write(&path, image).expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("an empty table is a complete answer");
    assert!(!geometry.blank);
    assert!(geometry.partitions.is_empty());
    assert!(geometry.volumes.is_empty());
    drop(disk);

    std::fs::remove_file(&path).ok();
}

#[test]
fn an_unreadable_image_is_refused_by_name_distinct_from_blank() {
    let path = temp_path("unreadable");
    // Non-zero data that is neither a supported filesystem nor a
    // partition table (no boot signature).
    std::fs::write(&path, vec![0x51u8; 8000 * 512]).expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let error = disk.geometry().expect_err("an unreadable image is refused");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    let message = error.to_string();
    assert!(
        message.contains("blank"),
        "the refusal is kept distinct from blank: {message}"
    );
    drop(disk);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_row_outside_the_claim_stays_and_nothing_renumbers() {
    let fat = synthetic_fat16();
    let foreign = vec![0u8; 8000 * 512]; // never read: refused at the type
    let path = temp_path("foreign-type");
    std::fs::write(
        &path,
        synthetic_multi_mbr(&[(0x06, &fat), (0x83, &foreign), (0x06, &fat)]),
    )
    .expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("one foreign row does not fail the disk");

    assert_eq!(geometry.partitions.len(), 3);
    let numbers: Vec<u32> =
        geometry.partitions.iter().map(|partition| partition.number).collect();
    assert_eq!(numbers, [1, 2, 3], "rows never renumber");

    let foreign_row = &geometry.partitions[1];
    assert_eq!(foreign_row.kind, PartitionKind::Primary);
    assert_eq!(foreign_row.type_byte, 0x83);
    assert_eq!(foreign_row.type_name, None, "no pinned name outside the claim");
    let issue = foreign_row.issue.as_ref().expect("the row carries its issue");
    assert_eq!(issue.category(), ErrorCategory::Unsupported);
    assert!(issue.to_string().contains("0x83"), "the refusal names the type");
    assert!(geometry.partitions[0].issue.is_none());
    assert!(geometry.partitions[2].issue.is_none());

    let referenced: Vec<Option<u32>> =
        geometry.volumes.iter().map(|volume| volume.partition_number).collect();
    assert_eq!(referenced, [Some(1), Some(3)], "the volumes behind it keep their rows");

    // The file verbs still address the readable slots on either side.
    assert!(disk.entries(0, "").is_ok());
    assert!(disk.entries(1, "").is_err(), "the foreign slot is a named refusal");
    assert!(disk.entries(2, "").is_ok());
    drop(disk);

    std::fs::remove_file(&path).ok();
}

#[test]
fn an_unreadable_volume_keeps_its_row_with_the_issue() {
    let fat = synthetic_fat16();
    let garbage = vec![0x51u8; 8000 * 512]; // claimed FAT16B, no BPB inside
    let path = temp_path("unreadable-volume");
    std::fs::write(&path, synthetic_multi_mbr(&[(0x06, &garbage), (0x06, &fat)]))
        .expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("one unreadable volume does not fail the disk");

    assert_eq!(geometry.partitions.len(), 2);
    let broken = &geometry.partitions[0];
    assert_eq!(broken.type_name.as_deref(), Some("FAT16B"));
    let issue = broken.issue.as_ref().expect("the row carries why it has no volume");
    assert_eq!(issue.category(), ErrorCategory::InvalidImage);
    assert!(geometry.partitions[1].issue.is_none());

    assert_eq!(geometry.volumes.len(), 1);
    assert_eq!(geometry.volumes[0].partition_number, Some(2));
    drop(disk);

    std::fs::remove_file(&path).ok();
}

#[test]
fn the_extended_chain_reports_primary_and_logical_kinds() {
    let fat = synthetic_fat16();
    let path = temp_path("extended");
    std::fs::write(&path, synthetic_extended_disk(&fat, false)).expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("geometry reads");

    assert_eq!(geometry.partitions.len(), 4);
    let kinds: Vec<PartitionKind> =
        geometry.partitions.iter().map(|partition| partition.kind).collect();
    assert_eq!(
        kinds,
        [
            PartitionKind::Primary,
            PartitionKind::Primary, // the extended container
            PartitionKind::Logical,
            PartitionKind::Logical,
        ]
    );
    assert_eq!(geometry.partitions[1].type_name.as_deref(), Some("extended"));
    assert!(geometry.partitions.iter().all(|partition| partition.issue.is_none()));

    let referenced: Vec<Option<u32>> =
        geometry.volumes.iter().map(|volume| volume.partition_number).collect();
    assert_eq!(referenced, [Some(1), Some(3), Some(4)]);

    // The file verbs reach all three volumes (the container takes no slot).
    for volume in 0..3 {
        assert!(disk.entries(volume, "").is_ok(), "volume {volume} readable");
    }
    drop(disk);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_broken_chain_keeps_what_it_found() {
    let fat = synthetic_fat16();
    let path = temp_path("broken-chain");
    std::fs::write(&path, synthetic_extended_disk(&fat, true)).expect("image writes");

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("a broken link does not fail the disk");

    // The primary, the container, and the first logical all stay; the
    // container row carries why the walk stopped.
    assert_eq!(geometry.partitions.len(), 3);
    let numbers: Vec<u32> =
        geometry.partitions.iter().map(|partition| partition.number).collect();
    assert_eq!(numbers, [1, 2, 3], "nothing renumbers");
    let container = &geometry.partitions[1];
    let issue = container.issue.as_ref().expect("the container carries the issue");
    assert_eq!(issue.category(), ErrorCategory::InvalidImage);
    assert!(issue.to_string().contains("signature"), "the refusal says why");

    let referenced: Vec<Option<u32>> =
        geometry.volumes.iter().map(|volume| volume.partition_number).collect();
    assert_eq!(referenced, [Some(1), Some(3)]);
    drop(disk);

    std::fs::remove_file(&path).ok();
}

#[test]
fn cylinders_are_reported_only_where_the_derivation_is_exact() {
    // 2880 sectors at 18 sectors/track x 2 heads: exactly 80 cylinders.
    let path = temp_path("cylinders-exact");
    std::fs::write(&path, synthetic_fat12_floppy()).expect("image writes");
    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("geometry reads");
    assert_eq!(geometry.volumes.len(), 1);
    assert_eq!(geometry.volumes[0].kind, FatKind::Fat12);
    assert_eq!(geometry.volumes[0].cylinders, Some(80));
    drop(disk);
    std::fs::remove_file(&path).ok();

    // The same volume with no stated track geometry: nothing to derive
    // from, so nothing is invented.
    let path = temp_path("cylinders-unstated");
    let mut image = synthetic_fat12_floppy();
    image[24..28].fill(0); // sectors/track and heads unstated
    std::fs::write(&path, image).expect("image writes");
    let mut disk = Disk::open(&path, AccessIntent::Read).expect("disk opens");
    let geometry = disk.geometry().expect("geometry reads");
    assert_eq!(geometry.volumes[0].sectors_per_track, None);
    assert_eq!(geometry.volumes[0].heads, None);
    assert_eq!(geometry.volumes[0].cylinders, None);
    drop(disk);
    std::fs::remove_file(&path).ok();
}

#[test]
fn p7_declared_intent_claims_and_refusals() {
    let path = temp_path("lock");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    // A writable session admits no observers: while it holds the claim,
    // a second open fails fast whatever its intent.
    let writer = Disk::open(&path, AccessIntent::Write).expect("writable open");
    assert_eq!(
        Disk::open(&path, AccessIntent::Read)
            .expect_err("a reader is excluded while a writable session lives")
            .category(),
        ErrorCategory::Locked
    );
    assert_eq!(
        Disk::open(&path, AccessIntent::Write)
            .expect_err("a second writer is excluded")
            .category(),
        ErrorCategory::Locked
    );
    drop(writer);

    // A read session keeps admitting other readers and still denies
    // every writer.
    let reader = Disk::open(&path, AccessIntent::Read).expect("read open");
    let second = Disk::open(&path, AccessIntent::Read).expect("second reader admitted");
    assert_eq!(
        Disk::open(&path, AccessIntent::Write)
            .expect_err("a writer is refused while readers hold the file")
            .category(),
        ErrorCategory::Locked
    );
    drop(second);
    drop(reader);

    // A read-only file denies us write permission: a writable open
    // fails at the open — never a silent fallback — while a read open
    // proceeds and write actions are refused by name.
    let mut permissions =
        std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).expect("set readonly");

    assert!(
        Disk::open(&path, AccessIntent::Write).is_err(),
        "a writable open on a read-only file fails at the open"
    );
    let mut readonly = Disk::open(&path, AccessIntent::Read).expect("read open proceeds");
    assert_eq!(readonly.mode(), AccessMode::ReadOnly);
    assert!(readonly.geometry().is_ok(), "analysis proceeds");
    let refused = readonly
        .write_file(0, "NO.TXT", b"denied")
        .expect_err("write actions are denied on a read session");
    assert_eq!(refused.category(), ErrorCategory::ReadOnly);
    drop(readonly);

    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).expect("clear readonly");
    std::fs::remove_file(&path).ok();
}

#[test]
fn p8_refuses_a_future_qcow2_version_by_name() {
    let path = temp_path("qcow2-future");
    // A minimal header claiming version 9.
    let mut header = vec![0u8; 512];
    header[..4].copy_from_slice(b"QFI\xfb");
    header[4..8].copy_from_slice(&9u32.to_be_bytes());
    std::fs::write(&path, header).expect("image writes");

    let error = Disk::open(&path, AccessIntent::Read).expect_err("future version refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    let message = error.to_string();
    assert!(
        message.contains("version 9") && message.contains("ceiling"),
        "refusal names the version and the ceiling: {message}"
    );

    std::fs::remove_file(&path).ok();
}
