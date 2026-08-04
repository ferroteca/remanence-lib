// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! `Disk`-surface unit tests over synthetic images the project
//! owns outright: a hand-built FAT16 volume, bare and behind an MBR, on
//! raw disks. (qcow2 round-trips are unit-tested inside the crate, where the
//! writer can build the image.)

use std::path::PathBuf;

use remanence::{
    AccessIntent, AccessMode, AttachmentId, Disk, DiskContent, DiskFormat, DosNameRule,
    ErrorCategory, FatEntryKind, FatKind, RegionRole, Session, VolumeId, VolumeOrigin,
};

/// Attaches `path` to a fresh session and returns both, because a medium
/// is reachable only through the device holding it (P32). Tests keep the
/// session alive for as long as they use the medium.
fn attach(
    path: impl AsRef<std::path::Path>,
    intent: AccessIntent,
) -> remanence::Result<(Session, AttachmentId)> {
    let mut session = Session::new();
    let attachment = session.attach(path, intent)?;
    Ok((session, attachment))
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

/// The volume a caller works in, named the only way a caller can name
/// one: by asking the library what it reported. Nothing here builds an
/// identity or parses one.
fn only_volume(disk: &mut Disk) -> remanence::VolumeId {
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(report.volumes.len(), 1, "these images compose one volume");
    report.volumes[0].id
}

#[test]
fn fat16_roundtrip_on_a_bare_raw_image() {
    let path = temp_path("bare");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    assert_eq!(
        disk.mode(),
        AccessMode::ReadWrite,
        "the mode echoes the intent"
    );
    assert_eq!(disk.format(), DiskFormat::Raw);

    let report = disk.inspect().expect("inspection reads");
    // The device reports the medium attached to it (P14): a raw image is
    // logical-block media, named from the media-type catalog and stated
    // apart from what turned out to be recorded on it.
    assert_eq!(report.device.media_type, "logical-block-512");
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
    disk.make_directory(volume, "SUB").expect("mkdir");
    let payload: Vec<u8> = (0..2000u32).flat_map(|n| n.to_le_bytes()).collect();
    disk.write_file(volume, "SUB/HELLO.BIN", &payload)
        .expect("write");
    assert!(disk.is_modified());

    let entries = disk.entries(volume, "SUB").expect("list");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "HELLO.BIN");
    assert_eq!(entries[0].kind, FatEntryKind::File);
    assert_eq!(entries[0].size_bytes, payload.len() as u64);
    assert_eq!(
        disk.read_file(volume, "SUB/HELLO.BIN")
            .expect("read"),
        payload
    );
    assert_eq!(
        disk.read_file(volume, "SUB")
            .expect_err("directory is not a file")
            .category(),
        ErrorCategory::IsDirectory
    );
    assert_eq!(
        disk.entries(volume, "SUB/HELLO.BIN")
            .expect_err("file is not a directory")
            .category(),
        ErrorCategory::NotDirectory
    );
    assert_eq!(
        disk.read_file(volume, "SUB/MISSING.BIN")
            .expect_err("missing file is refused")
            .category(),
        ErrorCategory::NotFound
    );
    assert_eq!(
        disk.write_file(volume, "TOO-BIG.BIN", &vec![0u8; 5_000_000],)
            .expect_err("allocation exhaustion is refused")
            .category(),
        ErrorCategory::NoSpace
    );

    // Rollback: the image is untouched.
    disk.rollback();
    assert!(!disk.is_modified());
    assert!(disk.entries(volume, "SUB").is_err());

    // Write again and commit this time; overwriting replaces the
    // contents rather than refusing (U3).
    disk.write_file(volume, "KEPT.TXT", b"the first draft, rather longer")
        .expect("write");
    disk.write_file(volume, "KEPT.TXT", b"kept bytes")
        .expect("overwrite");
    disk.commit().expect("commit");
    assert!(!disk.is_modified());
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, AccessIntent::Read).expect("reopens");
    let reopened = reopened_session.medium(reopened_at).expect("the medium is attached");

    let volume_reopened = only_volume(reopened);
    assert_eq!(
        reopened.mode(),
        AccessMode::ReadOnly,
        "the mode echoes the intent"
    );
    assert_eq!(
        reopened
            .read_file(volume_reopened, "KEPT.TXT")
            .expect("read"),
        b"kept bytes"
    );
    drop(reopened_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn fat16_behind_an_mbr_partition() {
    let path = temp_path("mbr");
    std::fs::write(&path, synthetic_mbr_disk(&synthetic_fat16())).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
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
    assert_eq!(report.volumes[0].origin, VolumeOrigin::Regions(vec![region.id]));
    assert_eq!(
        report
            .filesystem_on(report.volumes[0].id)
            .and_then(label_of),
        Some("REMANENCE")
    );

    disk.write_file(volume, "ROOT.TXT", b"in the partition")
        .expect("write");
    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, AccessIntent::Read).expect("reopens");
    let reopened = reopened_session.medium(reopened_at).expect("the medium is attached");

    let volume_reopened = only_volume(reopened);
    assert_eq!(
        reopened.read_file(volume_reopened, "ROOT.TXT").expect("read"),
        b"in the partition"
    );
    drop(reopened_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn stat_answers_presence_and_absence_distinctly() {
    let path = temp_path("stat");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    disk.make_directory(volume, "SUB").expect("mkdir");
    disk.write_file(volume, "SUB/FILE.BIN", b"1234567890")
        .expect("write");

    let file = disk
        .stat(volume, "sub/file.bin")
        .expect("stat succeeds")
        .expect("the file exists");
    assert_eq!(file.name, "FILE.BIN");
    assert_eq!(file.kind, FatEntryKind::File);
    assert_eq!(file.size_bytes, 10);

    let directory = disk
        .stat(volume, "SUB")
        .expect("stat succeeds")
        .expect("the directory exists");
    assert_eq!(directory.kind, FatEntryKind::Directory);

    // Absence is an answer, not a failure: a missing leaf, a missing
    // parent, and a parent that is a file all answer None.
    assert_eq!(
        disk.stat(volume, "SUB/MISSING.BIN")
            .expect("an answer"),
        None
    );
    assert_eq!(
        disk.stat(volume, "NOWHERE/FILE.BIN")
            .expect("an answer"),
        None
    );
    assert_eq!(
        disk.stat(volume, "SUB/FILE.BIN/DEEPER.BIN")
            .expect("an answer"),
        None
    );

    // Failure stays failure: a missing volume identity, an empty path.
    assert_eq!(
        disk.stat(VolumeId::from_value(0xdead_beef), "FILE.BIN")
            .expect_err("no such volume")
            .category(),
        ErrorCategory::NotFound
    );
    assert!(
        disk.stat(volume, "").is_err(),
        "the root has no entry to answer with"
    );
    drop(disk_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn overwrite_releases_and_reclaims_clusters() {
    let path = temp_path("overwrite");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    // 7000 of the volume's 7903 data clusters: two of these can never
    // coexist, so each rewrite below only fits by releasing the last.
    let big = vec![0xabu8; 7000 * 512];
    disk.write_file(volume, "BIG.BIN", &big)
        .expect("first write");

    let replacement = vec![0xcdu8; 7000 * 512];
    disk.write_file(volume, "BIG.BIN", &replacement)
        .expect("overwriting releases the old clusters first");
    assert_eq!(
        disk.read_file(volume, "BIG.BIN").expect("read"),
        replacement
    );

    // Shrinking releases clusters for other files to claim.
    disk.write_file(volume, "BIG.BIN", b"now tiny")
        .expect("shrinking overwrite");
    disk.write_file(volume, "OTHER.BIN", &big)
        .expect("the released clusters are claimable again");

    // Overwriting a directory is refused by name.
    disk.make_directory(volume, "DIR").expect("mkdir");
    assert_eq!(
        disk.write_file(volume, "DIR", b"not a file")
            .expect_err("a directory is not overwritable")
            .category(),
        ErrorCategory::IsDirectory
    );

    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, AccessIntent::Read).expect("reopens");
    let reopened = reopened_session.medium(reopened_at).expect("the medium is attached");

    let volume_reopened = only_volume(reopened);
    assert_eq!(
        reopened.read_file(volume_reopened, "BIG.BIN").expect("read"),
        b"now tiny"
    );
    assert_eq!(
        reopened.read_file(volume_reopened, "OTHER.BIN").expect("read"),
        big
    );
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

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);

    // Missing parents are created in one call.
    disk.make_directory(volume, "A/B/C")
        .expect("missing parents are created");
    assert_eq!(
        disk.stat(volume, "A/B/C")
            .expect("stat")
            .expect("exists")
            .kind,
        FatEntryKind::Directory
    );

    // Already existing — wholly, partly, or the root itself — succeeds
    // unchanged, and the chain extends from wherever it stops.
    disk.make_directory(volume, "A/B/C")
        .expect("idempotent");
    disk.make_directory(volume, "A/B")
        .expect("an existing prefix succeeds");
    disk.make_directory(volume, "")
        .expect("the root already exists");
    disk.make_directory(volume, "A/B/C/D")
        .expect("extends the existing chain");

    // The created directories hold files like any other.
    disk.write_file(volume, "A/B/C/D/DEEP.TXT", b"nested payload")
        .expect("write");

    // A file in the way is refused by name, at the leaf or mid-path.
    assert_eq!(
        disk.make_directory(volume, "A/B/C/D/DEEP.TXT")
            .expect_err("a file at the leaf is refused")
            .category(),
        ErrorCategory::NotDirectory
    );
    assert_eq!(
        disk.make_directory(volume, "A/B/C/D/DEEP.TXT/E")
            .expect_err("a file mid-path is refused")
            .category(),
        ErrorCategory::NotDirectory
    );

    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, AccessIntent::Read).expect("reopens");
    let reopened = reopened_session.medium(reopened_at).expect("the medium is attached");

    let volume_reopened = only_volume(reopened);
    assert_eq!(
        reopened
            .read_file(volume_reopened, "A/B/C/D/DEEP.TXT")
            .expect("read"),
        b"nested payload"
    );
    drop(reopened_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_growing_subdirectory_never_collides_with_file_clusters() {
    let path = temp_path("dir-growth");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    disk.make_directory(volume, "SUB").expect("mkdir");

    // One 512-byte cluster holds 16 records; "." and ".." take two, so
    // the fifteenth file forces the directory to grow mid-write, and
    // the grown cluster must never collide with a file's data clusters.
    let names: Vec<String> = (0..20).map(|n| format!("FILE{n:02}.BIN")).collect();
    for (n, name) in names.iter().enumerate() {
        disk.write_file(volume, &format!("SUB/{name}"), &vec![n as u8; 700])
            .expect("write");
    }
    disk.commit().expect("commit");
    drop(disk_session);

    let (mut reopened_session, reopened_at) = attach(&path, AccessIntent::Read).expect("reopens");
    let reopened = reopened_session.medium(reopened_at).expect("the medium is attached");

    let volume_reopened = only_volume(reopened);
    let entries = reopened.entries(volume_reopened, "SUB").expect("list");
    assert_eq!(entries.len(), 20);
    for (n, name) in names.iter().enumerate() {
        assert_eq!(
            reopened
                .read_file(volume_reopened, &format!("SUB/{name}"))
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

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let report = disk.inspect().expect("blank is an answer, not an error");
    assert_eq!(report.content, DiskContent::Blank);
    assert!(report.regions.is_empty());
    assert!(report.volumes.is_empty());

    // Zero volumes means no identity names one. A value this disk never
    // issued resolves to nothing rather than to something plausible.
    assert_eq!(
        disk.entries(VolumeId::from_value(0), "")
            .expect_err("an unissued identity is refused")
            .category(),
        ErrorCategory::NotFound
    );
    drop(disk_session);

    std::fs::remove_file(&path).ok();
}

#[test]
fn the_extended_chain_reports_primary_and_logical_kinds() {
    let fat = synthetic_fat16();
    let path = temp_path("extended");
    std::fs::write(&path, synthetic_extended_disk(&fat, false)).expect("image writes");

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
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
            "primary",
            "primary", // the extended container
            "logical",
            "logical",
        ]
    );
    // Placement and role are different axes: the container occupies a
    // primary slot, and its role is structural rather than data.
    assert_eq!(report.regions[1].role, RegionRole::Container);
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

    // The file verbs take exactly the identities the report supplied.
    let volumes: Vec<_> = report.volumes.iter().map(|volume| volume.id).collect();
    for volume in volumes {
        assert!(
            disk.entries(volume, "").is_ok(),
            "volume {volume:?} readable"
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

    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let report = disk
        .inspect()
        .expect("a broken link does not fail the disk");

    // The primary, the container, and the first logical all stay; the
    // container region carries why the walk stopped.
    assert_eq!(report.regions.len(), 3);
    let numbers: Vec<u32> = report
        .regions
        .iter()
        .map(|region| region.declared_number)
        .collect();
    assert_eq!(numbers, [1, 2, 3], "nothing renumbers");
    let container = &report.regions[1];
    let issue = container
        .issue
        .as_ref()
        .expect("the container carries the issue");
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
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
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
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
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
fn p7_declared_intent_claims_and_refusals() {
    let path = temp_path("lock");
    std::fs::write(&path, synthetic_fat16()).expect("image writes");

    // A writable session admits no observers: while it holds the claim,
    // a second open fails fast whatever its intent.
    let (mut writer_session, writer_at) = attach(&path, AccessIntent::Write).expect("writable open");
    let writer = writer_session.medium(writer_at).expect("the medium is attached");
    assert_eq!(
        attach(&path, AccessIntent::Read)
            .expect_err("a reader is excluded while a writable session lives")
            .category(),
        ErrorCategory::Locked
    );
    assert_eq!(
        attach(&path, AccessIntent::Write)
            .expect_err("a second writer is excluded")
            .category(),
        ErrorCategory::Locked
    );
    drop(writer_session);

    // A read session keeps admitting other readers and still denies
    // every writer.
    let (mut reader_session, reader_at) = attach(&path, AccessIntent::Read).expect("read open");
    let reader = reader_session.medium(reader_at).expect("the medium is attached");
    let (mut second_session, second_at) = attach(&path, AccessIntent::Read).expect("second reader admitted");
    let second = second_session.medium(second_at).expect("the medium is attached");
    assert_eq!(
        attach(&path, AccessIntent::Write)
            .expect_err("a writer is refused while readers hold the file")
            .category(),
        ErrorCategory::Locked
    );
    drop(second_session);
    drop(reader_session);

    // A read-only file denies us write permission: a writable open
    // fails at the open — never a silent fallback — while a read open
    // proceeds and write actions are refused by name.
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).expect("set readonly");

    assert!(
        attach(&path, AccessIntent::Write).is_err(),
        "a writable open on a read-only file fails at the open"
    );
    let (mut readonly_session, readonly_at) = attach(&path, AccessIntent::Read).expect("read open proceeds");
    let readonly = readonly_session.medium(readonly_at).expect("the medium is attached");
    let volume_readonly = only_volume(readonly);
    assert_eq!(readonly.mode(), AccessMode::ReadOnly);
    assert!(readonly.inspect().is_ok(), "analysis proceeds");
    let refused = readonly
        .write_file(volume_readonly, "NO.TXT", b"denied")
        .expect_err("write actions are denied on a read session");
    assert_eq!(refused.category(), ErrorCategory::ReadOnly);
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

    let error = attach(&path, AccessIntent::Read).expect_err("future version refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    let message = error.to_string();
    assert!(
        message.contains("version 9") && message.contains("ceiling"),
        "refusal names the version and the ceiling: {message}"
    );

    std::fs::remove_file(&path).ok();
}

// The layered inspection report. These exercise `Disk::inspect` through
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
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.content, DiskContent::DirectVolume);
    assert!(report.partition_schema.is_none(), "no schema was recognized");
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
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

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
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

    let report = disk.inspect().expect("inspection succeeds on unknown content");
    let DiskContent::UnknownNonblank { evidence } = &report.content else {
        panic!("expected the unknown-nonblank outcome, got {:?}", report.content);
    };
    assert!(
        !evidence.is_empty(),
        "the outcome carries why nothing claimed it"
    );
    assert_ne!(report.content, DiskContent::Blank, "never confused with blank");
    assert!(report.volumes.is_empty());
    assert!(report.filesystems.is_empty());

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_partitioned_disk_reports_schema_regions_and_composed_volumes() {
    let path = image_at("inspect-mbr", &synthetic_mbr_disk(&synthetic_fat16()));
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

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
    assert_eq!(report.region(region.id).map(|found| found.id), Some(region.id));

    std::fs::remove_file(&path).ok();
}

/// A structural container is reported and is not thereby a volume, and a
/// region this release will not read keeps its place: the regions behind
/// an unread one never renumber, and its reading still explains it.
#[test]
fn an_unread_region_is_explained_kept_and_composes_nothing() {
    let volume = synthetic_fat16();
    let disk_bytes = synthetic_multi_mbr(&[(0x06, &volume), (0x07, &volume), (0x06, &volume)]);
    let path = image_at("inspect-unread", &disk_bytes);
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

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
        let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
        let disk = disk_session.medium(disk_at).expect("the medium is attached");
        disk.inspect().expect("inspection reads")
    };
    let second = {
        let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image reopens");
        let disk = disk_session.medium(disk_at).expect("the medium is attached");
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
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

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

/// An extended container is a region and not a volume; its logicals are
/// regions in their own right, each composing one.
#[test]
fn a_structural_container_is_reported_and_is_not_a_volume() {
    let path = image_at(
        "inspect-extended",
        &synthetic_extended_disk(&synthetic_fat16(), false),
    );
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

    let report = disk.inspect().expect("inspection reads");

    let containers: Vec<_> = report
        .regions
        .iter()
        .filter(|region| region.role == RegionRole::Container)
        .collect();
    assert_eq!(containers.len(), 1, "one extended container");
    assert_eq!(containers[0].declared_type, 0x05);
    assert_eq!(
        containers[0].declared_type_reading,
        "an extended partition, CHS-addressed"
    );

    let container = containers[0].id;
    assert!(
        !report.volumes.iter().any(|volume| matches!(
            &volume.origin,
            VolumeOrigin::Regions(regions) if regions.contains(&container)
        )),
        "the container composed no volume"
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
    let (mut session, attachment) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = session.medium(attachment).expect("the medium is attached");
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
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.content, DiskContent::Schema);
    assert!(report.partition_schema.is_some(), "the schema was recognized");
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
    let (mut session, at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = session.medium(at).expect("the medium is attached");
    let volume = only_volume(disk);

    disk.make_directory(volume, "out").expect("mkdir");
    disk.write_file(volume, "out/x.txt", b"payload")
        .expect("write");

    let entries = disk.entries(volume, "OUT").expect("list");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].name, "X.TXT",
        "the listing returns the name as stored, not as supplied"
    );
    let root = disk.entries(volume, "").expect("list root");
    assert!(
        root.iter().any(|entry| entry.name == "OUT"),
        "the directory name was uppercased at the seam too"
    );

    for spelling in ["out/x.txt", "OUT/X.TXT", "Out/X.Txt"] {
        assert_eq!(
            disk.read_file(volume, spelling).expect("read"),
            b"payload",
            "'{spelling}' matches the stored name without regard to case"
        );
    }
    assert_eq!(
        disk.stat(volume, "out/x.txt")
            .expect("stat reads")
            .expect("the file is there")
            .name,
        "X.TXT"
    );

    // Case is not a second file: writing the same name in another case
    // overwrites the one record rather than adding a second.
    disk.write_file(volume, "OUT/X.TXT", b"replaced")
        .expect("overwrite");
    assert_eq!(disk.entries(volume, "out").expect("list").len(), 1);

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
    let (mut session, at) = attach(&path, AccessIntent::Write).expect("disk opens");
    let disk = session.medium(at).expect("the medium is attached");
    let volume = only_volume(disk);

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
        let error = disk
            .write_file(volume, name, b"contents")
            .expect_err("a name outside the namespace is refused");
        assert_eq!(refused_rule(error), expected, "writing '{name}'");

        let error = disk
            .make_directory(volume, name)
            .expect_err("a directory name takes the same rules");
        assert_eq!(refused_rule(error), expected, "creating '{name}'");
    }

    assert!(
        disk.entries(volume, "").expect("list root").is_empty(),
        "a refused name is refused, not repaired into some other name"
    );
    assert!(!disk.is_modified(), "nothing was staged for commit");

    // The rule sits beside the category rather than replacing it, and a
    // refusal belonging to no rule set carries none at all.
    let error = disk
        .write_file(volume, "con", b"contents")
        .expect_err("reserved");
    assert_eq!(error.category(), ErrorCategory::Io);
    assert_eq!(
        disk.read_file(volume, "MISSING.TXT")
            .expect_err("absent")
            .rule(),
        None
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
