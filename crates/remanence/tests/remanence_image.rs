// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The remanence image and its artifact, from outside the crate.
//!
//! The image is the flux family's physical stratum — what the medium
//! holds, distinct from any capture of it — and the `.remanence`
//! artifact is claimed in both directions. Both are exercised here
//! through the public surface alone: nothing below the root is
//! reachable, which is the point, so what is checked is the shape the
//! image reports and the artifact it writes.
//!
//! The worked example is built by hand rather than fetched, so the
//! grammar is tested on every host. The reader accepts any valid zlib
//! stream, which is what lets a test emit a stored block and still be
//! read by the library's own inflater. The golden artifact the research
//! lineage wrote is the scale check beside it, and skips when absent.

use std::path::PathBuf;

use remanence::RemanenceImage;

mod common;

/// The format's worked example: one hole at 3/8 of a turn, one orbit at
/// 57,150 µm opening positive with the standard write geometry,
/// alternating to negative 500 divisions on.
const EXAMPLE_PAYLOAD: [u8; 21] = [
    0x01, // form factor: 5.25-inch
    0x01, // one hole
    0x06, 0x08, // 3/8 of a turn
    0x02, 0x32, // 1/50 extent
    0x01, // one surface
    0x00, // surface 0
    0x01, // one orbit
    0xbe, 0xbe, 0x03, // centre radius 57150
    0x02, // two points
    0x03, 0x00, 0xca, 0x02, 0xb0, 0x03, // +0, positive, plateau 330, guard 432
    0xd0, 0x0f, // +500, nothing stated: alternates to negative
];

const MAGIC: &[u8; 23] = b"REMANENCE_PHYSICAL_DISK";
const SENTINEL: u8 = 0x1a;
const VERSION: u8 = 1;

fn adler32(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

/// The payload as one zlib stream of stored DEFLATE blocks — the
/// simplest valid encoding, and one no compressor is needed to emit.
fn zlib_stored(payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.push(0x01); // final block, stored
    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(&(!(payload.len() as u16)).to_le_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&adler32(payload).to_be_bytes());
    out
}

fn artifact(payload: &[u8], version: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.push(SENTINEL);
    bytes.push(version);
    bytes.extend_from_slice(&zlib_stored(payload));
    bytes
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "remanence-image-{tag}-{}.remanence",
        std::process::id()
    ))
}

/// Puts `bytes` at a scratch path, replacing whatever was there — the
/// library refuses to overwrite, so the test clears the ground itself.
fn placed(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = scratch(tag);
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, bytes).expect("the scratch artifact is written");
    path
}

#[test]
fn a_hand_built_artifact_opens_and_reports_its_disk() {
    let path = placed("worked-example", &artifact(&EXAMPLE_PAYLOAD, VERSION));
    let image = RemanenceImage::open(&path).expect("the worked example opens");

    assert_eq!(image.format_id(), "remanence");
    assert_eq!(image.path(), Some(path.as_path()));
    assert_eq!(image.access_mode(), Some(remanence::AccessMode::ReadOnly));

    let report = image.inspect();
    assert_eq!(report.form_factor, "5.25-inch");
    assert_eq!(report.angular_divisions, 1 << 28);
    assert_eq!(report.surfaces, vec![0]);

    assert_eq!(report.holes.len(), 1);
    let hole = &report.holes[0];
    assert_eq!((hole.center_numerator, hole.center_denominator), (3, 8));
    assert_eq!((hole.extent_numerator, hole.extent_denominator), (1, 50));

    assert_eq!(report.orbits.len(), 1);
    let orbit = &report.orbits[0];
    assert_eq!(orbit.surface, 0);
    assert_eq!(orbit.radius_microns, 57_150);
    assert_eq!(orbit.points, 2);
    assert_eq!(orbit.coherent_points, 2);
    assert_eq!(orbit.unaligned_spans, 0);

    // The points live in private session storage, never in the image.
    assert!(image.backing_bytes() > 0, "the points are backed");
    assert!(
        !report.provenance.is_empty(),
        "an image carries the account of what produced it"
    );

    drop(image);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_image_writes_back_and_reopens_unchanged() {
    let source = placed("round-trip-source", &artifact(&EXAMPLE_PAYLOAD, VERSION));
    let image = RemanenceImage::open(&source).expect("the worked example opens");

    let destination = scratch("round-trip-written");
    let _ = std::fs::remove_file(&destination);
    let written = image.write(&destination).expect("the image writes");

    assert_eq!(written.orbits, 1);
    assert_eq!(written.points, 2);
    assert_eq!(
        written.artifact_bytes,
        std::fs::metadata(&destination)
            .expect("the artifact is there")
            .len()
    );
    // The remanence artifact is the model's own, so the P29 account is
    // empty: nothing of the image was left behind.
    assert!(
        written.declared_loss.is_empty(),
        "the model's own artifact carries every fact it holds"
    );

    let reopened = RemanenceImage::open(&destination).expect("our own artifact opens");
    assert_eq!(reopened.inspect().form_factor, image.inspect().form_factor);
    assert_eq!(reopened.inspect().holes, image.inspect().holes);
    assert_eq!(reopened.inspect().orbits, image.inspect().orbits);

    // Determinism: the same image spells the same artifact, every time.
    let again = scratch("round-trip-again");
    let _ = std::fs::remove_file(&again);
    image.write(&again).expect("the image writes again");
    assert_eq!(
        std::fs::read(&destination).expect("first written"),
        std::fs::read(&again).expect("second written"),
        "the same image writes the same bytes"
    );

    drop(image);
    drop(reopened);
    for path in [source, destination, again] {
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn an_occupied_destination_is_refused_rather_than_overwritten() {
    let source = placed("occupied-source", &artifact(&EXAMPLE_PAYLOAD, VERSION));
    let image = RemanenceImage::open(&source).expect("the worked example opens");

    let occupied = placed("occupied-destination", b"content this library did not write");
    let refusal = image
        .write(&occupied)
        .expect_err("an occupied destination is not written through");
    let said = refusal.to_string();
    assert!(
        said.contains("already there"),
        "the refusal names what is in the way: {said}"
    );
    assert_eq!(
        std::fs::read(&occupied).expect("the occupant is still there"),
        b"content this library did not write",
        "the refusal left the occupant untouched"
    );

    drop(image);
    for path in [source, occupied] {
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn the_header_is_gated_before_anything_is_believed() {
    // A version this reader does not know is refused rather than
    // guessed at (P8): the layout is implicit, so a reader that guesses
    // does not fail, it misreads.
    let future = placed("future-version", &artifact(&EXAMPLE_PAYLOAD, VERSION + 1));
    let refusal = RemanenceImage::open(&future)
        .expect_err("a layout version this build does not know is refused");
    assert!(
        refusal.to_string().contains("version"),
        "the refusal names the version: {refusal}"
    );

    let stranger = placed("not-an-artifact", b"this is not a remanence artifact at all");
    assert!(
        RemanenceImage::open(&stranger).is_err(),
        "a file that is not an artifact is refused"
    );

    let absent = scratch("never-written");
    let _ = std::fs::remove_file(&absent);
    assert!(
        RemanenceImage::open(&absent).is_err(),
        "an artifact that is not there is refused"
    );

    for path in [future, stranger] {
        let _ = std::fs::remove_file(&path);
    }
}
