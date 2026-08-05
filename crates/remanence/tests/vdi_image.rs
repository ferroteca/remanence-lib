// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The VDI container through the public surface (U1, U3, U4, U6):
//! synthetic images the project owns outright — a dynamically allocated
//! one whose unwritten blocks are genuinely absent, a fixed one whose
//! blocks are all present, and differencing chains over both — each
//! carrying the same hand-built FAT16 volume. Block-map semantics and
//! copy-on-write are unit-tested inside the crate; everything here runs
//! through `Session`, `StorageDevice` and the reports they issue, including the
//! parent resolution that only exists once an image has a path.

use std::path::PathBuf;

use remanence::{
    AccessIntent, AccessMode, AttachmentId, ContainerKind, DeviceFamily, DiskFormat, ErrorCategory,
    Session,
    VolumeId,
};

/// Attaches `path` to a fresh session and returns both, because a medium
/// is reachable only through the device holding it (P32).
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

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("remanence-vdi-{tag}-{}-{nonce}.vdi", std::process::id()))
}

/// The one volume a synthetic image composes, named the only way a caller
/// can name one: from the report the library issued.
fn only_volume(disk: &mut remanence::StorageDevice) -> VolumeId {
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(report.volumes.len(), 1, "these images compose one volume");
    report.volumes[0].id
}

/// A minimal FAT16 volume: 512-byte sectors, 1 sector/cluster, 2 FATs of
/// 32 sectors, 8000 total sectors, labeled REMANENCE.
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
    }
    let root = (1 + 2 * 32) * 512;
    image[root..root + 11].copy_from_slice(b"REMANENCE  ");
    image[root + 11] = 0x08;
    image
}

const BLOCK: u64 = 64 * 1024;
const BLOCK_FREE: u32 = 0xffff_ffff;

/// The version 1.1 header, block map and data area of an image over
/// `disk_size`, with every map entry free and no data block present.
fn vdi_shell(disk_size: u64, image_type: u32) -> (Vec<u8>, usize, usize, u32) {
    let block_count = disk_size.div_ceil(BLOCK) as u32;
    let map_at = 0x200usize;
    let data_at = (map_at + block_count as usize * 4).div_ceil(512) * 512;

    let mut image = vec![0u8; data_at];
    image[..37].copy_from_slice(b"<<< remanence synthetic VDI image >>>");
    image[0x40..0x44].copy_from_slice(&0xbeda_107fu32.to_le_bytes());
    image[0x44..0x48].copy_from_slice(&0x0001_0001u32.to_le_bytes()); // version 1.1
    image[0x48..0x4c].copy_from_slice(&0x190u32.to_le_bytes()); // header size
    image[0x4c..0x50].copy_from_slice(&image_type.to_le_bytes());
    image[0x154..0x158].copy_from_slice(&(map_at as u32).to_le_bytes());
    image[0x158..0x15c].copy_from_slice(&(data_at as u32).to_le_bytes());
    image[0x170..0x178].copy_from_slice(&disk_size.to_le_bytes());
    image[0x178..0x17c].copy_from_slice(&(BLOCK as u32).to_le_bytes());
    image[0x180..0x184].copy_from_slice(&block_count.to_le_bytes());
    for block in 0..block_count as usize {
        let at = map_at + block * 4;
        image[at..at + 4].copy_from_slice(&BLOCK_FREE.to_le_bytes());
    }
    (image, map_at, data_at, block_count)
}

/// A dynamically allocated image carrying `content`: only the blocks the
/// content actually fills exist, and the rest read as zeroes because the
/// map says they were never allocated.
fn dynamic_vdi(content: &[u8]) -> Vec<u8> {
    let (mut image, map_at, data_at, block_count) = vdi_shell(content.len() as u64, 1);
    let mut allocated = 0u32;
    for block in 0..block_count as usize {
        let start = block * BLOCK as usize;
        let end = (start + BLOCK as usize).min(content.len());
        let slice = &content[start..end];
        if slice.iter().all(|&byte| byte == 0) {
            continue;
        }
        let index = allocated;
        allocated += 1;
        let at = data_at + index as usize * BLOCK as usize;
        image.resize(at + BLOCK as usize, 0);
        image[at..at + slice.len()].copy_from_slice(slice);
        let entry = map_at + block * 4;
        image[entry..entry + 4].copy_from_slice(&index.to_le_bytes());
    }
    image[0x184..0x188].copy_from_slice(&allocated.to_le_bytes());
    image
}

