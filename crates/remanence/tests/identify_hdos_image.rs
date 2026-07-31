// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Unit tests over the HDOS fixture image, raw and inside a ZIP.

use std::path::PathBuf;

use remanence::{
    ArchiveLayout, ContainerKind, ContainerLayout, Identification, ImageLayout,
    PhysicalMediaLayout, SectorLayout, Session,
};

mod common;

const IMAGE_NAME: &str = "HDOS_1-0_Issue_#50-00-00_890-1.h8d";
const ZIP_NAME: &str = "HDOS_1-0_Issue_#50-00-00_890-1.zip";

fn fixture_path(name: &str) -> PathBuf {
    common::ensure_fixture(name)
}

/// The session holds the P7 deny-write claim on its source for its whole
/// lifetime, so tests that open the same fixture concurrently would
/// collide by design — each takes a private copy instead.
fn private_copy(name: &str, tag: &str) -> PathBuf {
    let target = std::env::temp_dir().join(format!("{tag}-{}-{name}", std::process::id()));
    std::fs::copy(fixture_path(name), &target).expect("fixture copies");
    target
}

fn assert_hdos_identification(identification: &Identification) {
    assert!(!identification.modified);
    let count = identification.containers.len();

    let image = &identification.containers[count - 3];
    assert_eq!(image.kind, ContainerKind::Image);
    assert_eq!(image.id, "h8d");
    assert_eq!(image.name, "Heathkit H8 H17 disk image");
    assert_eq!(image.size.current_bytes, Some(102_400));
    assert_eq!(image.size.expected_bytes, Some(102_400));
    assert!(matches!(image.layout, ContainerLayout::Image(ImageLayout { .. })));

    let media = &identification.containers[count - 2];
    assert_eq!(media.kind, ContainerKind::PhysicalMedia);
    assert_eq!(media.id, "floppy");
    let ContainerLayout::PhysicalMedia(PhysicalMediaLayout::Disk(disk)) = &media.layout
    else {
        panic!("expected disk layout, found {:?}", media.layout);
    };
    assert_eq!(disk.cylinders, Some(40));
    assert_eq!(disk.sides, Some(1));
    assert_eq!(disk.sectors, SectorLayout::Fixed { sectors_per_track: 10 });

    let filesystem = identification.containers.last().expect("filesystem container");
    assert_eq!(filesystem.kind, ContainerKind::Filesystem);
    assert_eq!(filesystem.id, "hdos");
    assert_eq!(filesystem.name, "Heath Disk Operating System");
}

fn archive_layout(identification: &Identification) -> &ArchiveLayout {
    let archive = &identification.containers[0];
    assert_eq!(archive.kind, ContainerKind::Archive);
    assert_eq!(archive.id, "zip");
    let ContainerLayout::Archive(layout) = &archive.layout else {
        panic!("expected archive layout, found {:?}", archive.layout);
    };
    layout
}

#[test]
fn identifies_hdos_fixture_image() {
    let image_path = fixture_path(IMAGE_NAME);

    let session = Session::open(&image_path).expect("session opens");
    let identification = session.identify();

    assert_eq!(identification.containers.len(), 3);
    assert_hdos_identification(&identification);
}

#[test]
fn identifies_single_image_inside_zip_fixture() {
    let zip_path = private_copy(ZIP_NAME, "single");

    let session = Session::open(&zip_path).expect("session opens");
    let identification = session.identify();

    let archive = &identification.containers[0];
    assert_eq!(archive.name, "ZIP archive");
    let layout = archive_layout(&identification);
    assert_eq!(layout.entry_name, IMAGE_NAME);
    assert_eq!(layout.uncompressed_size, Some(102_400));
    assert_eq!(identification.containers.len(), 4);
    assert_eq!(session.path(), zip_path);
    assert_eq!(session.image_path(), PathBuf::from(IMAGE_NAME));
    assert_hdos_identification(&identification);

    drop(session);
    std::fs::remove_file(&zip_path).ok();
}

#[test]
fn identifies_explicit_image_inside_zip_fixture() {
    let zip_path = private_copy(ZIP_NAME, "explicit");
    let image_path = zip_path.join(IMAGE_NAME);

    let session = Session::open(&image_path).expect("session opens");
    let identification = session.identify();

    let layout = archive_layout(&identification);
    assert_eq!(layout.entry_name, IMAGE_NAME);
    assert_eq!(identification.containers.len(), 4);
    assert_hdos_identification(&identification);

    drop(session);
    std::fs::remove_file(&zip_path).ok();
}
