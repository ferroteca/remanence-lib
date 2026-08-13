// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! `Medium`-surface unit tests over synthetic images the project owns
//! outright: a hand-built FAT16 volume, bare and behind an MBR, on raw
//! disks. (qcow2 round-trips are unit-tested inside the crate, where the
//! writer can build the image.)
//!
//! The content of every one of them is reached the one way there is: the
//! partition pool the medium was loaded with, by the scheme's own ordinal
//! (P16, P17). An MBR table numbers its own entries from one; an image
//! that records no scheme bears the direct partition at zero, which is
//! the library's own composition of the whole content and says so.

use std::path::PathBuf;

use remanence::{
    AccessMode, DiskContent, DiskFormat, DosNameRule, EntryKind, ErrorCategory, FatKind, Format,
    HardDrive, MediaId, Medium, PartitionRule, PartitionScheme, PartitionType, RegionRole, Session,
    SpaceRule, StorageSpace, VolumeOrigin,
};

mod common;
use common::{open_read, open_write};

/// What the caller's own open affords, in the shape these tests declare
/// it: the amended P7 asks the handle one question, so the test says
/// which answer it wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Afford {
    Read,
    Write,
}

/// The space one partition of `medium` composes, reached through the
/// door that opens on it. One node, both vantages (D26): the doors hand
/// out the same `StorageSpace`, and the file verbs live on it and
/// nowhere else (P19).
fn fs(medium: &mut Medium, ordinal: u32) -> StorageSpace<'_> {
    let partition = medium
        .partition(ordinal)
        .expect("the pool bears this partition");
    if partition.partition().bears_namespace() {
        partition
            .filesystem()
            .expect("the declared type determines one")
    } else {
        // A medium recording no scheme declares no type, so the reading
        // is the caller's and the check is the library's.
        partition
            .filesystem_as("fat")
            .expect("these images are FAT")
    }
}

/// Pools `path` in a fresh session under the raw declaration and returns
/// both, because a medium lives in its session's pool. Tests keep the
/// session alive for as long as they use the medium.
fn attach(
    path: impl AsRef<std::path::Path>,
    afford: Afford,
) -> remanence::Result<(Session, MediaId)> {
    let source = match afford {
        Afford::Read => open_read(path),
        Afford::Write => open_write(path),
    };
    let mut session = Session::new();
    let id = session
        .load_media(
            source,
            Format::Raw {
                device: HardDrive::MbrSector,
                block_bytes: 512,
            },
        )?
        .id();
    Ok((session, id))
}

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
/// placed consecutively from LBA 2048. Each entry states its type value,
/// whether the slot carries the boot flag — byte 0 of the slot, `0x80`
/// and nothing else — and the volume to lay behind it.
fn synthetic_flagged_mbr(entries: &[(u8, bool, &[u8])]) -> Vec<u8> {
    let mut start_lba = 2048usize;
    let mut layout = Vec::new();
    for (type_byte, active, volume) in entries {
        let sectors = volume.len() / 512;
        layout.push((*type_byte, *active, start_lba, sectors, *volume));
        start_lba += sectors;
    }

    let mut disk = vec![0u8; start_lba * 512];
    for (slot, (type_byte, active, start, sectors, volume)) in layout.iter().enumerate() {
        let at = 446 + slot * 16;
        if *active {
            disk[at] = 0x80;
        }
        disk[at + 4] = *type_byte;
        disk[at + 8..at + 12].copy_from_slice(&(*start as u32).to_le_bytes());
        disk[at + 12..at + 16].copy_from_slice(&(*sectors as u32).to_le_bytes());
        disk[start * 512..start * 512 + volume.len()].copy_from_slice(volume);
    }
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk
}

