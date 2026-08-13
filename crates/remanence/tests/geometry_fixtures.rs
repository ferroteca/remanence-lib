// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Discovered geometry, over artifacts that are their own evidence.
//!
//! The rest of this seam is tested over images the project builds
//! byte by byte (`geometry.rs`). These two readings cannot be: one is
//! a format that declares a geometry for every image it claims, and
//! the other is that format nested in an archive. Both need the real
//! artifact, so both are behind the `fixtures` feature and need
//! `python test-fixture-prep/prep_fixtures.py` to have run.

use remanence::{
    ErrorCategory, Format, GeometrySource, GeometryState, MediaId,
    RecordingGeometry, Session,
};

mod common;
use common::{ensure_fixture, open_read};

/// The rule identity a refusal from this seam carries.
const NOT_SECTOR_ADDRESSED: &str = "not-sector-addressed";

/// One medium pooled from a declared reading, as `geometry.rs` does it.
fn pool(source: std::fs::File, format: Format) -> (Session, MediaId) {
    let mut session = Session::new();
    let id = session
        .load_media(source, format)
        .expect("the declaration is borne")
        .id();
    (session, id)
}

#[test]
fn a_format_that_declares_a_geometry_states_it_for_every_image_it_claims() {
    // The H8D adapter records a Heathkit H-17 recording: 40 cylinders of
    // one side at ten 256-byte sectors, which the extent confirms
    // exactly. Nothing else on this disk states a geometry at all.
    let source = ensure_fixture("HDOS_1-0_Issue_#50-00-00_890-1.h8d");
    let (mut session, id) = pool(open_read(&source), Format::H8d);
    let medium = session.medium_mut(id).expect("pooled");

    assert_eq!(
        medium.geometry().determined(),
        Some(RecordingGeometry {
            cylinders: 40,
            heads: 1,
            sectors_per_track: 10,
            sector_bytes: 256,
        })
    );
    let declaration = medium
        .geometry()
        .readings()
        .iter()
        .find(|reading| reading.source == GeometrySource::FormatDeclaration)
        .expect("the format declares one");
    assert!(
        declaration.at.contains("h8d"),
        "the reading names the format that declared it: {}",
        declaration.at
    );

    // The recording's coordinates and the artifact's own bytes agree,
    // which is what a format-declared geometry over a raw block layout
    // means: cylinder 1, head 0, sector 1 is the eleventh record.
    let mut recorded = [0u8; 256];
    medium
        .read_sector(1, 0, 1, &mut recorded)
        .expect("the record reads");
    let mut raw = [0u8; 256];
    medium.read_at(2_560, &mut raw).expect("the bytes read");
    assert_eq!(recorded, raw);

    // A read-only claim refuses the write before anything else does.
    let error = medium
        .write_sector(1, 0, 1, &recorded)
        .expect_err("the handle affords no write");
    assert_eq!(error.category(), ErrorCategory::ReadOnly);

    drop(session);
}

#[test]
fn an_archive_has_no_coordinates_at_all_and_says_so() {
    let source = ensure_fixture("HDOS_1-0.zip");
    let (mut session, id) = pool(open_read(&source), Format::Zip);
    let medium = session.medium_mut(id).expect("pooled");

    assert_eq!(medium.geometry().state(), GeometryState::Unstated);
    assert!(
        medium.geometry().readings().is_empty(),
        "nothing was read, because there is nothing there to read"
    );
    let error = medium
        .read_sector(0, 0, 1, &mut [0u8; 512])
        .expect_err("an archive has no sector");
    assert_eq!(error.rule(), Some(NOT_SECTOR_ADDRESSED));
    assert!(
        error.to_string().contains("recorded by no device"),
        "the refusal says why rather than naming a missing geometry: {error}"
    );

    drop(session);
}
