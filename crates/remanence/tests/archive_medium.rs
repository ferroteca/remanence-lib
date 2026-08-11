// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The archive as a medium (P14): it enters a machine the way every
//! medium does — a device of its own family, `load_media` into it,
//! content walked through the namespace that device resolves to.
//!
//! What these tests hold to is the *uniformity*. There is no archive
//! journey beside the storage model any more: the same two acts, the
//! same one storage handle, the same one node carrying file verbs, and
//! the vantage the medium actually has deciding what it answers. A
//! namespace-native medium refuses the space verbs by name rather than
//! inventing a phantom volume to satisfy them.
//!
//! These tests build their zip by hand, so they run without fixtures.

use std::path::PathBuf;

use remanence::{
    AccessIntent, AttachmentId, DeviceFamily, DosAssignmentRule, EntryKind, ErrorCategory, Session,
};

fn temp_path(tag: &str, extension: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-archive-{tag}-{}-{nonce}.{extension}",
        std::process::id()
    ))
}

/// The h8d-sized payload one member holds, so the entry loaded out of
/// the archive is a medium a real drive is served.
const IMAGE_LEN: usize = 102_400;

fn payload() -> Vec<u8> {
    (0..IMAGE_LEN as u32).map(|n| (n % 251) as u8).collect()
}

/// A zip of stored entries, in the order given: local headers, then the
/// central directory, then the end record.
fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = Vec::new();
    let mut offsets = Vec::new();

    for (name, data) in entries {
        offsets.push(zip.len() as u32);
        zip.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // stored
        zip.extend_from_slice(&[0u8; 4]);
        zip.extend_from_slice(&[0u8; 4]);
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(name.as_bytes());
        zip.extend_from_slice(data);
    }

    let central_offset = zip.len() as u32;
    for ((name, data), offset) in entries.iter().zip(&offsets) {
        zip.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&[0u8; 4]);
        zip.extend_from_slice(&[0u8; 4]);
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&offset.to_le_bytes());
        zip.extend_from_slice(name.as_bytes());
    }
    let central_size = zip.len() as u32 - central_offset;

    zip.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    zip.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    zip.extend_from_slice(&central_size.to_le_bytes());
    zip.extend_from_slice(&central_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip
}

fn write_zip(tag: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = temp_path(tag, "zip");
    std::fs::write(&path, build_zip(entries)).expect("zip writes");
    path
}

/// A 1 MiB raw image, which is logical-block media and belongs in a hard
/// disk.
fn write_image(tag: &str) -> PathBuf {
    let path = temp_path(tag, "img");
    std::fs::write(&path, vec![0u8; 1024 * 1024]).expect("image writes");
    path
}

fn arc0() -> AttachmentId {
    AttachmentId::parse("arc0").expect("parses")
}

#[test]
fn an_archive_loads_into_its_own_device_and_answers_for_the_medium() {
    let bytes = payload();
    let path = write_zip("medium", &[("disk.h8d", &bytes)]);
    let mut session = Session::new();

    let device = session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("the slot is added");
    assert_eq!(device.attachment().to_string(), "arc0");
    assert!(!device.is_occupied(), "a slot with no archive in it");

    device
        .load_media(&path, AccessIntent::Read)
        .expect("the archive loads");
    assert!(device.is_occupied());
    assert_eq!(device.path().expect("occupied"), path.display().to_string());

    // The identification reports the nesting it was reached through, and
    // nothing was recognized inside: an entry is recognized when it is
    // opened as an artifact of its own.
    let identification = device.identify().expect("occupied");
    assert_eq!(identification.layers.len(), 1);
    assert_eq!(identification.layers[0].id, "zip");
    assert!(!identification.modified);

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_archive_slot_is_a_device_like_any_other() {
    // The question this feature settles: the slot is visible in its
    // machine's attachment namespace. A device a caller cannot see would
    // be the one kind that behaves unlike every other, and the composer
    // below already passes it over by family.
    let bytes = payload();
    let path = write_zip("visible", &[("disk.h8d", &bytes)]);
    let image = write_image("beside");
    let mut session = Session::new();

    session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("added")
        .load_media(&path, AccessIntent::Read)
        .expect("the archive loads");
    session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("added")
        .load_media(&image, AccessIntent::Read)
        .expect("the disk loads");

    let attachments: Vec<String> = session
        .attachments()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(attachments, vec!["arc0".to_owned(), "hdd0".to_owned()]);

    // And a namespace composer passes it over **by family**, which is
    // what makes the visibility harmless: an archive has no partition or
    // volume for an assignment rule to reach.
    let map = session
        .anonymous_mut()
        .compose_dos_letters(Some(DosAssignmentRule::MsDos5), &[])
        .expect("composes");
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("passed over by family") && line.contains("arc0")),
        "the archive slot is passed over and said so: {:?}",
        map.provenance
    );

    drop(session);
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&image).ok();
}

