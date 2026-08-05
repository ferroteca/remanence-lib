// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Tests for the HDOS namespace over the fixture image, reached through
//! the node that carries file verbs.
//!
//! The H8D composes no volume — its leading sector is neither blank, an
//! MBR, nor a FAT boot record — so `device.filesystem()` resolves to the
//! namespace the medium bears itself. That the resolver takes no volume
//! selector here is not an inconsistency but its transparent form: the
//! walk had exactly one supported answer at every seam (P19).

use std::path::PathBuf;

use remanence::{
    AccessIntent, AttachmentId, DeviceFamily, EntryKind, ErrorCategory, NamespaceRule,
    Session,
};

/// Attaches `path` to a fresh session and returns both, because a medium
/// is reachable only through the device holding it (P32). Tests keep the
/// session alive for as long as they use the medium.
fn attach(
    path: impl AsRef<std::path::Path>,
    intent: AccessIntent,
) -> remanence::Result<(Session, AttachmentId)> {
    let mut session = Session::new();
    let device = session.add_device(DeviceFamily::HEATHKIT_H17)?;
    let attachment = device.attachment();
    device.load_media(path, intent)?;
    Ok((session, attachment))
}

mod common;

fn fixture_h8d() -> PathBuf {
    common::ensure_fixture("HDOS_1-0_Issue_#50-00-00_890-1.h8d")
}

/// The disk holds the P7 deny-write claim for its lifetime, so tests
/// opening the fixture concurrently take private copies.
fn private_copy(tag: &str) -> PathBuf {
    let target = std::env::temp_dir().join(format!(
        "remanence-hdos-{tag}-{}.h8d",
        std::process::id()
    ));
    std::fs::copy(fixture_h8d(), &target).expect("fixture copies");
    target
}

#[test]
fn lists_files_from_hdos_fixture_image() {
    let path = private_copy("list");
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");

    let mut filesystem = disk.filesystem().expect("the resolver walks to one namespace");
    assert_eq!(filesystem.kind(), "hdos");
    assert_eq!(
        filesystem.volume_id(),
        None,
        "the medium bears this namespace itself; no volume composed"
    );

    let files = filesystem.entries("").expect("directory parses");
    assert_eq!(files.len(), 31);

    // U2's four facts: the real names, sizes, dates and flags. The last
    // two have no named field in the node's vocabulary, so HDOS declares
    // them in its own spelling rather than being normalized into one.
    let fact = |entry: &remanence::Entry, key: &str| -> String {
        entry
            .declared
            .iter()
            .find(|fact| fact.key == key)
            .unwrap_or_else(|| panic!("HDOS declares '{key}'"))
            .value
            .clone()
    };
    assert_eq!(files[0].name, "HDOS.SYS");
    assert_eq!(files[0].kind, EntryKind::File);
    assert_eq!(files[0].size_bytes, 24 * 256);
    assert_eq!(fact(&files[0], "size-sectors"), "24");
    assert_eq!(fact(&files[0], "modified-date"), "09-May-78");
    assert_eq!(fact(&files[0], "flags"), "SLWC");

    assert_eq!(files[1].name, "HELP", "a file with no extension keeps its bare name");

    let last = files.last().expect("at least one file");
    assert_eq!(last.name, "DIRECT.SYS");
    assert_eq!(fact(last, "size-sectors"), "18");

    let expected_names = [
        "HDOS.SYS",
        "HELP",
        "ONECOPY.ABS",
        "FLAGS.ABS",
        "SET.ABS",
        "AT.DVD",
        "ND.DVD",
        "PIP.ABS",
        "SYSHELP.DOC",
        "SYSCMD.SYS",
        "HDOSOVL.SYS",
        "ERRORMSG.SYS",
        "SYSGEN.ABS",
        "TEST17.ABS",
        "INIT17.ABS",
        "TXTCON.ABS",
        "BASCON.ABS",
        "PATCH.ABS",
        "DBUG.ABS",
        "EDIT.ABS",
        "ASM.ABS",
        "BASIC.ABS",
        "DEMO.ASM",
        "DEMO2.ASM",
        "DEMO3.ASM",
        "HDOS.ACM",
        "DEMO.BAS",
        "AT2.DVD",
        "RGT.SYS",
        "GRT.SYS",
        "DIRECT.SYS",
    ];
    for (file, expected) in files.iter().zip(expected_names) {
        assert_eq!(file.name, expected);
    }

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn reads_a_file_out_through_the_grt_chain() {
    let path = private_copy("read");
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");

    let mut filesystem = disk.filesystem().expect("the resolver walks to one namespace");
    let contents = filesystem
        .get_file("DEMO.BAS")
        .expect("the catalog names it")
        .bytes()
        .expect("file reads");
    // 3 sectors cataloged: two full groups of 2 plus a final partial group
    // truncates to the last_sector_index — the byte size is
    // sector-granular in HDOS terms.
    assert!(!contents.is_empty());
    assert_eq!(contents.len() % 256, 0);
    // BASIC source: the bytes should be dominated by printable ASCII.
    let printable = contents
        .iter()
        .filter(|&&byte| byte == 0 || byte == b'\r' || byte == b'\n' || (0x20..0x7f).contains(&byte))
        .count();
    assert!(printable * 10 >= contents.len() * 9, "mostly text/zero bytes");

    // Absence is an answer at `stat` and a refusal at `get_file`: one
    // asks whether something is there, the other asks for the file.
    assert!(
        filesystem.stat("NOPE.NOP").expect("the catalog answers").is_none()
    );
    let missing = filesystem.get_file("NOPE.NOP").expect_err("nothing is there");
    assert_eq!(missing.category(), ErrorCategory::NotFound);

    // Read-only, and it says so by name rather than by failing later.
    let refusal = filesystem
        .write_file("NEW.TXT", b"denied")
        .expect_err("this release reads HDOS and does not write it");
    assert_eq!(refusal.category(), ErrorCategory::ReadOnly);
    assert_eq!(refusal.rule(), Some(NamespaceRule::NotWritable.as_str()));

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

/// A blank medium composes no volume and bears no namespace, and the
/// resolver says exactly that — a named absence carrying its rule
/// identity (P10), never an empty listing.
#[test]
fn a_medium_bearing_no_namespace_is_a_named_absence() {
    let path = std::env::temp_dir().join(format!(
        "remanence-hdos-absent-{}.h8d",
        std::process::id()
    ));
    std::fs::write(&path, vec![0u8; 102_400]).expect("blank image writes");

    let (mut session, attachment) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = session.require_device(attachment).expect("the medium is attached");
    let error = disk.filesystem().expect_err("there is no namespace to resolve to");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    assert_eq!(error.rule(), Some(NamespaceRule::NoNamespace.as_str()));

    drop(session);
    std::fs::remove_file(&path).ok();
}
