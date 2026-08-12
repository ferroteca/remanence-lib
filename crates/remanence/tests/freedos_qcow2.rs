// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Unit tests against the FreeDOS rig artifact: a QEMU-authored qcow2
//! whose disk carries two primary partitions and an extended chain of two
//! logicals, each FAT volume labeled and marked.
//!
//! **Unit tests require pre-built fixtures**:
//! If `tests/fixtures/freedos-parttest.qcow2` is missing, run
//! `python test-fixture-prep/prep_fixtures.py` from the testing venv
//! (test-fixture-prep/test-rigs/README.md).

use std::path::PathBuf;

use remanence::{DiskFormat, Format, MediaId, Medium, PartitionScheme, RegionRole, Session};

/// The space one partition of `medium` composes, reached through the
/// door that opens on it. One node, both vantages (D26), and the file
/// verbs live on it and nowhere else (P19).
fn fs(medium: &mut remanence::Medium, ordinal: u32) -> remanence::StorageSpace<'_> {
    let partition = medium
        .partition(ordinal)
        .expect("the pool bears this partition");
    if partition.partition().bears_namespace() {
        partition.filesystem().expect("the declared type determines one")
    } else {
        partition.filesystem_as("fat").expect("these images are FAT")
    }
}

/// Every ordinal on this disk whose partition composes an addressable
/// extent — the data partitions this release reads, in the table's own
/// order. A structural region and a type outside the claim keep their
/// place in the pool and compose nothing (U4).
fn addressable_partitions(medium: &Medium) -> Vec<u32> {
    medium
        .partitions()
        .iter()
        .filter(|partition| partition.is_addressable())
        .map(|partition| partition.ordinal())
        .collect()
}

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
    let id = session.load_media(source, Format::Qcow2)?.id();
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
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("rig artifact opens");
    let disk = disk_session.medium_mut(disk_at).expect("the medium is pooled");
    assert!(matches!(disk.format().expect("a medium is pooled"), DiskFormat::Qcow2 { .. }));

    let report = disk.inspect().expect("inspection reads");
    assert_ne!(report.content, remanence::DiskContent::Blank);
    assert!(
        report.regions.iter().all(|region| region.issue.is_none()),
        "every declared region reads cleanly"
    );

    // Placement and role are different axes: the extended partition is a
    // primary slot whose role is structural, and every chain entry is data.
    let structural = report
        .regions
        .iter()
        .filter(|region| region.role == RegionRole::Structure)
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
    assert_eq!(structural, 1, "one extended partition");
    assert!(logicals >= 2, "the chain's rows report as logical");
    assert!(data >= 4, "two primaries and two logicals");
    assert!(
        report
            .regions
            .iter()
            .any(|region| region.declared_placement == "primary"
                && region.role == RegionRole::Structure),
        "the extended partition is a primary slot with a structural role"
    );
    assert!(report.composed_volume_count() >= 4, "every data region composed");

    // The report above is a reading of the pool beneath it, so the pool
    // is asserted in its own right (P16). This medium's content is laid
    // out under the table it records, and the scheme answers by name.
    assert_eq!(
        disk.partition_scheme(),
        Some(PartitionScheme::Mbr),
        "the rig disk records the scheme its content is laid out under"
    );
    let partitions = disk.partitions();
    assert!(
        partitions.iter().all(|partition| !partition.is_direct()),
        "a medium that records a scheme bears the scheme's own entries and \
         nothing of the library's: the direct partition stands only where \
         no scheme was recorded"
    );
    assert!(
        partitions
            .iter()
            .all(|partition| partition.provenance().is_none()
                && !partition.evidence().is_empty()),
        "and every one of them is evidence — what the adapter read to \
         declare it — rather than an act the library states (P4)"
    );
    assert_eq!(
        partitions
            .iter()
            .map(|partition| partition.ordinal())
            .collect::<Vec<u32>>(),
        (1..=partitions.len() as u32).collect::<Vec<u32>>(),
        "the ordinals are the table's own, numbered from one and running \
         contiguously across the primary slots and the extended chain (U4)"
    );

    // Placement and role travel with the partition, and the report's rows
    // are those same facts read out one seam up rather than a second
    // reading of the table.
    assert_eq!(
        report.regions.len(),
        partitions.len(),
        "one row per declared partition, and no row derived from nothing"
    );
    for partition in &partitions {
        let region = report
            .regions
            .iter()
            .find(|region| region.declared_number == partition.ordinal())
            .unwrap_or_else(|| {
                panic!("partition {} has its own region row", partition.ordinal())
            });
        assert_eq!(
            region.declared_placement,
            partition.placement(),
            "partition {} is placed the same way in both",
            partition.ordinal()
        );
        assert_eq!(
            region.role,
            partition.role(),
            "partition {} carries the same role in both",
            partition.ordinal()
        );
        assert_eq!(
            region.declared_type,
            partition.type_byte().expect("a declared entry records its type"),
            "partition {} records the same type value in both",
            partition.ordinal()
        );
    }

    // Exactly the data partitions this release reads compose an extent:
    // the extended container is structure and composes none, and the
    // addressable set is what the report composed volumes for.
    let addressable = addressable_partitions(disk);
    assert_eq!(
        addressable,
        partitions
            .iter()
            .filter(|partition| partition.role() == RegionRole::Data
                && partition.is_claimed()
                && partition.issue().is_none())
            .map(|partition| partition.ordinal())
            .collect::<Vec<u32>>(),
        "a structural region and an unread type keep their place and \
         compose nothing"
    );
    assert_eq!(
        addressable.len(),
        report.composed_volume_count(),
        "and the report composed one volume for each of them"
    );
    assert!(
        partitions
            .iter()
            .filter(|partition| partition.is_addressable())
            .all(|partition| partition.bears_namespace()),
        "each of them declares a DOS data partition, which determines FAT, \
         so the plain namespace door opens on every one (P19)"
    );

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
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("rig artifact opens");
    let disk = disk_session.medium_mut(disk_at).expect("the medium is pooled");

    // The pool is what is walked, not a list of identities taken off a
    // report: **the content of a medium is reached through the partition
    // that composes it** (P17), and the namespace door opens on each of
    // them because the type each declares determines FAT (P19).
    let ordinals = addressable_partitions(disk);
    assert!(
        ordinals.len() >= 4,
        "two primaries and two logicals compose a volume each: {ordinals:?}"
    );
    for ordinal in ordinals {
        let marker = disk
            .partition(ordinal)
            .expect("the pool bears this partition")
            .filesystem()
            .expect("its declared type determines a namespace")
            .read_file("RMNMARK.TXT")
            .unwrap_or_else(|error| panic!("marker in partition {ordinal}: {error}"));
        assert!(
            marker.starts_with(b"remanence marker:"),
            "partition {ordinal} carries its marker"
        );
    }

    drop(disk_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn write_roundtrip_and_rollback_on_the_installer_built_image() {
    let path = private_artifact("roundtrip");
    let (mut disk_session, disk_at) = attach(&path, Afford::Write).expect("rig artifact opens");
    let disk = disk_session.medium_mut(disk_at).expect("the medium is pooled");

    let partition = addressable_partitions(disk)
        .first()
        .copied()
        .expect("the rig disk's first data partition composes a volume");
    fs(disk, partition).write_file("RMNDIR/RTRIP.BIN",
        b"buffered write on a real image",
    )
    .expect("write buffers");
    assert_eq!(
        fs(disk, partition).read_file("RMNDIR/RTRIP.BIN")
            .expect("reads back"),
        b"buffered write on a real image"
    );
    disk.rollback().expect("a medium is pooled");
    assert!(
        fs(disk, partition).read_file("RMNDIR/RTRIP.BIN").is_err(),
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
    let (mut disk_session, disk_at) = attach(&path, Afford::Read).expect("rig artifact opens");
    let disk = disk_session.medium_mut(disk_at).expect("the medium is pooled");

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
