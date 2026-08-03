// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The 7z catalog over the prepared KryoFlux capture fixture: the
//! archive lists its members from bounded header reads alone, and one
//! member of the solid folder streams through a session without the
//! folder ever being resident whole (P27). The catalog reads the
//! members; it says nothing about what a KryoFlux stream means.

use std::path::{Path, PathBuf};

use remanence::{Archive, ContainerKind, ErrorCategory, Session};

mod common;

const CAPTURE_ZERO: &str =
    "Bill Budge Pinball Construction Set [Commodore 64] (1of2) - Capture 0.7z";
const CAPTURE_ONE: &str =
    "Bill Budge Pinball Construction Set [Commodore 64] (1of2) - Capture 1.7z";
const ARCHIVE_BYTES: u64 = 11_014_196;
const MEMBER_COUNT: usize = 84;
/// Members keep the capture tool's `.0.raw` / `.1.raw` head designator.
/// A KryoFlux stream records no track or side in its own out-of-band
/// data, so the member name is the only place a capture's position
/// exists — which is why the fixture carries the real convention rather
/// than a spelling the prep script invented.
const FIRST_MEMBER: &str = "Bill Budge Pinball Construction Set[Commodore 64](1of2)00.0.raw";
const FIRST_MEMBER_BYTES: u64 = 184_534;
const SECOND_MEMBER: &str = "Bill Budge Pinball Construction Set[Commodore 64](1of2)01.0.raw";
const SECOND_MEMBER_BYTES: u64 = 210_257;
const LAST_MEMBER: &str = "Bill Budge Pinball Construction Set[Commodore 64](1of2)83.0.raw";
/// The same position on the disk's second side, whose folder decodes
/// to more than its declared dictionary.
const SIDE_ONE_LAST_MEMBER: &str =
    "Bill Budge Pinball Construction Set[Commodore 64](1of2)83.1.raw";
const SIDE_ONE_LAST_BYTES: u64 = 270_987;

/// Every KryoFlux stream opens with the same out-of-band record.
const STREAM_PREFIX: [u8; 16] = [
    0x0d, 0x04, 0x29, 0x00, 0x68, 0x6f, 0x73, 0x74, 0x5f, 0x64, 0x61, 0x74, 0x65, 0x3d, 0x32, 0x30,
];

/// The session holds the P7 deny-write claim for its lifetime, so tests
/// opening a fixture concurrently take private copies.
fn private_copy_of(fixture: &str, tag: &str) -> PathBuf {
    let target = std::env::temp_dir().join(format!(
        "remanence-7z-{tag}-{}.7z",
        std::process::id()
    ));
    std::fs::copy(common::ensure_fixture(fixture), &target).expect("fixture copies");
    target
}

fn private_copy(tag: &str) -> PathBuf {
    private_copy_of(CAPTURE_ZERO, tag)
}

fn entry_path(archive: &Path, member: &str) -> PathBuf {
    archive.join(member)
}

#[test]
fn the_catalog_lists_every_member_in_archive_order() {
    let path = private_copy("list");
    let archive = Archive::open(&path).expect("the archive opens");

    assert_eq!(archive.format_id(), "7z");
    assert_eq!(archive.format_name(), "7z archive");
    assert_eq!(archive.size_bytes(), ARCHIVE_BYTES);

    let entries = archive.entries();
    assert_eq!(entries.len(), MEMBER_COUNT);
    assert_eq!(entries[0].name, FIRST_MEMBER);
    assert_eq!(entries[0].uncompressed_size, FIRST_MEMBER_BYTES);
    assert!(!entries[0].is_dir);
    // One solid folder holds every member, so no member owns a share of
    // the packed bytes.
    assert_eq!(entries[0].compressed_size, None);
    assert_eq!(entries[1].name, SECOND_MEMBER);
    assert_eq!(entries[1].uncompressed_size, SECOND_MEMBER_BYTES);
    assert_eq!(entries[MEMBER_COUNT - 1].name, LAST_MEMBER);

    // Archive order, not sorted order: the members read back in the
    // capture's own track sequence.
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(entry.name, format!("Bill Budge Pinball Construction Set[Commodore 64](1of2){index:02}.0.raw"));
        assert!(!entry.is_dir);
    }

    drop(archive);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_member_of_the_solid_folder_streams_through_a_session() {
    let path = private_copy("member");
    // The second member: reached by decoding past the first, which is
    // what a solid folder costs, and no further.
    let session = Session::open(entry_path(&path, SECOND_MEMBER)).expect("the member opens");
    assert_eq!(session.size_bytes(), SECOND_MEMBER_BYTES);

    let mut front = [0u8; 16];
    session.read_at(0, &mut front).expect("the front reads");
    assert_eq!(front, STREAM_PREFIX);

    // A bounded read across the tail, without the member resident whole.
    let mut tail = [0u8; 32];
    session
        .read_at(SECOND_MEMBER_BYTES - 32, &mut tail)
        .expect("the tail reads");

    let identification = session.identify();
    let container = &identification.containers[0];
    assert_eq!(container.kind, ContainerKind::Archive);
    assert_eq!(container.id, "7z");
    assert_eq!(container.name, "7z archive");

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_folder_longer_than_its_dictionary_streams_through_the_window() {
    // The second side decodes to more than the 16 MiB
    // dictionary it declares, so the LZ window flushes mid-stream and
    // back-references reach across the flush. The last member still
    // arrives whole — and the archive's own per-member CRC, which the
    // catalog checks as it spools, is what says so.
    let path = private_copy_of(CAPTURE_ONE, "long");
    let session =
        Session::open(entry_path(&path, SIDE_ONE_LAST_MEMBER)).expect("the last member opens");
    assert_eq!(session.size_bytes(), SIDE_ONE_LAST_BYTES);

    let mut front = [0u8; 16];
    session.read_at(0, &mut front).expect("the front reads");
    assert_eq!(front, STREAM_PREFIX);

    let mut tail = [0u8; 16];
    session
        .read_at(SIDE_ONE_LAST_BYTES - 16, &mut tail)
        .expect("the tail reads");
    assert_eq!(
        tail,
        [0x00, 0xf4, 0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x0d, 0x0d, 0x0d, 0x0d, 0x0d, 0x0d]
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_member_the_archive_does_not_hold_is_refused_by_name() {
    let path = private_copy("missing");
    let error = Session::open(entry_path(&path, "track99.raw")).expect_err("the member is absent");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    assert!(error.to_string().contains("track99.raw"), "{error}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_corrupted_solid_folder_fails_closed() {
    let path = private_copy("corrupt");
    // One flipped bit deep inside the packed stream. The header sits at
    // the end of the file and still reads cleanly, so the archive lists
    // exactly as before — and the member is still refused rather than
    // delivered wrong (P6).
    let mut bytes = std::fs::read(&path).expect("the copy reads");
    bytes[1000] ^= 0x01;
    std::fs::write(&path, &bytes).expect("the copy rewrites");

    let archive = Archive::open(&path).expect("the header is untouched");
    assert_eq!(archive.entries().len(), MEMBER_COUNT);
    drop(archive);

    let error = Session::open(entry_path(&path, FIRST_MEMBER))
        .expect_err("a corrupted member is refused, never delivered");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_archive_holding_many_members_names_the_ambiguity() {
    let path = private_copy("ambiguous");
    let error = Session::open(&path).expect_err("84 members is ambiguous");
    assert!(
        error.to_string().contains("contains multiple files"),
        "{error}"
    );
    std::fs::remove_file(&path).ok();
}
