// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The media pool and the edge into the device tier (P32): a session
//! holds media as state and machines as configuration, and the two meet
//! at exactly one place — `insert` and `eject`. These reach the
//! anonymous machine's device set through the session's own verbs; the
//! machine scope itself is `machines.rs`. These tests build their images
//! by hand, so they run without fixtures.

use std::path::PathBuf;

use remanence::{AttachmentId, DeviceFamily, ErrorCategory, Format, MediaId, Session};

mod common;
use common::{open_read, open_write};

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-devices-{tag}-{}-{nonce}.img",
        std::process::id()
    ))
}

/// A 1 MiB raw image — enough to open as a block medium, with no
/// filesystem claimed.
fn write_image(tag: &str) -> PathBuf {
    let path = temp_path(tag);
    std::fs::write(&path, vec![0u8; 1024 * 1024]).expect("image writes");
    path
}

/// The three acts, where a test cares about the result rather than the
/// sequence: pool the disk, add the drive, and link them.
fn seat(session: &mut Session, path: &PathBuf) -> remanence::Result<(MediaId, AttachmentId)> {
    let media = session.load_media(open_read(path), Format::Raw)?.id();
    let mut device = session.add_device(DeviceFamily::HARD_DISK)?;
    let attachment = device.attachment();
    device.insert(media)?;
    Ok((media, attachment))
}

