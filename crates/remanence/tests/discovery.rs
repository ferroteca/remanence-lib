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
    AccessIntent, AccessMode, AttachmentId, DeviceFamily, ErrorCategory, Format, Session,
    discover_media,
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
    assert_eq!(discovery.media_type(), "flexible-5.25-hard-10");
    assert_eq!(discovery.image_path(), Some(disk.as_path()));

    // Two different questions with two different answers. Where the
    // medium *could* go is derived by asking the families what they
    // accept; where it came *from* is the image format's declaration.
    let accepting: Vec<&str> = discovery
        .accepting_families()
        .iter()
        .map(|family| family.id())
        .collect();
    assert_eq!(accepting, vec!["heathkit-h17"]);
    assert_eq!(discovery.default_device(), Some(DeviceFamily::HEATHKIT_H17));

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
    assert_eq!(device.family(), DeviceFamily::HEATHKIT_H17);
    assert!(device.is_occupied(), "the medium was loaded, not just found");
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
fn a_format_declaring_no_default_refuses_by_name_toward_the_two_acts() {
    // P3: a declaration nobody makes is a refusal, not a guess. A raw
    // image says nothing about its machine, so the caller states the
    // drive — and the refusal says which drives would take the medium.
    let image = write_raw("no-default");
    let mut session = Session::new();

    let discovery = discover_media(&image, AccessIntent::Read).expect("it is still a medium");
    assert_eq!(discovery.default_device(), None, "raw declares no drive");
    assert_eq!(
        discovery
            .accepting_families()
            .iter()
            .map(|family| family.id())
            .collect::<Vec<_>>(),
        vec!["hard-disk"],
        "which is a different question, and it has an answer"
    );
    drop(discovery);

    let error = session
        .add_device_for(&image, AccessIntent::Read)
        .expect_err("no default is a refusal");
    let message = error.to_string();
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        message.contains("Raw disk image"),
        "names what was found: {message}"
    );
    assert!(
        message.contains("no default device"),
        "names what is missing: {message}"
    );
    assert!(
        message.contains("hard-disk"),
        "names the drive to state instead: {message}"
    );

    // And it left nothing behind: the refusal is one act's refusal, not
    // a half-configured machine.
    assert!(
        session.attachments().is_empty(),
        "a refused convenience adds no device"
    );

    // The explicit acts remain available and are what the refusal points
    // at: declare the format, then state the drive.
    let media = session
        .load_media(open_read(&image), Format::Raw)
        .expect("the caller declares the format")
        .id();
    session
        .add_device(DeviceFamily::HARD_DISK)
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
    let image = write_raw("consumed");
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
        .add_device(DeviceFamily::HARD_DISK)
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
        .add_device(DeviceFamily::HARD_DISK)
        .expect("the wrong drive for it");
    let error = device
        .insert(media)
        .expect_err("a hard disk is not served a floppy");

    let message = error.to_string();
    assert!(
        message.contains("hard-sectored"),
        "names the medium: {message}"
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
    assert!(message.contains("flux"), "names the family found: {message}");
    assert!(
        message.contains("own type"),
        "names where it is read instead: {message}"
    );

    std::fs::remove_file(&path).ok();
}
