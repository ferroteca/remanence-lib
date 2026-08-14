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

use remanence::{AttachmentId, ErrorCategory, Format, HardDrive, MediaId, Session};

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

/// The acts against one machine, where a test cares about the result
/// rather than the sequence: pool the disk, add the drive, link them.
/// `machine` is null for the session's anonymous machine.
fn load(
    session: &mut Session,
    machine: Option<&str>,
    path: &PathBuf,
    afford: Afford,
) -> remanence::Result<AttachmentId> {
    let source = match afford {
        Afford::Read => open_read(path),
        Afford::Write => open_write(path),
    };
    let media = session
        .load_media(
            source,
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )?
        .id();
    // A lookup answers with absence; the demand is the caller's to
    // write, and every call site here added the machine first.
    let mut view = match machine {
        Some(identity) => session
            .machine_mut(identity)
            .expect("the machine was added first"),
        None => session.anonymous_mut(),
    };
    let mut device = view.add_device(HardDrive::MbrSector)?;
    let attachment = device.attachment();
    device.insert(media)?;
    Ok(attachment)
}

/// The medium in one machine's slot, for the content verbs.
fn medium_at<'a>(
    session: &'a mut Session,
    machine: Option<&str>,
    attachment: AttachmentId,
) -> Option<&'a mut remanence::Medium> {
    let media = match machine {
        Some(identity) => session.machine(identity)?.device(attachment)?.media_id()?,
        None => session.device(attachment)?.media_id()?,
    };
    session.medium_mut(media)
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

    let id = load(&mut session, None, &a, Afford::Read).expect("loads");

    assert_eq!(session.attachments(), vec![id]);
    assert_eq!(
        session.anonymous().attachments(),
        vec![id],
        "the session's set and the anonymous machine's are one set"
    );
    assert_eq!(
        session.machines().len(),
        1,
        "adding a device adds no machine"
    );
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

    let host = load(&mut session, None, &a, Afford::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    let inner = load(&mut session, Some("h89"), &b, Afford::Read).expect("loads inside it");

    assert_eq!(host.to_string(), "hdd0");
    assert_eq!(
        inner.to_string(),
        "hdd0",
        "the named machine's first slot is its own hdd0, not hdd1"
    );

    let host_image = medium_at(&mut session, None, host)
        .expect("the anonymous machine's medium")
        .image_path()
        .expect("this host names its handles")
        .to_owned();
    let inner_image = medium_at(&mut session, Some("h89"), inner)
        .expect("the named machine's medium")
        .image_path()
        .expect("this host names its handles")
        .to_owned();
    assert_eq!(
        std::fs::canonicalize(&host_image).expect("resolves"),
        std::fs::canonicalize(&a).expect("resolves")
    );
    assert_eq!(
        std::fs::canonicalize(&inner_image).expect("resolves"),
        std::fs::canonicalize(&b).expect("resolves")
    );
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

    let host = load(&mut session, None, &a, Afford::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    {
        let h89 = session.machine_mut("h89").expect("is there");
        assert!(
            h89.device(host).is_none(),
            "the anonymous machine's hdd0 is not this machine's"
        );
        assert!(
            h89.devices().is_empty(),
            "and no device arrives in it by being added elsewhere"
        );
    }
    let inner = load(&mut session, Some("h89"), &b, Afford::Read).expect("loads its own");

    session
        .machine_mut("h89")
        .expect("is there")
        .release_device(inner)
        .expect("releases its own");

    assert!(
        session
            .machine("h89")
            .expect("is there")
            .devices()
            .is_empty(),
        "the named machine's slot is free again"
    );
    assert!(
        session.device(host).is_some(),
        "and the anonymous machine's device is untouched"
    );

    // Releasing what belongs to another machine is refused, not honored
    // across the boundary.
    let error = session
        .machine_mut("h89")
        .expect("is there")
        .release_device(host)
        .expect_err("a device of another machine is not this one's to release");
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

    let first = session
        .load_media(
            open_read(&a),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("loads")
        .id();
    session
        .add_device_at(HardDrive::MbrSector, 2)
        .expect("the anonymous machine takes hdd2 first")
        .insert(first)
        .expect("the disk goes in");
    load(&mut session, None, &b, Afford::Read).expect("then fills hdd0");

    session.add_machine("h89").expect("the machine is added");
    load(&mut session, Some("h89"), &c, Afford::Read)
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
fn an_unknown_machine_answers_with_absence_rather_than_being_created() {
    // Every in-memory lookup answers with absence: nothing is
    // manufactured to report it, and asking never creates what was
    // asked for. A caller who wants a demand writes it.
    let mut session = Session::new();

    assert!(session.machine("h89").is_none(), "absence is the answer");
    assert!(
        session.machine_mut("h89").is_none(),
        "and the working form agrees"
    );
    assert_eq!(
        session.machines().len(),
        1,
        "asking for a machine never creates one"
    );

    // Releasing is a different act, and it names what resolves to
    // nothing.
    let error = session
        .release_machine("h89")
        .expect_err("a machine that was never added cannot be released");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    assert!(
        error.to_string().contains("h89"),
        "names what was asked for: {error}"
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

    let id = load(&mut session, Some("h89"), &a, Afford::Write).expect("loads");

    let media = session
        .machine("h89")
        .expect("is there")
        .device(id)
        .expect("the device is there")
        .media_id()
        .expect("occupied");
    let medium = session.medium_mut(media).expect("the medium is pooled");
    assert_eq!(medium.image_size_bytes(), 1024 * 1024);
    assert_eq!(medium.mode(), remanence::AccessMode::ReadWrite);
    assert!(
        medium.inspect().is_ok(),
        "the layered inspection reads a medium seated in a named machine"
    );

    // Tearing the machine's configuration down takes nothing with it:
    // the device goes, the medium stays, and its claim with it.
    session
        .machine_mut("h89")
        .expect("is there")
        .release_device(id)
        .expect("releases the device");
    assert!(
        session
            .medium(media)
            .expect("still pooled")
            .is_linked()
            .eq(&false),
        "releasing the device severed the link and destroyed nothing"
    );
    assert_eq!(
        session
            .medium_mut(media)
            .expect("pooled")
            .image_size_bytes(),
        1024 * 1024
    );

    session
        .release_media(media)
        .expect("the one destroying verb");
    assert!(session.medium(media).is_none());

    drop(session);
    std::fs::remove_file(&a).ok();
}

#[test]
fn releasing_a_machine_cascades_through_configuration_and_takes_no_state() {
    // The cascade in full: every device is ejected first — severing, so
    // each medium stays pooled with its claim and everything buffered —
    // then the devices go, then the machine. Configuration falls with
    // its owner; state never does.
    let a = write_image("cascade-a");
    let b = write_image("cascade-b");
    let mut session = Session::new();

    session.add_machine("h89").expect("the machine is added");
    let first = load(&mut session, Some("h89"), &a, Afford::Read).expect("first seats");
    let second = load(&mut session, Some("h89"), &b, Afford::Read).expect("second seats");

    let mut media: Vec<MediaId> = Vec::new();
    for attachment in [first, second] {
        media.push(
            session
                .machine("h89")
                .expect("is there")
                .device(attachment)
                .expect("the device is there")
                .media_id()
                .expect("occupied"),
        );
    }
    assert_ne!(first, second, "two slots, two devices");

    session.release_machine("h89").expect("released");
    assert!(session.machine("h89").is_none(), "the machine is gone");

    assert_eq!(session.media().len(), 2, "and it destroyed nothing");
    for id in media {
        assert!(
            !session.medium(id).expect("the pool kept it").is_linked(),
            "each link was severed rather than followed"
        );
        assert_eq!(
            session.medium_mut(id).expect("pooled").image_size_bytes(),
            1024 * 1024,
            "and each medium still answers"
        );
    }

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}

#[test]
fn the_same_artifact_may_back_a_device_in_two_machines_at_once() {
    // The session owns every machine's lifetime, so a read claim taken
    // in one machine does not stand in the way of another — which is
    // what lets one machine's device be backed by state another machine
    // holds, with no lifetime question between them.
    let a = write_image("shared");
    let mut session = Session::new();

    let host = load(&mut session, None, &a, Afford::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    let inner = load(&mut session, Some("h89"), &a, Afford::Read)
        .expect("the same artifact loads in another machine");

    assert_eq!(host.to_string(), inner.to_string());
    let host_bytes = medium_at(&mut session, None, host)
        .expect("medium")
        .image_size_bytes();
    let inner_bytes = medium_at(&mut session, Some("h89"), inner)
        .expect("medium")
        .image_size_bytes();
    assert_eq!(host_bytes, inner_bytes);

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

    load(&mut session, None, &a, Afford::Read).expect("host loads");
    session.add_machine("h89").expect("the machine is added");
    load(&mut session, Some("h89"), &b, Afford::Read).expect("loads inside it");

    let anonymous = medium_at(&mut session, None, hdd0)
        .expect("the anonymous machine's")
        .image_path()
        .expect("this host names its handles")
        .to_owned();
    let named = medium_at(&mut session, Some("h89"), hdd0)
        .expect("the named machine's")
        .image_path()
        .expect("this host names its handles")
        .to_owned();
    assert_eq!(
        std::fs::canonicalize(&anonymous).expect("resolves"),
        std::fs::canonicalize(&a).expect("resolves")
    );
    assert_eq!(
        std::fs::canonicalize(&named).expect("resolves"),
        std::fs::canonicalize(&b).expect("resolves")
    );

    drop(session);
    std::fs::remove_file(&a).ok();
    std::fs::remove_file(&b).ok();
}
