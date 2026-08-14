// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Discovery, declared defaults, and the one-step convenience over them:
//! asking what an artifact is before a machine has been configured for
//! it, consuming that answer into the media pool, and the convenience
//! that composes the acts where a format declares the drive it records.
//!
//! **This is the half of P7 the amendment leaves untouched**: discovery
//! names an artifact by path, so the library opens it and the mandatory
//! write-denial applies in full. `load_media` is the other half.
//! These tests build their images by hand, so they run without fixtures.

use std::path::PathBuf;

use remanence::{
    AccessIntent, AccessMode, AssuranceOutcome, AttachmentId, DeviceType, ErrorCategory,
    FloppyDrive, Format, HardDrive, Session, discover_media,
};

mod common;
use common::open_read;

fn temp_path(tag: &str, extension: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-discovery-{tag}-{}-{nonce}.{extension}",
        std::process::id()
    ))
}

/// A 1 MiB raw image — a block medium with no format declaring anything
/// about the machine it came from.
fn write_raw(tag: &str) -> PathBuf {
    let path = temp_path(tag, "img");
    std::fs::write(&path, vec![0u8; 1024 * 1024]).expect("image writes");
    path
}

/// An H8D-shaped image: 40 tracks of ten 256-byte records, which is the
/// size the format declares and the extension it is recognized by.
fn write_h8d(tag: &str) -> PathBuf {
    let path = temp_path(tag, "h8d");
    std::fs::write(&path, vec![0u8; 40 * 10 * 256]).expect("image writes");
    path
}

#[test]
fn a_discovery_says_what_the_artifact_is_and_where_it_could_go() {
    // Discovery is on no handle: no session, no machine, no device — it
    // consults catalogs and evidence, never configuration.
    let disk = write_h8d("what-it-is");
    let discovery = discover_media(&disk, AccessIntent::Read).expect("the artifact identifies");

    assert_eq!(discovery.image_format(), "h8d");
    assert_eq!(discovery.article(), "flexible-5.25-hard-10");
    assert_eq!(discovery.image_path(), Some(disk.as_path()));

    // Two different questions with two different answers. Where the
    // medium *could* go is derived by asking the device catalog what is
    // served the article; what wrote it is the image format's own
    // declaration, and an H8D records one device and no other.
    let accepting: Vec<&str> = discovery
        .accepting_devices()
        .iter()
        .map(|device| device.id())
        .collect();
    assert_eq!(accepting, vec!["h17"]);
    assert_eq!(
        discovery.device_type(),
        Some(DeviceType::Floppy(FloppyDrive::HeathH17))
    );

    // And it mutates nothing: the artifact is byte-for-byte what it was.
    drop(discovery);
    assert_eq!(
        std::fs::metadata(&disk).expect("still there").len(),
        40 * 10 * 256
    );
    std::fs::remove_file(&disk).ok();
}

#[test]
fn the_convenience_adds_the_declared_drive_and_loads_the_medium() {
    // One act to the caller, the same access path underneath: the device
    // it answers with is an ordinary device in the machine's own set.
    let disk = write_h8d("convenience");
    let mut session = Session::new();

    let device = session
        .add_device_for(&disk, AccessIntent::Read)
        .expect("the format declares the drive it records");
    assert_eq!(device.attachment().to_string(), "heathfloppy0");
    assert_eq!(
        device.device_type(),
        Some(DeviceType::Floppy(FloppyDrive::HeathH17))
    );
    assert!(
        device.is_occupied(),
        "the medium was loaded, not just found"
    );
    assert_eq!(
        device.medium().expect("occupied").image_path(),
        Some(disk.as_path())
    );

    // A fresh device every call, never a silent reuse of the slot that
    // is already there.
    let second = write_h8d("convenience-second");
    let other = session
        .add_device_for(&second, AccessIntent::Read)
        .expect("a second drive");
    assert_eq!(other.attachment().to_string(), "heathfloppy1");
    assert_eq!(
        session.attachments(),
        vec![
            AttachmentId::parse("heathfloppy0").expect("parses"),
            AttachmentId::parse("heathfloppy1").expect("parses"),
        ],
        "the attachment order a namespace composer reads is the order \
         the conveniences ran in"
    );

    drop(session);
    std::fs::remove_file(&disk).ok();
    std::fs::remove_file(&second).ok();
}