/// A fixed image carrying `content`: every block is present from
/// creation, and the map addresses each in turn.
fn fixed_vdi(content: &[u8]) -> Vec<u8> {
    let (mut image, map_at, data_at, block_count) = vdi_shell(content.len() as u64, 2);
    image.resize(data_at + block_count as usize * BLOCK as usize, 0);
    for block in 0..block_count as usize {
        let entry = map_at + block * 4;
        image[entry..entry + 4].copy_from_slice(&(block as u32).to_le_bytes());
    }
    image[data_at..data_at + content.len()].copy_from_slice(content);
    image[0x184..0x188].copy_from_slice(&block_count.to_le_bytes());
    image
}

fn write_image(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_path(tag);
    std::fs::write(&path, bytes).expect("image writes");
    path
}

/// A directory of its own. A differencing image's parent is found by
/// searching where the image sits, because the format records the
/// parent's identity and no path, so a chain test owns its directory.
fn temp_directory(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "remanence-vdi-chain-{tag}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("chain directory");
    directory
}

/// A recognizable 16-byte identity, distinct per `tag`.
fn identity(tag: u8) -> [u8; 16] {
    let mut bytes = [tag; 16];
    bytes[0] = 0x11;
    bytes[15] = tag;
    bytes
}

/// The identity spelled the way the format's own tooling spells it —
/// first three groups read little-endian, as a Microsoft GUID is. Written
/// out here rather than borrowed from the library, so the name a chain is
/// searched by is checked against an independent reading.
fn identity_text(bytes: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
         {:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[3],
        bytes[2],
        bytes[1],
        bytes[0],
        bytes[5],
        bytes[4],
        bytes[7],
        bytes[6],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

fn stamp(image: &mut [u8], create: &[u8; 16], parent: &[u8; 16]) {
    image[0x188..0x198].copy_from_slice(create);
    image[0x1a8..0x1b8].copy_from_slice(parent);
}

/// A dynamically allocated image carrying `content` and declaring
/// `create` as its own identity — the base a differencing image names.
fn base_vdi(content: &[u8], create: &[u8; 16]) -> Vec<u8> {
    let mut image = dynamic_vdi(content);
    stamp(&mut image, create, &[0; 16]);
    image
}

/// A differencing image over `parent`, presenting `disk_size` bytes with
/// every block free: the whole disk is the parent's until something is
/// written into it.
fn differencing_vdi(disk_size: u64, create: &[u8; 16], parent: &[u8; 16]) -> Vec<u8> {
    let mut image = dynamic_vdi(&vec![0u8; disk_size as usize]);
    image[0x4c..0x50].copy_from_slice(&4u32.to_le_bytes());
    stamp(&mut image, create, parent);
    image
}

/// Writes the base of a chain into `directory` under an ordinary name —
/// not one derived from its identity — commits `BASE.BIN` into its
/// volume, and answers with the path and the virtual size a differencing
/// image over it must present.
fn committed_base(directory: &std::path::Path, create: &[u8; 16], content: &[u8]) -> (PathBuf, u64) {
    let volume_bytes = synthetic_fat16();
    let path = directory.join("base.vdi");
    std::fs::write(&path, base_vdi(&volume_bytes, create)).expect("base writes");

    let (mut session, at) = attach(&path, AccessIntent::Write).expect("base opens");
    let disk = session.require_device(at).expect("the medium is attached");
    let volume = only_volume(disk);
    disk.write_file(volume, "BASE.BIN", content).expect("write");
    disk.commit().expect("commit");
    drop(session);
    (path, volume_bytes.len() as u64)
}

#[test]
fn a_vdi_identifies_layer_by_layer_with_its_evidence() {
    let path = write_image("identify", &dynamic_vdi(&synthetic_fat16()));
    let (mut session, at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = session.require_device(at).expect("the medium is attached");

    let identification = disk.identify().expect("a medium is attached");
    let image = &identification.containers[0];
    assert_eq!(image.kind, ContainerKind::Image);
    assert_eq!(image.id, "vdi");
    assert_eq!(image.name, "VirtualBox disk image");
    assert_eq!(image.confidence, 100);
    assert!(image.known);

    let media = &identification.containers[1];
    assert_eq!(media.kind, ContainerKind::PhysicalMedia);
    // A virtual disk's medium is logical-block media (P14): the
    // addressable unit is the whole of what a controller can be
    // compatible with, and there is no cylinder, head or mechanism fact
    // to state beside it.
    assert_eq!(media.id, "logical-block-512");

    let filesystem = identification
        .containers
        .last()
        .expect("filesystem container");
    assert_eq!(filesystem.kind, ContainerKind::Filesystem);
    assert_eq!(filesystem.id, "fat16");

    // No verdict without the observations behind it (P4).
    let evidence = identification.evidence.join("\n");
    assert!(evidence.contains("VDI signature"), "{evidence}");
    assert!(evidence.contains("VDI version 1.1"), "{evidence}");
    assert!(
        evidence.contains("dynamically allocated"),
        "the declared image type is evidence too: {evidence}"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_dynamic_vdi_reads_writes_and_commits_as_any_other_image_does() {
    let volume_bytes = synthetic_fat16();
    let virtual_size = volume_bytes.len() as u64;
    let path = write_image("dynamic", &dynamic_vdi(&volume_bytes));
    let before = std::fs::metadata(&path).expect("metadata").len();
    assert!(
        before < virtual_size,
        "the dynamic image is smaller than the disk it presents"
    );

    let (mut session, at) = attach(&path, AccessIntent::Write).expect("image opens");
    let disk = session.require_device(at).expect("the medium is attached");
    assert_eq!(disk.format().expect("a medium is attached"), DiskFormat::Vdi { major: 1, minor: 1 });
    assert_eq!(disk.size().expect("a medium is attached"), virtual_size);
    assert_eq!(disk.mode().expect("a medium is attached"), AccessMode::ReadWrite);

    let volume = only_volume(disk);
    let contents: Vec<u8> = (0..40_000u32).map(|n| (n % 251) as u8).collect();
    disk.make_directory(volume, "GUEST").expect("mkdir");
    disk.write_file(volume, "GUEST/PAYLOAD.BIN", &contents)
        .expect("write");
    assert!(disk.is_modified().expect("a medium is attached"));
    assert_eq!(
        std::fs::metadata(&path).expect("metadata").len(),
        before,
        "nothing reaches the file before the commit (P2)"
    );

    disk.commit().expect("commit");
    assert!(
        std::fs::metadata(&path).expect("metadata").len() > before,
        "the commit allocated the blocks the write needed"
    );
    drop(session);

    let (mut reopened_session, reopened_at) =
        attach(&path, AccessIntent::Read).expect("image reopens");
    let reopened = reopened_session.require_device(reopened_at).expect("attached");
    assert_eq!(
        reopened
            .read_file(volume, "GUEST/PAYLOAD.BIN")
            .expect("read"),
        contents
    );
    drop(reopened_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_fixed_vdi_presents_the_same_disk_a_dynamic_one_does() {
    let volume_bytes = synthetic_fat16();
    let path = write_image("fixed", &fixed_vdi(&volume_bytes));
    let (mut session, at) = attach(&path, AccessIntent::Write).expect("image opens");
    let disk = session.require_device(at).expect("the medium is attached");
    assert_eq!(disk.format().expect("a medium is attached"), DiskFormat::Vdi { major: 1, minor: 1 });
    assert_eq!(disk.size().expect("a medium is attached"), volume_bytes.len() as u64);

    let volume = only_volume(disk);
    let report = disk.inspect().expect("inspection reads");
    assert_eq!(
        report
            .filesystem_on(volume)
            .and_then(|fs| fs.label.as_ref())
            .and_then(|label| label.name.clone()),
        Some("REMANENCE".to_owned())
    );

    disk.write_file(volume, "FIXED.BIN", b"written in place")
        .expect("write");
    disk.commit().expect("commit");
    drop(session);

    let (mut reopened_session, reopened_at) =
        attach(&path, AccessIntent::Read).expect("image reopens");
    let reopened = reopened_session.require_device(reopened_at).expect("attached");
    assert_eq!(
        reopened.read_file(volume, "FIXED.BIN").expect("read"),
        b"written in place"
    );
    drop(reopened_session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn reading_a_dynamic_vdi_allocates_nothing() {
    let path = write_image("read-only", &dynamic_vdi(&synthetic_fat16()));
    let before = std::fs::read(&path).expect("image reads");

    let (mut session, at) = attach(&path, AccessIntent::Read).expect("image opens");
    let disk = session.require_device(at).expect("the medium is attached");
    let volume = only_volume(disk);
    assert!(disk.entries(volume, "").expect("listing reads").is_empty());

    // A read that crosses the unallocated blocks: it answers with zeroes
    // and touches nothing (P2).
    let mut probe = vec![0xffu8; 4096];
    disk.read_at(before.len() as u64 - 4096, &mut probe)
        .expect("bounded read");
    assert!(
        disk.write_file(volume, "NO.BIN", b"denied").is_err(),
        "a read session denies write actions"
    );
    drop(session);

    assert_eq!(
        std::fs::read(&path).expect("image reads"),
        before,
        "not one byte of the image moved"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn p8_refuses_a_vdi_version_past_the_claim_by_name() {
    let mut bytes = dynamic_vdi(&synthetic_fat16());
    bytes[0x44..0x48].copy_from_slice(&0x0002_0000u32.to_le_bytes()); // version 2.0
    let path = write_image("future-version", &bytes);

    let error = attach(&path, AccessIntent::Read).expect_err("a future version is refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    let message = error.to_string();
    assert!(
        message.contains("2.0") && message.contains("ceiling"),
        "the refusal names what it found and what it supports: {message}"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn an_image_type_outside_the_claim_is_refused_by_name() {
    let mut bytes = dynamic_vdi(&synthetic_fat16());
    bytes[0x4c..0x50].copy_from_slice(&3u32.to_le_bytes());
    let path = write_image("undo", &bytes);

    let error = attach(&path, AccessIntent::Read).expect_err("the type is refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    let message = error.to_string();
    assert!(
        message.contains("undo"),
        "the refusal names the type rather than attempting it: {message}"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_differencing_chain_composes_as_one_disk() {
    let directory = temp_directory("composes");
    let base_id = identity(0xb0);
    let base_content: Vec<u8> = (0..30_000u32).map(|n| (n % 251) as u8).collect();
    let (base, virtual_size) = committed_base(&directory, &base_id, &base_content);
    let base_before = std::fs::read(&base).expect("base reads");

    // The top image is named after nothing at all, and the base is named
    // the way a person names it: what joins them is the identity the
    // child declares, which is the whole of what the format records.
    let top = directory.join("top.vdi");
    std::fs::write(
        &top,
        differencing_vdi(virtual_size, &identity(0x70), &base_id),
    )
    .expect("top writes");

    let (mut session, at) = attach(&top, AccessIntent::Write).expect("the chain opens");
    let disk = session.require_device(at).expect("the medium is attached");
    assert_eq!(disk.format().expect("a medium is attached"), DiskFormat::Vdi { major: 1, minor: 1 });
    assert_eq!(disk.size().expect("a medium is attached"), virtual_size);

    // Identification is deliberately untouched: the top image identifies
    // as the VDI container it is (U5), with its type and its parent among
    // the evidence (P4).
    let identification = disk.identify().expect("a medium is attached");
    assert_eq!(identification.containers[0].id, "vdi");
    let evidence = identification.evidence.join("\n");
    assert!(evidence.contains("differencing"), "{evidence}");
    assert!(
        evidence.contains(&identity_text(&base_id)),
        "the evidence names the parent the image declares: {evidence}"
    );

    // Every block is free in the top image, so the volume, its listing
    // and its files are all the parent's, read through (U6).
    let volume = only_volume(disk);
    assert_eq!(
        disk.read_file(volume, "BASE.BIN").expect("read through"),
        base_content
    );

    let added: Vec<u8> = (0..70_000u32).map(|n| (n % 241) as u8).collect();
    disk.write_file(volume, "TOP.BIN", &added).expect("write");
    disk.commit().expect("commit");
    drop(session);

    assert_eq!(
        std::fs::read(&base).expect("base reads"),
        base_before,
        "writes allocate into the top image only; the parent is never modified"
    );

    let (mut reopened_session, reopened_at) =
        attach(&top, AccessIntent::Read).expect("the chain reopens");
    let reopened = reopened_session.require_device(reopened_at).expect("attached");
    let volume = only_volume(reopened);
    assert_eq!(
        reopened.read_file(volume, "TOP.BIN").expect("read"),
        added,
        "what the top image took is read back from the top image"
    );
    assert_eq!(
        reopened.read_file(volume, "BASE.BIN").expect("read"),
        base_content,
        "and what it never took still reads through to the parent"
    );
    drop(reopened_session);
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_parent_named_for_the_identity_is_never_a_substitute_for_it() {
    let directory = temp_directory("substitute");
    let wanted = identity(0xc1);

    // A file named for the identity the child declares, carrying another
    // identity entirely. It is the nominated parent, so it is refused
    // rather than passed over in favour of a search.
    let volume_bytes = synthetic_fat16();
    let impostor = directory.join(format!("{{{}}}.vdi", identity_text(&wanted)));
    std::fs::write(&impostor, base_vdi(&volume_bytes, &identity(0xc2)))
        .expect("impostor writes");

    let top = directory.join("top.vdi");
    std::fs::write(
        &top,
        differencing_vdi(volume_bytes.len() as u64, &identity(0x71), &wanted),
    )
    .expect("top writes");

    let error = attach(&top, AccessIntent::Read).expect_err("a substitute is refused");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    let message = error.to_string();
    assert!(
        message.contains(&identity_text(&wanted))
            && message.contains(&identity_text(&identity(0xc2))),
        "the refusal names what was asked for and what was found: {message}"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_missing_parent_is_a_named_refusal_rather_than_the_top_image_alone() {
    let directory = temp_directory("missing");
    let wanted = identity(0xc3);
    let top = directory.join("top.vdi");
    std::fs::write(
        &top,
        differencing_vdi(synthetic_fat16().len() as u64, &identity(0x72), &wanted),
    )
    .expect("top writes");

    let error = attach(&top, AccessIntent::Read).expect_err("a missing parent is refused");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    let message = error.to_string();
    assert!(
        message.contains(&identity_text(&wanted)),
        "the refusal names the identity it looked for: {message}"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_cycle_in_the_chain_is_refused_at_the_open() {
    let directory = temp_directory("cycle");
    let first = identity(0xe1);
    let second = identity(0xe2);
    let size = synthetic_fat16().len() as u64;

    // Two differencing images, each naming the other. Both are written
    // under the name the format's tooling gives a differencing image, so
    // resolution finds them without searching.
    for (own, parent) in [(first, second), (second, first)] {
        let path = directory.join(format!("{{{}}}.vdi", identity_text(&own)));
        std::fs::write(&path, differencing_vdi(size, &own, &parent)).expect("member writes");
    }

    let top = directory.join(format!("{{{}}}.vdi", identity_text(&first)));
    let error = attach(&top, AccessIntent::Read).expect_err("a cycle is refused");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    assert!(
        error.to_string().contains("cycles"),
        "the refusal says what it found: {error}"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_chain_past_the_claimed_bound_is_refused_rather_than_walked_partway() {
    let directory = temp_directory("deep");
    let size = synthetic_fat16().len() as u64;

    // Seventeen differencing images, each naming the next: one past the
    // sixteen files this release claims, and never a base to end on.
    for step in 0..17u8 {
        let own = identity(0x40 + step);
        let parent = identity(0x40 + step + 1);
        let path = directory.join(format!("{{{}}}.vdi", identity_text(&own)));
        std::fs::write(&path, differencing_vdi(size, &own, &parent)).expect("member writes");
    }

    let top = directory.join(format!("{{{}}}.vdi", identity_text(&identity(0x40))));
    let error = attach(&top, AccessIntent::Read).expect_err("a deep chain is refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    let message = error.to_string();
    assert!(
        message.contains("16") && message.contains("partway"),
        "the refusal names the bound rather than opening what fits: {message}"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_parent_outside_the_claim_refuses_in_its_own_name() {
    let directory = temp_directory("bad-parent");
    let parent_id = identity(0xc4);
    let volume_bytes = synthetic_fat16();

    // The parent is an undo image: a type the release refuses by name,
    // and it refuses there rather than being read as something else.
    let mut parent = base_vdi(&volume_bytes, &parent_id);
    parent[0x4c..0x50].copy_from_slice(&3u32.to_le_bytes());
    std::fs::write(directory.join("base.vdi"), parent).expect("parent writes");

    let top = directory.join("top.vdi");
    std::fs::write(
        &top,
        differencing_vdi(volume_bytes.len() as u64, &identity(0x73), &parent_id),
    )
    .expect("top writes");

    let error = attach(&top, AccessIntent::Read).expect_err("the parent is refused");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        error.to_string().contains("undo"),
        "the parent's own refusal is what surfaces: {error}"
    );
    std::fs::remove_dir_all(&directory).ok();
}
