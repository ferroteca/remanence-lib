// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The machine scope (P32): a session holds machines, a machine holds
//! devices, and the session's anonymous machine is the one whose identity
//! is null. What these tests are for is the boundary between two machines
//! in one session — that each owns its own slot namespace and its own
//! attachment order, and that neither can reach the other's devices —
//! because that separation is the whole reason the tier exists: an
//! archive on the host was never part of the machine whose disk it
//! contains.
//!
//! These tests build their images by hand, so they run without fixtures.

use std::path::PathBuf;

use remanence::{AccessIntent, AttachmentId, DeviceFamily, ErrorCategory, Session};

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-machines-{tag}-{}-{nonce}.img",
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

/// The two acts against one machine, where a test cares about the result
/// rather than the pair.
fn load(
    machine: &mut remanence::Machine,
    path: &PathBuf,
    intent: AccessIntent,
) -> remanence::Result<AttachmentId> {
    let device = machine.add_device(DeviceFamily::HARD_DISK)?;
    let attachment = device.attachment();
    device.load_media(path, intent)?;
    Ok(attachment)
}

#[test]
fn a_session_holds_exactly_one_anonymous_machine_from_the_start() {
    let session = Session::new();

    assert_eq!(session.machines().len(), 1, "one machine, and it is there");
    assert_eq!(
        session.machines()[0].identity(),
        None,
        "the anonymous machine is the one whose identity is null (D23)"
    );
    assert_eq!(session.anonymous().identity(), None);
    assert!(
        session.anonymous().devices().is_empty(),
        "a fresh machine holds no devices"
    );
}

