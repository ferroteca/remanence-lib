// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The archive path streams (pledged P27): a stored entry is read in
//! place from the claimed archive, a compressed entry decodes once into
//! private disk storage, and either way the disk serves bounded
//! reads without the entry resident whole. These tests build their zip
//! by hand, so they run without fixtures.

use remanence::{AccessIntent, Archive, ContainerKind, Disk};

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
    let disk = Disk::open(path, AccessIntent::Read).expect("disk opens");
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
    let archive = &identification.containers[0];
    assert_eq!(archive.kind, ContainerKind::Archive);
    assert_eq!(archive.id, "zip");
    let image = &identification.containers[1];
    assert_eq!(image.kind, ContainerKind::Image);
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
fn the_zip_catalog_lists_its_entries_without_touching_their_data() {
    let expected = payload();
    let zip = build_zip("disk.h8d", 0, &expected, IMAGE_LEN as u32);
    let path = temp_zip("listing", &zip);

    let archive = Archive::open(&path).expect("the archive opens");
    assert_eq!(archive.format_id(), "zip");
    assert_eq!(archive.format_name(), "ZIP archive");
    assert_eq!(archive.size_bytes(), zip.len() as u64);

    let entries = archive.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "disk.h8d");
    assert!(!entries[0].is_dir);
    assert_eq!(entries[0].uncompressed_size, IMAGE_LEN as u64);
    assert_eq!(entries[0].compressed_size, Some(IMAGE_LEN as u64));

    drop(archive);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_lying_uncompressed_size_is_refused_by_name() {
    let expected = payload();
    let compressed = stored_deflate(&expected);
    // The directory claims one byte more than the stream decodes to.
    let zip = build_zip("disk.h8d", 8, &compressed, IMAGE_LEN as u32 + 1);
    let path = temp_zip("lying", &zip);

    let error = Disk::open(&path, AccessIntent::Read).expect_err("the size lie is refused");
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

    let mut disk = Disk::open(&path, AccessIntent::Read).expect("the archived image opens");

    // The raw plane: the archive wrapper is still reported.
    let identification = disk.identify();
    assert_eq!(identification.containers[0].kind, ContainerKind::Archive);
    assert_eq!(identification.containers[0].id, "zip");

    // The presented plane, over the very same claim — the new capability.
    let report = disk.inspect().expect("an archived image inspects");
    assert_eq!(report.device.length_bytes, IMAGE_LEN as u64);

    // And both planes agree about the medium's size.
    assert_eq!(disk.image_size_bytes(), IMAGE_LEN as u64);

    drop(disk);
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_archive_entry_refuses_a_write_open_naming_the_reason() {
    // The honest half of the merge: gaining the disk verbs over an
    // archive entry must not imply gaining writes to one. A write would
    // have to be encoded back into the archive's own grammar, and no
    // adapter claims that (P13), so the refusal names it rather than
    // degrading to read-only.
    let expected = payload();
    let zip = build_zip("disk.h8d", 0, &expected, IMAGE_LEN as u32);
    let path = temp_zip("nowrite", &zip);

    let error = Disk::open(&path, AccessIntent::Write).expect_err("a write open is refused");
    let message = error.to_string();
    assert!(message.contains("archive"), "names the archive: {message}");
    assert!(message.contains("writing"), "names the refusal: {message}");

    std::fs::remove_file(&path).ok();
}
