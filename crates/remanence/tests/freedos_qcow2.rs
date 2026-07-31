// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Opt-in integration tests against the FreeDOS rig artifact
//! (testing/freedos-rig/README.md): a QEMU-authored qcow2 whose disk
//! carries two primary partitions and an extended chain of two
//! logicals, each FAT volume labeled and marked.
//!
//! **The suite builds the artifact itself** when it is missing, by
//! driving reliquary against the checked-in blueprint — one command,
//! no manual steps:
//!
//! `cargo test -p remanence --test freedos_qcow2 -- --ignored`
//!
//! Reliquary is provisioned automatically through
//! `uv tool run --from reliquary rlq` (install-if-missing, cached env,
//! Python fetched by uv when absent). Prerequisites shrink to **uv and
//! QEMU** — and the tests fail naming the gap, they do not skip.
//! Delete `tests/fixtures/freedos-parttest.qcow2` to force a rebuild;
//! set `REMANENCE_RIG_RELIQUARY` to a path (e.g. a local reliquary
//! checkout) to run against unpublished reliquary changes.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use remanence::{AccessIntent, Disk, DiskFormat};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir().parent().and_then(Path::parent).expect("workspace root").to_path_buf()
}

fn run_rlq(home: &Path, args: &[&str]) -> Result<String, String> {
    // uv provisions reliquary on demand: install-if-missing into uv's
    // cached tool environment, Python fetched by uv when absent.
    // REMANENCE_RIG_RELIQUARY overrides the package spec (e.g. a local
    // reliquary checkout while iterating on unpublished changes).
    let spec = std::env::var("REMANENCE_RIG_RELIQUARY")
        .unwrap_or_else(|_| "reliquary".to_owned());
    let mut command = Command::new("uv");
    command
        .args(["tool", "run", "--from", &spec, "rlq"])
        .args(args)
        .arg("--home-dir")
        .arg(home);
    let output = command.output().map_err(|error| {
        format!(
            "could not run 'uv' ({error}).\n\
             The FreeDOS rig provisions reliquary through uv; it needs\n\
             - uv on PATH (https://docs.astral.sh/uv/)\n\
             - QEMU installed where reliquary can discover it (a standard\n\
               install location is sufficient; PATH also works)\n\
             See testing/freedos-rig/README.md."
        )
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail = |text: &str| -> String {
            let start = text.len().saturating_sub(2000);
            text[start..].to_owned()
        };
        return Err(format!(
            "'uv tool run --from reliquary rlq {}' failed with {}.\n\
             --- stdout tail ---\n{}\n--- stderr tail ---\n{}\n\
             (QEMU missing? script drift? See testing/freedos-rig/README.md;\n\
             the install script's to-converge points are tracked as T6.)",
            args.join(" "),
            output.status,
            tail(&stdout),
            tail(&stderr)
        ));
    }
    Ok(stdout)
}

/// Builds (or reuses) the rig artifact. Runs at most once per test
/// process; every failure is cached and repeated verbatim.
fn build_or_reuse() -> Result<PathBuf, String> {
    let fixtures = manifest_dir().join("tests/fixtures");
    let artifact = fixtures.join("freedos-parttest.qcow2");
    if artifact.exists() {
        return Ok(artifact);
    }
    std::fs::create_dir_all(&fixtures)
        .map_err(|error| format!("cannot create {}: {error}", fixtures.display()))?;

    let rig = repo_root().join("testing/freedos-rig");
    let home = std::env::var_os("REMANENCE_RIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target/freedos-rig-home"));
    std::fs::create_dir_all(&home)
        .map_err(|error| format!("cannot create {}: {error}", home.display()))?;

    let blueprints = rig.join("blueprints");
    let scripts = rig.join("scripts");

    // Build: reliquary creates the machine if none exists, fetches the
    // pinned LiveCD into its cache, and drives the install script.
    run_rlq(
        &home,
        &[
            "run-script",
            "install",
            "--blueprint",
            "remanence-parttest",
            "--blueprints-dir",
            blueprints.to_str().expect("utf-8 path"),
            "--scripts-dir",
            scripts.to_str().expect("utf-8 path"),
        ],
    )?;

    // Harvest the machine's hdd0 image.
    let machine_dir = PathBuf::from(
        run_rlq(&home, &["get-machine-dir", "--blueprint", "remanence-parttest"])?
            .trim(),
    );
    let media = machine_dir.join("media");
    let image = std::fs::read_dir(&media)
        .map_err(|error| format!("cannot read {}: {error}", media.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.extension().is_some_and(|extension| extension == "qcow2")
        })
        .ok_or_else(|| {
            format!(
                "no qcow2 image found under {} — if reliquary materialized \
                 hdd0 as another format, the rig blueprint needs adjusting",
                media.display()
            )
        })?;

    std::fs::copy(&image, &artifact).map_err(|error| {
        format!("copying {} to {}: {error}", image.display(), artifact.display())
    })?;
    // The machine stays in the rig home for inspection;
    // `rlq destroy --blueprint remanence-parttest --home-dir <home>`
    // resets it.
    Ok(artifact)
}