#[test]
fn the_sessions_device_verbs_land_in_the_anonymous_machine() {
    // The session's device verbs are the anonymous machine's, which is
    // what makes the machine tier a structural change with no behavior in
    // it: a caller who never names a machine sees exactly what it saw
    // before.
    let a = write_image("anonymous");
    let mut session = Session::new();

    let id = load(session.anonymous_mut(), &a, AccessIntent::Read).expect("loads");

    assert_eq!(session.attachments(), vec![id]);
    assert_eq!(
        session.anonymous().attachments(),
        vec![id],
        "the session's set and the anonymous machine's are one set"
    );
    assert_eq!(session.machines().len(), 1, "adding a device adds no machine");
    assert!(session.anonymous().device(id).is_some());

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn each_machine_owns_its_own_slot_namespace() {
    // Two machines may each hold an `hdd0`, and the two are different
    // devices holding different media. A slot is a machine's own
    // configuration, so the namespace cannot be the session's.
    let a = write_image("namespace-a");
    let b = write_image("namespace-b");
    let mut session = Session::new();

    let host = load(session.anonymous_mut(), &a, AccessIntent::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    let inner = load(
        session.require_machine("h89").expect("is there"),
        &b,
        AccessIntent::Read,
    )
    .expect("loads inside it");

    assert_eq!(host.to_string(), "hdd0");
    assert_eq!(
        inner.to_string(),
        "hdd0",
        "the named machine's first slot is its own hdd0, not hdd1"
    );

    let host_image = session
        .require_device(host)
        .expect("the anonymous machine's device")
        .image_path()
        .expect("a medium is attached")
        .to_owned();
    let inner_image = session
        .require_machine("h89")
        .expect("is there")
        .require_device(inner)
        .expect("the named machine's device")
        .image_path()
        .expect("a medium is attached")
        .to_owned();
    assert_eq!(host_image, a);
    assert_eq!(inner_image, b);
    assert_ne!(host_image, inner_image, "one identity, two devices");

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn a_machine_reaches_only_its_own_devices() {
    // Machines in a session do not know about each other. A slot filled
    // in one is free in the other, and removing a device in one leaves
    // the other's exactly where it was.
    let a = write_image("scoped-a");
    let b = write_image("scoped-b");
    let mut session = Session::new();

    let host = load(session.anonymous_mut(), &a, AccessIntent::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    let h89 = session.require_machine("h89").expect("is there");
    assert!(
        h89.device(host).is_none(),
        "the anonymous machine's hdd0 is not this machine's"
    );
    assert!(
        h89.devices().is_empty(),
        "and no device arrives in it by being added elsewhere"
    );
    let inner = load(h89, &b, AccessIntent::Read).expect("loads its own");

    session
        .require_machine("h89")
        .expect("is there")
        .remove_device(inner)
        .expect("removes its own");

    assert!(
        session
            .require_machine("h89")
            .expect("is there")
            .devices()
            .is_empty(),
        "the named machine's slot is free again"
    );
    assert!(
        session.device(host).is_some(),
        "and the anonymous machine's device is untouched"
    );

    // Removing what belongs to another machine is refused, not honored
    // across the boundary.
    let error = session
        .require_machine("h89")
        .expect("is there")
        .remove_device(host)
        .expect_err("a device of another machine is not this one's to remove");
    assert_eq!(error.category(), ErrorCategory::NotFound);

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn attachment_order_is_each_machines_own_fact() {
    // Attachment order is what a namespace composer reads, and it is the
    // machine's rather than the session's — the anonymous machine's
    // order says nothing about a named machine's.
    let a = write_image("order-a");
    let b = write_image("order-b");
    let c = write_image("order-c");
    let mut session = Session::new();

    session
        .add_device_at(DeviceFamily::HARD_DISK, 2)
        .expect("the anonymous machine takes hdd2 first")
        .load_media(&a, AccessIntent::Read)
        .expect("loads");
    load(session.anonymous_mut(), &b, AccessIntent::Read).expect("then fills hdd0");

    session.add_machine("h89").expect("the machine is added");
    load(
        session.require_machine("h89").expect("is there"),
        &c,
        AccessIntent::Read,
    )
    .expect("the named machine starts at its own hdd0");

    let anonymous: Vec<String> = session
        .anonymous()
        .attachments()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        anonymous,
        vec!["hdd2".to_owned(), "hdd0".to_owned()],
        "slot-fill order, not slot order"
    );

    let named: Vec<String> = session
        .machine("h89")
        .expect("is there")
        .attachments()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        named,
        vec!["hdd0".to_owned()],
        "the named machine's order is its own, and starts fresh"
    );

    drop(session);
    for path in [&a, &b, &c] {
        std::fs::remove_file(path).ok();
    }
}

#[test]
fn a_machine_identity_is_unique_and_never_empty() {
    let mut session = Session::new();
    session.add_machine("h89").expect("the first is added");

    let duplicate = session
        .add_machine("h89")
        .expect_err("a second machine of the same identity is refused");
    let message = duplicate.to_string();
    assert!(message.contains("h89"), "names the identity: {message}");

    // The machine with no identity is the anonymous one, and a session
    // has exactly one of it — so the empty identity is refused rather
    // than minting a second.
    let empty = session
        .add_machine("")
        .expect_err("the empty identity is refused");
    assert!(
        empty.to_string().contains("anonymous"),
        "says which machine the empty identity already belongs to: {empty}"
    );

    assert_eq!(
        session.machines().len(),
        2,
        "one anonymous machine and one named, and neither refusal added a third"
    );
}

#[test]
fn an_unknown_machine_is_refused_by_name_rather_than_created() {
    let mut session = Session::new();

    assert!(session.machine("h89").is_none());
    let error = session
        .require_machine("h89")
        .expect_err("a machine that was never added is refused");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    assert!(
        error.to_string().contains("h89"),
        "names what was asked for: {error}"
    );
    assert_eq!(
        session.machines().len(),
        1,
        "asking for a machine never creates one"
    );
}

#[test]
fn a_medium_in_a_named_machine_is_read_and_claimed_like_any_other() {
    // The anonymous machine is not privileged: a device in a named
    // machine answers the content verbs the same way, and its medium
    // holds its own P7 claim for as long as it stays attached.
    let a = write_image("named-medium");
    let mut session = Session::new();
    session.add_machine("h89").expect("the machine is added");

    let id = load(
        session.require_machine("h89").expect("is there"),
        &a,
        AccessIntent::Write,
    )
    .expect("loads");

    let device = session
        .require_machine("h89")
        .expect("is there")
        .require_device(id)
        .expect("the device is there");
    assert!(device.is_occupied());
    assert_eq!(device.attachment(), id);
    assert_eq!(
        device.image_size_bytes().expect("a medium is attached"),
        1024 * 1024
    );
    assert!(
        device.inspect().is_ok(),
        "the layered inspection reads through a named machine's device"
    );

    let mut rival = Session::new();
    let contested = load(rival.anonymous_mut(), &a, AccessIntent::Write)
        .expect_err("the claim is the medium's, wherever the device sits");
    assert_eq!(contested.category(), ErrorCategory::Locked);

    session
        .require_machine("h89")
        .expect("is there")
        .remove_device(id)
        .expect("removes the device");
    let mut after = Session::new();
    load(after.anonymous_mut(), &a, AccessIntent::Write)
        .expect("the claim was released with the medium");

    drop(after);
    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn the_same_artifact_may_back_a_device_in_two_machines_at_once() {
    // The session owns every machine's lifetime, so a read claim taken
    // in one machine does not stand in the way of another — which is
    // what lets one machine's device be backed by state another machine
    // holds, with no lifetime question between them.
    let a = write_image("shared");
    let mut session = Session::new();

    let host = load(session.anonymous_mut(), &a, AccessIntent::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    let inner = load(
        session.require_machine("h89").expect("is there"),
        &a,
        AccessIntent::Read,
    )
    .expect("the same artifact loads in another machine");

    assert_eq!(host.to_string(), inner.to_string());
    assert_eq!(
        session
            .require_device(host)
            .expect("device")
            .image_size_bytes()
            .expect("a medium is attached"),
        session
            .require_machine("h89")
            .expect("is there")
            .require_device(inner)
            .expect("device")
            .image_size_bytes()
            .expect("a medium is attached"),
    );

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn a_parsed_attachment_identity_means_whatever_machine_it_is_asked_of() {
    // An attachment identity is caller-facing and predictable (P21), and
    // it is resolved within one machine: the same parsed `hdd0` names a
    // different device in each.
    let a = write_image("resolve-a");
    let b = write_image("resolve-b");
    let hdd0 = AttachmentId::parse("hdd0").expect("parses");
    let mut session = Session::new();

    load(session.anonymous_mut(), &a, AccessIntent::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    load(
        session.require_machine("h89").expect("is there"),
        &b,
        AccessIntent::Read,
    )
    .expect("loads inside it");

    assert_eq!(
        session
            .require_device(hdd0)
            .expect("the anonymous machine's")
            .image_path()
            .expect("a medium is attached"),
        a
    );
    assert_eq!(
        session
            .require_machine("h89")
            .expect("is there")
            .require_device(hdd0)
            .expect("the named machine's")
            .image_path()
            .expect("a medium is attached"),
        b
    );

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}
