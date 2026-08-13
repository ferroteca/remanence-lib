// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The rig layout, built rather than downloaded (D49).
//!
//! `dos_drive_letters_rig.rs` needed the FreeDOS artifact for one thing:
//! a disk carrying **two DOS primaries and an extended chain of two
//! logicals**, which is the shape the claimed drive-letter variants
//! disagree over. That shape is wholly specified, so this suite builds
//! it — and these tests are what say the built one is the shape claimed,
//! before anything else relies on it.
//!
//! Written first and separately on purpose: a synthetic fixture that
//! quietly differs from the artifact it replaces would move every test
//! that trusts it onto a false footing.

use remanence::{Format, HardDrive, RegionRole, Session};

mod dos_letters;
use dos_letters::{synthetic_rig_disk, write_image};

fn pooled(path: &std::path::PathBuf) -> (Session, remanence::MediaId) {
    let mut session = Session::new();
    let id = session
        .load_media(
            std::fs::File::open(path).expect("the built image opens"),
            Format::Raw {
                device: HardDrive::MbrBlock,
                block_bytes: 512,
            },
        )
        .expect("a raw reading of a built image is borne")
        .id();
    (session, id)
}

#[test]
fn the_built_disk_carries_two_primaries_and_two_logicals() {
    let path = write_image("rig-layout", synthetic_rig_disk());
    let (mut session, id) = pooled(&path);
    let report = session
        .medium_mut(id)
        .expect("pooled")
        .inspect()
        .expect("inspection reads");

    let primaries: Vec<_> = report
        .regions
        .iter()
        .filter(|region| region.declared_placement == "primary" && region.role == RegionRole::Data)
        .collect();
    let logicals: Vec<_> = report
        .regions
        .iter()
        .filter(|region| region.declared_placement == "logical" && region.role == RegionRole::Data)
        .collect();

    assert_eq!(primaries.len(), 2, "two DOS primaries: {:?}", report.regions);
    assert_eq!(logicals.len(), 2, "an extended chain of two logicals");

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn every_volume_composes_and_reads_as_fat() {
    let path = write_image("rig-volumes", synthetic_rig_disk());
    let (mut session, id) = pooled(&path);
    let report = session
        .medium_mut(id)
        .expect("pooled")
        .inspect()
        .expect("inspection reads");

    assert_eq!(
        report.volumes.len(),
        4,
        "four data volumes compose: {:?}",
        report.volumes
    );
    assert_eq!(
        report.filesystems.len(),
        4,
        "each one reads as a filesystem: {:?}",
        report.filesystems
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_first_primary_carries_the_marker_a_letter_is_proved_by() {
    let path = write_image("rig-marker", synthetic_rig_disk());
    let (mut session, id) = pooled(&path);
    let medium = session.medium_mut(id).expect("pooled");

    let marker = medium
        .partition(1)
        .expect("the first table entry is pooled")
        .filesystem()
        .expect("a DOS data partition determines FAT")
        .read_file("RMNMARK.TXT")
        .expect("the first primary carries the marker");
    assert!(
        marker.starts_with(b"remanence marker:"),
        "the marker reads back: {:?}",
        String::from_utf8_lossy(&marker)
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn each_volume_states_the_label_it_was_built_with() {
    let path = write_image("rig-labels", synthetic_rig_disk());
    let (mut session, id) = pooled(&path);
    let report = session
        .medium_mut(id)
        .expect("pooled")
        .inspect()
        .expect("inspection reads");

    let mut labels: Vec<String> = report
        .filesystems
        .iter()
        .filter_map(|filesystem| filesystem.label.as_ref())
        .filter_map(|label| label.name.clone())
        .collect();
    labels.sort();
    assert_eq!(
        labels,
        vec![
            "RMNLOG1".to_owned(),
            "RMNLOG2".to_owned(),
            "RMNPRI1".to_owned(),
            "RMNPRI2".to_owned(),
        ],
        "the four volumes name themselves as built"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
