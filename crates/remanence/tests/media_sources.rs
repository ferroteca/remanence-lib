// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The namespace-file source shapes of `load_media` (F59), on the
//! block side: an archive holding a disk image is two media, taken one
//! declared step at a time — the archive by its format, then the image
//! by its own, a `File` from the first medium's namespace being an
//! ordinary source for the second (U34).

use std::fs::File;

use remanence::{DeviceType, FloppyDrive, Format, Session};

mod common;

const ARCHIVE: &str = "HDOS_1-0.zip";
const IMAGE: &str = "HDOS_1-0_Issue_#50-00-00_890-1.h8d";

#[test]
fn the_one_image_inside_an_archive_loads_by_naming_it() {
    let mut session = Session::new();

    let arc_id = session
        .load_media(
            File::open(common::ensure_fixture(ARCHIVE)).expect("the fixture opens"),
            Format::Zip,
        )
        .expect("the archive loads")
        .id();

    // The entry is named rather than served as "the only file", and the
    // source is free-standing: it rides the archive's claim, so the
    // walk ends before the load begins.
    let file = {
        let arc = session.medium_mut(arc_id).expect("the archive is pooled");
        arc.partition(0)
            .expect("an archive bears its direct partition")
            .filesystem()
            .expect("an archive's content is its namespace")
            .get_file(IMAGE)
            .expect("the entry is named")
            .source()
            .expect("the entry detaches as a load's source")
    };
    assert_eq!(file.name(), IMAGE);

    let disk = session
        .load_media(file, Format::H8d)
        .expect("a File of ours is an ordinary source");
    assert_eq!(
        disk.device_type(),
        Some(DeviceType::Floppy(FloppyDrive::HeathH17))
    );
    let disk_id = disk.id();

    // The declared filesystem reads — the reading the caller's, the
    // check the library's, at every rung (U34).
    let disk = session.medium_mut(disk_id).expect("the disk is pooled");
    let mut hdos = disk
        .partition(0)
        .expect("flexible media record no scheme: the direct partition")
        .filesystem_as("hdos")
        .expect("the declared reading is borne");
    let entries = hdos
        .entries("")
        .expect("a flat catalog: one root of leaves");
    assert!(!entries.is_empty());

    drop(hdos);
    drop(entries);

    // And the disk outlives its source (U33's independence, on the
    // block side).
    session.release_media(arc_id).expect("the archive releases");
    let disk = session.medium_mut(disk_id).expect("still pooled");
    let mut head = [0u8; 2];
    disk.read_at(0, &mut head).expect("the image still reads");
}

#[test]
fn a_declaration_the_entry_cannot_bear_is_refused_by_name() {
    let mut session = Session::new();
    let arc_id = session
        .load_media(
            File::open(common::ensure_fixture(ARCHIVE)).expect("the fixture opens"),
            Format::Zip,
        )
        .expect("the archive loads")
        .id();
    let file = {
        let arc = session.medium_mut(arc_id).expect("pooled");
        arc.partition(0)
            .expect("the direct partition")
            .filesystem()
            .expect("the namespace")
            .get_file(IMAGE)
            .expect("named")
            .source()
            .expect("detached")
    };

    // An h8d declared qcow2: the one adapter the format names is asked,
    // and it refuses naming both sides.
    let error = session
        .load_media(
            file,
            Format::Qcow2 {
                device: remanence::HardDrive::MbrBlock,
            },
        )
        .expect_err("the declaration is checked, never trusted");
    assert!(error.to_string().contains("qcow2"), "{error}");
}
