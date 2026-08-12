// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Tests for the HDOS namespace over the fixture image, reached through
//! the node that carries file verbs.
//!
//! The H8D records no partition scheme, so the pool established when the
//! medium was loaded bears exactly one member: the direct partition, the
//! library's own composition of the whole content, carried as provenance
//! and never as evidence. Nothing there determines a namespace — a
//! declared partition type is what would, and no scheme declared one —
//! so the reading is the caller's and the check is the library's. These
//! tests declare `"hdos"`, and the adapter that declaration names is
//! what verifies it against the evidence rather than probing for a
//! filesystem on the caller's behalf (P18, P19).

use std::path::PathBuf;

use remanence::{EntryKind, ErrorCategory, Format, MediaId, Session, SpaceRule};

/// Pools `path` in a fresh session under the declaration these tests
/// make, and returns both: a medium lives in its session's pool, so
/// tests keep the session alive for as long as they use the medium.
fn attach(
    path: impl AsRef<std::path::Path>,
    afford: Afford,
) -> remanence::Result<(Session, MediaId)> {
    let source = match afford {
        Afford::Read => open_read(path),
        Afford::Write => open_write(path),
    };
    let mut session = Session::new();
    let id = session.load_media(source, Format::H8d)?.id();
    Ok((session, id))
}

/// What the caller's own open affords, in the shape these tests declare
/// it: the amended P7 asks the handle one question, so the test says
/// which answer it wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Afford {
    Read,
    Write,
}

mod common;
use common::{open_read, open_write};

fn fixture_h8d() -> PathBuf {
    common::ensure_fixture("HDOS_1-0_Issue_#50-00-00_890-1.h8d")
}

/// The disk holds the P7 deny-write claim for its lifetime, so tests
/// opening the fixture concurrently take private copies.
fn private_copy(tag: &str) -> PathBuf {
    let target =
        std::env::temp_dir().join(format!("remanence-hdos-{tag}-{}.h8d", std::process::id()));
    std::fs::copy(fixture_h8d(), &target).expect("fixture copies");
    target
}

#[test]
fn lists_files_from_hdos_fixture_image() {
    let path = private_copy("list");
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let mut filesystem = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("hdos")
        .expect("the declaration is the caller's and the adapter verifies it");
    assert_eq!(
        filesystem.kind().expect("the medium bears a namespace"),
        "hdos"
    );
    // The direct partition over an h8d is composed over the whole
    // content, so the addressable vantage opens — and nothing composed a
    // volume over it, so the report issued no identity to answer with.
    // The two facts sit side by side rather than one implying the other:
    // a namespace the medium bears itself gets no phantom volume minted
    // to carry it (D26, U4).
    assert!(
        filesystem.is_addressable(),
        "the direct partition addresses the whole content"
    );
    assert_eq!(
        filesystem.volume_id(),
        None,
        "and composes no volume; the inspection report issued no identity here"
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

    assert_eq!(
        files[1].name, "HELP",
        "a file with no extension keeps its bare name"
    );

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

    drop(filesystem);
    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn reads_a_file_out_through_the_grt_chain() {
    let path = private_copy("read");
    // The handle affords writing, so the refusal at the end of this test
    // is the namespace's own rather than the claim's (P7 as amended):
    // this release reads the HDOS catalog and does not write it, whatever
    // the caller's open allows.
    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("disk opens");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    let mut filesystem = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("hdos")
        .expect("the declaration is the caller's and the adapter verifies it");
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
        .filter(|&&byte| {
            byte == 0 || byte == b'\r' || byte == b'\n' || (0x20..0x7f).contains(&byte)
        })
        .count();
    assert!(
        printable * 10 >= contents.len() * 9,
        "mostly text/zero bytes"
    );

    // Absence is an answer at `stat` and a refusal at `get_file`: one
    // asks whether something is there, the other asks for the file.
    assert!(
        filesystem
            .stat("NOPE.NOP")
            .expect("the catalog answers")
            .is_none()
    );
    let missing = filesystem
        .get_file("NOPE.NOP")
        .expect_err("nothing is there");
    assert_eq!(missing.category(), ErrorCategory::NotFound);

    // Read-only, and it says so by name rather than by failing later.
    let refusal = filesystem
        .write_file("NEW.TXT", b"denied")
        .expect_err("this release reads HDOS and does not write it");
    assert_eq!(refusal.category(), ErrorCategory::ReadOnly);
    assert_eq!(refusal.rule(), Some(SpaceRule::NotWritable.as_str()));

    drop(filesystem);
    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

/// Two absences, each stated by the seam that owns it (P10): nothing
/// determines a namespace over the direct partition, so the plain door
/// answers `None`; and a declaration the content cannot bear is refused
/// by the adapter the declaration named. Neither is an empty listing.
#[test]
fn a_medium_bearing_no_namespace_is_a_named_absence() {
    let path =
        std::env::temp_dir().join(format!("remanence-hdos-absent-{}.h8d", std::process::id()));
    std::fs::write(&path, vec![0u8; 102_400]).expect("blank image writes");

    let (mut session, attachment) = attach(&path, Afford::Read).expect("disk opens");
    let disk = session
        .medium_mut(attachment)
        .expect("the medium is pooled");

    // This medium records no scheme, so it bears the direct partition —
    // and nothing there declares a namespace, a declared partition type
    // being what would. The plain door answers the honest absence P19
    // requires rather than probing for one.
    let direct = disk.partition(0).expect("the direct partition");
    assert!(
        direct.is_direct(),
        "a medium recording no scheme bears exactly this"
    );
    assert!(
        !direct.partition().bears_namespace(),
        "no type is declared here, so nothing determines a namespace"
    );
    assert!(
        direct.filesystem().is_none(),
        "and the plain door says so by answering nothing at all"
    );

    // The declared reading is where a caller who knows says so, and the
    // check is the library's: a blank image bears no HDOS label, and the
    // adapter the declaration named is what refuses — attributed to that
    // seam rather than to the walk that reached it (P4, P18).
    let error = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("hdos")
        .expect_err("a blank image is no HDOS volume");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    let message = error.to_string();
    assert!(
        message.contains("hdos"),
        "the refusal names the adapter that made it: {message}"
    );
    assert!(
        message.contains("HDOS label"),
        "and what it read to make it: {message}"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