#[test]
fn a_medium_is_loaded_unlinked_and_answers_before_any_device_exists() {
    // The pool is state and the machine is configuration; the medium is
    // the content handle and needs no drive to be one.
    let a = write_image("unlinked");
    let mut session = Session::new();

    let medium = session
        .load_media(open_read(&a), Format::Raw)
        .expect("the declaration is borne out");
    assert!(!medium.is_linked(), "a load links nothing");
    assert_eq!(medium.image_size_bytes(), 1024 * 1024);
    assert_eq!(medium.media_type(), "logical-block-512");
    let id = medium.id();

    assert_eq!(session.media(), vec![id]);
    assert!(session.medium(id).is_some());

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn a_device_is_added_empty_and_the_medium_is_inserted_into_it() {
    // The acts, and the empty device between them: a drive with no disk
    // in it is configuration in its own right (U22 letters one), not a
    // half-finished attach.
    let a = write_image("three-acts");
    let mut session = Session::new();

    let media = session
        .load_media(open_read(&a), Format::Raw)
        .expect("loads")
        .id();

    let mut device = session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("the drive is added");
    let id = device.attachment();
    assert_eq!(id.to_string(), "hdd0");
    assert!(!device.is_occupied(), "a fresh device holds nothing");

    // The content verbs are on the medium, and an empty slot holds none:
    // the lookup answers with absence rather than a manufactured error,
    // and a caller who wants a demand writes it.
    assert!(device.medium().is_none(), "absence is an answer");
    assert!(device.medium_mut().is_none(), "and the working form agrees");

    device.insert(media).expect("the disk goes in");
    assert!(device.is_occupied());
    assert_eq!(device.media_id(), Some(media));
    assert_eq!(
        device.medium().expect("occupied").image_size_bytes(),
        1024 * 1024
    );
    assert_eq!(
        device.attachment(),
        id,
        "the handle is the slot, and inserting did not move it"
    );

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn an_added_device_takes_the_lowest_free_slot_in_its_family() {
    let a = write_image("auto-a");
    let b = write_image("auto-b");
    let mut session = Session::new();

    let (_, first) = seat(&mut session, &a).expect("first seats");
    let (_, second) = seat(&mut session, &b).expect("second seats");

    assert_eq!(first.to_string(), "hdd0");
    assert_eq!(second.to_string(), "hdd1");
    assert_eq!(session.devices().len(), 2);

    // The identity is composed and predictable — deliberately unlike the
    // opaque region and volume identities a report issues, because a
    // device is machine configuration the caller supplied (P21).
    assert_eq!(first.family(), DeviceFamily::HARD_DISK);
    assert_eq!(first.index(), 0);

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn a_caller_may_choose_the_slot_and_leave_a_gap() {
    let mut session = Session::new();

    let named = session
        .add_device_at(DeviceFamily::HARD_DISK, 3)
        .expect("the named slot takes a device")
        .attachment();
    assert_eq!(named.to_string(), "hdd3");

    // The gap is real: the next added device fills slot 0, not slot 4.
    let auto = session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("added")
        .attachment();
    assert_eq!(auto.to_string(), "hdd0");
}

#[test]
fn a_taken_slot_is_refused_by_name_rather_than_displaced() {
    let mut session = Session::new();

    session
        .add_device_at(DeviceFamily::HARD_DISK, 0)
        .expect("first is added");
    let error = session
        .add_device_at(DeviceFamily::HARD_DISK, 0)
        .expect_err("the taken slot is refused");

    let message = error.to_string();
    assert!(message.contains("hdd0"), "names the slot: {message}");
    assert!(message.contains("release"), "names the remedy: {message}");
}

#[test]
fn an_occupied_device_is_refused_a_second_medium() {
    let a = write_image("occupied-a");
    let b = write_image("occupied-b");
    let mut session = Session::new();

    let (_, attachment) = seat(&mut session, &a).expect("first seats");
    let second = session
        .load_media(open_read(&b), Format::Raw)
        .expect("loads")
        .id();

    let error = session
        .device_mut(attachment)
        .expect("device")
        .insert(second)
        .expect_err("the occupied device is refused");

    let message = error.to_string();
    assert!(message.contains("hdd0"), "names the slot: {message}");
    assert!(message.contains("eject"), "names the remedy: {message}");

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn one_medium_is_in_one_drive_at_a_time() {
    // Two drives sharing one disk would be a machine no machine was, so
    // the second insert is refused naming where the medium already is.
    let a = write_image("one-drive");
    let mut session = Session::new();

    let (media, _) = seat(&mut session, &a).expect("seats");
    let error = session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("second drive")
        .insert(media)
        .expect_err("the medium is already in a drive");

    let message = error.to_string();
    assert!(message.contains("hdd0"), "names where it is: {message}");
    assert!(message.contains("eject"), "names the remedy: {message}");

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn ejecting_severs_and_the_medium_stays_in_the_pool() {
    // **Eject severs only.** The claim, the assurance and everything
    // buffered survive in the pool, which is what makes the medium the
    // held node and the device the slot it may sit in.
    let a = write_image("eject-a");
    let b = write_image("eject-b");
    let mut session = Session::new();

    let (media, attachment) = seat(&mut session, &a).expect("seats");

    let ejected = session
        .device_mut(attachment)
        .expect("device")
        .eject()
        .expect("ejects");
    assert_eq!(ejected, media, "eject answers with what left the slot");
    assert!(
        !session
            .device(attachment)
            .expect("the device stays")
            .is_occupied(),
        "the slot is empty again"
    );

    // The medium is still here, still answering, still claimed.
    let medium = session.medium(media).expect("the pool kept it");
    assert!(!medium.is_linked());
    assert_eq!(medium.image_size_bytes(), 1024 * 1024);

    let again = session
        .device_mut(attachment)
        .expect("device")
        .eject()
        .expect_err("there is nothing to eject twice");
    assert!(again.to_string().contains("hdd0"), "names the empty slot");

    // The same slot takes the next disk, and answers for that one.
    let second = session
        .load_media(open_read(&b), Format::Raw)
        .expect("loads")
        .id();
    session
        .device_mut(attachment)
        .expect("device")
        .insert(second)
        .expect("reseats");
    assert_eq!(
        session
            .device(attachment)
            .expect("device")
            .media_id()
            .expect("occupied"),
        second,
        "the device answers for what is in it now"
    );

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn releasing_a_device_ejects_first_and_destroys_nothing() {
    let a = write_image("release-device-a");
    let b = write_image("release-device-b");
    let mut session = Session::new();

    let (media, first) = seat(&mut session, &a).expect("seats");
    assert!(session.device(first).is_some());

    session.release_device(first).expect("released");
    assert!(
        session.device(first).is_none(),
        "a released identity resolves to nothing"
    );
    assert!(session.devices().is_empty());
    assert!(
        session.medium(media).is_some(),
        "configuration falls with its owner; state never does"
    );

    // Reuse is deliberate and safe: adding and releasing a device are
    // machine-down operations, so nothing live refers to the old
    // occupant. This is not the renumbering U4 refuses, because a slot is
    // configuration rather than evidence.
    let (_, reused) = seat(&mut session, &b).expect("re-adds");
    assert_eq!(reused.to_string(), "hdd0", "the freed slot is reused");

    // A release is not a lookup: an identity that resolves to nothing is
    // refused by name rather than answered with success.
    let missing = session
        .release_device(AttachmentId::parse("hdd7").expect("parses"))
        .expect_err("releasing nothing is refused");
    assert_eq!(missing.category(), ErrorCategory::NotFound);

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn release_media_is_the_one_state_destroying_verb() {
    let a = write_image("release");
    let mut session = Session::new();

    let (media, attachment) = seat(&mut session, &a).expect("seats");
    session.release_media(media).expect("released");

    assert!(session.medium(media).is_none(), "the state is gone");
    assert!(session.media().is_empty());
    assert!(
        !session
            .device(attachment)
            .expect("the device stays")
            .is_occupied(),
        "the release severed its own link"
    );
    assert!(
        session.release_media(media).is_err(),
        "the second release names an identity that resolves to nothing"
    );

    // The claim went with it: the artifact is free again.
    let mut after = Session::new();
    after
        .load_media(open_write(&a), Format::Raw)
        .expect("the claim was released with the medium");

    drop(after);
    std::fs::remove_file(&a).ok();
}

#[test]
fn only_a_concrete_family_instantiates() {
    // The P32 amendment's rule, and the reason for it: a device added as
    // "some floppy" declares nothing an insert could be checked against
    // and no drive a machine ever had.
    let mut session = Session::new();

    for interior in [
        DeviceFamily::STORAGE_DEVICE,
        DeviceFamily::FLOPPY_DRIVE,
        DeviceFamily::CBM_FLOPPY_DRIVE,
    ] {
        let error = session
            .add_device(interior)
            .expect_err("an interior name instantiates nothing");
        let message = error.to_string();
        assert!(
            message.contains(interior.id()),
            "names what was asked: {message}"
        );
        assert!(
            message.contains("classifies"),
            "says what such a name is for: {message}"
        );
    }

    // And the concrete entries below them do, each in its own slot.
    for concrete in DeviceFamily::concrete() {
        let device = session.add_device(concrete).expect("a concrete family");
        assert_eq!(device.family(), concrete);
        assert!(!device.is_occupied());
    }
    assert_eq!(session.devices().len(), DeviceFamily::concrete().len());
}

#[test]
fn a_medium_belonging_in_another_drive_is_refused_naming_both_sides() {
    // P14: a device accepts only the media its family is served, and this
    // is the check a concrete family exists to make possible. A raw image
    // holds logical-block media, which a hard disk takes and a Commodore
    // 1541 — served soft-sectored 5.25-inch disks — does not.
    let a = write_image("wrong-drive");
    let mut session = Session::new();

    let media = session
        .load_media(open_read(&a), Format::Raw)
        .expect("loads")
        .id();
    let mut drive = session
        .add_device(DeviceFamily::COMMODORE_1541)
        .expect("the drive is added");
    let error = drive
        .insert(media)
        .expect_err("a block image is not what a 1541 is served");

    let message = error.to_string();
    assert!(
        message.contains("cbmfloppy0"),
        "names the device: {message}"
    );
    assert!(
        message.contains("Commodore 1541"),
        "names the family: {message}"
    );
    assert!(
        message.contains("logical-block"),
        "names what the medium is: {message}"
    );
    assert!(
        message.contains("5.25-inch"),
        "names what the family is served: {message}"
    );
    assert!(
        !drive.is_occupied(),
        "a refused insert leaves the slot empty"
    );

    // Both sides survive the refusal: the slot is the caller's
    // configuration and the medium is the session's state.
    assert_eq!(session.devices().len(), 1);
    assert!(session.medium(media).is_some());

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn two_media_keep_their_volume_identities_medium_scoped() {
    // P21: device identity qualifies otherwise-local identifiers only
    // where more than one device makes it necessary, and an interface
    // already scoped to one disk may keep a disk-local identity.
    let a = write_image("scoped-a");
    let b = write_image("scoped-b");
    let mut session = Session::new();

    let (first, first_slot) = seat(&mut session, &a).expect("first seats");
    let (second, second_slot) = seat(&mut session, &b).expect("second seats");

    let first_report = session
        .medium_mut(first)
        .expect("medium")
        .inspect()
        .expect("first inspects");
    let first_volumes: Vec<u64> = first_report.volumes.iter().map(|v| v.id.value()).collect();

    let second_report = session
        .medium_mut(second)
        .expect("medium")
        .inspect()
        .expect("second inspects");
    let second_volumes: Vec<u64> = second_report.volumes.iter().map(|v| v.id.value()).collect();

    assert_eq!(
        first_volumes, second_volumes,
        "identities are disk-local, so two like disks issue like values"
    );
    assert_ne!(
        first_slot, second_slot,
        "the attachment identity is what tells the two apart"
    );
    assert_ne!(first, second, "and so is the pool identity");

    // The ordinal is the scheme's evidence and the identity is still the
    // library's, and the two answer alike here for different reasons. The
    // partition pools carry the same ordinals because each disk was read
    // on its own terms — a megabyte of zeroes records no scheme, so each
    // bears the library's own composition of the whole content at ordinal
    // 0 (P16) — while the identities their reports issue are compared
    // exactly as they were before any pool existed (P21).
    let ordinals_of = |session: &Session, media: MediaId| -> Vec<u32> {
        session
            .medium(media)
            .expect("the pool kept it")
            .partitions()
            .iter()
            .map(|partition| partition.ordinal())
            .collect()
    };
    assert_eq!(
        ordinals_of(&session, first),
        ordinals_of(&session, second),
        "the ordinals are each scheme's own, and neither is qualified by \
         the drive its medium sits in"
    );
    assert!(
        session
            .medium(first)
            .expect("the pool kept it")
            .partitions()
            .iter()
            .all(|partition| partition.is_direct() && partition.provenance().is_some()),
        "and where no scheme was recorded, the pool says in its own words \
         that what stands there is a composition act rather than evidence"
    );

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn a_medium_is_claimed_by_the_callers_own_open() {
    // P7 as amended: whoever opens owns the lock. The caller's exclusive
    // write open is the claim, and it holds for as long as the session
    // holds the medium — through an eject, which severs and nothing more.
    let a = write_image("claim");
    let mut session = Session::new();

    let media = session
        .load_media(open_write(&a), Format::Raw)
        .expect("loads")
        .id();
    assert_eq!(
        session.medium(media).expect("pooled").assurance().claim,
        remanence::Claim::CallerOpened
    );
    assert_eq!(
        session.medium(media).expect("pooled").mode(),
        remanence::AccessMode::ReadWrite,
        "the handle affords a write, so the session has one"
    );

    // A read-only handle affords no write, and the library never
    // escalates one it was handed.
    let mut reader = Session::new();
    let read_only = reader
        .load_media(open_read(&a), Format::Raw)
        .expect("a second reader is the caller's business, not ours");
    assert_eq!(read_only.mode(), remanence::AccessMode::ReadOnly);

    drop(reader);
    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn a_declaration_the_evidence_cannot_bear_is_refused_by_name() {
    // The declared reading is checked by exactly one adapter: nothing
    // probes for a second answer, and the refusal names both sides.
    let a = write_image("declared");
    let mut session = Session::new();

    let error = session
        .load_media(open_read(&a), Format::H8d)
        .expect_err("a megabyte of zeroes is no H17 disk");
    let message = error.to_string();
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    assert!(
        message.contains("h8d"),
        "names what was declared: {message}"
    );

    assert!(
        session.media().is_empty(),
        "a refused declaration pools nothing"
    );

    // The same artifact under a declaration it can bear loads.
    session
        .load_media(open_read(&a), Format::Raw)
        .expect("bytes are always bytes");

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn a_flux_family_artifact_is_refused_whatever_was_declared() {
    // P13: the raw reading opens anything, so without this check a P64
    // loaded happily and read as raw — declaring the block layer
    // authoritative when P64's own adapter declares flux. No device in
    // this release holds flux state; the artifact is reached through its
    // own type.
    let path = temp_path("flux-artifact");
    let mut bytes = b"P64-1541".to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    std::fs::write(&path, &bytes).expect("artifact writes");

    let mut session = Session::new();
    let error = session
        .load_media(open_read(&path), Format::Raw)
        .expect_err("a flux artifact is no block medium");

    let message = error.to_string();
    assert!(
        message.contains("flux"),
        "names the family found: {message}"
    );
    assert!(
        message.contains("own type"),
        "names where it is read instead: {message}"
    );
    assert!(session.media().is_empty(), "and nothing was pooled");

    drop(session);
    std::fs::remove_file(&path).ok();
}
