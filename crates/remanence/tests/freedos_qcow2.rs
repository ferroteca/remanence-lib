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

use remanence::{AttachmentId, DeviceFamily, Session, AccessIntent, DiskFormat, RegionRole};

/// Attaches `path` to a fresh session and returns both, because a medium
/// is reachable only through the device holding it (P32). Tests keep the
/// session alive for as long as they use the medium.
fn attach(
    path: impl AsRef<std::path::Path>,
    intent: AccessIntent,
) -> remanence::Result<(Session, AttachmentId)> {
    let mut session = Session::new();
    let device = session.add_device(DeviceFamily::HARD_DISK)?;
    let attachment = device.attachment();
    device.load_media(path, intent)?;
    Ok((session, attachment))
}

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
fn inspection_reports_primaries_extended_and_logicals() {
    let path = private_artifact("regions");
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("rig artifact opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");
    assert!(matches!(disk.format().expect("a medium is attached"), DiskFormat::Qcow2 { .. }));

    let report = disk.inspect().expect("inspection reads");
    assert_ne!(report.content, remanence::DiskContent::Blank);
    assert!(
        report.regions.iter().all(|region| region.issue.is_none()),
        "every declared region reads cleanly"
    );

    // Placement and role are different axes: the extended container is a
    // primary slot whose role is structural, and every chain entry is data.
    let containers = report
        .regions
        .iter()
        .filter(|region| region.role == RegionRole::Container)
        .count();
    let logicals = report
        .regions
        .iter()
        .filter(|region| region.declared_placement == "logical")
        .count();
    let data = report
        .regions
        .iter()
        .filter(|region| region.role == RegionRole::Data)
        .count();
    assert_eq!(containers, 1, "one extended container");
    assert!(logicals >= 2, "the chain's rows report as logical");
    assert!(data >= 4, "two primaries and two logicals");
    assert!(
        report
            .regions
            .iter()
            .any(|region| region.declared_placement == "primary"
                && region.role == RegionRole::Container),
        "the extended container is a primary slot with a structural role"
    );
    assert!(report.composed_volume_count() >= 4, "every data region composed");

    let labels: Vec<_> = report
        .filesystems
        .iter()
        .filter_map(|filesystem| filesystem.label.as_ref().and_then(|label| label.name.clone()))
        .collect();
    for expected in ["RMNPRI1", "RMNPRI2", "RMNLOG1", "RMNLOG2"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "label {expected}"
        );
    }

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn marker_files_read_out_of_every_volume() {
    let path = private_artifact("markers");
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("rig artifact opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");
    let volumes: Vec<_> = disk
        .inspect()
        .expect("inspection reads")
        .volumes
        .iter()
        .map(|volume| volume.id)
        .collect();
    for volume in volumes {
        let marker = disk
            .read_file(volume, "RMNMARK.TXT")
            .unwrap_or_else(|error| panic!("marker in volume {volume:?}: {error}"));
        assert!(
            marker.starts_with(b"remanence marker:"),
            "volume {volume:?} carries its marker"
        );
    }

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn write_roundtrip_and_rollback_on_the_installer_built_image() {
    let path = private_artifact("roundtrip");
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Write).expect("rig artifact opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");

    let volume_id = disk.inspect().expect("inspection reads").volumes[0].id;
    disk.write_file(
        volume_id,
        "RMNDIR/RTRIP.BIN",
        b"buffered write on a real image",
    )
    .expect("write buffers");
    assert_eq!(
        disk.read_file(volume_id, "RMNDIR/RTRIP.BIN")
            .expect("reads back"),
        b"buffered write on a real image"
    );
    disk.rollback().expect("a medium is attached");
    assert!(
        disk.read_file(volume_id, "RMNDIR/RTRIP.BIN").is_err(),
        "rollback leaves the image untouched"
    );

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

/// The layered report over a real qcow2: the device is block-active, and
/// the schema, its regions, and its volumes are all reported.
#[test]
fn inspection_reports_the_qcow2_device_schema_and_volumes() {
    let path = private_artifact("inspect");
    let (mut disk_session, disk_at) = attach(&path, AccessIntent::Read).expect("rig artifact opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");

    let report = disk.inspect().expect("inspection reads");

    assert_eq!(report.device.image_format, "qcow2");
    assert_eq!(report.device.active_layer, "block");
    assert!(report.device.length_bytes > 0, "the device is addressed");

    assert_eq!(report.content, remanence::DiskContent::Schema);
    assert_eq!(
        report.partition_schema.as_ref().map(|s| s.kind.as_str()),
        Some("mbr")
    );
    assert!(
        report.regions.iter().all(|region| region.issue.is_none()),
        "every declared region reads cleanly"
    );
    assert!(
        report
            .regions
            .iter()
            .all(|region| !region.declared_type_reading.is_empty()),
        "every region explains what its type declares"
    );

    // Every composed volume carries a filesystem the host read.
    assert_eq!(
        report.readable_filesystem_volume_count(),
        report.composed_volume_count()
    );

    std::fs::remove_file(&path).ok();
}
