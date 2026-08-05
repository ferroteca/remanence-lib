// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Unit tests over the HDOS fixture image, raw and inside a ZIP.

use std::path::PathBuf;

use remanence::{
    AccessIntent, ArchiveLayout, AttachmentId, LayerKind, LayerLayout, DeviceFamily,
    Identification, ImageLayout, PhysicalMediaLayout, SectorLayout, Session,
};

/// Attaches `path` to a fresh session and returns both, because a medium
/// is reachable only through the device holding it (P32). Tests keep the
/// session alive for as long as they use the medium.
fn attach(
    path: impl AsRef<std::path::Path>,
    intent: AccessIntent,
) -> remanence::Result<(Session, AttachmentId)> {
    let mut session = Session::new();
    let device = session.add_device(DeviceFamily::HEATHKIT_H17)?;
    let attachment = device.attachment();
    device.load_media(path, intent)?;
    Ok((session, attachment))
}

mod common;

const IMAGE_NAME: &str = "HDOS_1-0_Issue_#50-00-00_890-1.h8d";
const ZIP_NAME: &str = "HDOS_1-0_Issue_#50-00-00_890-1.zip";

fn fixture_path(name: &str) -> PathBuf {
    common::ensure_fixture(name)
}

/// The disk holds the P7 deny-write claim on its source for its whole
/// lifetime, so tests that open the same fixture concurrently would
/// collide by design — each takes a private copy instead.
fn private_copy(name: &str, tag: &str) -> PathBuf {
    let target = std::env::temp_dir().join(format!("{tag}-{}-{name}", std::process::id()));
    std::fs::copy(fixture_path(name), &target).expect("fixture copies");
    target
}

fn assert_hdos_identification(identification: &Identification) {
    assert!(!identification.modified);
    let count = identification.layers.len();

    let image = &identification.layers[count - 3];
    assert_eq!(image.kind, LayerKind::Image);
    assert_eq!(image.id, "h8d");
    assert_eq!(image.name, "Heathkit H8 H17 disk image");
    assert_eq!(image.size.current_bytes, Some(102_400));
    assert_eq!(image.size.expected_bytes, Some(102_400));
    assert!(matches!(image.layout, LayerLayout::Image(ImageLayout { .. })));

    let media = &identification.layers[count - 2];
    assert_eq!(media.kind, LayerKind::PhysicalMedia);
    // The medium is the article, named from the media-type catalog
    // (P14): the ten-sector hard-sectored 5.25-inch disk an H17 records
    // on. The ten records to a track below are the recording, and they
    // follow the medium's ten sector holes without being them.
    assert_eq!(media.id, "flexible-5.25-hard-10");
    let LayerLayout::PhysicalMedia(PhysicalMediaLayout::Disk(disk)) = &media.layout
    else {
        panic!("expected disk layout, found {:?}", media.layout);
    };
    assert_eq!(disk.cylinders, Some(40));
    assert_eq!(disk.sides, Some(1));
    assert_eq!(disk.sectors, SectorLayout::Fixed { sectors_per_track: 10 });
    assert_eq!(disk.media_type, "flexible-5.25-hard-10");

    let filesystem = identification.layers.last().expect("filesystem layer");
    assert_eq!(filesystem.kind, LayerKind::Filesystem);
    assert_eq!(filesystem.id, "hdos");
    assert_eq!(filesystem.name, "Heath Disk Operating System");
}

fn archive_layout(identification: &Identification) -> &ArchiveLayout {
    let archive = &identification.layers[0];
    assert_eq!(archive.kind, LayerKind::Archive);
    assert_eq!(archive.id, "zip");
    let LayerLayout::Archive(layout) = &archive.layout else {
        panic!("expected archive layout, found {:?}", archive.layout);
    };
    layout
}

#[test]
fn identifies_hdos_fixture_image() {
    let image_path = fixture_path(IMAGE_NAME);

    let (mut disk_session, disk_at) = attach(&image_path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");
    let identification = disk.identify().expect("a medium is attached");

    assert_eq!(identification.layers.len(), 3);
    assert_hdos_identification(&identification);
}

#[test]
fn identifies_single_image_inside_zip_fixture() {
    let zip_path = private_copy(ZIP_NAME, "single");

    let (mut disk_session, disk_at) = attach(&zip_path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");
    let identification = disk.identify().expect("a medium is attached");

    let archive = &identification.layers[0];
    assert_eq!(archive.name, "ZIP archive");
    let layout = archive_layout(&identification);
    assert_eq!(layout.entry_name, IMAGE_NAME);
    assert_eq!(layout.uncompressed_size, Some(102_400));
    assert_eq!(identification.layers.len(), 4);
    assert_eq!(disk.path().expect("a medium is attached"), zip_path.display().to_string());
    assert_eq!(disk.image_path().expect("a medium is attached"), PathBuf::from(IMAGE_NAME));
    assert_hdos_identification(&identification);

    drop(disk_session);
    std::fs::remove_file(&zip_path).ok();
}

#[test]
fn identifies_explicit_image_inside_zip_fixture() {
    let zip_path = private_copy(ZIP_NAME, "explicit");
    let image_path = zip_path.join(IMAGE_NAME);

    let (mut disk_session, disk_at) = attach(&image_path, AccessIntent::Read).expect("disk opens");
    let disk = disk_session.require_device(disk_at).expect("the medium is attached");
    let identification = disk.identify().expect("a medium is attached");

    let layout = archive_layout(&identification);
    assert_eq!(layout.entry_name, IMAGE_NAME);
    assert_eq!(identification.layers.len(), 4);
    assert_hdos_identification(&identification);

    drop(disk_session);
    std::fs::remove_file(&zip_path).ok();
}
