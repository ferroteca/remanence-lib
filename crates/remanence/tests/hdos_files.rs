// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Integration tests for the HDOS directory lister over the fixture image.

use std::path::PathBuf;

use remanence::{Error, Session, list_hdos_files};

fn fixture_h8d() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join("HDOS_1-0_Issue_#50-00-00_890-1.h8d")
}

#[test]
fn lists_files_from_hdos_fixture_image() {
    let session = Session::open(fixture_h8d()).expect("session opens");

    let files = list_hdos_files(session.bytes()).expect("directory parses");
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
}

#[test]
fn list_hdos_files_rejects_truncated_image() {
    let tiny = vec![0u8; 128];
    let error = list_hdos_files(&tiny).expect_err("truncated image rejected");
    assert!(matches!(error, Error::InvalidImage { .. }));
}
