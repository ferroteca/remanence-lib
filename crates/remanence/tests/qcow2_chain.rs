// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Backing-chain unit tests (U6) over synthetic images the
//! project owns outright: hand-built qcow2 overlays whose chains bottom
//! out in a hand-built FAT16 volume. Everything runs through the public
//! `Disk` surface; cluster-level composition semantics are unit-tested
//! inside the crate.

use std::path::{Path, PathBuf};
use std::process::Command;

use remanence::{AttachmentId, Session, AccessIntent, Disk, DiskFormat, ErrorCategory};

/// Attaches `path` to a fresh session and returns both, because a medium
/// is reachable only through the device holding it (P32). Tests keep the
/// session alive for as long as they use the medium.
fn attach(
    path: impl AsRef<std::path::Path>,
    intent: AccessIntent,
) -> remanence::Result<(Session, AttachmentId)> {
    let mut session = Session::new();
    let attachment = session.attach(path, intent)?;
    Ok((session, attachment))
}

const CLUSTER_BITS: u32 = 12;
const CLUSTER: u64 = 1 << CLUSTER_BITS;

/// A fresh directory for one test's chain, so relative backing names
/// never collide across concurrently running tests.
fn chain_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "remanence-chain-{tag}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("chain dir creates");
    dir
}

/// Builds a minimal FAT16 volume: 512-byte sectors, 1 sector/cluster,
/// 2 FATs of 32 sectors, 8000 total sectors, labeled REMANENCE, with
/// one file MARKER.TXT in the root directory.
fn synthetic_fat16() -> Vec<u8> {
    const TOTAL_SECTORS: usize = 8000;
    let mut image = vec![0u8; TOTAL_SECTORS * 512];
    image[0] = 0xeb;
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC");
    image[11..13].copy_from_slice(&512u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1u16.to_le_bytes());
    image[16] = 2;
    image[17..19].copy_from_slice(&512u16.to_le_bytes());
    image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
    image[21] = 0xf8;
    image[22..24].copy_from_slice(&32u16.to_le_bytes());
    image[510] = 0x55;
    image[511] = 0xaa;
    for fat in 0..2usize {
        let base = (1 + fat * 32) * 512;
        image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
        image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
        // MARKER.TXT occupies cluster 2.
        image[base + 4..base + 6].copy_from_slice(&0xffffu16.to_le_bytes());
    }
    let root = (1 + 2 * 32) * 512;
    image[root..root + 11].copy_from_slice(b"REMANENCE  ");
    image[root + 11] = 0x08;
    let marker = root + 32;
    image[marker..marker + 11].copy_from_slice(b"MARKER  TXT");
    image[marker + 26..marker + 28].copy_from_slice(&2u16.to_le_bytes());
    let contents = b"read through the chain";
    image[marker + 28..marker + 32].copy_from_slice(&(contents.len() as u32).to_le_bytes());
    let data_start = (1 + 2 * 32 + 32) * 512; // reserved + FATs + root
    image[data_start..data_start + contents.len()].copy_from_slice(contents);
    image
}

