// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Image builders shared by this module's tests.
//!
//! The commit tests need a raw, a qcow2, a VDI and a differencing VDI
//! chain each staged to the same known content, and the end-to-end
//! tests need a FAT16 volume inside two of those containers. Building
//! them is fixture work rather than test work, and it is here so the
//! tests that use them can live beside the code they exercise.

use std::path::PathBuf;
use std::process::Command;

use crate::image::qcow2::QCOW2_MAGIC;
use crate::io::device::{AccessIntent, Device};
use crate::model::disk::MediaState;
use crate::partition::PartitionPool;

/// The pool a partitionless test image bears, established the way a
/// load establishes one. These tests drive the state below the medium
/// tier, so they run the same act `Session::admit` runs.
pub(super) fn pool_of(disk: &mut MediaState) -> PartitionPool {
    let discovery = disk.check_scheme().expect("the scheme is checked");
    PartitionPool::over_space(crate::PartitionScheme::Mbr, &discovery, disk.size())
}

/// The extent the one partition of a partitionless test image
/// composes — the position the file verbs work within.
pub(super) fn only_extent(disk: &mut MediaState) -> u64 {
    let pool = pool_of(disk);
    let report = disk.inspect(&pool).expect("inspection reads");
    assert_eq!(report.volumes.len(), 1, "these images compose one volume");
    report.volumes[0].start_bytes
}

/// A minimal empty v3 qcow2 sized for the synthetic FAT16 volume
/// (mirrors the qcow2 unit-test builder).
pub(super) fn empty_qcow2_bytes(virtual_size: u64) -> Vec<u8> {
    const CLUSTER_BITS: u32 = 12;
    const CLUSTER: u64 = 1 << CLUSTER_BITS;

    let l2_entries = CLUSTER / 8;
    let l1_size = virtual_size.div_ceil(CLUSTER * l2_entries) as u32;
    let mut image = vec![0u8; 4 * CLUSTER as usize];
    image[..4].copy_from_slice(&QCOW2_MAGIC);
    image[4..8].copy_from_slice(&3u32.to_be_bytes());
    image[20..24].copy_from_slice(&CLUSTER_BITS.to_be_bytes());
    image[24..32].copy_from_slice(&virtual_size.to_be_bytes());
    image[36..40].copy_from_slice(&l1_size.to_be_bytes());
    image[40..48].copy_from_slice(&(3 * CLUSTER).to_be_bytes());
    image[48..56].copy_from_slice(&CLUSTER.to_be_bytes());
    image[56..60].copy_from_slice(&1u32.to_be_bytes());
    image[96..100].copy_from_slice(&4u32.to_be_bytes());
    image[100..104].copy_from_slice(&112u32.to_be_bytes());
    image[CLUSTER as usize..CLUSTER as usize + 8].copy_from_slice(&(2 * CLUSTER).to_be_bytes());
    for cluster in 0..4usize {
        let at = 2 * CLUSTER as usize + cluster * 2;
        image[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
    }
    image
}

/// Builds a qcow2 file at `path` whose virtual disk carries the
/// synthetic FAT16 volume, using the crate's own writer.
pub(super) fn build_fat16_qcow2(path: &std::path::Path) -> u64 {
    let virtual_size = 4_096_000u64; // the synthetic FAT16 volume size
    std::fs::write(path, empty_qcow2_bytes(virtual_size)).expect("qcow2 writes");

    // Format the virtual disk: write a FAT16 volume into guest space
    // through the crate's own qcow2 writer.
    let file = crate::io::device::MediumDevice::open(path, AccessIntent::Write).expect("opens");
    let mut qcow2 = crate::image::qcow2::Qcow2::open(file).expect("parses");
    let volume = fat16_volume_bytes();
    assert_eq!(volume.len() as u64, virtual_size);
    qcow2.write_at(0, &volume).expect("formats");
    qcow2.flush().expect("flushes");
    virtual_size
}

/// The block size the synthetic VDI images use — small enough that a
/// modest test volume spans several blocks, which is what makes the
/// allocated/free distinction visible.
pub(super) const VDI_BLOCK: u64 = 64 * 1024;

/// A dynamically allocated VDI whose virtual disk holds `content`.
/// Only the blocks the content actually fills are allocated; the rest
/// stay free, which is both the shape a real dynamic image has and
/// the one a later write must allocate into.
pub(super) fn dynamic_vdi_bytes(content: &[u8]) -> Vec<u8> {
    let disk_size = content.len() as u64;
    let block_count = disk_size.div_ceil(VDI_BLOCK) as u32;
    let map_at = 0x200usize;
    let data_at = (map_at + block_count as usize * 4).div_ceil(512) * 512;

    let mut image = vec![0u8; data_at];
    image[..37].copy_from_slice(b"<<< remanence synthetic VDI image >>>");
    image[0x40..0x44].copy_from_slice(&0xbeda_107fu32.to_le_bytes());
    image[0x44..0x48].copy_from_slice(&0x0001_0001u32.to_le_bytes()); // version 1.1
    image[0x48..0x4c].copy_from_slice(&0x190u32.to_le_bytes()); // header size
    image[0x4c..0x50].copy_from_slice(&1u32.to_le_bytes()); // dynamically allocated
    image[0x154..0x158].copy_from_slice(&(map_at as u32).to_le_bytes());
    image[0x158..0x15c].copy_from_slice(&(data_at as u32).to_le_bytes());
    image[0x170..0x178].copy_from_slice(&disk_size.to_le_bytes());
    image[0x178..0x17c].copy_from_slice(&(VDI_BLOCK as u32).to_le_bytes());
    image[0x180..0x184].copy_from_slice(&block_count.to_le_bytes());

    let mut allocated = 0u32;
    for block in 0..block_count as usize {
        let start = block * VDI_BLOCK as usize;
        let end = (start + VDI_BLOCK as usize).min(content.len());
        let slice = &content[start..end];
        let entry = if slice.iter().all(|&byte| byte == 0) {
            0xffff_ffffu32 // free: it reads as zeroes
        } else {
            let index = allocated;
            allocated += 1;
            let at = data_at + index as usize * VDI_BLOCK as usize;
            image.resize(at + VDI_BLOCK as usize, 0);
            image[at..at + slice.len()].copy_from_slice(slice);
            index
        };
        let at = map_at + block * 4;
        image[at..at + 4].copy_from_slice(&entry.to_le_bytes());
    }
    image[0x184..0x188].copy_from_slice(&allocated.to_le_bytes());
    image
}

/// Builds a VDI file at `path` whose virtual disk carries the
/// synthetic FAT16 volume, and returns that virtual size.
pub(super) fn build_fat16_vdi(path: &std::path::Path) -> u64 {
    let volume = fat16_volume_bytes();
    std::fs::write(path, dynamic_vdi_bytes(&volume)).expect("vdi writes");
    volume.len() as u64
}

pub(super) fn temp_image(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "remanence-durable-{tag}-{}.img",
        std::process::id()
    ))
}

