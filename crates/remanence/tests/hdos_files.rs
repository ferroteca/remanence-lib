// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Unit tests for the HDOS directory lister over the fixture image.

use std::path::PathBuf;

use remanence::{AttachmentId, DeviceFamily, Session, AccessIntent, Error, list_hdos_files};

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

    let files = disk.list_hdos_files().expect("directory parses");
    assert_eq!(files.len(), 31);

    assert_eq!(files[0].display_name(), "HDOS.SYS");
    assert_eq!(files[0].size_sectors, 24);
    assert_eq!(files[0].modified_date_string(), "09-May-78");
    assert_eq!(files[0].flags_string(), "SLWC");

    assert_eq!(files[1].display_name(), "HELP");
    assert_eq!(files[1].extension, "");

    let last = files.last().expect("at least one file");
    assert_eq!(last.display_name(), "DIRECT.SYS");
    assert_eq!(last.size_sectors, 18);

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
        assert_eq!(file.display_name(), expected);
    }

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn reads_a_file_out_through_the_grt_chain() {
    let path = private_copy("read");
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");

    let contents =
        disk.read_hdos_file("DEMO.BAS").expect("file reads");
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

    let missing = disk.read_hdos_file("NOPE.NOP");
    assert!(missing.is_err());

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn list_hdos_files_rejects_truncated_image() {
    let tiny = vec![0u8; 128];
    let error = list_hdos_files(&tiny).expect_err("truncated image rejected");
    assert!(matches!(error, Error::InvalidImage { .. }));
}