#[test]
fn a_format_recording_several_devices_refuses_by_name_toward_the_declaration() {
    // P3: a declaration nobody makes is a refusal, not a guess. Nothing
    // in a raw image says which drive wrote it — not even which family,
    // bytes belonging to none — so discovery reports what the artifact
    // is and asserts no device, and every path that would need one
    // refuses, naming the types a declaration may state.
    let image = write_raw("no-declaration");
    let mut session = Session::new();

    let discovery = discover_media(&image, AccessIntent::Read).expect("it is still a medium");
    assert_eq!(
        discovery.device_type(),
        None,
        "the artifact says which format it is, never which drive wrote it"
    );
    assert_eq!(
        discovery
            .device_types()
            .iter()
            .map(|device| device.id())
            .collect::<Vec<_>>(),
        vec!["mbr-sector-hd", "mbr-block-hd", "sector-floppy"],
        "and the format says which declarations it accepts, across families"
    );
    assert_eq!(
        discovery
            .accepting_devices()
            .iter()
            .map(|device| device.id())
            .collect::<Vec<_>>(),
        vec!["mbr-sector-hd", "mbr-block-hd", "gpt-hd"],
        "which is the other question entirely: every device served the \
         article, the one no adapter records included"
    );
    drop(discovery);

    let error = session
        .add_device_for(&image, AccessIntent::Read)
        .expect_err("an undeclared device is a refusal");
    let message = error.to_string();
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        message.contains("Raw disk image"),
        "names what was found: {message}"
    );
    assert!(
        message.contains("mbr-block-hd"),
        "names what may be declared: {message}"
    );
    assert!(
        message.contains("load_media"),
        "names where to declare it: {message}"
    );

    // And it left nothing behind: the refusal is one act's refusal, not
    // a half-configured machine.
    assert!(
        session.attachments().is_empty(),
        "a refused convenience adds no device"
    );

    // The same refusal guards the pool's plain door, so no medium is
    // ever seated or laid out without knowing what recorded it — and it
    // points at the door that takes the declaration.
    let discovery = discover_media(&image, AccessIntent::Read).expect("identifies");
    let error = session
        .load_discovery(discovery)
        .expect_err("the pool takes no medium that cannot say what recorded it");
    assert!(
        error.to_string().contains("load_discovery_as"),
        "names the door that takes the declaration: {error}"
    );

    // Which is the `_as` door, and it checks the declaration: the type
    // must be one the recognizing format records.
    let discovery = discover_media(&image, AccessIntent::Read).expect("identifies");
    let error = session
        .load_discovery_as(discovery, DeviceType::Floppy(FloppyDrive::Commodore1541))
        .expect_err("a raw image is no 1541 recording");
    assert!(
        error.to_string().contains("mbr-sector-hd"),
        "names what the format does record: {error}"
    );

    let discovery = discover_media(&image, AccessIntent::Read).expect("identifies");
    let declared = session
        .load_discovery_as(discovery, DeviceType::HardDrive(HardDrive::MbrBlock))
        .expect("the caller declares what the artifact could not");
    assert_eq!(
        declared.device_type(),
        Some(DeviceType::HardDrive(HardDrive::MbrBlock)),
        "and the medium carries the declaration, claim unbroken"
    );
    let declared = declared.id();
    session.release_media(declared).expect("released");

    // The explicit act remains available and is what the refusal points
    // at: declare the format and the device together, then state the
    // slot.
    let media = session
        .load_media(
            open_read(&image),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("the caller declares the format and the device")
        .id();
    assert_eq!(
        session
            .medium(media)
            .expect("pooled")
            .device_type()
            .expect("declared"),
        DeviceType::HardDrive(HardDrive::MbrSector)
    );
    session
        .add_device(HardDrive::MbrSector)
        .expect("the caller states the drive")
        .insert(media)
        .expect("and the medium goes into it");

    drop(session);
    std::fs::remove_file(&image).ok();
}

#[test]
fn a_discovery_is_consumed_by_the_load_and_the_claim_never_lapses() {
    // The discovery holds the claim taken when the artifact was
    // identified, so nothing can change the file between the question
    // and the load (P7 continuity) — and the load runs no second open.
    // The artifact is an H8D because the pool takes a medium that can
    // say what recorded it, and that format records one device.
    let image = write_h8d("consumed");
    let mut session = Session::new();

    let discovery =
        discover_media(&image, AccessIntent::Write).expect("the write claim is taken here");
    assert_eq!(discovery.mode(), AccessMode::ReadWrite);

    // Held, and demonstrably: a rival open of the same artifact is
    // refused while the discovery is alive.
    let rival = discover_media(&image, AccessIntent::Write)
        .expect_err("a second exclusive claim on the same file is refused");
    assert_eq!(rival.category(), ErrorCategory::Locked);

    let medium = session
        .load_discovery(discovery)
        .expect("the state moves into the pool");
    assert_eq!(
        medium.mode(),
        AccessMode::ReadWrite,
        "the intent is the discovery's own, not a second open's"
    );
    assert_eq!(
        medium.assurance().claim,
        remanence::Claim::LibraryOpened,
        "the library opened this one, and P7's denial is its own"
    );
    let media = medium.id();
    session
        .add_device(FloppyDrive::HeathH17)
        .expect("a drive to seat it in")
        .insert(media)
        .expect("the medium goes in");

    // The claim is now the pool's, and it is still the same one.
    let after = discover_media(&image, AccessIntent::Write)
        .expect_err("the medium is still claimed, by the pool now");
    assert_eq!(after.category(), ErrorCategory::Locked);

    drop(session);
    discover_media(&image, AccessIntent::Write).expect("dropping the session released it");
    std::fs::remove_file(&image).ok();
}

