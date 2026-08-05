// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The storage-device tier (P32): a machine holds a dynamic set of
//! family-typed devices, each a durable slot distinct from the medium in
//! it. These reach that set through the session's own verbs, which are
//! its anonymous machine's; the machine scope itself is `machines.rs`.
//! These tests build their images by hand, so they run without
//! fixtures.

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

#[test]
fn an_auto_attach_takes_the_lowest_free_slot_in_its_family() {
    let a = write_image("auto-a");
    let b = write_image("auto-b");
    let mut session = Session::new();

    let first = session.attach(&a, AccessIntent::Read).expect("first attaches");
    let second = session.attach(&b, AccessIntent::Read).expect("second attaches");

    assert_eq!(first.to_string(), "hdd0");
    assert_eq!(second.to_string(), "hdd1");
    assert_eq!(session.devices().len(), 2);

    // The identity is composed and predictable — deliberately unlike the
    // opaque region and volume identities a report issues, because a
    // device is machine configuration the caller supplied (P21).
    assert_eq!(first.family(), DeviceFamily::Hdd);
    assert_eq!(first.index(), 0);

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn a_caller_may_choose_the_slot_and_leave_a_gap() {
    let a = write_image("slot-a");
    let b = write_image("slot-b");
    let mut session = Session::new();

    let named = session
        .attach_at(DeviceFamily::Hdd, 3, &a, AccessIntent::Read)
        .expect("the named slot attaches");
    assert_eq!(named.to_string(), "hdd3");

    // The gap is real: the next auto-attach fills slot 0, not slot 4.
    let auto = session.attach(&b, AccessIntent::Read).expect("auto attaches");
    assert_eq!(auto.to_string(), "hdd0");

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn an_occupied_slot_is_refused_by_name_rather_than_displaced() {
    let a = write_image("occupied-a");
    let b = write_image("occupied-b");
    let mut session = Session::new();

    session
        .attach_at(DeviceFamily::Hdd, 0, &a, AccessIntent::Read)
        .expect("first attaches");
    let error = session
        .attach_at(DeviceFamily::Hdd, 0, &b, AccessIntent::Read)
        .expect_err("the occupied slot is refused");

    let message = error.to_string();
    assert!(message.contains("hdd0"), "names the slot: {message}");
    assert!(message.contains("detach"), "names the remedy: {message}");

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn a_detached_slot_is_free_again_and_its_identity_stops_resolving() {
    let a = write_image("detach-a");
    let b = write_image("detach-b");
    let mut session = Session::new();

    let first = session.attach(&a, AccessIntent::Read).expect("attaches");
    assert!(session.device(first).is_some());

    session.detach(first).expect("detaches");
    assert!(
        session.device(first).is_none(),
        "a detached identity resolves to nothing"
    );
    assert!(session.devices().is_empty());

    // Reuse is deliberate and safe: attach and detach are machine-down
    // operations, so nothing live refers to the old occupant. This is not
    // the renumbering U4 refuses, because a slot is configuration rather
    // than evidence.
    let reused = session.attach(&b, AccessIntent::Read).expect("reattaches");
    assert_eq!(reused.to_string(), "hdd0", "the freed slot is reused");

    let missing = session
        .detach(AttachmentId::parse("hdd7").expect("parses"))
        .expect_err("detaching nothing is refused");
    assert_eq!(missing.category(), ErrorCategory::NotFound);

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn the_device_is_the_slot_and_the_medium_is_what_occupies_it() {
    let a = write_image("slot-vs-medium");
    let mut session = Session::new();

    let id = session.attach(&a, AccessIntent::Read).expect("attaches");
    let device = session.device(id).expect("device exists");
    assert!(device.is_occupied());
    assert_eq!(device.attachment(), id);
    assert_eq!(device.family(), DeviceFamily::Hdd);

    // The medium is reached through the device, and nothing else.
    let medium = session.require_device(id).expect("the medium is reachable");
    assert_eq!(medium.image_size_bytes().expect("a medium is attached"), 1024 * 1024);

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

    let first = session.attach(&a, AccessIntent::Read).expect("first attaches");
    let second = session.attach(&b, AccessIntent::Read).expect("second attaches");

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
fn a_medium_is_claimed_for_as_long_as_it_is_attached() {
    // Each attached medium holds its own P7 claim (F43), and detaching
    // is what releases it — the device staying put either way.
    let a = write_image("claim");
    let mut session = Session::new();

    let id = session.attach(&a, AccessIntent::Write).expect("attaches");
    let second = Session::new()
        .attach(&a, AccessIntent::Write)
        .expect_err("a second exclusive claim on the same file is refused");
    assert_eq!(second.category(), ErrorCategory::Locked);

    session.detach(id).expect("detaches");
    let mut after = Session::new();
    after
        .attach(&a, AccessIntent::Write)
        .expect("the claim was released with the medium");

    drop(after);
    std::fs::remove_file(&a).ok();
}

#[test]
fn a_flux_family_artifact_is_refused_by_a_block_device() {
    // P14: a device accepts only its own family's media. This is not a
    // vacuous clause even with one family claimed, because the block
    // catalog opens anything it cannot identify at the raw adapter —
    // so before this check a P64 attached happily as `hdd0` and read as
    // raw, declaring the block layer authoritative when P64's own
    // adapter declares flux. In-force P13 forbids exactly that.
    let path = temp_path("flux-artifact");
    let mut bytes = b"P64-1541".to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    std::fs::write(&path, &bytes).expect("artifact writes");

    let mut session = Session::new();
    let error = session
        .attach(&path, AccessIntent::Read)
        .expect_err("a flux container is not block-family media");

    let message = error.to_string();
    assert!(message.contains("flux"), "names the family found: {message}");
    assert!(message.contains("hdd0"), "names the device refusing: {message}");
    assert!(
        session.devices().is_empty(),
        "a refused attach leaves no device behind"
    );

    // And the claim went with it: the refused medium is dropped, so the
    // artifact is free for whatever does claim the flux family.
    let mut second = Session::new();
    assert!(
        second.attach(&path, AccessIntent::Read).is_err(),
        "still refused, and refused for the same reason rather than a lock"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
