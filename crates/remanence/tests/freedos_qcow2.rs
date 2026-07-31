// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Unit tests against the FreeDOS rig artifact: a QEMU-authored qcow2
//! whose disk carries two primary partitions and an extended chain of two
//! logicals, each FAT volume labeled and marked.
//!
//! **Unit tests require pre-built fixtures**:
//! If `tests/fixtures/freedos-parttest.qcow2` is missing, run
//! `python testing-prep/prep_fixtures.py` from the testing venv
//! (testing-prep/test-rigs/README.md).

use std::path::PathBuf;

use remanence::{AccessIntent, Disk, DiskFormat};

mod common;

/// Returns a private copy of the FreeDOS rig artifact for testing.
fn private_artifact(tag: &str) -> PathBuf {
    let master = common::ensure_fixture("freedos-parttest.qcow2");
    let copy = std::env::temp_dir().join(format!(
        "remanence-freedos-{tag}-{}.qcow2",
        std::process::id()
    ));
    std::fs::copy(master, &copy).expect("artifact copies");
    copy
}

#[test]
fn geometry_reports_primaries_extended_and_logicals() {
    let path = private_artifact("geometry");
    let mut disk = Disk::open(&path, AccessIntent::Read).expect("rig artifact opens");
    assert!(matches!(disk.format(), DiskFormat::Qcow2 { .. }));

    let geometry = disk.geometry().expect("geometry reads");
    assert!(!geometry.blank, "an installed disk is not blank");
    assert!(
        geometry
            .partitions
            .iter()
            .all(|partition| partition.issue.is_none()),
        "every declared row reads cleanly"
    );
    let extended = geometry
        .partitions
        .iter()
        .filter(|partition| {
            partition
                .type_name
                .as_deref()
                .is_some_and(|name| name.starts_with("extended"))
        })
        .count();
    let logicals = geometry
        .partitions
        .iter()
        .filter(|partition| partition.kind == remanence::PartitionKind::Logical)
        .count();
    let data_partitions = geometry.partitions.len() - extended;
    assert_eq!(extended, 1, "one extended partition");
    assert!(logicals >= 2, "the chain's rows report as logical");
    assert!(data_partitions >= 4, "two primaries and two logicals");
    assert!(geometry.volumes.len() >= 4, "every data partition readable");

    let labels: Vec<_> = geometry
        .volumes
        .iter()
        .filter_map(|volume| volume.label.clone())
        .collect();
    for expected in ["RMNPRI1", "RMNPRI2", "RMNLOG1", "RMNLOG2"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "label {expected}"
        );
    }

    drop(disk);
    std::fs::remove_file(&path).ok();
}

#[test]
fn marker_files_read_out_of_every_volume() {
    let path = private_artifact("markers");
    let mut disk = Disk::open(&path, AccessIntent::Read).expect("rig artifact opens");
    let volumes = disk.geometry().expect("geometry").volumes;
    for volume in volumes {
        let marker = disk
            .read_file(&volume.id, "RMNMARK.TXT")
            .unwrap_or_else(|error| panic!("marker in volume {}: {error}", volume.id));
        assert!(
            marker.starts_with(b"remanence marker:"),
            "volume {} carries its marker",
            volume.id
        );
    }

    drop(disk);
    std::fs::remove_file(&path).ok();
}

#[test]
fn write_roundtrip_and_rollback_on_the_installer_built_image() {
    let path = private_artifact("roundtrip");
    let mut disk = Disk::open(&path, AccessIntent::Write).expect("rig artifact opens");

    let volume_id = disk.geometry().expect("geometry").volumes[0].id.clone();
    disk.write_file(
        &volume_id,
        "RMNDIR/RTRIP.BIN",
        b"buffered write on a real image",
    )
    .expect("write buffers");
    assert_eq!(
        disk.read_file(&volume_id, "RMNDIR/RTRIP.BIN")
            .expect("reads back"),
        b"buffered write on a real image"
    );
    disk.rollback();
    assert!(
        disk.read_file(&volume_id, "RMNDIR/RTRIP.BIN").is_err(),
        "rollback leaves the image untouched"
    );

    drop(disk);
    std::fs::remove_file(&path).ok();
}
