// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The archive journey streams (P27): a stored entry is read in place
//! from the claimed archive, a compressed entry decodes once into
//! private session storage, and either way the device serves bounded
//! reads without the entry resident whole.
//!
//! The journey itself is the model's: the archive is a medium loaded
//! into an archive-family device, its entries are the namespace that
//! device resolves to, and an entry recognized as an artifact of its own
//! is loaded into a device of its own. These tests build their zip by
//! hand, so they run without fixtures.

use remanence::{
    DeviceFamily, EntryKind, ErrorCategory, Format, LayerKind, MediaId, Session, SpaceRule,
};

mod common;
use common::open_read;

/// Attaches `path` to a fresh session and returns both, because a medium
/// is reachable only through the device holding it (P32). Tests keep the
/// session alive for as long as they use the medium.
fn archive_session(
    path: impl AsRef<std::path::Path>,
) -> remanence::Result<(Session, MediaId)> {
    let mut session = Session::new();
    let id = session.load_media(open_read(path), Format::Zip)?.id();
    Ok((session, id))
}

/// The nested journey: the archive into its own device, the entry named
/// through the namespace that device bears, and the artifact it holds
/// loaded into a drive **in a machine of its own** — the host's archive
/// was never part of the machine whose disk it holds.
fn load_entry(
    path: impl AsRef<std::path::Path>,
    entry: &str,
) -> remanence::Result<(Session, MediaId)> {
    let (mut session, archive) = archive_session(path)?;
    let discovery = session
        .medium_mut(archive)
        .expect("the archive is pooled")
        .filesystem()?
        .get_file(entry)?
        .discover()?;
    let disk = session.load_discovery(discovery)?.id();
    session.add_machine("h89")?;
    session
        .require_machine("h89")?
        .add_device(DeviceFamily::HEATHKIT_H17)?
        .insert(disk)?;
    Ok((session, disk))
}

const IMAGE_LEN: usize = 102_400; // h8d-sized, so identification bites

fn payload() -> Vec<u8> {
    (0..IMAGE_LEN as u32).map(|n| (n % 247) as u8).collect()
}

/// A DEFLATE stream of stored blocks wrapping `data` — legal RFC 1951,
/// hand-craftable without a compressor.
fn stored_deflate(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunks = data.chunks(0xffff).peekable();
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        out.push(u8::from(last));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

/// One-entry zip: local header, entry data, central directory, EOCD.
fn build_zip(name: &str, method: u16, data: &[u8], uncompressed_size: u32) -> Vec<u8> {
    let mut zip = Vec::new();

    let local_header_offset = zip.len() as u32;
    zip.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&method.to_le_bytes());
    zip.extend_from_slice(&[0u8; 4]); // time, date
    zip.extend_from_slice(&[0u8; 4]); // crc-32 (unchecked)
    zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
    zip.extend_from_slice(&uncompressed_size.to_le_bytes());
    zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra
    zip.extend_from_slice(name.as_bytes());
    zip.extend_from_slice(data);

    let central_offset = zip.len() as u32;
    zip.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    zip.extend_from_slice(&20u16.to_le_bytes()); // version made by
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&method.to_le_bytes());
    zip.extend_from_slice(&[0u8; 4]); // time, date
    zip.extend_from_slice(&[0u8; 4]); // crc-32
    zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
    zip.extend_from_slice(&uncompressed_size.to_le_bytes());
    zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment
    zip.extend_from_slice(&0u16.to_le_bytes()); // disk number
    zip.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
    zip.extend_from_slice(&0u32.to_le_bytes()); // external attributes
    zip.extend_from_slice(&local_header_offset.to_le_bytes());
    zip.extend_from_slice(name.as_bytes());
    let central_size = zip.len() as u32 - central_offset;

    zip.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    zip.extend_from_slice(&0u16.to_le_bytes()); // disk number
    zip.extend_from_slice(&0u16.to_le_bytes()); // central directory disk
    zip.extend_from_slice(&1u16.to_le_bytes()); // entries on this disk
    zip.extend_from_slice(&1u16.to_le_bytes()); // entries total
    zip.extend_from_slice(&central_size.to_le_bytes());
    zip.extend_from_slice(&central_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment
    zip
}