/// The same disk with no slot flagged active, which is what most of these
/// images want to say nothing about.
fn synthetic_multi_mbr(entries: &[(u8, &[u8])]) -> Vec<u8> {
    let flagged: Vec<(u8, bool, &[u8])> = entries
        .iter()
        .map(|(type_byte, volume)| (*type_byte, false, *volume))
        .collect();
    synthetic_flagged_mbr(&flagged)
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

    // MBR: slot 0 = FAT16B primary, slot 1 = the extended partition.
    disk[446 + 4] = 0x06;
    disk[446 + 8..446 + 12].copy_from_slice(&(primary_start as u32).to_le_bytes());
    disk[446 + 12..446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[462 + 4] = 0x05;
    disk[462 + 8..462 + 12].copy_from_slice(&(ext_base as u32).to_le_bytes());
    disk[462 + 12..462 + 16].copy_from_slice(&(ext_len as u32).to_le_bytes());
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk[primary_start * 512..primary_start * 512 + volume.len()].copy_from_slice(volume);

    // EBR 1: logical 1 at +2048 (relative to this EBR), link to EBR 2
    // (relative to the extended base).
    let ebr1 = ext_base * 512;
    disk[ebr1 + 446 + 4] = 0x06;
    disk[ebr1 + 446 + 8..ebr1 + 446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    disk[ebr1 + 446 + 12..ebr1 + 446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[ebr1 + 462 + 4] = 0x05;
    disk[ebr1 + 462 + 8..ebr1 + 462 + 12].copy_from_slice(&(link_span as u32).to_le_bytes());
    disk[ebr1 + 462 + 12..ebr1 + 462 + 16].copy_from_slice(&(link_span as u32).to_le_bytes());
    disk[ebr1 + 510] = 0x55;
    disk[ebr1 + 511] = 0xaa;
    let logical1 = (ext_base + 2048) * 512;
    disk[logical1..logical1 + volume.len()].copy_from_slice(volume);

    // EBR 2: logical 2, end of the chain.
    let ebr2_lba = ext_base + link_span;
    let ebr2 = ebr2_lba * 512;
    disk[ebr2 + 446 + 4] = 0x06;
    disk[ebr2 + 446 + 8..ebr2 + 446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    disk[ebr2 + 446 + 12..ebr2 + 446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    if !corrupt_second_ebr {
        disk[ebr2 + 510] = 0x55;
        disk[ebr2 + 511] = 0xaa;
    }
    let logical2 = (ebr2_lba + 2048) * 512;
    disk[logical2..logical2 + volume.len()].copy_from_slice(volume);

    disk
}

/// The label a recognized filesystem answered with, or `None` where the
/// volume has none.
fn label_of(filesystem: &remanence::FilesystemInfo) -> Option<&str> {
    filesystem
        .label
        .as_ref()
        .expect("a recognized filesystem answers the label question")
        .name
        .as_deref()
}

#[test]
fn fat16_roundtrip_on_a_bare_raw_image() {
    let path = temp_path("bare");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    assert_eq!(
        disk.mode(),
        AccessMode::ReadWrite,
        "the mode echoes the intent"
    );
    assert_eq!(disk.format().expect("a medium is pooled"), DiskFormat::Raw);

    let report = disk.inspect().expect("inspection reads");
    // The device reports the medium attached to it (P14): a raw image is
    // logical-block media, named from the media-type catalog and stated
    // apart from what turned out to be recorded on it.
    assert_eq!(report.device.article, "logical-block-512");
    assert_eq!(report.content, DiskContent::DirectVolume);
    assert!(report.regions.is_empty());
    assert_eq!(report.volumes.len(), 1);
    let volume = report.volumes[0].id;
    let filesystem = report.filesystem_on(volume).expect("FAT recognized");
    assert_eq!(filesystem.kind.as_deref(), Some(FatKind::Fat16.name()));
    assert_eq!(label_of(filesystem), Some("REMANENCE"));
    assert_eq!(filesystem.declared_geometry.sectors_per_track, Some(18));
    assert_eq!(filesystem.declared_geometry.heads, Some(2));
    // 8000 sectors do not divide into 18x2 tracks: omitted, not invented.
    assert_eq!(filesystem.declared_geometry.cylinders, None);

    // Write a directory and a file; read them back through the overlay.
    fs(disk, 0).make_directory("SUB").expect("mkdir");
    let payload: Vec<u8> = (0..2000u32).flat_map(|n| n.to_le_bytes()).collect();
    fs(disk, 0)
        .write_file("SUB/HELLO.BIN", &payload)
        .expect("write");
    assert!(disk.is_modified());

    let entries = fs(disk, 0).entries("SUB").expect("list");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "HELLO.BIN");
    assert_eq!(entries[0].kind, EntryKind::File);
    assert_eq!(entries[0].size_bytes, payload.len() as u64);
    assert_eq!(
        fs(disk, 0).read_file("SUB/HELLO.BIN").expect("read"),
        payload
    );
    assert_eq!(
        fs(disk, 0)
            .read_file("SUB")
            .expect_err("directory is not a file")
            .category(),
        ErrorCategory::IsDirectory
    );
    assert_eq!(
        fs(disk, 0)
            .entries("SUB/HELLO.BIN")
            .expect_err("file is not a directory")
            .category(),
        ErrorCategory::NotDirectory
    );
    assert_eq!(
        fs(disk, 0)
            .read_file("SUB/MISSING.BIN")
            .expect_err("missing file is refused")
            .category(),
        ErrorCategory::NotFound
    );
    assert_eq!(
        fs(disk, 0)
            .write_file("TOO-BIG.BIN", &vec![0u8; 5_000_000],)
            .expect_err("allocation exhaustion is refused")
            .category(),
        ErrorCategory::NoSpace
    );

    // Rollback: the image is untouched.
    disk.rollback().expect("a medium is pooled");
    assert!(!disk.is_modified());
    assert!(fs(disk, 0).entries("SUB").is_err());

    // Write again and commit this time; overwriting replaces the
    // contents rather than refusing (U3).
    fs(disk, 0)
        .write_file("KEPT.TXT", b"the first draft, rather longer")
        .expect("write");
    fs(disk, 0)
        .write_file("KEPT.TXT", b"kept bytes")
        .expect("overwrite");
    disk.commit().expect("commit");
    assert!(!disk.is_modified());
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, Afford::Read).expect("reopens");
    let reopened = reopened_session
        .medium_mut(reopened_at)
        .expect("the medium is pooled");

    assert_eq!(
        reopened.mode(),
        AccessMode::ReadOnly,
        "the mode echoes the intent"
    );
    assert_eq!(
        fs(reopened, 0).read_file("KEPT.TXT").expect("read"),
        b"kept bytes"
    );
    drop(reopened_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn fat16_behind_an_mbr_partition() {
    let path = temp_path("mbr");
    std::fs::write(&path, synthetic_mbr_disk(&synthetic_fat16())).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(report.content, DiskContent::Schema);
    assert_eq!(report.regions.len(), 1);
    let region = &report.regions[0];
    assert_eq!(region.declared_placement, "primary");
    assert_eq!(region.role, RegionRole::Data);
    assert_eq!(region.declared_type, 0x06);
    assert_eq!(region.declared_type_reading, "FAT16B");
    assert!(region.claimed);
    assert_eq!(region.start_bytes, 2048 * 512);
    assert_eq!(region.issue, None);
    assert_eq!(report.volumes.len(), 1);
    assert_eq!(
        report.volumes[0].origin,
        VolumeOrigin::Regions(vec![region.id])
    );
    assert_eq!(
        report
            .filesystem_on(report.volumes[0].id)
            .and_then(label_of),
        Some("REMANENCE")
    );

    // Entry 1 of the declared table, which is the scheme's own number for
    // it and the only way this partition is named.
    fs(disk, 1)
        .write_file("ROOT.TXT", b"in the partition")
        .expect("write");
    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, Afford::Read).expect("reopens");
    let reopened = reopened_session
        .medium_mut(reopened_at)
        .expect("the medium is pooled");

    assert_eq!(
        fs(reopened, 1).read_file("ROOT.TXT").expect("read"),
        b"in the partition"
    );
    drop(reopened_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn stat_answers_presence_and_absence_distinctly() {
    let path = temp_path("stat");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    fs(disk, 0).make_directory("SUB").expect("mkdir");
    fs(disk, 0)
        .write_file("SUB/FILE.BIN", b"1234567890")
        .expect("write");

    let file = fs(disk, 0)
        .stat("sub/file.bin")
        .expect("stat succeeds")
        .expect("the file exists");
    assert_eq!(file.name, "FILE.BIN");
    assert_eq!(file.kind, EntryKind::File);
    assert_eq!(file.size_bytes, 10);

    let directory = fs(disk, 0)
        .stat("SUB")
        .expect("stat succeeds")
        .expect("the directory exists");
    assert_eq!(directory.kind, EntryKind::Directory);

    // Absence is an answer, not a failure: a missing leaf, a missing
    // parent, and a parent that is a file all answer None.
    assert_eq!(
        fs(disk, 0).stat("SUB/MISSING.BIN").expect("an answer"),
        None
    );
    assert_eq!(
        fs(disk, 0).stat("NOWHERE/FILE.BIN").expect("an answer"),
        None
    );
    assert_eq!(
        fs(disk, 0)
            .stat("SUB/FILE.BIN/DEEPER.BIN")
            .expect("an answer"),
        None
    );

    // An ordinal the pool never issued is absence answered, not a
    // failure: this image records no scheme, so the direct partition at
    // zero is the whole of what it bears and nothing is manufactured to
    // stand at 1.
    assert!(
        disk.partition(1).is_none(),
        "the pool bears no partition at that ordinal"
    );
    // Failure stays failure where a verb is asked something it cannot
    // answer: the root is a directory and not an entry in one.
    assert!(
        fs(disk, 0).stat("").is_err(),
        "the root has no entry to answer with"
    );
    drop(disk_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn overwrite_releases_and_reclaims_clusters() {
    let path = temp_path("overwrite");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    // 7000 of the volume's 7903 data clusters: two of these can never
    // coexist, so each rewrite below only fits by releasing the last.
    let big = vec![0xabu8; 7000 * 512];
    fs(disk, 0)
        .write_file("BIG.BIN", &big)
        .expect("first write");

    let replacement = vec![0xcdu8; 7000 * 512];
    fs(disk, 0)
        .write_file("BIG.BIN", &replacement)
        .expect("overwriting releases the old clusters first");
    assert_eq!(fs(disk, 0).read_file("BIG.BIN").expect("read"), replacement);

    // Shrinking releases clusters for other files to claim.
    fs(disk, 0)
        .write_file("BIG.BIN", b"now tiny")
        .expect("shrinking overwrite");
    fs(disk, 0)
        .write_file("OTHER.BIN", &big)
        .expect("the released clusters are claimable again");

    // Overwriting a directory is refused by name.
    fs(disk, 0).make_directory("DIR").expect("mkdir");
    assert_eq!(
        fs(disk, 0)
            .write_file("DIR", b"not a file")
            .expect_err("a directory is not overwritable")
            .category(),
        ErrorCategory::IsDirectory
    );

    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, Afford::Read).expect("reopens");
    let reopened = reopened_session
        .medium_mut(reopened_at)
        .expect("the medium is pooled");

    assert_eq!(
        fs(reopened, 0).read_file("BIG.BIN").expect("read"),
        b"now tiny"
    );
    assert_eq!(fs(reopened, 0).read_file("OTHER.BIN").expect("read"), big);
    drop(reopened_session);

    // Both FAT copies were kept in step through release and reclaim.
    let image = std::fs::read(&path).expect("image reads");
    let fat_bytes = 32 * 512;
    let first = &image[512..512 + fat_bytes];
    let second = &image[512 + fat_bytes..512 + 2 * fat_bytes];
    assert_eq!(first, second, "both FAT copies stay consistent");

    std::fs::remove_file(&path).ok();
}

#[test]
fn make_directory_creates_parents_and_is_idempotent() {
    let path = temp_path("mkdirs");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    // Missing parents are created in one call.
    fs(disk, 0)
        .make_directory("A/B/C")
        .expect("missing parents are created");
    assert_eq!(
        fs(disk, 0)
            .stat("A/B/C")
            .expect("stat")
            .expect("exists")
            .kind,
        EntryKind::Directory
    );

    // Already existing — wholly, partly, or the root itself — succeeds
    // unchanged, and the chain extends from wherever it stops.
    fs(disk, 0).make_directory("A/B/C").expect("idempotent");
    fs(disk, 0)
        .make_directory("A/B")
        .expect("an existing prefix succeeds");
    fs(disk, 0)
        .make_directory("")
        .expect("the root already exists");
    fs(disk, 0)
        .make_directory("A/B/C/D")
        .expect("extends the existing chain");

    // The created directories hold files like any other.
    fs(disk, 0)
        .write_file("A/B/C/D/DEEP.TXT", b"nested payload")
        .expect("write");

    // A file in the way is refused by name, at the leaf or mid-path.
    assert_eq!(
        fs(disk, 0)
            .make_directory("A/B/C/D/DEEP.TXT")
            .expect_err("a file at the leaf is refused")
            .category(),
        ErrorCategory::NotDirectory
    );
    assert_eq!(
        fs(disk, 0)
            .make_directory("A/B/C/D/DEEP.TXT/E")
            .expect_err("a file mid-path is refused")
            .category(),
        ErrorCategory::NotDirectory
    );

    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, Afford::Read).expect("reopens");
    let reopened = reopened_session
        .medium_mut(reopened_at)
        .expect("the medium is pooled");

    assert_eq!(
        fs(reopened, 0).read_file("A/B/C/D/DEEP.TXT").expect("read"),
        b"nested payload"
    );
    drop(reopened_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_growing_subdirectory_never_collides_with_file_clusters() {
    let path = temp_path("dir-growth");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    fs(disk, 0).make_directory("SUB").expect("mkdir");

    // One 512-byte cluster holds 16 records; "." and ".." take two, so
    // the fifteenth file forces the directory to grow mid-write, and
    // the grown cluster must never collide with a file's data clusters.
    let names: Vec<String> = (0..20).map(|n| format!("FILE{n:02}.BIN")).collect();
    for (n, name) in names.iter().enumerate() {
        fs(disk, 0)
            .write_file(&format!("SUB/{name}"), &vec![n as u8; 700])
            .expect("write");
    }
    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, Afford::Read).expect("reopens");
    let reopened = reopened_session
        .medium_mut(reopened_at)
        .expect("the medium is pooled");

    let entries = fs(reopened, 0).entries("SUB").expect("list");
    assert_eq!(entries.len(), 20);
    for (n, name) in names.iter().enumerate() {
        assert_eq!(
            fs(reopened, 0)
                .read_file(&format!("SUB/{name}"))
                .expect("read"),
            vec![n as u8; 700],
            "{name} reads back intact"
        );
    }
    drop(reopened_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_blank_disk_is_an_answer_with_zero_volumes() {
    let path = temp_path("blank");
    std::fs::write(&path, vec![0u8; 8000 * 512]).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    let report = disk.inspect().expect("blank is an answer, not an error");
    assert_eq!(report.content, DiskContent::Blank);
    assert!(report.regions.is_empty());
    assert!(report.volumes.is_empty());

    // Zero volumes composed, and yet the medium still bears the direct
    // partition: blank is a fact about what was recorded, not a reason to
    // withhold the door the content is reached through (P17). It is the
    // library's own composition, so it is addressable over the whole
    // content and its account is provenance rather than evidence.
    let direct = disk
        .partition(0)
        .expect("every medium bears a partition to be reached through");
    assert!(
        direct.is_direct(),
        "nothing recorded a scheme to declare one"
    );
    assert_eq!(
        direct.type_byte(),
        None,
        "and so nothing recorded a type either"
    );
    assert!(
        direct.partition().provenance().is_some(),
        "a composition act states itself as one"
    );
    assert!(
        direct.partition().evidence().is_empty(),
        "and never as something the medium said"
    );
    assert_eq!(
        direct.partition().volume_id(),
        None,
        "the report composed no volume here, and the pool says the same"
    );

    let mut space = direct
        .volume()
        .expect("the whole content is one addressed extent");
    assert_eq!(
        space.start_bytes(),
        Some(0),
        "it begins where the presented content does"
    );
    assert_eq!(
        space.length_bytes(),
        Some(8000 * 512),
        "the direct partition runs the whole of the presented content"
    );
    assert!(
        !space.has_namespace(),
        "a blank medium bears no namespace, and the absence is answered"
    );
    let mut head = [0u8; 16];
    space.read_at(0, &mut head).expect("the extent reads");
    assert_eq!(head, [0u8; 16], "and reads back the zeros that are there");
    drop(space);

    // The namespace door answers the same absence, rather than a failure
    // to have read one.
    assert!(
        disk.partition(0)
            .expect("the direct partition")
            .filesystem()
            .is_none(),
        "nothing declares a namespace over a blank medium"
    );
    drop(disk_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn the_extended_chain_reports_primary_and_logical_kinds() {
    let fat = synthetic_fat16();
    let path = temp_path("extended");
    std::fs::write(&path, synthetic_extended_disk(&fat, false)).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.regions.len(), 4);
    let placements: Vec<&str> = report
        .regions
        .iter()
        .map(|region| region.declared_placement.as_str())
        .collect();
    assert_eq!(
        placements,
        [
            "primary", "primary", // the extended partition
            "logical", "logical",
        ]
    );
    // Placement and role are different axes: the extended partition occupies a
    // primary slot, and its role is structural rather than data.
    assert_eq!(report.regions[1].role, RegionRole::Structure);
    assert_eq!(
        report.regions[1].declared_type_reading,
        "an extended partition, CHS-addressed"
    );
    assert!(report.regions.iter().all(|region| region.issue.is_none()));

    let referenced: Vec<u32> = report
        .volumes
        .iter()
        .filter_map(|volume| match &volume.origin {
            VolumeOrigin::Regions(regions) => regions.first().copied(),
            VolumeOrigin::WholeDevice => None,
        })
        .filter_map(|region| report.region(region).map(|found| found.declared_number))
        .collect();
    assert_eq!(referenced, [1, 3, 4]);

    // The file verbs answer on exactly the partitions the scheme declared
    // as data, by the scheme's own numbers: the extended container at 2
    // composes nothing, and the logicals behind it are still 3 and 4 (U4).
    for ordinal in referenced {
        assert!(
            fs(disk, ordinal).entries("").is_ok(),
            "partition {ordinal} is readable through the door that opens on it"
        );
    }
    drop(disk_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_broken_chain_keeps_what_it_found() {
    let fat = synthetic_fat16();
    let path = temp_path("broken-chain");
    std::fs::write(&path, synthetic_extended_disk(&fat, true)).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    let report = disk
        .inspect()
        .expect("a broken link does not fail the disk");

    // The primary, the extended partition, and the first logical all
    // stay; the extended region carries why the walk stopped.
    assert_eq!(report.regions.len(), 3);
    let numbers: Vec<u32> = report
        .regions
        .iter()
        .map(|region| region.declared_number)
        .collect();
    assert_eq!(numbers, [1, 2, 3], "nothing renumbers");
    let extended = &report.regions[1];
    let issue = extended
        .issue
        .as_ref()
        .expect("the extended region carries the issue");
    assert_eq!(issue.category(), ErrorCategory::InvalidImage);
    assert!(
        issue.to_string().contains("signature"),
        "the refusal says why"
    );

    let referenced: Vec<u32> = report
        .volumes
        .iter()
        .filter_map(|volume| match &volume.origin {
            VolumeOrigin::Regions(regions) => regions.first().copied(),
            VolumeOrigin::WholeDevice => None,
        })
        .filter_map(|region| report.region(region).map(|r| r.declared_number))
        .collect();
    assert_eq!(referenced, [1, 3]);
    drop(disk_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn cylinders_are_reported_only_where_the_derivation_is_exact() {
    // 2880 sectors at 18 sectors/track x 2 heads: exactly 80 cylinders.
    let path = temp_path("cylinders-exact");
    std::fs::write(&path, synthetic_fat12_floppy()).expect("image writes");
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(report.volumes.len(), 1);
    let filesystem = report
        .filesystem_on(report.volumes[0].id)
        .expect("FAT recognized");
    assert_eq!(filesystem.kind.as_deref(), Some(FatKind::Fat12.name()));
    assert_eq!(filesystem.declared_geometry.cylinders, Some(80));
    drop(disk_session);
    std::fs::remove_file(&path).ok();

    // The same volume with no stated track geometry: nothing to derive
    // from, so nothing is invented.
    let path = temp_path("cylinders-unstated");
    let mut image = synthetic_fat12_floppy();
    image[24..28].fill(0); // sectors/track and heads unstated
    std::fs::write(&path, image).expect("image writes");
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    let report = disk.inspect().expect("inspection reads");
    let filesystem = report
        .filesystem_on(report.volumes[0].id)
        .expect("FAT recognized");
    assert_eq!(filesystem.declared_geometry.sectors_per_track, None);
    assert_eq!(filesystem.declared_geometry.heads, None);
    assert_eq!(filesystem.declared_geometry.cylinders, None);
    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn p7_the_callers_own_open_is_the_claim_and_is_honoured_exactly() {
    // **Whoever opens owns the lock.** In-force P7's mandatory
    // write-denial is scoped to where the library opens; a local artifact
    // arrives as the caller's own handle, and the library asks it exactly
    // one question — may it write? — honours the answer, and adds no lock
    // of its own.
    let path = temp_path("claim");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut writer_session, writer_at) = attach(&path, Afford::Write).expect("writable handle");
    let writer = writer_session
        .medium_mut(writer_at)
        .expect("the medium is pooled");
    assert_eq!(writer.mode(), AccessMode::ReadWrite);
    assert_eq!(writer.assurance().claim, remanence::Claim::CallerOpened);

    // No lock of the library's own: the caller opened with the sharing
    // they chose, so a second ordinary handle over the same artifact is
    // the caller's business and loads. A caller who wants exclusivity
    // asks their own open for it.
    let (mut observer_session, observer_at) =
        attach(&path, Afford::Read).expect("the library adds no claim of its own");
    assert_eq!(
        observer_session
            .medium_mut(observer_at)
            .expect("the medium is pooled")
            .mode(),
        AccessMode::ReadOnly,
        "a read-only handle affords no write, and none is escalated"
    );
    drop(observer_session);
    drop(writer_session);

    // A read-only file denies the *caller* write permission, so the
    // handle they can open is read-only and the medium says so: analysis
    // proceeds and every write action is refused by name.
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).expect("set readonly");

    assert!(
        std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .is_err(),
        "the caller cannot open a read-only file for writing, which is the \
         whole of what the library then has"
    );
    let (mut readonly_session, readonly_at) =
        attach(&path, Afford::Read).expect("a read handle opens");
    let readonly = readonly_session
        .medium_mut(readonly_at)
        .expect("the medium is pooled");
    assert_eq!(readonly.mode(), AccessMode::ReadOnly);
    assert!(readonly.inspect().is_ok(), "analysis proceeds");
    let refused = fs(readonly, 0)
        .write_file("NO.TXT", b"denied")
        .expect_err("write actions are denied on a handle affording none");
    assert_eq!(refused.category(), ErrorCategory::ReadOnly);
    assert!(
        refused.to_string().contains("affords no write"),
        "the refusal names whose open it was: {refused}"
    );
    drop(readonly_session);

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

    // The declaration is the caller's, and the version gate is what
    // checks it: declaring qcow2 over a version this release does not
    // claim is refused before anything else is read (P8).
    let mut session = Session::new();
    let error = session
        .load_media(
            open_read(&path),
            Format::Qcow2 {
                device: HardDrive::MbrBlock,
            },
        )
        .expect_err("future version refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    let message = error.to_string();
    assert!(
        message.contains("version 9") && message.contains("ceiling"),
        "refusal names the version and the ceiling: {message}"
    );

    std::fs::remove_file(&path).ok();
}

// The layered inspection report. These exercise `Medium::inspect` through
// the public surface only: no test reaches for an internal record, and
// every relationship is traversed by the identity the report issued.

/// Writes `bytes` to a fresh temp image and returns its path.
fn image_at(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_path(tag);
    std::fs::write(&path, bytes).expect("image writes");
    path
}

#[test]
fn a_partitionless_volume_inspects_as_one_whole_device_volume() {
    let path = image_at("inspect-bare", &synthetic_fat16());
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.content, DiskContent::DirectVolume);
    assert!(
        report.partition_schema.is_none(),
        "no schema was recognized"
    );
    assert!(report.regions.is_empty(), "nothing declared a region");
    assert_eq!(report.volumes.len(), 1);
    assert_eq!(report.volumes[0].origin, VolumeOrigin::WholeDevice);

    // The filesystem is reached by identity, never by position.
    let volume = report.volumes[0].id;
    let filesystem = report
        .filesystem_on(volume)
        .expect("FAT is recognized on the whole-device volume");
    assert_eq!(filesystem.kind.as_deref(), Some("FAT16"));
    assert_eq!(filesystem.volume, volume);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_blank_disk_states_that_it_is_blank() {
    let path = image_at("inspect-blank", &vec![0u8; 4096]);
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");

    // Blank is stated, not left to be inferred from two empty lists.
    assert_eq!(report.content, DiskContent::Blank);
    assert!(report.volumes.is_empty());
    assert!(report.filesystems.is_empty());
    assert_eq!(report.composed_volume_count(), 0);

    std::fs::remove_file(&path).ok();
}

/// Content no adapter claims is an outcome carrying its evidence, not a
/// refusal. It stays distinct from blank, and it composes nothing.
#[test]
fn unclaimed_nonblank_content_is_a_reported_outcome() {
    let mut bytes = vec![0u8; 4096];
    bytes[..4].copy_from_slice(b"\xde\xad\xbe\xef");
    let path = image_at("inspect-unknown", &bytes);
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk
        .inspect()
        .expect("inspection succeeds on unknown content");
    let DiskContent::UnknownNonblank { evidence } = &report.content else {
        panic!(
            "expected the unknown-nonblank outcome, got {:?}",
            report.content
        );
    };
    assert!(
        !evidence.is_empty(),
        "the outcome carries why nothing claimed it"
    );
    assert_ne!(
        report.content,
        DiskContent::Blank,
        "never confused with blank"
    );
    assert!(report.volumes.is_empty());
    assert!(report.filesystems.is_empty());

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_partitioned_disk_reports_schema_regions_and_composed_volumes() {
    let path = image_at("inspect-mbr", &synthetic_mbr_disk(&synthetic_fat16()));
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.content, DiskContent::Schema);
    let schema = report.partition_schema.as_ref().expect("MBR recognized");
    assert_eq!(schema.kind, "mbr");
    assert!(!schema.evidence.is_empty(), "recognition carries evidence");

    assert_eq!(report.regions.len(), 1);
    let region = &report.regions[0];
    assert_eq!(region.role, RegionRole::Data);
    assert_eq!(region.declared_type, 0x06);
    assert_eq!(region.declared_type_reading, "FAT16B");
    assert!(region.claimed);
    assert!(region.issue.is_none());

    // The volume names the region it came from by identity.
    assert_eq!(report.volumes.len(), 1);
    assert_eq!(
        report.volumes[0].origin,
        VolumeOrigin::Regions(vec![region.id])
    );
    assert_eq!(report.volumes[0].start_bytes, region.start_bytes);

    // And the whole chain is traversable without a single array index.
    let volume = report.volumes[0].id;
    assert_eq!(report.volume(volume).map(|found| found.id), Some(volume));
    assert_eq!(
        report.filesystem_on(volume).and_then(|fs| fs.kind.clone()),
        Some("FAT16".to_owned())
    );
    assert_eq!(
        report.region(region.id).map(|found| found.id),
        Some(region.id)
    );

    std::fs::remove_file(&path).ok();
}

/// A structural region is reported and is not thereby a volume, and a
/// region this release will not read keeps its place: the regions behind
/// an unread one never renumber, and its reading still explains it.
#[test]
fn an_unread_region_is_explained_kept_and_composes_nothing() {
    let volume = synthetic_fat16();
    let disk_bytes = synthetic_multi_mbr(&[(0x06, &volume), (0x07, &volume), (0x06, &volume)]);
    let path = image_at("inspect-unread", &disk_bytes);
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.regions.len(), 3, "every declared region is reported");
    let unread = &report.regions[1];
    assert_eq!(unread.declared_type, 0x07);
    assert!(!unread.claimed, "0x07 is outside this release's claim");
    assert_eq!(
        unread.declared_type_reading, "NTFS or exFAT",
        "the refusal is quotable without a second type table"
    );
    assert!(unread.issue.is_some(), "the refusal stays on the region");

    // The region behind it kept its declared number and its identity.
    assert_eq!(report.regions[2].declared_number, 3);
    assert!(report.regions[2].claimed);

    // Two volumes composed, from the first and third regions only.
    assert_eq!(report.composed_volume_count(), 2);
    let origins: Vec<&VolumeOrigin> = report.volumes.iter().map(|v| &v.origin).collect();
    assert_eq!(
        origins,
        vec![
            &VolumeOrigin::Regions(vec![report.regions[0].id]),
            &VolumeOrigin::Regions(vec![report.regions[2].id]),
        ],
        "the unread region composed nothing and shifted nothing"
    );

    std::fs::remove_file(&path).ok();
}

/// Identity is a function of the layout's structure, so a later open in a
/// later process names the same objects. Nothing here parses the value.
#[test]
fn identities_survive_a_separate_open_of_an_unchanged_layout() {
    let path = image_at("inspect-stable", &synthetic_mbr_disk(&synthetic_fat16()));

    let first = {
        let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
        let disk = disk_session
            .medium_mut(disk_at)
            .expect("the medium is pooled");
        disk.inspect().expect("inspection reads")
    };
    let second = {
        let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image reopens");
        let disk = disk_session
            .medium_mut(disk_at)
            .expect("the medium is pooled");
        disk.inspect().expect("inspection reads")
    };

    assert_eq!(first.regions[0].id, second.regions[0].id);
    assert_eq!(first.volumes[0].id, second.volumes[0].id);
    assert_eq!(first.filesystems[0].id, second.filesystems[0].id);
    // A volume identity from one report resolves in the other.
    assert!(second.volume(first.volumes[0].id).is_some());

    std::fs::remove_file(&path).ok();
}

/// Recognition failing does not erase the volume it failed on, and the
/// two counts are separately available because of it.
#[test]
fn a_volume_whose_filesystem_is_unrecognized_stays_a_volume() {
    // A second region declared FAT16B whose payload is not in fact FAT.
    let good = synthetic_fat16();
    let mut rubbish = vec![0u8; good.len()];
    rubbish[..2].copy_from_slice(b"\xeb\x3c");
    rubbish[510] = 0x55;
    rubbish[511] = 0xaa;
    let path = image_at(
        "inspect-unrecognized",
        &synthetic_multi_mbr(&[(0x06, &good), (0x06, &rubbish)]),
    );
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.composed_volume_count(), 2, "both regions composed");
    assert_eq!(
        report.readable_filesystem_volume_count(),
        1,
        "only one carries a filesystem the host read"
    );
    let failed = report.volumes[1].id;
    assert!(
        report.volume(failed).is_some(),
        "the volume survives its filesystem's refusal"
    );
    let attempt = report
        .filesystem_on(failed)
        .expect("the attempt is recorded at the filesystem seam");
    assert!(attempt.kind.is_none(), "nothing was recognized");
    assert!(!attempt.issues.is_empty(), "and the refusal says why");
    assert!(
        report.volumes[1].issues.is_empty(),
        "the refusal belongs to the filesystem seam, not the volume"
    );

    std::fs::remove_file(&path).ok();
}

/// An extended partition is a region and not a volume; its logicals are
/// regions in their own right, each composing one.
#[test]
fn a_structural_region_is_reported_and_is_not_a_volume() {
    let path = image_at(
        "inspect-extended",
        &synthetic_extended_disk(&synthetic_fat16(), false),
    );
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");

    let structural: Vec<_> = report
        .regions
        .iter()
        .filter(|region| region.role == RegionRole::Structure)
        .collect();
    assert_eq!(structural.len(), 1, "one extended partition");
    assert_eq!(structural[0].declared_type, 0x05);
    assert_eq!(
        structural[0].declared_type_reading,
        "an extended partition, CHS-addressed"
    );

    let extended = structural[0].id;
    assert!(
        !report.volumes.iter().any(|volume| matches!(
            &volume.origin,
            VolumeOrigin::Regions(regions) if regions.contains(&extended)
        )),
        "the extended partition composed no volume"
    );
    // One primary plus two logicals.
    assert_eq!(report.composed_volume_count(), 3);
    assert_eq!(report.readable_filesystem_volume_count(), 3);

    std::fs::remove_file(&path).ok();
}

/// A valid schema with no data regions is the schema outcome with zero
/// volumes — not blank, and not an unknown payload.
// The FAT label answer. FAT records a label in two places and a volume
// may carry either, both, or disagreeing values; these fix the policy the
// filesystem seam owns, and the evidence it keeps beside the answer.

/// The root directory of `synthetic_fat16`: 1 reserved sector plus two
/// 32-sector FATs.
const FAT16_ROOT_OFFSET: usize = (1 + 2 * 32) * 512;

/// Writes an 11-byte fixed-width name field, space padded as FAT does.
fn write_field(image: &mut [u8], at: usize, text: &str) {
    image[at..at + 11].fill(b' ');
    image[at..at + text.len()].copy_from_slice(text.as_bytes());
}

/// Sets the root directory's volume-ID entry, or removes it entirely.
fn set_root_label(image: &mut [u8], label: Option<&str>) {
    match label {
        Some(text) => {
            write_field(image, FAT16_ROOT_OFFSET, text);
            image[FAT16_ROOT_OFFSET + 11] = 0x08; // volume-id attribute
        }
        None => image[FAT16_ROOT_OFFSET..FAT16_ROOT_OFFSET + 32].fill(0),
    }
}

/// Gives the volume an extended boot record under `signature`, with
/// `text` at the label field's offset. `synthetic_fat16` carries no
/// extended boot record at all, which is the third state.
fn set_boot_record(image: &mut [u8], signature: u8, text: &str) {
    image[38] = signature;
    write_field(image, 43, text);
}

/// Inspects `image` and returns the one volume's label answer.
fn label_answer(tag: &str, image: &[u8]) -> remanence::VolumeLabel {
    let path = image_at(tag, image);
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");
    let report = disk.inspect().expect("inspection reads");
    let answer = report
        .filesystem_on(report.volumes[0].id)
        .expect("FAT recognized")
        .label
        .clone()
        .expect("a recognized filesystem answers the label question");
    drop(session);
    std::fs::remove_file(&path).ok();
    answer
}

/// One source's reading, found by the name the seam gave it.
fn reading<'a>(label: &'a remanence::VolumeLabel, source: &str) -> Option<&'a str> {
    label
        .readings
        .iter()
        .find(|reading| reading.source == source)
        .expect("the source was read")
        .stored
        .as_deref()
}

#[test]
fn the_root_directory_entry_answers_and_both_readings_stay_beside_it() {
    // Only a root-directory entry: it answers, and the boot record's
    // field is absent rather than blank — the third state.
    let answer = label_answer("label-root-only", &synthetic_fat16());
    assert_eq!(answer.name.as_deref(), Some("REMANENCE"));
    assert_eq!(answer.answered_by.as_deref(), Some("root-directory-entry"));
    assert_eq!(reading(&answer, "root-directory-entry"), Some("REMANENCE"));
    assert_eq!(
        reading(&answer, "boot-record-field"),
        None,
        "no extended boot record means no such field, not a blank one"
    );

    // Both sources, disagreeing: the root entry is what DOS displays, so
    // it answers — and the boot record's own reading is still there for a
    // caller that wants it, without opening a sector.
    let mut image = synthetic_fat16();
    set_root_label(&mut image, Some("ROOTNAME"));
    set_boot_record(&mut image, 0x29, "BOOTNAME");
    let answer = label_answer("label-disagreeing", &image);
    assert_eq!(answer.name.as_deref(), Some("ROOTNAME"));
    assert_eq!(answer.answered_by.as_deref(), Some("root-directory-entry"));
    assert_eq!(reading(&answer, "root-directory-entry"), Some("ROOTNAME"));
    assert_eq!(reading(&answer, "boot-record-field"), Some("BOOTNAME"));

    // No root entry: the boot record's field answers.
    let mut image = synthetic_fat16();
    set_root_label(&mut image, None);
    set_boot_record(&mut image, 0x29, "BOOTNAME");
    let answer = label_answer("label-boot-only", &image);
    assert_eq!(answer.name.as_deref(), Some("BOOTNAME"));
    assert_eq!(answer.answered_by.as_deref(), Some("boot-record-field"));
    assert_eq!(reading(&answer, "root-directory-entry"), None);
}

#[test]
fn no_name_is_absence_and_absence_is_reported_as_absence() {
    // `NO NAME` is the format's own spelling of unlabeled, so the source
    // that holds it answers "none" rather than falling through to the
    // other one.
    let mut image = synthetic_fat16();
    set_root_label(&mut image, Some("NO NAME"));
    set_boot_record(&mut image, 0x29, "BOOTNAME");
    let answer = label_answer("label-no-name-root", &image);
    assert_eq!(answer.name, None, "the volume has no label");
    assert_eq!(
        answer.answered_by.as_deref(),
        Some("root-directory-entry"),
        "the source that decided is still named"
    );
    assert_eq!(reading(&answer, "root-directory-entry"), Some("NO NAME"));

    // The same at the other source.
    let mut image = synthetic_fat16();
    set_root_label(&mut image, None);
    set_boot_record(&mut image, 0x29, "NO NAME");
    let answer = label_answer("label-no-name-boot", &image);
    assert_eq!(answer.name, None);
    assert_eq!(answer.answered_by.as_deref(), Some("boot-record-field"));

    // An entry that exists and is blank is present, and answers absence.
    let mut image = synthetic_fat16();
    set_root_label(&mut image, Some(""));
    let answer = label_answer("label-blank-entry", &image);
    assert_eq!(answer.name, None);
    assert_eq!(answer.answered_by.as_deref(), Some("root-directory-entry"));
    assert_eq!(
        reading(&answer, "root-directory-entry"),
        Some(""),
        "present and blank, which is not the same as no such field"
    );

    // Neither source exists: nothing answered, and nothing was invented.
    let mut image = synthetic_fat16();
    set_root_label(&mut image, None);
    let answer = label_answer("label-neither", &image);
    assert_eq!(answer.name, None);
    assert_eq!(answer.answered_by, None);
    assert_eq!(reading(&answer, "root-directory-entry"), None);
    assert_eq!(reading(&answer, "boot-record-field"), None);
}

/// The boot record's field is only a field where the format says it is.
/// Signature 0x28 declares the shorter extended boot record, which stops
/// at the volume serial: reading the label offset regardless would
/// manufacture a label out of whatever bytes happen to sit there.
#[test]
fn the_boot_record_field_exists_only_under_its_own_signature() {
    let mut image = synthetic_fat16();
    set_root_label(&mut image, None);
    set_boot_record(&mut image, 0x28, "NOTALABEL");
    let answer = label_answer("label-short-ebr", &image);
    assert_eq!(answer.name, None, "nothing was manufactured from the bytes");
    assert_eq!(answer.answered_by, None, "no source existed to answer");
    assert_eq!(reading(&answer, "boot-record-field"), None);
}

#[test]
fn an_empty_partition_table_inspects_as_a_schema_with_no_volumes() {
    let mut bytes = vec![0u8; 4096];
    bytes[510] = 0x55;
    bytes[511] = 0xaa;
    let path = image_at("inspect-empty-table", &bytes);
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("image opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.content, DiskContent::Schema);
    assert!(
        report.partition_schema.is_some(),
        "the schema was recognized"
    );
    assert!(report.regions.is_empty(), "it declares no region");
    assert_eq!(report.composed_volume_count(), 0);
    assert_ne!(report.content, DiskContent::Blank, "distinct from blank");

    std::fs::remove_file(&path).ok();
}

// The DOS 8.3 namespace at the file-access seam (U3, U22): what a read
// matches, what a write stores, and which rule a refused name broke.

/// Returns the rule a refused name broke, insisting the refusal names one.
fn refused_rule(error: remanence::Error) -> DosNameRule {
    let identity = error
        .rule()
        .unwrap_or_else(|| panic!("a name refusal names its rule: {error}"));
    DosNameRule::from_identity(identity)
        .unwrap_or_else(|| panic!("'{identity}' is a rule of the DOS 8.3 set"))
}

/// A caller hands over the name it has and the library stores the DOS one:
/// uppercased and padded into the record. A read then matches without
/// regard to case and gives back the name the directory holds, so what a
/// caller shows a user is what is actually there.
#[test]
fn the_seam_normalizes_a_written_name_and_matches_a_read_one_without_case() {
    let path = temp_path("dos-names");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");
    let (mut session, at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = session.medium_mut(at).expect("the medium is pooled");

    fs(disk, 0).make_directory("out").expect("mkdir");
    fs(disk, 0)
        .write_file("out/x.txt", b"payload")
        .expect("write");

    let entries = fs(disk, 0).entries("OUT").expect("list");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].name, "X.TXT",
        "the listing returns the name as stored, not as supplied"
    );
    let root = fs(disk, 0).entries("").expect("list root");
    assert!(
        root.iter().any(|entry| entry.name == "OUT"),
        "the directory name was uppercased at the seam too"
    );

    for spelling in ["out/x.txt", "OUT/X.TXT", "Out/X.Txt"] {
        assert_eq!(
            fs(disk, 0).read_file(spelling).expect("read"),
            b"payload",
            "'{spelling}' matches the stored name without regard to case"
        );
    }
    assert_eq!(
        fs(disk, 0)
            .stat("out/x.txt")
            .expect("stat reads")
            .expect("the file is there")
            .name,
        "X.TXT"
    );

    // Case is not a second file: writing the same name in another case
    // overwrites the one record rather than adding a second.
    fs(disk, 0)
        .write_file("OUT/X.TXT", b"replaced")
        .expect("overwrite");
    assert_eq!(fs(disk, 0).entries("out").expect("list").len(), 1);

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// Every rule of the namespace is reachable through the file verbs, and
/// each refusal names the rule it broke rather than leaving a consumer to
/// reimplement the set to find out (P10). Nothing is truncated,
/// transliterated, or repaired to fit (P6).
#[test]
fn a_refused_name_names_the_rule_it_broke_and_writes_nothing() {
    let path = temp_path("dos-name-rules");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");
    let (mut session, at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = session.medium_mut(at).expect("the medium is pooled");

    let cases = [
        (".txt", DosNameRule::EmptyBase),
        ("longfilename.txt", DosNameRule::BaseTooLong),
        ("index.html", DosNameRule::ExtensionTooLong),
        ("archive.tar.gz", DosNameRule::Separator),
        ("draft.", DosNameRule::Separator),
        ("my file.txt", DosNameRule::ExcludedCharacter),
        ("report+1.txt", DosNameRule::ExcludedCharacter),
        (" lead.txt", DosNameRule::SurroundingSpace),
        ("trail .txt", DosNameRule::SurroundingSpace),
        ("con", DosNameRule::ReservedDeviceName),
        ("AUX.TXT", DosNameRule::ReservedDeviceName),
        ("com9", DosNameRule::ReservedDeviceName),
        ("lpt1", DosNameRule::ReservedDeviceName),
    ];
    for (name, expected) in cases {
        let error = fs(disk, 0)
            .write_file(name, b"contents")
            .expect_err("a name outside the namespace is refused");
        assert_eq!(refused_rule(error), expected, "writing '{name}'");

        let error = fs(disk, 0)
            .make_directory(name)
            .expect_err("a directory name takes the same rules");
        assert_eq!(refused_rule(error), expected, "creating '{name}'");
    }

    assert!(
        fs(disk, 0).entries("").expect("list root").is_empty(),
        "a refused name is refused, not repaired into some other name"
    );
    assert!(!disk.is_modified(), "nothing was staged for commit");

    // The rule sits beside the category rather than replacing it, and a
    // refusal belonging to no rule set carries none at all.
    let error = fs(disk, 0)
        .write_file("con", b"contents")
        .expect_err("reserved");
    assert_eq!(error.category(), ErrorCategory::Io);
    assert_eq!(
        fs(disk, 0)
            .read_file("MISSING.TXT")
            .expect_err("absent")
            .rule(),
        None
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// The partition pool and the vantage doors (P16, P17, P19, D26). A
// medium's content is reached through the partition that composes it, by
// the scheme's own ordinal — and where a medium records no scheme,
// through the direct partition the library composes and states as its
// own. The pool is established at the load and is evidence from then on;
// nothing below discovers a layout on demand.

/// A medium that records no scheme bears the direct partition, and the
/// library states it as its own composition rather than dressing it up as
/// something the medium said (P4).
#[test]
fn the_direct_partition_stands_where_a_medium_records_no_scheme() {
    let path = image_at("pool-direct", &synthetic_fat16());
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    assert_eq!(
        disk.partition_scheme(),
        None,
        "this image records no partition table, and the pool says so \
         rather than naming a scheme it was populated under"
    );
    let pool = disk.partitions();
    assert_eq!(
        pool.len(),
        1,
        "the direct partition is the whole of the pool"
    );
    let direct = &pool[0];
    assert_eq!(
        direct.ordinal(),
        0,
        "a scheme numbers its own entries from one, so zero is the \
         library's to spend on the partition no scheme declared"
    );
    assert!(
        direct.is_direct(),
        "it is the one member of the pool the library composes rather than reads"
    );
    assert_eq!(
        direct.placement(),
        "direct",
        "placement is the scheme's own vocabulary, and no scheme placed this"
    );
    assert_eq!(
        direct.role(),
        RegionRole::Data,
        "the whole content is data; there is no structure to be one of"
    );
    assert!(
        !direct.active(),
        "nothing flagged it, and nothing is derived"
    );
    assert!(
        !direct.is_claimed(),
        "there is no declared type to have read"
    );

    // A composition act is provenance, never evidence: the library says
    // what it composed, and never offers that as something it read.
    assert!(
        direct
            .provenance()
            .is_some_and(|account| !account.is_empty()),
        "the direct partition accounts for itself"
    );
    assert!(
        direct.evidence().is_empty(),
        "and read nothing to do it, so it claims no evidence"
    );
    assert_eq!(direct.type_byte(), None, "no scheme recorded a type here");
    assert_eq!(direct.type_reading(), None, "so there is nothing to read");

    // A declared reading is weighed against a recorded byte, and there is
    // none: the refusal names that rather than accepting a reading of
    // nothing (P3, P10).
    let refusal = direct
        .check_type(PartitionType::DosPrimary)
        .expect_err("nothing was recorded for the reading to be checked against");
    assert_eq!(
        refusal.rule(),
        Some(PartitionRule::NoDeclaredType.as_str()),
        "the refusal carries which rule of the seam's set it broke"
    );
    assert_eq!(
        refusal.category(),
        ErrorCategory::Unsupported,
        "the category answers how the caller should behave, beside the rule"
    );

    // The identity the pool carries is the identity the report issued, so
    // one volume is the same volume wherever it is met (P21, U4).
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(report.volumes.len(), 1, "this image composes one volume");
    assert_eq!(
        direct.volume_id(),
        Some(report.volumes[0].id),
        "the pool and the report name one volume between them, not two"
    );

    // The addressable door opens over the whole presented content.
    let mut space = disk
        .partition(0)
        .expect("the pool bears the direct partition")
        .volume()
        .expect("the whole content is one addressed extent");
    assert_eq!(
        space.start_bytes(),
        Some(0),
        "it begins where the presented content does"
    );
    assert_eq!(
        space.length_bytes(),
        Some(8000 * 512),
        "the direct partition runs the whole of the content"
    );
    assert_eq!(
        space.volume_id(),
        Some(report.volumes[0].id),
        "and the space answers with the identity the report issued"
    );
    let mut boot = [0_u8; 3];
    space.read_at(0, &mut boot).expect("the extent reads");
    assert_eq!(
        boot[0], 0xeb,
        "position zero within the partition is the volume's own boot record"
    );
    drop(space);

    // The namespace door answers absence, because no scheme declared a
    // type here and so no type determines a namespace. That is the honest
    // absence P19 requires, not a failure to have read one.
    assert!(
        disk.partition(0)
            .expect("the direct partition")
            .filesystem()
            .is_none(),
        "nothing declares a namespace over this partition"
    );

    // Which is exactly what the declared reading is for: the reading is
    // the caller's and the check is the library's (P18).
    let mut named = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("this image is FAT, and the recognizer verifies the declaration");
    assert_eq!(
        named.kind().expect("the declaration was verified"),
        FatKind::Fat16.name(),
        "the recognizer filled the values under the caller's reading"
    );
    assert!(
        named.entries("").is_ok(),
        "the file verbs answer on the node the door handed out"
    );
    assert!(
        named.is_addressable(),
        "and the same node still addresses its own extent"
    );

    drop(named);
    drop(session);
    std::fs::remove_file(&path).ok();
}

/// An MBR disk's pool is the table's own entries, in the table's own
/// order and under the table's own numbers.
#[test]
fn an_mbr_table_populates_the_pool_under_its_own_numbers() {
    let fat = synthetic_fat16();
    let path = image_at(
        "pool-mbr",
        &synthetic_multi_mbr(&[(0x06, &fat), (0x06, &fat)]),
    );
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    assert_eq!(
        disk.partition_scheme(),
        Some(PartitionScheme::Mbr),
        "the medium's family names the scheme its content is laid out under"
    );
    let pool = disk.partitions();
    let ordinals: Vec<u32> = pool.iter().map(|partition| partition.ordinal()).collect();
    assert_eq!(
        ordinals,
        [1, 2],
        "the pool keeps the table's own order and the table's own numbers"
    );
    assert!(
        pool.iter().all(|partition| !partition.is_direct()),
        "a medium that records a scheme bears no direct partition: there \
         was nothing for the library to compose"
    );
    assert!(
        pool.iter()
            .all(|partition| !partition.evidence().is_empty()),
        "and every declared partition says what was read to declare it (P4)"
    );
    assert!(
        pool.iter()
            .all(|partition| partition.provenance().is_none()),
        "none of them is a composition act, so none accounts for one"
    );

    let first = &pool[0];
    assert_eq!(
        first.type_byte(),
        Some(0x06),
        "the type value is kept exactly as the slot records it"
    );
    assert_eq!(
        first.type_reading(),
        Some("FAT16B"),
        "and what that value declares is quotable without a second type table"
    );
    assert!(first.is_claimed(), "0x06 is a type this release reads");
    assert_eq!(
        first.placement(),
        "primary",
        "it occupies one of the table's four slots"
    );
    assert_eq!(
        first.role(),
        RegionRole::Data,
        "and the scheme declares it as data rather than as structure"
    );
    assert_eq!(
        first.start_bytes(),
        Some(2048 * 512),
        "the extent is where the table said it is"
    );
    assert_eq!(
        first.length_bytes(),
        Some(fat.len() as u64),
        "and runs as far as the table said it runs"
    );
    assert!(
        first.is_addressable(),
        "a claimed data partition composes an extent to address"
    );
    assert!(
        first.bears_namespace(),
        "a DOS data partition determines FAT"
    );
    assert!(first.issue().is_none(), "nothing about it was refused");

    // The caller's own reading, checked against the recorded byte: 0x06
    // bears a DOS data partition and no extended container, and the
    // refusal names both sides rather than saying only that something
    // disagreed (P3, P10).
    let view = disk.partition(1).expect("the declared table bears entry 1");
    assert_eq!(
        view.ordinal(),
        1,
        "the view answers the same number the record does"
    );
    assert_eq!(
        view.type_byte(),
        Some(0x06),
        "and the same recorded value, without a second lookup"
    );
    assert_eq!(view.type_reading(), Some("FAT16B"), "and the same reading");
    assert!(
        !view.is_direct(),
        "the table declared it; the library did not"
    );
    view.check_type(PartitionType::DosPrimary)
        .expect("0x06 is one of the values that reading covers");
    let refusal = view
        .check_type(PartitionType::DosExtended)
        .expect_err("0x06 declares no extended container");
    assert_eq!(
        refusal.rule(),
        Some(PartitionRule::TypeDisagrees.as_str()),
        "the refusal carries which rule of the seam's set it broke"
    );
    assert_eq!(
        refusal.category(),
        ErrorCategory::InvalidImage,
        "a declaration the image contradicts is a fact about the image"
    );
    let message = refusal.to_string();
    assert!(
        message.contains("0x06"),
        "the refusal names the byte recorded: {message}"
    );
    assert!(
        message.contains("dos-extended"),
        "and the reading declared: {message}"
    );

    // And the declared type is what opens the namespace door: nothing was
    // probed for, because the declaration was settled at the load.
    let mut filesystem = view
        .filesystem()
        .expect("a DOS data partition determines FAT");
    assert_eq!(
        filesystem.kind().expect("the declaration was verified"),
        FatKind::Fat16.name(),
        "the recognizer verified what the partition type declared"
    );
    assert!(
        filesystem.entries("").is_ok(),
        "and the file verbs answer on the node that door handed out"
    );
    drop(filesystem);

    // Each partition is reached by the scheme's own number and answers
    // with the identity the report issued for it — never by position in a
    // list, and never by a door guessing which of the two was meant.
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(report.volumes.len(), 2, "both regions composed a volume");
    for (ordinal, composed) in [1_u32, 2].into_iter().zip(&report.volumes) {
        let mut space = fs(disk, ordinal);
        assert_eq!(
            space.volume_id(),
            Some(composed.id),
            "partition {ordinal} composes the volume the report issued for it"
        );
        assert!(
            space.entries("").is_ok(),
            "and partition {ordinal} is readable through its own door"
        );
    }

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// The boot flag is one of the scheme's own facts, read exactly as the
/// slot records it and derived from nothing else.
#[test]
fn the_boot_flag_is_read_as_the_table_records_it() {
    let fat = synthetic_fat16();
    let path = image_at(
        "pool-active",
        &synthetic_flagged_mbr(&[(0x06, false, &fat), (0x06, true, &fat)]),
    );
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let pool = disk.partitions();
    assert_eq!(pool.len(), 2, "the table declares two entries");
    assert!(
        !pool[0].active(),
        "the first slot's flag byte is zero, and no default is invented for it"
    );
    assert!(
        pool[1].active(),
        "the second slot records 0x80, which is the whole of what the flag is"
    );
    assert_eq!(
        pool[1].ordinal(),
        2,
        "the flag is a fact about the entry and changes nothing else about it"
    );
    assert!(
        disk.partition(2).expect("the table bears entry 2").active(),
        "the view answers the same fact the record does"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// A type outside this release's claim keeps its place in the pool: it is
/// still declared, still numbered, still explained — and the partitions
/// behind it never renumber (U4).
#[test]
fn a_type_outside_the_claim_keeps_its_place_and_opens_neither_door() {
    let fat = synthetic_fat16();
    let path = image_at(
        "pool-unread",
        &synthetic_multi_mbr(&[(0x06, &fat), (0x07, &fat), (0x06, &fat)]),
    );
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let pool = disk.partitions();
    let ordinals: Vec<u32> = pool.iter().map(|partition| partition.ordinal()).collect();
    assert_eq!(ordinals, [1, 2, 3], "every declared entry is in the pool");

    let unread = &pool[1];
    assert_eq!(
        unread.type_byte(),
        Some(0x07),
        "the raw value is kept exactly as the slot records it"
    );
    assert_eq!(
        unread.type_reading(),
        Some("NTFS or exFAT"),
        "the partition a caller most needs explained is the one the \
         library declines to read, so the reading is present regardless — \
         and it describes the declaration, never the content"
    );
    assert!(!unread.is_claimed(), "0x07 is outside this release's claim");
    assert!(
        unread.issue().is_some(),
        "the refusal stays on the partition rather than failing the disk"
    );
    assert!(!unread.is_addressable(), "it composes no extent");
    assert!(!unread.bears_namespace(), "and determines no namespace");
    assert_eq!(unread.volume_id(), None, "so it composed no volume either");

    assert!(
        disk.partition(2)
            .expect("entry 2 is still in the pool")
            .volume()
            .is_none(),
        "there is no extent to read by position within"
    );
    assert!(
        disk.partition(2)
            .expect("entry 2 is still in the pool")
            .filesystem()
            .is_none(),
        "and nothing determines a namespace over it"
    );

    // Nothing behind it shifted: entry 3 is still entry 3, and its own
    // door opens exactly as it would have.
    assert_eq!(pool[2].ordinal(), 3, "the unread entry renumbered nothing");
    assert!(
        pool[2].is_claimed(),
        "and the refusal in front of it cost it nothing"
    );
    assert!(
        fs(disk, 3).entries("").is_ok(),
        "the partition behind an unread one is reached by its own number"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// The extended container is structure rather than data. It is declared,
/// with the extent the table gave it, *and* it composes nothing: two
/// different facts, and the pool keeps both.
#[test]
fn a_structural_partition_is_declared_with_its_extent_and_opens_neither_door() {
    let path = image_at(
        "pool-structure",
        &synthetic_extended_disk(&synthetic_fat16(), false),
    );
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let pool = disk.partitions();
    let placements: Vec<&str> = pool.iter().map(|partition| partition.placement()).collect();
    assert_eq!(
        placements,
        ["primary", "primary", "logical", "logical"],
        "placement is the scheme's own vocabulary for where an entry sits"
    );

    let extended = &pool[1];
    assert_eq!(
        extended.role(),
        RegionRole::Structure,
        "placement and role are different axes and neither implies the \
         other: this one occupies a primary slot and its role is structural"
    );
    assert_eq!(
        extended.type_byte(),
        Some(0x05),
        "the value the slot records is what makes it the container"
    );
    assert_eq!(
        extended.type_reading(),
        Some("an extended partition, CHS-addressed"),
        "and the reading explains it in a sentence fit to show a user"
    );
    extended
        .check_type(PartitionType::DosExtended)
        .expect("0x05 is one of the values that reading covers");
    assert!(
        extended.start_bytes().is_some() && extended.length_bytes().is_some(),
        "it is still declared, and its extent is still what the table said"
    );
    assert_eq!(
        extended.ordinal(),
        2,
        "and it still numbers what follows it"
    );

    assert!(
        disk.partition(2)
            .expect("the extended container is in the pool")
            .volume()
            .is_none(),
        "a structural partition composes no volume to address"
    );
    assert!(
        disk.partition(2)
            .expect("the extended container is in the pool")
            .filesystem()
            .is_none(),
        "and determines no namespace to name files in"
    );

    // The logicals on its chain are partitions in their own right, each
    // reached by the number the walk gave it.
    for ordinal in [3_u32, 4] {
        assert!(
            fs(disk, ordinal).entries("").is_ok(),
            "logical partition {ordinal} is reached by its own number"
        );
    }

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// Both doors hand out the same node (D26). Which door was opened says
/// which question was asked, not which object comes back: the space
/// carries whichever vantages the partition has, either way.
#[test]
fn both_vantage_doors_hand_out_one_node() {
    let path = image_at("pool-one-node", &synthetic_mbr_disk(&synthetic_fat16()));
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let mut through_volume = disk
        .partition(1)
        .expect("the declared table bears entry 1")
        .volume()
        .expect("a DOS data partition composes an extent");
    assert!(
        through_volume.is_addressable(),
        "the door that was opened is the addressable one"
    );
    assert!(
        through_volume.has_namespace(),
        "and the node names its files too, because the partition has both"
    );
    let start = through_volume.start_bytes();
    let length = through_volume.length_bytes();
    let identity = through_volume.volume_id();
    let kind = through_volume
        .kind()
        .expect("the declared type was verified")
        .to_owned();
    assert!(
        through_volume.entries("").is_ok(),
        "the file verbs answer here, whichever door was opened"
    );
    drop(through_volume);

    let mut through_filesystem = disk
        .partition(1)
        .expect("the declared table bears entry 1")
        .filesystem()
        .expect("a DOS data partition determines FAT");
    assert!(
        through_filesystem.has_namespace(),
        "the door that was opened is the namespace one"
    );
    assert!(
        through_filesystem.is_addressable(),
        "the other door hands out the same node, extent and all"
    );
    assert_eq!(
        through_filesystem.start_bytes(),
        start,
        "one partition, one extent, one start"
    );
    assert_eq!(through_filesystem.length_bytes(), length, "and one length");
    assert_eq!(
        through_filesystem.volume_id(),
        identity,
        "and one volume identity, whichever door issued it"
    );
    assert_eq!(
        through_filesystem
            .kind()
            .expect("the declared type was verified"),
        kind,
        "and one namespace, recognized once per space rather than per door"
    );

    // The addressable vantage of the space the namespace door handed out
    // reads within the partition rather than within the medium, which is
    // the whole point of addressing within one.
    let mut boot = [0_u8; 1];
    through_filesystem
        .read_at(0, &mut boot)
        .expect("the extent reads");
    assert_eq!(
        boot[0], 0xeb,
        "position zero is the partition's first sector, not the disk's"
    );
    assert_eq!(
        start,
        Some(2048 * 512),
        "and the partition does not start where the medium does"
    );

    drop(through_filesystem);
    drop(session);
    std::fs::remove_file(&path).ok();
}

/// A namespace spelling this release does not read is refused by name,
/// and the refusal says what *is* read rather than only what is not (P3).
#[test]
fn a_declared_namespace_outside_the_claim_is_refused_by_name() {
    let path = image_at("pool-unclaimed-namespace", &synthetic_fat16());
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let refusal = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("ext4")
        .expect_err("this release reads no ext4");
    assert_eq!(
        refusal.rule(),
        Some(PartitionRule::UnclaimedNamespace.as_str()),
        "the refusal carries which rule of the seam's set it broke"
    );
    assert_eq!(
        refusal.category(),
        ErrorCategory::Unsupported,
        "a spelling outside the claim is a limit of this release, not a \
         fault in the image"
    );
    let message = refusal.to_string();
    assert!(
        message.contains("ext4"),
        "the refusal names what was read: {message}"
    );
    assert!(
        message.contains("fat") && message.contains("hdos"),
        "and what this release reads instead: {message}"
    );

    // A claimed spelling is claimed at this seam and still answerable at
    // the next: CP/M is a namespace this release recognizes and does not
    // read, and it refuses at the open under its own rule, because
    // recognizing and reading are separate claims.
    let recognized = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm")
        .expect_err("this release recognizes CP/M and does not read it");
    assert_eq!(
        recognized.rule(),
        Some(SpaceRule::RecognizedNotRead.as_str()),
        "the refusal belongs to the seam that made it, not to this one"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// An ordinal the pool never issued is answered with absence. Nothing is
/// manufactured to stand at it, and no error is invented for a question
/// that has a plain answer.
#[test]
fn an_ordinal_the_pool_never_issued_is_answered_with_absence() {
    let fat = synthetic_fat16();
    let path = image_at("pool-absent-ordinal", &synthetic_mbr_disk(&fat));
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    assert!(disk.partition(1).is_some(), "the table declares entry 1");
    assert!(
        disk.partition(2).is_none(),
        "the table declares no entry 2, and none is composed to fill the gap"
    );
    assert!(
        disk.partition(0).is_none(),
        "the direct partition stands only where a medium records no scheme, \
         and this one records an MBR table"
    );
    drop(session);
    std::fs::remove_file(&path).ok();

    // And the converse: an image recording no scheme bears the direct
    // partition, and nothing at the numbers a scheme would have used.
    let path = image_at("pool-absent-declared", &fat);
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");
    assert!(
        disk.partition(0).is_some(),
        "the direct partition is what its content is reached through"
    );
    assert!(
        disk.partition(1).is_none(),
        "and no table numbered anything for 1 to name"
    );
    drop(session);
    std::fs::remove_file(&path).ok();
}

/// A partition whose declared type determines a namespace the content
/// turns out not to bear is an ordinary partition still: the node answers
/// a named absence rather than an empty listing, and its addressable
/// vantage is untouched by the namespace's failure to be there.
#[test]
fn a_partition_bearing_no_readable_namespace_is_a_named_absence() {
    let good = synthetic_fat16();
    let mut rubbish = vec![0u8; good.len()];
    rubbish[..2].copy_from_slice(b"\xeb\x3c");
    rubbish[510] = 0x55;
    rubbish[511] = 0xaa;
    let path = image_at(
        "pool-absent-namespace",
        &synthetic_multi_mbr(&[(0x06, &good), (0x06, &rubbish)]),
    );
    let (mut session, attachment) = attach(&path, Afford::Read).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let report = disk.inspect().expect("inspection reads");
    let bare = report.volumes[1].id;
    let bare_length = report.volumes[1].length_bytes;

    // The declared type still determines a namespace, so the door opens;
    // what it hands over carries the recognizing seam's own refusal (P4).
    let refusal = disk
        .partition(2)
        .expect("the table declares entry 2")
        .filesystem()
        .expect("0x06 determines FAT, whatever the content turns out to be")
        .kind()
        .expect_err("it bears nothing this release recognizes");
    assert!(
        !refusal.to_string().is_empty(),
        "the absence is named rather than silent"
    );

    // And the space is still addressable, with its own extent: the 0..1 is
    // trait presence, so a partition bearing no namespace is an ordinary
    // volume rather than a failure to have composed one.
    let mut space = disk
        .partition(2)
        .expect("the table declares entry 2")
        .volume()
        .expect("the partition composed an extent");
    assert_eq!(
        space.volume_id(),
        Some(bare),
        "the identity is the one the report issued for this partition"
    );
    assert!(
        space.is_addressable(),
        "the partition composed its extent, whatever the extent holds"
    );
    assert!(
        !space.has_namespace(),
        "and bears no namespace, which is a fact about the content rather \
         than about the partition"
    );
    assert_eq!(
        space.length_bytes(),
        Some(bare_length),
        "the extent is the one the report stated for this volume"
    );

    // The addressable vantage reaches what no namespace names, and the
    // bound is the space's own rather than the medium's.
    let mut head = [0_u8; 16];
    space.read_at(0, &mut head).expect("the extent reads");
    let past = space
        .read_at(bare_length, &mut head)
        .expect_err("a read past the space's end is refused");
    assert_eq!(
        past.rule(),
        Some(SpaceRule::OutsideExtent.as_str()),
        "the bound is the space's own, not the medium's"
    );

    drop(space);
    drop(session);
    std::fs::remove_file(&path).ok();
}

/// `get_file` is where absence stops being an answer, and the file it
/// answers with offers both the whole-value and the bounded form (P27).
#[test]
fn a_file_view_offers_the_whole_value_and_the_bounded_form() {
    let path = temp_path("file-view");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");
    let (mut session, attachment) = attach(&path, Afford::Write).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let payload: Vec<u8> = (0..4000u32).map(|n| (n % 251) as u8).collect();
    fs(disk, 0)
        .write_file("PAYLOAD.BIN", &payload)
        .expect("write buffers");

    let mut filesystem = fs(disk, 0);
    let mut file = filesystem
        .get_file("payload.bin")
        .expect("matched case-insensitively");
    assert_eq!(file.name(), "PAYLOAD.BIN", "the name comes back as stored");
    assert_eq!(file.size_bytes(), payload.len() as u64);
    assert_eq!(file.bytes().expect("whole value"), payload);

    let mut window = [0u8; 64];
    file.read_at(1000, &mut window).expect("bounded form");
    assert_eq!(window[..], payload[1000..1064]);

    file.write_at(0, b"edited")
        .expect("the ranged write buffers");
    assert_eq!(
        &filesystem.read_file("PAYLOAD.BIN").expect("read")[..6],
        b"edited"
    );

    // Absence and a directory are both refusals here, unlike `stat`.
    filesystem.make_directory("SUB").expect("mkdir");
    assert!(filesystem.stat("NOPE.BIN").expect("an answer").is_none());
    assert_eq!(
        filesystem
            .get_file("NOPE.BIN")
            .expect_err("nothing is there")
            .category(),
        ErrorCategory::NotFound
    );
    assert_eq!(
        filesystem
            .get_file("SUB")
            .expect_err("a directory holds no bytes")
            .category(),
        ErrorCategory::IsDirectory
    );

    drop(filesystem);
    drop(session);
    std::fs::remove_file(&path).ok();
}

/// One node, both vantages: the space a door hands out addresses its own
/// extent *and* names its files, and a write through one vantage is
/// visible through the other.
///
/// This is what the addressable vantage exists for — reaching what a
/// namespace does not name, without the caller computing offsets against
/// the medium by hand.
#[test]
fn a_space_addresses_its_extent_and_names_its_files() {
    let path = temp_path("space-vantages");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut session, attachment) = attach(&path, Afford::Write).expect("image opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    let mut space = fs(disk, 0);

    // Both vantages, on the one object the door handed out.
    assert!(space.has_namespace(), "a FAT volume names files");
    assert!(space.is_addressable(), "and a volume composed its extent");
    let length = space.length_bytes().expect("it is addressable");

    // The addressable vantage reads within the space, not the medium: a
    // FAT volume's first bytes are its own boot record.
    let mut boot = [0_u8; 11];
    space.read_at(0, &mut boot).expect("the extent reads");
    assert_eq!(&boot[0..1], &[0xEB], "the volume's own boot record");

    // The bound is the space's own.
    let past = space
        .read_at(length, &mut boot)
        .expect_err("a read past the space's end is refused");
    assert_eq!(
        past.rule(),
        Some(SpaceRule::OutsideExtent.as_str()),
        "the bound is the space's own, not the medium's"
    );

    // A namespace write and an addressable read see one state, because
    // they are two vantages of one node over one active layer (P23).
    space
        .write_file("VANTAGE.TXT", b"one node")
        .expect("the namespace writes");
    let entry = space
        .stat("VANTAGE.TXT")
        .expect("the namespace reads")
        .expect("the file is there");
    assert_eq!(entry.size_bytes, 8);

    // And an addressable write lands in the same buffered state, rolled
    // back with everything else.
    let mut before = [0_u8; 4];
    space.read_at(0, &mut before).expect("reads");
    space.write_at(0, b"\xEB\x3C\x90\x00").expect("writes");
    let mut after = [0_u8; 4];
    space.read_at(0, &mut after).expect("reads back");
    assert_eq!(&after, b"\xEB\x3C\x90\x00", "the write is visible");

    drop(space);
    disk.rollback().expect("nothing reached the image");

    drop(session);
    std::fs::remove_file(&path).ok();
}