/// The built artifact, or a panic naming exactly what is missing. Each
/// test takes a private copy: sessions hold the P7 deny-write claim, so
/// concurrent tests must not share an open.
fn private_artifact(tag: &str) -> PathBuf {
    static ARTIFACT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    let master = match ARTIFACT.get_or_init(build_or_reuse) {
        Ok(path) => path,
        Err(message) => panic!("{message}"),
    };
    let copy = std::env::temp_dir().join(format!(
        "remanence-freedos-{tag}-{}.qcow2",
        std::process::id()
    ));
    std::fs::copy(master, &copy).expect("artifact copies");
    copy
}

#[test]
#[ignore = "builds and tests the reliquary rig artifact; needs rlq + QEMU (testing/freedos-rig)"]
fn geometry_reports_primaries_extended_and_logicals() {
    let path = private_artifact("geometry");
    let mut disk = Disk::open(&path, AccessIntent::Read).expect("rig artifact opens");
    assert!(matches!(disk.format(), DiskFormat::Qcow2 { .. }));

    let geometry = disk.geometry().expect("geometry reads");
    assert!(!geometry.blank, "an installed disk is not blank");
    assert!(
        geometry.partitions.iter().all(|partition| partition.issue.is_none()),
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

    let labels: Vec<_> =
        geometry.volumes.iter().filter_map(|volume| volume.label.clone()).collect();
    for expected in ["RMNPRI1", "RMNPRI2", "RMNLOG1", "RMNLOG2"] {
        assert!(labels.iter().any(|label| label == expected), "label {expected}");
    }

    drop(disk);
    std::fs::remove_file(&path).ok();
}

#[test]
#[ignore = "builds and tests the reliquary rig artifact; needs rlq + QEMU (testing/freedos-rig)"]
fn marker_files_read_out_of_every_volume() {
    let path = private_artifact("markers");
    let mut disk = Disk::open(&path, AccessIntent::Read).expect("rig artifact opens");
    let volumes = disk.geometry().expect("geometry").volumes.len();
    for volume in 0..volumes {
        let marker = disk
            .read_file(volume, "RMNMARK.TXT")
            .unwrap_or_else(|error| panic!("marker in volume {volume}: {error}"));
        assert!(
            marker.starts_with(b"remanence marker:"),
            "volume {volume} carries its marker"
        );
    }

    drop(disk);
    std::fs::remove_file(&path).ok();
}

#[test]
#[ignore = "builds and tests the reliquary rig artifact; needs rlq + QEMU (testing/freedos-rig)"]
fn write_roundtrip_and_rollback_on_the_installer_built_image() {
    let path = private_artifact("roundtrip");
    let mut disk = Disk::open(&path, AccessIntent::Write).expect("rig artifact opens");

    disk.write_file(0, "RMNDIR/RTRIP.BIN", b"buffered write on a real image")
        .expect("write buffers");
    assert_eq!(
        disk.read_file(0, "RMNDIR/RTRIP.BIN").expect("reads back"),
        b"buffered write on a real image"
    );
    disk.rollback();
    assert!(
        disk.read_file(0, "RMNDIR/RTRIP.BIN").is_err(),
        "rollback leaves the image untouched"
    );

    drop(disk);
    std::fs::remove_file(&path).ok();
}