fn temp_zip(tag: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "remanence-zipstream-{tag}-{}.zip",
        std::process::id()
    ));
    std::fs::write(&path, bytes).expect("zip writes");
    path
}

fn assert_streamed_session(path: &std::path::Path, expected: &[u8]) {
    let (mut disk_session, disk_at) = load_entry(path, "disk.h8d").expect("the entry loads");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");
    assert_eq!(disk.image_size_bytes(), expected.len() as u64);

    // Bounded reads round-trip, at the front and across the tail.
    let mut front = [0u8; 64];
    disk.read_at(0, &mut front).expect("front reads");
    assert_eq!(&front[..], &expected[..64]);
    let mut tail = [0u8; 64];
    disk.read_at(expected.len() as u64 - 64, &mut tail).expect("tail reads");
    assert_eq!(&tail[..], &expected[expected.len() - 64..]);

    // The layers report the archive wrapper and the h8d-sized image.
    let identification = disk.identify();
    let archive = &identification.layers[0];
    assert_eq!(archive.kind, LayerKind::Archive);
    assert_eq!(archive.id, "zip");
    let image = &identification.layers[1];
    assert_eq!(image.kind, LayerKind::Image);
    assert_eq!(image.id, "h8d");
}

#[test]
fn a_stored_entry_streams_in_place_from_the_claimed_archive() {
    let expected = payload();
    let zip = build_zip("disk.h8d", 0, &expected, IMAGE_LEN as u32);
    let path = temp_zip("stored", &zip);

    assert_streamed_session(&path, &expected);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_deflated_entry_decodes_into_session_storage_and_streams() {
    let expected = payload();
    let compressed = stored_deflate(&expected);
    let zip = build_zip("disk.h8d", 8, &compressed, IMAGE_LEN as u32);
    let path = temp_zip("deflated", &zip);

    assert_streamed_session(&path, &expected);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_archive_lists_its_entries_without_touching_their_data() {
    // The listing is the namespace the archive medium bears, reached
    // through the one node that carries file verbs — no second journey,
    // and no entry's data touched to produce it.
    let expected = payload();
    let zip = build_zip("disk.h8d", 0, &expected, IMAGE_LEN as u32);
    let path = temp_zip("listing", &zip);

    let (mut session, archive) = archive_session(&path).expect("the archive loads");
    let device = session.medium_mut(archive).expect("the medium is pooled");
    assert_eq!(device.image_size_bytes(), zip.len() as u64);

    let mut namespace = device.filesystem().expect("an archive is its namespace");
    assert_eq!(namespace.kind().expect("a kind"), "zip");
    assert!(namespace.has_namespace());
    assert!(
        !namespace.is_addressable(),
        "an archive has no addressed extent beneath its names"
    );

    let entries = namespace.entries("").expect("the root lists");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "disk.h8d");
    assert_eq!(entries[0].kind, EntryKind::File);
    assert_eq!(entries[0].size_bytes, IMAGE_LEN as u64);
    assert!(
        entries[0]
            .declared
            .iter()
            .any(|fact| fact.key == "compressed-size" && fact.value == IMAGE_LEN.to_string()),
        "the grammar's own facts travel with the entry: {:?}",
        entries[0].declared
    );

    drop(namespace);
    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_lying_uncompressed_size_is_refused_by_name() {
    let expected = payload();
    let compressed = stored_deflate(&expected);
    // The directory claims one byte more than the stream decodes to.
    let zip = build_zip("disk.h8d", 8, &compressed, IMAGE_LEN as u32 + 1);
    let path = temp_zip("lying", &zip);

    let error = load_entry(&path, "disk.h8d").expect_err("the size lie is refused");
    assert!(error.to_string().contains("expected"), "names the mismatch: {error}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_archived_image_now_inspects_and_refuses_writes_by_name() {
    // F43's proof. Before the merge these two facts could not coexist:
    // identification reached inside an archive and the disk verbs did
    // not, because each surface took its own P7 claim on the file and
    // only one of them knew what an archive was. One claim now serves
    // both planes, so the same handle answers for layers *and* for the
    // presented disk.
    let expected = payload();
    let zip = build_zip("disk.h8d", 0, &expected, IMAGE_LEN as u32);
    let path = temp_zip("inspects", &zip);

    let (mut disk_session, disk_at) = load_entry(&path, "disk.h8d").expect("the entry loads");
    let disk = disk_session
        .medium_mut(disk_at)
        .expect("the medium is pooled");

    // The raw plane: the archive wrapper is still reported.
    let identification = disk.identify();
    assert_eq!(identification.layers[0].kind, LayerKind::Archive);
    assert_eq!(identification.layers[0].id, "zip");

    // The presented plane, over the very same claim — the new capability.
    let report = disk.inspect().expect("an archived image inspects");
    assert_eq!(report.device.length_bytes, IMAGE_LEN as u64);

    // And both planes agree about the medium's size.
    assert_eq!(disk.image_size_bytes(), IMAGE_LEN as u64);

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_archive_writes_nothing_whatever_the_handle_affords() {
    // Read-only, as archives are: a write would have to be encoded back
    // into the archive's own grammar and no adapter claims that (P13),
    // so the refusal names it rather than degrading to read-only.
    let expected = payload();
    let zip = build_zip("disk.h8d", 0, &expected, IMAGE_LEN as u32);
    let path = temp_zip("nowrite", &zip);

    // A handle affording writes is not an error and buys nothing: the
    // medium loads, and the namespace refuses every write by name.
    let mut session = Session::new();
    let archive = session
        .load_media(common::open_write(&path), Format::Zip)
        .expect("a writable handle is the caller's business")
        .id();
    let error = session
        .medium_mut(archive)
        .expect("the medium is pooled")
        .filesystem()
        .expect("an archive is its namespace")
        .write_file("disk.h8d", b"denied")
        .expect_err("no adapter encodes an archive back into its grammar");
    let message = error.to_string();
    assert_eq!(error.rule(), Some(SpaceRule::NotWritable.as_str()));
    assert!(
        message.contains("does not write"),
        "names the refusal: {message}"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn an_image_past_the_hdos_bound_is_refused_by_size_never_loaded() {
    // F43's acceptance list: the merge must not have lost the P27 bound
    // the HDOS reader is held to. HDOS lives on small vintage disks, so
    // anything past the bound is refused by its size alone — the point
    // being that the refusal happens before a byte is materialized, not
    // after a large image has been read whole.
    let path = std::env::temp_dir().join(format!(
        "remanence-hdos-bound-{}.img",
        std::process::id()
    ));
    std::fs::write(&path, vec![0u8; 9 * 1024 * 1024]).expect("oversized image writes");

    let mut session = Session::new();
    let medium = session
        .load_media(open_read(&path), Format::Raw)
        .expect("the image itself opens; only the HDOS reader is bounded");

    // The medium composes no volume, so the resolver would look for a
    // namespace the medium bears itself — and that lookup is bounded
    // (P27), so a medium this size is a named absence rather than a full
    // scan.
    let error = medium
        .filesystem()
        .expect_err("a medium past the bound is refused");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    assert_eq!(
        error.rule(),
        Some(SpaceRule::NoNamespace.as_str()),
        "the resolver's refusals carry their rule identity (P10)"
    );
    let message = error.to_string();
    assert!(message.contains("bound"), "names the bound: {message}");

    drop(session);
    std::fs::remove_file(&path).ok();
}
