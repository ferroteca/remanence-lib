// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Backing-chain integration tests (U6's read half) over synthetic
//! images the project owns outright: hand-built qcow2 overlays whose
//! chains bottom out in a hand-built FAT16 volume. Everything runs
//! through the public `Disk` surface; cluster-level composition
//! semantics are unit-tested inside the crate.

use std::path::{Path, PathBuf};

use remanence::{AccessIntent, Disk, DiskFormat, ErrorCategory};

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

    let mut disk = Disk::open(&overlay, AccessIntent::Read).expect("the chain opens");
    assert_eq!(disk.format(), DiskFormat::Qcow2 { version: 3 });
    assert_eq!(disk.size(), base.len() as u64);

    // The FAT volume at the bottom of the chain reads as one disk.
    let geometry = disk.geometry().expect("geometry composes");
    assert_eq!(geometry.volumes.len(), 1);
    assert_eq!(geometry.volumes[0].label.as_deref(), Some("REMANENCE"));
    assert_eq!(
        disk.read_file(&geometry.volumes[0].id, "MARKER.TXT")
            .expect("the marker reads through the chain"),
        b"read through the chain"
    );
    drop(disk);

    // Writing through a chain is refused at the open, by name.
    let error = Disk::open(&overlay, AccessIntent::Write)
        .expect_err("a chained image refuses write intent");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(error.to_string().contains("backing chain"));

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

    let mut disk = Disk::open(&top, AccessIntent::Read).expect("the chain opens");
    let geometry = disk.geometry().expect("geometry composes");
    assert_eq!(geometry.volumes.len(), 1);
    assert_eq!(
        disk.read_file(&geometry.volumes[0].id, "MARKER.TXT")
            .expect("reads through two members"),
        b"read through the chain"
    );
    drop(disk);
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
    let mut disk = Disk::open(&top, AccessIntent::Read).expect("the chain opens");
    let geometry = disk.geometry().expect("geometry composes");
    assert_eq!(
        disk.read_file(&geometry.volumes[0].id, "MARKER.TXT")
            .expect("reads"),
        b"read through the chain"
    );
    drop(disk);
    cleanup(&dir);
}

#[test]
fn a_missing_backing_file_is_refused_by_name() {
    let dir = chain_dir("missing");
    let overlay = dir.join("overlay.qcow2");
    write(&overlay, &qcow2_shell(64 * CLUSTER, Some(("gone.img", None))));

    let error = Disk::open(&overlay, AccessIntent::Read).expect_err("missing member refused");
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

    let error = Disk::open(&a, AccessIntent::Read).expect_err("a cycle is refused");
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

    let error = Disk::open(dir.join("member0.qcow2"), AccessIntent::Read)
        .expect_err("a chain past the claim is refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        error.to_string().contains("16 files"),
        "the refusal names the claimed bound: {error}"
    );

    // One member shorter sits exactly at the claim, and opens.
    let disk = Disk::open(dir.join("member1.qcow2"), AccessIntent::Read)
        .expect("sixteen files are within the claim");
    drop(disk);
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

    let error = Disk::open(&overlay, AccessIntent::Read).expect_err("vmdk backing refused");
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

    let error = Disk::open(&overlay, AccessIntent::Read).expect_err("encrypted member refused");
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

    let disk = Disk::open(&overlay, AccessIntent::Read).expect("the chain opens");

    // The backing file is immutable while the chain holds it: another
    // writer is refused immediately, another reader stays admitted.
    assert_eq!(
        Disk::open(&base_path, AccessIntent::Write)
            .expect_err("a writer is refused on a claimed member")
            .category(),
        ErrorCategory::Locked
    );
    let reader = Disk::open(&base_path, AccessIntent::Read).expect("readers stay admitted");
    drop(reader);

    // The claim lasts exactly as long as the chain.
    drop(disk);
    let writer =
        Disk::open(&base_path, AccessIntent::Write).expect("the claim releases with the chain");
    drop(writer);
    cleanup(&dir);
}
