// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The storage-device tier (P32): a machine holds a dynamic set of
//! family-typed devices, each a durable slot distinct from the medium in
//! it, and **devices are added while media are loaded — as two acts**.
//! These reach that set through the session's own verbs, which are its
//! anonymous machine's; the machine scope itself is `machines.rs`. These
//! tests build their images by hand, so they run without fixtures.

use std::path::PathBuf;

use remanence::{AccessIntent, AttachmentId, DeviceFamily, ErrorCategory, Session};

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

/// The two acts, where a test cares about the result rather than the
/// pair: add the drive, load the disk, and answer with the slot it took.
fn load(session: &mut Session, path: &PathBuf, intent: AccessIntent) -> remanence::Result<AttachmentId> {
    let device = session.add_device(DeviceFamily::HARD_DISK)?;
    let attachment = device.attachment();
    device.load_media(path, intent)?;
    Ok(attachment)
}

#[test]
fn a_device_is_added_empty_and_the_medium_is_loaded_into_it() {
    // The two acts, and the empty device between them: a drive with no
    // disk in it is configuration in its own right (U22 letters one), not
    // a half-finished attach.
    let a = write_image("two-acts");
    let mut session = Session::new();

    let device = session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("the drive is added");
    let id = device.attachment();
    assert_eq!(id.to_string(), "hdd0");
    assert!(!device.is_occupied(), "a fresh device holds nothing");

    // Every content verb refuses by name while the slot is empty.
    let error = device.identify().expect_err("a content verb needs a medium");
    let message = error.to_string();
    assert_eq!(error.category(), ErrorCategory::NotFound);
    assert!(message.contains("hdd0"), "names the slot: {message}");

    device.load_media(&a, AccessIntent::Read).expect("the disk goes in");
    assert!(device.is_occupied());
    assert_eq!(device.image_size_bytes().expect("occupied"), 1024 * 1024);
    assert_eq!(
        device.attachment(),
        id,
        "the handle is the slot, and loading did not move it"
    );

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn an_added_device_takes_the_lowest_free_slot_in_its_family() {
    let a = write_image("auto-a");
    let b = write_image("auto-b");
    let mut session = Session::new();

    let first = load(&mut session, &a, AccessIntent::Read).expect("first loads");
    let second = load(&mut session, &b, AccessIntent::Read).expect("second loads");

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
    let a = write_image("slot-a");
    let mut session = Session::new();

    let named = session
        .add_device_at(DeviceFamily::HARD_DISK, 3)
        .expect("the named slot takes a device")
        .attachment();
    assert_eq!(named.to_string(), "hdd3");
    let _ = &a;

    // The gap is real: the next added device fills slot 0, not slot 4.
    let auto = session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("added")
        .attachment();
    assert_eq!(auto.to_string(), "hdd0");

    drop(session);
    std::fs::remove_file(&a).ok();
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
    assert!(message.contains("remove"), "names the remedy: {message}");
}

#[test]
fn an_occupied_device_is_refused_a_second_medium() {
    let a = write_image("occupied-a");
    let b = write_image("occupied-b");
    let mut session = Session::new();

    let device = session.add_device(DeviceFamily::HARD_DISK).expect("added");
    device.load_media(&a, AccessIntent::Read).expect("first loads");
    let error = device
        .load_media(&b, AccessIntent::Read)
        .expect_err("the occupied device is refused");

    let message = error.to_string();
    assert!(message.contains("hdd0"), "names the slot: {message}");
    assert!(message.contains("eject"), "names the remedy: {message}");

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn ejecting_leaves_the_device_and_takes_the_medium() {
    // The device outlives every medium loaded into it, which is what
    // makes this a tier rather than a rename of the medium.
    let a = write_image("eject-a");
    let b = write_image("eject-b");
    let mut session = Session::new();

    let device = session.add_device(DeviceFamily::HARD_DISK).expect("added");
    device.load_media(&a, AccessIntent::Read).expect("loads");
    let first = device.path().expect("occupied").to_owned();

    device.eject().expect("ejects");
    assert!(!device.is_occupied(), "the slot is empty again");
    let error = device.identify().expect_err("the view has nothing to view");
    assert_eq!(error.category(), ErrorCategory::NotFound);

    let again = device.eject().expect_err("there is nothing to eject twice");
    assert!(again.to_string().contains("hdd0"), "names the empty slot");

    // The same handle takes the next disk, and answers for that one.
    device.load_media(&b, AccessIntent::Read).expect("reloads");
    let second = device.path().expect("occupied").to_owned();
    assert_ne!(first, second, "the device answers for what is in it now");

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn a_removed_slot_is_free_again_and_its_identity_stops_resolving() {
    let a = write_image("remove-a");
    let b = write_image("remove-b");
    let mut session = Session::new();

    let first = load(&mut session, &a, AccessIntent::Read).expect("loads");
    assert!(session.device(first).is_some());

    session.remove_device(first).expect("removed");
    assert!(
        session.device(first).is_none(),
        "a removed identity resolves to nothing"
    );
    assert!(session.devices().is_empty());

    // Reuse is deliberate and safe: adding and removing a device are
    // machine-down operations, so nothing live refers to the old
    // occupant. This is not the renumbering U4 refuses, because a slot is
    // configuration rather than evidence.
    let reused = load(&mut session, &b, AccessIntent::Read).expect("re-adds");
    assert_eq!(reused.to_string(), "hdd0", "the freed slot is reused");

    let missing = session
        .remove_device(AttachmentId::parse("hdd7").expect("parses"))
        .expect_err("removing nothing is refused");
    assert_eq!(missing.category(), ErrorCategory::NotFound);

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn the_device_is_the_slot_and_the_medium_is_what_occupies_it() {
    let a = write_image("slot-vs-medium");
    let mut session = Session::new();

    let id = load(&mut session, &a, AccessIntent::Read).expect("loads");
    let device = session.device(id).expect("device exists");
    assert!(device.is_occupied());
    assert_eq!(device.attachment(), id);
    assert_eq!(device.family(), DeviceFamily::HARD_DISK);

    // The medium is reached through the device, and nothing else.
    let medium = session.require_device(id).expect("the medium is reachable");
    assert_eq!(medium.image_size_bytes().expect("a medium is attached"), 1024 * 1024);

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn only_a_concrete_family_instantiates() {
    // The P32 amendment's rule, and the reason for it: a device added as
    // "some floppy" declares nothing a load could be checked against and
    // no drive a machine ever had.
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

    let drive = session
        .add_device(DeviceFamily::COMMODORE_1541)
        .expect("the drive is added");
    let error = drive
        .load_media(&a, AccessIntent::Read)
        .expect_err("a block image is not what a 1541 is served");

    let message = error.to_string();
    assert!(message.contains("cbmfloppy0"), "names the device: {message}");
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
    assert!(!drive.is_occupied(), "a refused load leaves the slot empty");

    // The device is still there, which is the two acts working: the slot
    // is the caller's configuration and a wrong disk does not remove it.
    assert_eq!(session.devices().len(), 1);

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn two_devices_keep_their_volume_identities_device_scoped() {
    // P21: device identity qualifies otherwise-local identifiers only
    // where more than one device makes it necessary, and an interface
    // already scoped to one disk may keep a disk-local identity. Every
    // file verb is reached through the device that owns the medium, so
    // identical identities on two devices name different things and
    // neither is re-derived.
    let a = write_image("scoped-a");
    let b = write_image("scoped-b");
    let mut session = Session::new();

    let first = load(&mut session, &a, AccessIntent::Read).expect("first loads");
    let second = load(&mut session, &b, AccessIntent::Read).expect("second loads");

    let first_report = session
        .require_device(first)
        .expect("medium")
        .inspect()
        .expect("first inspects");
    let first_volumes: Vec<u64> = first_report.volumes.iter().map(|v| v.id.value()).collect();

    let second_report = session
        .require_device(second)
        .expect("medium")
        .inspect()
        .expect("second inspects");
    let second_volumes: Vec<u64> = second_report.volumes.iter().map(|v| v.id.value()).collect();

    assert_eq!(
        first_volumes, second_volumes,
        "identities are disk-local, so two like disks issue like values"
    );
    assert_ne!(
        first, second,
        "the attachment identity is what tells the two apart"
    );

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn a_medium_is_claimed_for_as_long_as_it_is_loaded() {
    // Each loaded medium holds its own P7 claim (F43), and ejecting it —
    // or removing the device under it — is what releases it.
    let a = write_image("claim");
    let mut session = Session::new();

    let id = load(&mut session, &a, AccessIntent::Write).expect("loads");
    let mut rival = Session::new();
    let second = load(&mut rival, &a, AccessIntent::Write)
        .expect_err("a second exclusive claim on the same file is refused");
    assert_eq!(second.category(), ErrorCategory::Locked);

    session
        .require_device(id)
        .expect("device")
        .eject()
        .expect("ejects");
    let mut after = Session::new();
    load(&mut after, &a, AccessIntent::Write).expect("the claim was released with the medium");

    drop(after);
    std::fs::remove_file(&a).ok();
}

#[test]
fn a_flux_family_artifact_is_refused_by_every_device() {
    // P13: the block catalog opens anything it cannot identify at the raw
    // adapter, so without this check a P64 loaded happily and read as
    // raw — declaring the block layer authoritative when P64's own
    // adapter declares flux. No device in this release holds flux state;
    // the artifact is reached through its own type.
    let path = temp_path("flux-artifact");
    let mut bytes = b"P64-1541".to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    std::fs::write(&path, &bytes).expect("artifact writes");

    let mut session = Session::new();
    let error = load(&mut session, &path, AccessIntent::Read)
        .expect_err("a flux artifact is no device's medium");

    let message = error.to_string();
    assert!(message.contains("flux"), "names the family found: {message}");
    assert!(
        message.contains("own type"),
        "names where it is read instead: {message}"
    );

    // Even the drive whose family claims that flux path refuses it: the
    // family declaration (P22) says what the drive records, not that this
    // release loads flux state into a device.
    let drive = session
        .add_device(DeviceFamily::COMMODORE_1541)
        .expect("added");
    assert!(
        drive.load_media(&path, AccessIntent::Read).is_err(),
        "a claimed flux path is a declaration, not a loading seam"
    );

    // And the claim went with it: the refused medium is dropped, so the
    // artifact is free for whatever does claim the flux family.
    let mut second = Session::new();
    assert!(
        load(&mut second, &path, AccessIntent::Read).is_err(),
        "still refused, and refused for the same reason rather than a lock"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