/// A raw FAT16 image at `path` holding `OLD.BIN` = `old_content()`,
/// committed and closed — the wholly-old state the durability tests
/// expect an interrupted commit to come back to.
pub(super) fn build_committed_raw(path: &std::path::Path) {
    std::fs::write(path, fat16_volume_bytes()).expect("image writes");
    let mut disk = MediaState::open(path, AccessIntent::Write).expect("opens");
    let volume = only_extent(&mut disk);
    disk.write_file(volume, "OLD.BIN", &old_content())
        .expect("writes");
    disk.commit().expect("commits");
}

pub(super) fn old_content() -> Vec<u8> {
    (0..48 * 1024u32).map(|n| (n % 240) as u8).collect()
}

pub(super) fn new_content() -> Vec<u8> {
    (0..64 * 1024u32).map(|n| (n % 251) as u8).collect()
}

pub(super) const CRASH_IMAGE: &str = "REMANENCE_CRASH_TEST_IMAGE";

pub(super) fn run_crashing_commit(path: &std::path::Path, boundary: &str) {
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--ignored")
        .arg("--exact")
        .arg("model::disk::commit::tests::crash_commit_child")
        .arg("--nocapture")
        .env(CRASH_IMAGE, path)
        .env("REMANENCE_CRASH_TEST_BOUNDARY", boundary)
        .status()
        .expect("crash child starts");
    assert_eq!(
        status.code(),
        Some(86),
        "the child terminates at the {boundary} durability boundary"
    );
}

pub(super) fn build_committed_qcow2(path: &std::path::Path) {
    build_fat16_qcow2(path);
    let mut disk = MediaState::open(path, AccessIntent::Write).expect("opens");
    let volume = only_extent(&mut disk);
    disk.write_file(volume, "OLD.BIN", &old_content())
        .expect("writes old state");
    disk.commit().expect("commits old state");
}

pub(super) fn build_committed_vdi(path: &std::path::Path) {
    build_fat16_vdi(path);
    let mut disk = MediaState::open(path, AccessIntent::Write).expect("opens");
    let volume = only_extent(&mut disk);
    disk.write_file(volume, "OLD.BIN", &old_content())
        .expect("writes old state");
    disk.commit().expect("commits old state");
}