/// Builds an empty qcow2 v3 shell — header, refcount table and block,
/// L1 — naming `backing` (a name and an optional pinned format) when
/// given. No guest cluster is allocated: every read falls through.
fn qcow2_shell(virtual_size: u64, backing: Option<(&str, Option<&str>)>) -> Vec<u8> {
    let l2_entries = CLUSTER / 8;
    let l1_size = virtual_size.div_ceil(CLUSTER * l2_entries) as u32;
    assert!(l1_size as u64 <= CLUSTER / 8, "test image L1 fits one cluster");

    let mut image = vec![0u8; 4 * CLUSTER as usize];
    image[..4].copy_from_slice(b"QFI\xfb");
    image[4..8].copy_from_slice(&3u32.to_be_bytes()); // version
    image[20..24].copy_from_slice(&CLUSTER_BITS.to_be_bytes());
    image[24..32].copy_from_slice(&virtual_size.to_be_bytes());
    image[36..40].copy_from_slice(&l1_size.to_be_bytes());
    image[40..48].copy_from_slice(&(3 * CLUSTER).to_be_bytes()); // L1 offset
    image[48..56].copy_from_slice(&CLUSTER.to_be_bytes()); // refcount table
    image[56..60].copy_from_slice(&1u32.to_be_bytes()); // its clusters
    image[96..100].copy_from_slice(&4u32.to_be_bytes()); // refcount_order
    image[100..104].copy_from_slice(&112u32.to_be_bytes()); // header_length

    // Refcount table entry 0 -> block at cluster 2; counts for 0..=3.
    image[CLUSTER as usize..CLUSTER as usize + 8]
        .copy_from_slice(&(2 * CLUSTER).to_be_bytes());
    for cluster in 0..4usize {
        let at = 2 * CLUSTER as usize + cluster * 2;
        image[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
    }

    if let Some((name, format)) = backing {
        let mut at = 112usize;
        if let Some(format) = format {
            // The backing-format extension, data padded to 8 bytes.
            image[at..at + 4].copy_from_slice(&0xe279_2acau32.to_be_bytes());
            image[at + 4..at + 8].copy_from_slice(&(format.len() as u32).to_be_bytes());
            image[at + 8..at + 8 + format.len()].copy_from_slice(format.as_bytes());
            at += 8 + format.len().div_ceil(8) * 8;
        }
        // The end-of-extensions marker is the zeroes already there; the
        // name follows the extension area.
        let name_at = (at + 8).max(0x200);
        image[name_at..name_at + name.len()].copy_from_slice(name.as_bytes());
        image[8..16].copy_from_slice(&(name_at as u64).to_be_bytes());
        image[16..20].copy_from_slice(&(name.len() as u32).to_be_bytes());
    }
    image
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).expect("test image writes");
}

fn cleanup(dir: &Path) {
    std::fs::remove_dir_all(dir).ok();
}

fn qemu_img() -> PathBuf {
    if let Some(path) = std::env::var_os("REMANENCE_QEMU_IMG") {
        return path.into();
    }
    for path in [
        PathBuf::from("qemu-img"),
        PathBuf::from(r"C:\Program Files\qemu\qemu-img.exe"),
    ] {
        if Command::new(&path).arg("--version").output().is_ok() {
            return path;
        }
    }
    panic!(
        "qemu-img was not found; put it on PATH or set REMANENCE_QEMU_IMG \
         (the tested Windows location is C:\\Program Files\\qemu\\qemu-img.exe)"
    );
}

fn run_qemu_img(qemu_img: &Path, args: &[&str]) -> String {
    let output = Command::new(qemu_img)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("could not run {}: {error}", qemu_img.display()));
    assert!(
        output.status.success(),
        "{} {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        qemu_img.display(),
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}


/// The volume a partitionless image composes, named the only way a caller
/// can name one: by asking the library what it reported.
fn only_volume(disk: &mut Disk) -> remanence::VolumeId {
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(report.volumes.len(), 1, "a partitionless image, one volume");
    report.volumes[0].id
}