#[test]
fn a_medium_in_the_wrong_device_is_refused_naming_both_sides() {
    let bytes = payload();
    let archive = write_zip("mismatch", &[("disk.h8d", &bytes)]);
    let image = write_image("mismatch");
    let mut session = Session::new();

    // A block image is not what an archive slot is served.
    let slot = session.add_device(DeviceFamily::ARCHIVE_DEVICE).expect("added");
    let error = slot
        .load_media(&image, AccessIntent::Read)
        .expect_err("a disk image is no archive");
    let message = error.to_string();
    assert!(message.contains("arc0"), "names the device: {message}");
    assert!(message.contains("archive medium"), "names the family's media: {message}");

    // And an archive is not what a hard disk is served.
    let disk = session.add_device(DeviceFamily::HARD_DISK).expect("added");
    let error = disk
        .load_media(&archive, AccessIntent::Read)
        .expect_err("an archive is no logical-block medium");
    let message = error.to_string();
    assert!(message.contains("hdd0"), "names the device: {message}");
    assert!(
        message.contains("archive medium") && message.contains("logical-block"),
        "names both sides: {message}"
    );

    drop(session);
    std::fs::remove_file(&archive).ok();
    std::fs::remove_file(&image).ok();
}

#[test]
fn a_namespace_native_medium_refuses_the_space_verbs_by_name() {
    // The vantage decides what a medium answers. An archive has no
    // partition, no volume and no sector, so the verbs that address a
    // space say so rather than inventing a phantom volume to satisfy
    // them (D26).
    let bytes = payload();
    let path = write_zip("vantage", &[("disk.h8d", &bytes)]);
    let mut session = Session::new();
    let device = session.add_device(DeviceFamily::ARCHIVE_DEVICE).expect("added");
    device
        .load_media(&path, AccessIntent::Read)
        .expect("the archive loads");

    let error = device.inspect().expect_err("an archive inspects no space");
    let message = error.to_string();
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(message.contains("inspect"), "names the verb: {message}");
    assert!(message.contains("namespace"), "names the vantage: {message}");

    for refusal in [
        device.size().err(),
        device.format().err(),
        device.commit().err(),
    ] {
        let message = refusal.expect("a space verb refuses").to_string();
        assert!(
            message.contains("archive medium"),
            "every space verb names the vantage: {message}"
        );
    }

    // The evidence plane is not a space, and every medium has one: the
    // artifact's own bytes read.
    let mut prefix = [0u8; 4];
    device.read_at(0, &mut prefix).expect("the artifact reads");
    assert_eq!(&prefix, b"PK\x03\x04");
    assert_eq!(
        device.image_size_bytes().expect("occupied"),
        std::fs::metadata(&path).expect("stat").len()
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_namespace_reports_the_grammars_own_hierarchy() {
    // A name with a `/` in it says there is a directory, and listing it
    // reads what the archive recorded rather than manufacturing a
    // pseudo-file (P19).
    let bytes = payload();
    let path = write_zip(
        "tree",
        &[
            ("readme.txt", b"remanence"),
            ("disks/boot.h8d", &bytes),
            ("disks/spare.h8d", &bytes),
        ],
    );
    let mut session = Session::new();
    let device = session.add_device(DeviceFamily::ARCHIVE_DEVICE).expect("added");
    device
        .load_media(&path, AccessIntent::Read)
        .expect("the archive loads");
    let mut namespace = device.filesystem().expect("an archive is its namespace");

    let root = namespace.entries("").expect("the root lists");
    let names: Vec<&str> = root.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, vec!["readme.txt", "disks"], "in the archive's own order");
    assert_eq!(root[0].kind, EntryKind::File);
    assert_eq!(root[1].kind, EntryKind::Directory);
    assert!(
        root[1]
            .declared
            .iter()
            .any(|fact| fact.key == "declared-by"),
        "a directory the grammar records no entry for says what put it there"
    );

    let inner = namespace.entries("disks").expect("the directory lists");
    let names: Vec<&str> = inner.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, vec!["boot.h8d", "spare.h8d"]);

    // Absence is an answer; a file is not a directory.
    assert!(namespace.stat("disks/nothing.h8d").expect("asked").is_none());
    let error = namespace
        .entries("readme.txt")
        .expect_err("a file holds no names");
    assert_eq!(error.category(), ErrorCategory::NotDirectory);

    // Bounded and whole reads both answer, and agree.
    let mut file = namespace.get_file("readme.txt").expect("the file is there");
    assert_eq!(file.bytes().expect("whole"), b"remanence");
    let mut part = [0u8; 4];
    file.read_at(2, &mut part).expect("a span reads");
    assert_eq!(&part, b"mane");

    drop(file);
    drop(namespace);
    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_entry_is_loaded_into_a_device_of_its_own_in_a_machine_of_its_own() {
    // Recursion is the same journey again. The host's archive was never
    // part of the machine whose disk it holds, so the disk goes in a
    // machine of its own — and the two slots do not know about each
    // other.
    let bytes = payload();
    let path = write_zip("nested", &[("disks/boot.h8d", &bytes)]);
    let mut session = Session::new();

    session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("added")
        .load_media(&path, AccessIntent::Read)
        .expect("the archive loads");

    let discovery = session
        .require_device(arc0())
        .expect("the archive device")
        .filesystem()
        .expect("a namespace")
        .get_file("disks/boot.h8d")
        .expect("the entry is there")
        .discover()
        .expect("the entry is an artifact of its own");

    // The discovery answers for the artifact inside, not for the archive
    // holding it: the medium is the disk, and the drive it belongs in is
    // the one the format records.
    assert_eq!(discovery.media_type(), "flexible-5.25-hard-10");
    assert_eq!(discovery.image_format(), "h8d");
    assert_eq!(
        discovery.default_device(),
        Some(DeviceFamily::HEATHKIT_H17),
        "the format declares the drive it records"
    );
    assert_eq!(discovery.image_path(), std::path::Path::new("disks/boot.h8d"));

    session.add_machine("h89").expect("the machine is added");
    let device = session
        .require_machine("h89")
        .expect("is there")
        .add_device(DeviceFamily::HEATHKIT_H17)
        .expect("added");
    device.load_discovery(discovery).expect("the disk loads");
    assert_eq!(device.image_size_bytes().expect("occupied"), IMAGE_LEN as u64);

    // Its identification carries the nesting it came through.
    let identification = device.identify().expect("occupied");
    assert_eq!(identification.layers[0].id, "zip");
    assert_eq!(identification.layers[1].id, "h8d");

    // The archive stays in the host machine, and the disk is in its own.
    assert_eq!(session.attachments(), vec![arc0()]);
    assert_eq!(
        session
            .machine("h89")
            .expect("is there")
            .attachments()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["heathfloppy0".to_owned()]
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_disk_loaded_from_an_archive_outlives_the_archive_being_ejected() {
    // The backing relationship this feature settles: a stored entry is
    // source-backed through the archive's own claim and a coded one is
    // session-backed in private storage, and either way the child holds
    // what it reads. Ejecting the archive under it takes nothing away.
    let bytes = payload();
    let path = write_zip("outlives", &[("disk.h8d", &bytes)]);
    let mut session = Session::new();

    session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("added")
        .load_media(&path, AccessIntent::Read)
        .expect("the archive loads");
    let discovery = session
        .require_device(arc0())
        .expect("the archive device")
        .filesystem()
        .expect("a namespace")
        .get_file("disk.h8d")
        .expect("the entry is there")
        .discover()
        .expect("an artifact of its own");

    let drive = session
        .add_device(DeviceFamily::HEATHKIT_H17)
        .expect("added");
    drive.load_discovery(discovery).expect("the disk loads");

    // Out comes the archive; the disk keeps reading.
    session
        .require_device(arc0())
        .expect("the archive device")
        .eject()
        .expect("ejects");
    let drive = session
        .require_device(AttachmentId::parse("heathfloppy0").expect("parses"))
        .expect("the drive is there");
    let mut tail = [0u8; 32];
    drive
        .read_at(IMAGE_LEN as u64 - 32, &mut tail)
        .expect("the disk still reads");
    assert_eq!(&tail[..], &bytes[IMAGE_LEN - 32..]);

    // And so does removing the archive's device altogether.
    session.remove_device(arc0()).expect("removed");
    let drive = session
        .require_device(AttachmentId::parse("heathfloppy0").expect("parses"))
        .expect("the drive is there");
    drive
        .read_at(0, &mut tail)
        .expect("the disk is still the disk");

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_file_on_a_volume_is_refused_as_an_artifact_by_name() {
    // The claim is bounded and says so: this release opens an archive
    // entry as an artifact of its own, and a file on a volume-backed
    // filesystem is read through the filesystem that names it (P3).
    let path = crate_fat_floppy("volume-file");
    let mut session = Session::new();
    let device = session.add_device(DeviceFamily::HARD_DISK).expect("added");
    device
        .load_media(&path, AccessIntent::Write)
        .expect("the floppy loads");

    let volume = device.inspect().expect("inspects").volumes[0].id;
    device
        .volume(volume)
        .expect("the volume")
        .write_file("NOTE.TXT", b"remanence")
        .expect("writes");

    let error = device
        .volume(volume)
        .expect("the volume")
        .get_file("NOTE.TXT")
        .expect("the file is there")
        .discover()
        .expect_err("a file on a volume is not minted as an artifact here");
    let message = error.to_string();
    assert!(message.contains("archive entry"), "names the claim: {message}");
    assert!(message.contains("NOTE.TXT"), "names the file: {message}");

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// A 1.44 MiB FAT12 floppy image, formatted enough to compose one volume.
fn crate_fat_floppy(tag: &str) -> PathBuf {
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
    let path = temp_path(tag, "img");
    std::fs::write(&path, image).expect("image writes");
    path
}
