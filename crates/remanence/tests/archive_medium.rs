// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The archive as a medium (P14): it is loaded by a declared reading
//! like every other medium, may be inserted into a device of its own
//! family, and its content is walked through the namespace door of the
//! direct partition it bears.
//!
//! What these tests hold to is the *uniformity*. There is no archive
//! journey beside the storage model any more: the same declared load,
//! the same partition pool, the same one node carrying file verbs, and
//! the vantage the medium actually has deciding what it answers. An
//! archive's grammar determines its namespace, so the plain door opens;
//! nothing composed an extent for it to be a position within, so the
//! addressable door answers nothing and the space verbs refuse by name
//! rather than inventing a phantom volume to satisfy them (D26).
//!
//! These tests build their zip by hand, so they run without fixtures.

use std::path::PathBuf;

use remanence::{
    AttachmentId, DeviceFamily, DosAssignmentRule, EntryKind, ErrorCategory, Format, MediaId,
    Session,
};

mod common;
use common::{open_read, open_write};

/// Pools an archive under its declared grammar and answers with its
/// identity — the load every test below starts from.
fn zip_medium(session: &mut Session, path: &PathBuf) -> MediaId {
    session
        .load_media(open_read(path), Format::Zip)
        .expect("the archive loads")
        .id()
}

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

    let arc = zip_medium(&mut session, &path);
    let mut device = session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("the slot is added");
    assert_eq!(device.attachment().to_string(), "arc0");
    assert!(!device.is_occupied(), "a slot with no archive in it");

    device.insert(arc).expect("the archive goes in");
    assert!(device.is_occupied());
    let medium = device.medium().expect("occupied");
    assert_eq!(
        std::fs::canonicalize(medium.path().expect("this host names its handles"))
            .expect("the recovered name resolves"),
        std::fs::canonicalize(&path).expect("the original resolves"),
        "the name is recovered from the handle, for location alone"
    );

    // The identification reports the nesting it was reached through, and
    // nothing was recognized inside: an entry is recognized when it is
    // opened as an artifact of its own.
    let identification = medium.identify();
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

    let arc = zip_medium(&mut session, &path);
    session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("added")
        .insert(arc)
        .expect("the archive goes in");
    let disk = session
        .load_media(open_read(&image), Format::Raw)
        .expect("the disk loads")
        .id();
    session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("added")
        .insert(disk)
        .expect("the disk goes in");

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
    let block = session
        .load_media(open_read(&image), Format::Raw)
        .expect("the disk loads")
        .id();
    let error = session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("added")
        .insert(block)
        .expect_err("a disk image is no archive");
    let message = error.to_string();
    assert!(message.contains("arc0"), "names the device: {message}");
    assert!(message.contains("archive medium"), "names the family's media: {message}");

    // And an archive is not what a hard disk is served.
    let arc = zip_medium(&mut session, &archive);
    let error = session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("added")
        .insert(arc)
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
    // The vantage decides what a medium answers. An archive *does* bear
    // a partition — the direct one, the library's own composition of its
    // whole content — but it records no scheme, composes no volume and
    // has no sector to address, so the verbs that address a space refuse
    // by name and point at the door the content is actually reached
    // through, rather than inventing a phantom volume to satisfy them
    // (D26).
    let bytes = payload();
    let path = write_zip("vantage", &[("disk.h8d", &bytes)]);
    let mut session = Session::new();
    let arc = zip_medium(&mut session, &path);
    let medium = session.medium_mut(arc).expect("the medium is pooled");

    let error = medium.inspect().expect_err("an archive inspects no space");
    let message = error.to_string();
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(message.contains("inspect"), "names the verb: {message}");
    assert!(message.contains("namespace"), "names the vantage: {message}");
    assert!(
        message.contains("direct partition"),
        "and names the door the content is reached through: {message}"
    );

    // Which is exactly what the pool holds. The direct partition is a
    // composition act — stated as provenance, never offered as something
    // the artifact said — and over a namespace-native medium it is
    // extent-less: there is no position for it to be a position within,
    // so the addressable door answers nothing rather than minting a
    // volume over the whole file (D26).
    assert_eq!(
        medium.partition_scheme(),
        None,
        "an archive records no partition scheme"
    );
    let partitions = medium.partitions();
    assert_eq!(partitions.len(), 1, "one direct partition, and nothing beside it");
    let direct = &partitions[0];
    assert!(direct.is_direct());
    assert_eq!(direct.ordinal(), 0, "the ordinal no scheme numbers");
    assert_eq!(
        direct.start_bytes(),
        None,
        "an archive's names sit over no addressed extent"
    );
    assert_eq!(direct.length_bytes(), None, "and run for no addressed length");
    assert!(!direct.is_addressable());
    assert!(direct.provenance().is_some(), "a composition act is provenance");
    assert!(direct.evidence().is_empty(), "and never evidence");
    assert!(
        medium
            .partition(0)
            .expect("an archive bears its direct partition")
            .volume()
            .is_none(),
        "and no phantom volume is invented to satisfy the space verbs"
    );

    for refusal in [
        medium.size().err(),
        medium.format().err(),
        medium.commit().err(),
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
    medium.read_at(0, &mut prefix).expect("the artifact reads");
    assert_eq!(&prefix, b"PK\x03\x04");
    assert_eq!(
        medium.image_size_bytes(),
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
    let arc = zip_medium(&mut session, &path);
    let mut namespace = session
        .medium_mut(arc)
        .expect("the medium is pooled")
        .partition(0)
        .expect("an archive bears its direct partition")
        .filesystem()
        .expect("an archive's content is its namespace");

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

    let arc = zip_medium(&mut session, &path);
    session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("added")
        .insert(arc)
        .expect("the archive goes in");

    let discovery = session
        .medium_mut(arc)
        .expect("the medium is pooled")
        .partition(0)
        .expect("an archive bears its direct partition")
        .filesystem()
        .expect("an archive's content is its namespace")
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
    assert_eq!(
        discovery.image_path(),
        Some(std::path::Path::new("disks/boot.h8d"))
    );

    let disk = session
        .load_discovery(discovery)
        .expect("the disk pools")
        .id();
    session.add_machine("h89").expect("the machine is added");
    session
        .machine_mut("h89")
        .expect("is there")
        .add_device(DeviceFamily::HEATHKIT_H17)
        .expect("added")
        .insert(disk)
        .expect("the disk goes in");
    let medium = session.medium_mut(disk).expect("the medium is pooled");
    assert_eq!(medium.image_size_bytes(), IMAGE_LEN as u64);

    // Its identification carries the nesting it came through.
    let identification = medium.identify();
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

    let arc = zip_medium(&mut session, &path);
    session
        .add_device(DeviceFamily::ARCHIVE_DEVICE)
        .expect("added")
        .insert(arc)
        .expect("the archive goes in");
    let discovery = session
        .medium_mut(arc)
        .expect("the medium is pooled")
        .partition(0)
        .expect("an archive bears its direct partition")
        .filesystem()
        .expect("an archive's content is its namespace")
        .get_file("disk.h8d")
        .expect("the entry is there")
        .discover()
        .expect("an artifact of its own");

    let disk = session
        .load_discovery(discovery)
        .expect("the disk pools")
        .id();
    session
        .add_device(DeviceFamily::HEATHKIT_H17)
        .expect("added")
        .insert(disk)
        .expect("the disk goes in");

    // Out comes the archive; the disk keeps reading.
    session
        .device_mut(arc0())
        .expect("the archive device")
        .eject()
        .expect("ejects");
    let mut tail = [0u8; 32];
    session
        .medium_mut(disk)
        .expect("the medium is pooled")
        .read_at(IMAGE_LEN as u64 - 32, &mut tail)
        .expect("the disk still reads");
    assert_eq!(&tail[..], &bytes[IMAGE_LEN - 32..]);

    // And so does releasing the archive's device altogether, and
    // releasing the archive itself — the disk is free-standing state.
    session.release_device(arc0()).expect("released");
    session.release_media(arc).expect("the archive leaves");
    session
        .medium_mut(disk)
        .expect("the medium is pooled")
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
    let device = session
        .load_media(open_write(&path), Format::Raw)
        .expect("the floppy loads");

    // The floppy records no partition scheme, so its content is reached
    // through the direct partition — which composes a volume over the
    // whole image, and determines no namespace, so the FAT reading is
    // the caller's declaration and the adapter's check (P18).
    let volume = device.inspect().expect("inspects").volumes[0].id;
    let mut space = device
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("the declaration is verified against the boot record");
    assert_eq!(
        space.volume_id(),
        Some(volume),
        "the identity the report issued names the same volume here (P21, U4)"
    );
    space.write_file("NOTE.TXT", b"remanence").expect("writes");
    drop(space);

    let error = device
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("the declaration still checks out")
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