#[test]
fn reads_compose_through_a_raw_backing_file() {
    let dir = chain_dir("raw-base");
    let base = synthetic_fat16();
    write(&dir.join("base.img"), &base);
    let overlay = dir.join("overlay.qcow2");
    write(
        &overlay,
        &qcow2_shell(base.len() as u64, Some(("base.img", Some("raw")))),
    );

    let (mut disk_session, disk_at) = attach(&overlay, AccessIntent::Read).expect("the chain opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    assert_eq!(disk.format(), DiskFormat::Qcow2 { version: 3 });
    assert_eq!(disk.size(), base.len() as u64);

    // The FAT volume at the bottom of the chain reads as one disk.
    let report = disk.inspect().expect("inspection composes");
    assert_eq!(report.volumes.len(), 1);
    let volume = report.volumes[0].id;
    assert_eq!(
        report
            .filesystem_on(volume)
            .and_then(|fs| fs.label.as_ref())
            .and_then(|label| label.name.clone()),
        Some("REMANENCE".to_owned())
    );
    assert_eq!(
        disk.read_file(volume, "MARKER.TXT")
            .expect("the marker reads through the chain"),
        b"read through the chain"
    );
    drop(disk_session);

    // A write is seeded from the composed view, then allocated into the
    // top image. The backing bytes and the top's recorded relationship
    // remain byte-for-byte unchanged.
    let untouched_top = std::fs::read(&overlay).expect("top reads");
    let top_header = untouched_top[..CLUSTER as usize].to_vec();
    let (mut disk_session, disk_at) = attach(&overlay, AccessIntent::Write).expect("write chain opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    disk.write_file(volume, "MARKER.TXT", b"rolled back")
        .expect("write buffers");
    disk.rollback();
    drop(disk_session);
    assert_eq!(
        std::fs::read(&overlay).expect("rolled-back top reads"),
        untouched_top,
        "rollback never reaches the top image"
    );

    let (mut disk_session, disk_at) = attach(&overlay, AccessIntent::Write).expect("write chain opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    disk.write_file(volume, "MARKER.TXT", b"changed in the top")
        .expect("write buffers");
    disk.commit().expect("write commits");
    drop(disk_session);

    assert_eq!(std::fs::read(dir.join("base.img")).expect("base reads"), base);
    assert_eq!(
        &std::fs::read(&overlay).expect("top reads")[..CLUSTER as usize],
        top_header.as_slice()
    );
    let (mut reopened_session, reopened_at) = attach(&overlay, AccessIntent::Read).expect("written chain reopens");
    let reopened = reopened_session.medium(reopened_at).expect("the medium is attached");
    let volume = only_volume(reopened);
    assert_eq!(
        reopened.read_file(volume, "MARKER.TXT").expect("changed file reads"),
        b"changed in the top"
    );

    cleanup(&dir);
}

#[test]
#[ignore = "requires qemu-img; proves the delivered host reads the preserved chain"]
fn qemu_reports_the_same_backing_and_reads_committed_guest_bytes() {
    let dir = chain_dir("qemu-cow");
    let base = dir.join("base.img");
    let overlay = dir.join("overlay.qcow2");
    let flattened = dir.join("flattened.img");
    write(&base, &synthetic_fat16());

    let qemu = qemu_img();
    let base_arg = base.to_str().expect("utf-8 base path");
    let overlay_arg = overlay.to_str().expect("utf-8 overlay path");
    run_qemu_img(
        &qemu,
        &["create", "-f", "qcow2", "-F", "raw", "-b", base_arg, overlay_arg],
    );
    let before = run_qemu_img(&qemu, &["info", overlay_arg]);

    let (mut disk_session, disk_at) = attach(&overlay, AccessIntent::Write).expect("QEMU chain opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    disk.write_file(volume, "MARKER.TXT", b"remanence copy-on-write")
        .expect("write buffers");
    disk.commit().expect("write commits");
    drop(disk_session);

    let after = run_qemu_img(&qemu, &["info", overlay_arg]);
    assert!(
        before.lines().any(|line| line.contains(base_arg))
            && after.lines().any(|line| line.contains(base_arg)),
        "qemu-img reports the same backing before and after commit\nbefore:\n{before}\nafter:\n{after}"
    );

    let flattened_arg = flattened.to_str().expect("utf-8 flattened path");
    run_qemu_img(&qemu, &["convert", "-O", "raw", overlay_arg, flattened_arg]);
    let (mut qemu_session, qemu_at) =
        attach(&flattened, AccessIntent::Read).expect("QEMU-rendered disk opens");
    let qemu_view = qemu_session.medium(qemu_at).expect("the medium is attached");
    assert_eq!(
        qemu_view
            .read_file(volume, "MARKER.TXT")
            .expect("QEMU-rendered changed file reads"),
        b"remanence copy-on-write"
    );

    cleanup(&dir);
}

#[test]
fn a_two_level_chain_resolves_each_name_from_its_own_image() {
    let dir = chain_dir("two-level");
    std::fs::create_dir_all(dir.join("sub")).expect("subdir creates");
    let base = synthetic_fat16();
    write(&dir.join("base.img"), &base);
    write(
        &dir.join("mid.qcow2"),
        &qcow2_shell(base.len() as u64, Some(("base.img", Some("raw")))),
    );
    // The top sits in a subdirectory and reaches its parent by a
    // relative name, pinning the middle member's format.
    let top = dir.join("sub").join("top.qcow2");
    write(
        &top,
        &qcow2_shell(base.len() as u64, Some(("../mid.qcow2", Some("qcow2")))),
    );

    let (mut disk_session, disk_at) = attach(&top, AccessIntent::Read).expect("the chain opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let report = disk.inspect().expect("inspection composes");
    assert_eq!(report.volumes.len(), 1);
    let volume = report.volumes[0].id;
    assert_eq!(
        disk.read_file(volume, "MARKER.TXT")
            .expect("reads through two members"),
        b"read through the chain"
    );
    drop(disk_session);

    let base_before = std::fs::read(dir.join("base.img")).expect("base reads");
    let mid_before = std::fs::read(dir.join("mid.qcow2")).expect("middle reads");
    let (mut disk_session, disk_at) = attach(&top, AccessIntent::Write).expect("two-level write opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    disk.write_file(volume, "MARKER.TXT", b"changed above two levels")
        .expect("write buffers");
    disk.commit().expect("write commits");
    drop(disk_session);
    assert_eq!(
        std::fs::read(dir.join("base.img")).expect("base reads"),
        base_before
    );
    assert_eq!(
        std::fs::read(dir.join("mid.qcow2")).expect("middle reads"),
        mid_before
    );
    let (mut reopened_session, reopened_at) = attach(&top, AccessIntent::Read).expect("changed chain reopens");
    let reopened = reopened_session.medium(reopened_at).expect("the medium is attached");
    assert_eq!(
        reopened
            .read_file(volume, "MARKER.TXT")
            .expect("changed marker reads"),
        b"changed above two levels"
    );

    cleanup(&dir);
}

#[test]
fn an_unpinned_backing_format_is_probed_by_magic() {
    let dir = chain_dir("probed");
    let base = synthetic_fat16();
    write(&dir.join("base.img"), &base);
    write(
        &dir.join("mid.qcow2"),
        &qcow2_shell(base.len() as u64, Some(("base.img", None))),
    );
    let top = dir.join("top.qcow2");
    write(
        &top,
        &qcow2_shell(base.len() as u64, Some(("mid.qcow2", None))),
    );

    // No format extension anywhere: the qcow2 middle and the raw base
    // are each told apart by magic, exactly as at the top.
    let (mut disk_session, disk_at) = attach(&top, AccessIntent::Read).expect("the chain opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");
    let volume = only_volume(disk);
    assert_eq!(
        disk.read_file(volume, "MARKER.TXT")
            .expect("reads"),
        b"read through the chain"
    );
    drop(disk_session);
    cleanup(&dir);
}

#[test]
fn a_missing_backing_file_is_refused_by_name() {
    let dir = chain_dir("missing");
    let overlay = dir.join("overlay.qcow2");
    write(&overlay, &qcow2_shell(64 * CLUSTER, Some(("gone.img", None))));

    let error = attach(&overlay, AccessIntent::Read).expect_err("missing member refused");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    let message = error.to_string();
    assert!(
        message.contains("gone.img") && message.contains("does not exist"),
        "the refusal names the missing member: {message}"
    );
    cleanup(&dir);
}

#[test]
fn a_backing_cycle_is_refused_by_name() {
    let dir = chain_dir("cycle");
    let a = dir.join("a.qcow2");
    let b = dir.join("b.qcow2");
    write(&a, &qcow2_shell(64 * CLUSTER, Some(("b.qcow2", Some("qcow2")))));
    write(&b, &qcow2_shell(64 * CLUSTER, Some(("a.qcow2", Some("qcow2")))));

    let error = attach(&a, AccessIntent::Read).expect_err("a cycle is refused");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    assert!(
        error.to_string().contains("cycle"),
        "the refusal says why: {error}"
    );
    cleanup(&dir);
}

#[test]
fn a_chain_past_the_claimed_depth_is_refused_by_name() {
    let dir = chain_dir("depth");
    // Seventeen files: member 16 stands alone, every other backs onto
    // the next — one past the sixteen the release claims.
    write(&dir.join("member16.qcow2"), &qcow2_shell(64 * CLUSTER, None));
    for member in (0..16).rev() {
        let next = format!("member{}.qcow2", member + 1);
        write(
            &dir.join(format!("member{member}.qcow2")),
            &qcow2_shell(64 * CLUSTER, Some((&next, Some("qcow2")))),
        );
    }

    let error = attach(dir.join("member0.qcow2"), AccessIntent::Read)
        .expect_err("a chain past the claim is refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        error.to_string().contains("16 files"),
        "the refusal names the claimed bound: {error}"
    );

    // One member shorter sits exactly at the claim, and opens.
    let (session, _at) = attach(dir.join("member1.qcow2"), AccessIntent::Read)
        .expect("sixteen files are within the claim");
    drop(session);
    cleanup(&dir);
}

#[test]
fn an_unclaimed_backing_format_is_refused_by_name() {
    let dir = chain_dir("format");
    let base = synthetic_fat16();
    write(&dir.join("base.img"), &base);
    let overlay = dir.join("overlay.qcow2");
    write(
        &overlay,
        &qcow2_shell(base.len() as u64, Some(("base.img", Some("vmdk")))),
    );

    let error = attach(&overlay, AccessIntent::Read).expect_err("vmdk backing refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    let message = error.to_string();
    assert!(
        message.contains("vmdk") && message.contains("raw and qcow2"),
        "the refusal names the format and the claim: {message}"
    );
    cleanup(&dir);
}

#[test]
fn the_p8_gates_run_for_every_chain_member() {
    let dir = chain_dir("member-gates");
    // An encrypted base behind a clean overlay.
    let mut encrypted = qcow2_shell(64 * CLUSTER, None);
    encrypted[32..36].copy_from_slice(&1u32.to_be_bytes()); // crypt_method
    write(&dir.join("base.qcow2"), &encrypted);
    let overlay = dir.join("overlay.qcow2");
    write(
        &overlay,
        &qcow2_shell(64 * CLUSTER, Some(("base.qcow2", Some("qcow2")))),
    );

    let error = attach(&overlay, AccessIntent::Read).expect_err("encrypted member refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        error.to_string().contains("encrypted"),
        "the refusal names the member's failing gate: {error}"
    );
    cleanup(&dir);
}

#[test]
fn every_chain_member_is_claimed_immutable() {
    let dir = chain_dir("claims");
    let base = synthetic_fat16();
    let base_path = dir.join("base.img");
    write(&base_path, &base);
    let overlay = dir.join("overlay.qcow2");
    write(
        &overlay,
        &qcow2_shell(base.len() as u64, Some(("base.img", Some("raw")))),
    );

    let (mut disk_session, disk_at) = attach(&overlay, AccessIntent::Read).expect("the chain opens");
    let disk = disk_session.medium(disk_at).expect("the medium is attached");

    // The backing file is immutable while the chain holds it: another
    // writer is refused immediately, another reader stays admitted.
    assert_eq!(
        attach(&base_path, AccessIntent::Write)
            .expect_err("a writer is refused on a claimed member")
            .category(),
        ErrorCategory::Locked
    );
    let (mut reader_session, reader_at) = attach(&base_path, AccessIntent::Read).expect("readers stay admitted");
    let reader = reader_session.medium(reader_at).expect("the medium is attached");
    drop(reader_session);

    // The claim lasts exactly as long as the chain.
    drop(disk_session);
    let writer =
        attach(&base_path, AccessIntent::Write).expect("the claim releases with the chain");
    drop(writer);
    cleanup(&dir);
}
