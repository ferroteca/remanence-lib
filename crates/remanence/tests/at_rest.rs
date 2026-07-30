// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! At-rest integration tests over synthetic images the project owns
//! outright: a hand-built FAT16 volume, bare and behind an MBR, on raw
//! disks. (qcow2 round-trips are unit-tested inside the crate, where the
//! writer can build the image.)

use std::path::PathBuf;

use remanence::{AccessMode, Disk, DiskFormat, FatEntryKind, FatKind};

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-at-rest-{tag}-{}-{nonce}.img",
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

/// Wraps a volume image in a one-partition MBR disk (partition 1 starts
/// at LBA 2048, type 0x06).
fn synthetic_mbr_disk(volume: &[u8]) -> Vec<u8> {
    let start_lba = 2048usize;
    let mut disk = vec![0u8; start_lba * 512 + volume.len()];
    let sectors = (volume.len() / 512) as u32;

    disk[446 + 4] = 0x06; // FAT16B
    disk[446 + 8..446 + 12].copy_from_slice(&(start_lba as u32).to_le_bytes());
    disk[446 + 12..446 + 16].copy_from_slice(&sectors.to_le_bytes());
    disk[510] = 0x55;
    disk[511] = 0xaa;

    disk[start_lba * 512..].copy_from_slice(volume);
    disk
}

#[test]
fn fat16_roundtrip_on_a_bare_raw_image() {
    let path = temp_path("bare");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let mut disk = Disk::open(&path).expect("disk opens");
    assert_eq!(disk.mode(), AccessMode::ReadWrite);
    assert_eq!(disk.format(), DiskFormat::Raw);

    let geometry = disk.geometry().expect("geometry reads");
    assert!(geometry.partitions.is_empty());
    assert_eq!(geometry.volumes.len(), 1);
    let volume = &geometry.volumes[0];
    assert_eq!(volume.kind, FatKind::Fat16);
    assert_eq!(volume.label.as_deref(), Some("REMANENCE"));
    assert_eq!(volume.sectors_per_track, Some(18));
    assert_eq!(volume.heads, Some(2));

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

    // Rollback: the image is untouched.
    disk.rollback();
    assert!(!disk.is_modified());
    assert!(disk.entries(0, "SUB").is_err());

    // Write again and commit this time.
    disk.write_file(0, "KEPT.TXT", b"kept bytes").expect("write");
    disk.commit().expect("commit");
    assert!(!disk.is_modified());
    drop(disk);

    let mut reopened = Disk::open(&path).expect("reopens");
    assert_eq!(reopened.read_file(0, "KEPT.TXT").expect("read"), b"kept bytes");
    drop(reopened);

    std::fs::remove_file(&path).ok();
}

#[test]
fn fat16_behind_an_mbr_partition() {
    let path = temp_path("mbr");
    std::fs::write(&path, synthetic_mbr_disk(&synthetic_fat16())).expect("image writes");

    let mut disk = Disk::open(&path).expect("disk opens");
    let geometry = disk.geometry().expect("geometry reads");
    assert_eq!(geometry.partitions.len(), 1);
    assert_eq!(geometry.partitions[0].type_byte, 0x06);
    assert_eq!(geometry.partitions[0].start_bytes, 2048 * 512);
    assert_eq!(geometry.volumes.len(), 1);
    assert_eq!(geometry.volumes[0].partition_number, Some(1));
    assert_eq!(geometry.volumes[0].label.as_deref(), Some("REMANENCE"));

    disk.write_file(0, "ROOT.TXT", b"in the partition").expect("write");
    disk.commit().expect("commit");
    drop(disk);

    let mut reopened = Disk::open(&path).expect("reopens");
    assert_eq!(
        reopened.read_file(0, "ROOT.TXT").expect("read"),
        b"in the partition"
    );
    drop(reopened);

    std::fs::remove_file(&path).ok();
}

#[test]
fn p7_second_writer_fails_fast_and_read_only_falls_back() {
    let path = temp_path("lock");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    // While one Disk holds the claim, a second open fails fast: the
    // deny-write invariant cannot be obtained at all.
    let disk = Disk::open(&path).expect("first open");
    let second = Disk::open(&path);
    assert!(second.is_err(), "second open must fail while the claim is held");
    drop(disk);

    // A read-only file denies *us* write permission: the open falls back
    // to read-only and write actions are refused by name.
    let mut permissions =
        std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).expect("set readonly");

    let mut readonly = Disk::open(&path).expect("read-only fallback opens");
    assert_eq!(readonly.mode(), AccessMode::ReadOnly);
    assert!(readonly.geometry().is_ok(), "analysis proceeds read-only");
    let refused = readonly.write_file(0, "NO.TXT", b"denied");
    assert!(refused.is_err(), "write actions are denied in the fallback");
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

    let error = Disk::open(&path).expect_err("future version refused");
    let message = error.to_string();
    assert!(
        message.contains("version 9") && message.contains("ceiling"),
        "refusal names the version and the ceiling: {message}"
    );

    std::fs::remove_file(&path).ok();
}