/// The identity a synthetic base VDI is stamped with, and the one the
/// differencing image over it names as its parent.
pub(super) const VDI_BASE_ID: [u8; 16] = [
    0x51, 0x42, 0x33, 0x24, 0x15, 0x06, 0x47, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f, 0x90,
];

/// A differencing image over [`VDI_BASE_ID`] presenting `disk_size`
/// bytes, with every block free: the whole disk is the parent's until
/// something is written into it.
pub(super) fn differencing_vdi_bytes(disk_size: u64) -> Vec<u8> {
    let mut image = dynamic_vdi_bytes(&vec![0u8; disk_size as usize]);
    image[0x4c..0x50].copy_from_slice(&4u32.to_le_bytes()); // differencing
    image[0x188..0x198].copy_from_slice(&[0xa5; 16]); // its own identity
    image[0x1a8..0x1b8].copy_from_slice(&VDI_BASE_ID); // its parent's
    image
}

/// A committed base VDI and an empty differencing image over it, in
/// their own directory. The base is named as a person names it, not
/// after its identity, so the open has to search the directory for
/// the file declaring the identity the child asked for.
pub(super) fn build_committed_vdi_chain(directory: &std::path::Path) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(directory).expect("chain directory");
    let base = directory.join("base.vdi");
    let top = directory.join("top.vdi");

    let volume_bytes = fat16_volume_bytes();
    let mut base_image = dynamic_vdi_bytes(&volume_bytes);
    base_image[0x188..0x198].copy_from_slice(&VDI_BASE_ID);
    std::fs::write(&base, base_image).expect("base writes");

    let mut base_disk = MediaState::open(&base, AccessIntent::Write).expect("base opens");
    let volume = only_extent(&mut base_disk);
    base_disk
        .write_file(volume, "OLD.BIN", &old_content())
        .expect("writes old state");
    base_disk.commit().expect("commits old state");
    drop(base_disk);

    std::fs::write(&top, differencing_vdi_bytes(volume_bytes.len() as u64)).expect("top writes");
    (top, base)
}

pub(super) fn build_committed_chain(directory: &std::path::Path) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(directory).expect("chain directory");
    let base = directory.join("base.qcow2");
    let top = directory.join("top.qcow2");
    let virtual_size = build_fat16_qcow2(&base);
    let mut base_disk = MediaState::open(&base, AccessIntent::Write).expect("base opens");
    let volume = only_extent(&mut base_disk);
    base_disk
        .write_file(volume, "OLD.BIN", &old_content())
        .expect("writes old state");
    base_disk.commit().expect("commits old state");
    drop(base_disk);

    let mut image = empty_qcow2_bytes(virtual_size);
    let name = b"base.qcow2";
    image[0x200..0x200 + name.len()].copy_from_slice(name);
    image[8..16].copy_from_slice(&0x200u64.to_be_bytes());
    image[16..20].copy_from_slice(&(name.len() as u32).to_be_bytes());
    std::fs::write(&top, image).expect("top writes");
    (top, base)
}

/// Runs a commit's staging and journal phases exactly as
/// [`MediaState::commit`] does, stopping at the durability boundary: the
/// journal is armed, the file untouched. Returns the staged host
/// writes so a test can apply any prefix of them before "crashing".
pub(super) fn stage_and_arm(disk: &mut MediaState) -> (Vec<(u64, Vec<u8>)>, u64) {
    let cache_bytes = disk.cache_bytes;
    disk.cache.join_offloads();
    disk.virtual_disk.host_mut().begin_capture(cache_bytes);
    disk.cache
        .write_through(disk.virtual_disk.device_mut())
        .expect("stages");
    let capture = disk.virtual_disk.host_mut().take_capture();
    let journal_path = disk
        .journal_path
        .clone()
        .expect("a test image is opened by a path this host names");
    crate::io::journal::record(&journal_path, disk.virtual_disk.host_mut(), &capture)
        .expect("journals");
    let mut blocks = Vec::new();
    capture
        .for_each_dirty(&mut |offset, data| {
            blocks.push((offset, data.to_vec()));
            Ok(())
        })
        .expect("collects");
    (blocks, capture.len())
}

/// The same synthetic FAT16 volume the unit tests build.
pub(super) fn fat16_volume_bytes() -> Vec<u8> {
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
    }
    let root = (1 + 2 * 32) * 512;
    image[root..root + 11].copy_from_slice(b"REMANENCE  ");
    image[root + 11] = 0x08;
    image
}