#[test]
fn a_discovered_medium_in_the_wrong_drive_is_refused_naming_both_sides() {
    // The family check is the same check whichever load made it: the
    // caller who names the drive and the format that declares it meet
    // one rule (P14).
    let disk = write_h8d("wrong-drive");
    let mut session = Session::new();

    let discovery = discover_media(&disk, AccessIntent::Read).expect("identifies");
    let media = session
        .load_discovery(discovery)
        .expect("the state pools whatever drive it belongs in")
        .id();
    let mut device = session
        .add_device(HardDrive::MbrSector)
        .expect("the wrong drive for it");
    let error = device
        .insert(media)
        .expect_err("a hard disk is not served a floppy");

    let message = error.to_string();
    assert!(
        message.contains("h17"),
        "names what recorded the medium: {message}"
    );
    assert!(message.contains("hdd0"), "names the slot: {message}");
    assert!(
        !device.is_occupied(),
        "a refused insert leaves the slot as it was"
    );

    // The medium survives the refused edge — it is state, and the slot
    // was configuration — and releasing it is what ends the claim.
    session.release_media(media).expect("released");
    discover_media(&disk, AccessIntent::Write).expect("the claim was released with it");

    drop(session);
    std::fs::remove_file(&disk).ok();
}

#[test]
fn discovery_refuses_a_foreign_family_artifact_where_a_load_would() {
    // P13: the block catalog opens anything it cannot identify at the
    // raw adapter, so a P64 must be refused at the same depth a load
    // refuses it. Reporting it as a logical-block medium would be the
    // library declaring the wrong layer authoritative.
    let path = temp_path("flux-artifact", "p64");
    let mut bytes = b"P64-1541".to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    std::fs::write(&path, &bytes).expect("artifact writes");

    let error =
        discover_media(&path, AccessIntent::Read).expect_err("a flux artifact is no device's");
    let message = error.to_string();
    assert!(
        message.contains("flux"),
        "names the family found: {message}"
    );
    assert!(
        message.contains("own declaration"),
        "names where it is read instead: {message}"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_discovery_creates_nothing_and_the_load_declares_the_bound() {
    // F67's constraint made observable: what a discovery answers is a
    // reading of the artifact, and the medium comes into existence at
    // the load, under the bound *that* verb declares. A discovery has
    // no bound to take — a verb that creates nothing has nothing to
    // bound (P27) — and every fact it reports comes off the claim it
    // holds rather than off a medium built ahead of the question.
    let disk = write_h8d("no-cache");
    let discovery = discover_media(&disk, AccessIntent::Read).expect("identifies");

    // Every fact answers before any medium exists.
    assert_eq!(discovery.image_format(), "h8d");
    assert_eq!(
        discovery.size().expect("a disk image presents a disk"),
        102_400
    );
    assert_eq!(discovery.image_size_bytes(), 102_400);
    assert_eq!(discovery.mode(), AccessMode::ReadOnly);
    assert_eq!(discovery.identify().layers.len(), 3);
    assert_eq!(discovery.assurance().outcome, AssuranceOutcome::Verified);

    // The load states the bound, and it is a real one: a single 64 KiB
    // extent, under which the medium still reads its whole extent
    // because unaltered extents evict and re-read (P27).
    let mut session = Session::new();
    let media = session
        .load_discovery_with_cache(discovery, 1)
        .expect("the discovery pools under the load's own bound")
        .id();
    let mut tail = [0u8; 16];
    session
        .medium(media)
        .expect("pooled")
        .read_at(102_384, &mut tail)
        .expect("the far end reads under a one-extent bound");
    assert_eq!(tail, [0u8; 16]);

    // And the claim was held from the question to the load: nothing was
    // re-opened, and the artifact is claimed by the pool now.
    let rival = discover_media(&disk, AccessIntent::Write)
        .expect_err("the medium the discovery became holds the claim");
    assert_eq!(rival.category(), ErrorCategory::Locked);

    session.release_media(media).expect("released");
    drop(session);
    std::fs::remove_file(&disk).ok();
}

#[test]
fn the_declared_door_takes_the_bound_the_same_way() {
    // The `_as` door creates the medium too, so the bound is declared
    // there for the same reason — the pair stays symmetrical.
    let image = write_raw("declared-bound");
    let discovery = discover_media(&image, AccessIntent::Read).expect("identifies");
    assert_eq!(
        discovery.device_type(),
        None,
        "a raw image records several device types and asserts none"
    );

    let mut session = Session::new();
    let media = session
        .load_discovery_as_with_cache(discovery, DeviceType::HardDrive(HardDrive::MbrBlock), 1)
        .expect("the caller declares the device and the bound at once");
    assert_eq!(
        media.device_type(),
        Some(DeviceType::HardDrive(HardDrive::MbrBlock))
    );
    let media = media.id();

    session.release_media(media).expect("released");
    drop(session);
    std::fs::remove_file(&image).ok();
}
