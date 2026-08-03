// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use std::path::{Path, PathBuf};

use crate::adapters::{
    self, DeviceIdentity, ImageFormatDescriptor, ImageIdentification, ProbeInput,
};
use crate::archive::{self, ArchiveLayer, ImageSource};
use crate::device::AccessMode;
use crate::error::Result;
use crate::hdos::HdosFile;

/// The largest image the HDOS reader will materialize (P27): HDOS lives
/// on small vintage disks, so anything larger is refused by size before
/// a byte of it is loaded.
const HDOS_IMAGE_BOUND: u64 = 8 * 1024 * 1024;

/// What role a detected container plays in the image's layering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Archive,
    Image,
    PhysicalMedia,
    Filesystem,
    Unknown,
}

/// Where the image bytes came from inside an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLayout {
    pub path: PathBuf,
    pub entry_name: String,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: Option<u64>,
}

/// Where the payload sits inside a raw image container.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageLayout {
    pub payload_offset_bytes: Option<u64>,
    pub payload_length_bytes: Option<u64>,
}

/// Per-track sector geometry for variable layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackSectorLayout {
    pub cylinder: u32,
    pub side: u32,
    pub sectors: u32,
    pub sector_size: Option<u64>,
}

/// Sector arrangement across the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectorLayout {
    Unknown,
    Fixed { sectors_per_track: u32 },
    Variable { tracks: Vec<TrackSectorLayout> },
}

/// Physical disk geometry derived from a container format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskLayout {
    pub media_kind: Option<String>,
    pub sector_size: Option<u64>,
    pub cylinders: Option<u32>,
    pub sides: Option<u32>,
    pub sectors: SectorLayout,
    pub total_sectors: Option<u64>,
}

impl DiskLayout {
    fn from_descriptor(descriptor: &ImageFormatDescriptor) -> Self {
        let Some(disk) = descriptor.disk else {
            return Self {
                media_kind: descriptor.media_kind.map(str::to_owned),
                sector_size: None,
                cylinders: None,
                sides: None,
                sectors: SectorLayout::Unknown,
                total_sectors: None,
            };
        };
        Self {
            media_kind: Some(disk.media_kind.to_owned()),
            sector_size: Some(disk.sector_size),
            cylinders: Some(disk.cylinders),
            sides: Some(disk.sides),
            sectors: SectorLayout::Fixed {
                sectors_per_track: disk.sectors_per_track,
            },
            total_sectors: Some(
                u64::from(disk.cylinders)
                    * u64::from(disk.sides)
                    * u64::from(disk.sectors_per_track),
            ),
        }
    }
}

/// Physical media description, when one can be derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysicalMediaLayout {
    Unknown,
    Disk(DiskLayout),
}

/// Where a filesystem sits inside the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilesystemLayout {
    pub offset_bytes: Option<u64>,
    pub length_bytes: Option<u64>,
}

impl FilesystemLayout {
    pub fn unknown() -> Self {
        Self {
            offset_bytes: None,
            length_bytes: None,
        }
    }
}

/// Layout details specific to each container kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerLayout {
    Unknown,
    Archive(ArchiveLayout),
    Image(ImageLayout),
    PhysicalMedia(PhysicalMediaLayout),
    Filesystem(FilesystemLayout),
}

/// Current and expected byte sizes, when known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeInformation {
    pub current_bytes: Option<u64>,
    pub expected_bytes: Option<u64>,
}

impl SizeInformation {
    pub fn unknown() -> Self {
        Self {
            current_bytes: None,
            expected_bytes: None,
        }
    }
}

/// One detected container layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub kind: ContainerKind,
    pub id: String,
    pub name: String,
    pub confidence: u8,
    pub known: bool,
    pub size: SizeInformation,
    pub layout: ContainerLayout,
}

/// The result of identifying a session's image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identification {
    pub containers: Vec<Container>,
    pub modified: bool,
    pub evidence: Vec<String>,
}

fn unknown_image(size: SizeInformation) -> Container {
    Container {
        kind: ContainerKind::Unknown,
        id: "unknown".to_owned(),
        name: "Unknown container format".to_owned(),
        confidence: 0,
        known: false,
        size,
        layout: ContainerLayout::Unknown,
    }
}

fn unknown_filesystem() -> Container {
    Container {
        kind: ContainerKind::Filesystem,
        id: "unknown".to_owned(),
        name: "Unknown filesystem".to_owned(),
        confidence: 0,
        known: false,
        size: SizeInformation::unknown(),
        layout: ContainerLayout::Filesystem(FilesystemLayout::unknown()),
    }
}

fn physical_media_from_descriptor(
    descriptor: &ImageFormatDescriptor,
    current_bytes: u64,
) -> Option<Container> {
    let media_kind = descriptor.media_kind?;
    let name = match media_kind {
        "floppy" => "Floppy disk",
        "hard_disk" => "Hard disk",
        _ => "Unknown physical media",
    };
    let expected_bytes = descriptor.disk.map(|disk| disk.expected_size());
    Some(Container {
        kind: ContainerKind::PhysicalMedia,
        id: media_kind.to_owned(),
        name: name.to_owned(),
        confidence: 100,
        known: true,
        size: SizeInformation {
            current_bytes: Some(current_bytes),
            expected_bytes,
        },
        layout: ContainerLayout::PhysicalMedia(PhysicalMediaLayout::Disk(
            DiskLayout::from_descriptor(descriptor),
        )),
    })
}

fn container_from_layer(layer: ArchiveLayer) -> Container {
    let layout = ArchiveLayout {
        path: layer.path,
        entry_name: layer.entry_name,
        compressed_size: layer.compressed_size,
        uncompressed_size: layer.uncompressed_size,
    };
    Container {
        kind: ContainerKind::Archive,
        id: layer.id,
        name: layer.name,
        confidence: 100,
        known: true,
        size: SizeInformation {
            current_bytes: layer.archive_size,
            expected_bytes: None,
        },
        layout: ContainerLayout::Archive(layout),
    }
}

/// An open analysis session over one disk image.
///
/// The session holds the P7 claim on its source file — writes denied to
/// every other process — from open until it is dropped. The claim is
/// also the read backing: image bytes are never loaded whole, they
/// stream from the claimed file through the session cache (pledged
/// P27), and access is by bounded reads.
#[derive(Debug)]
pub struct Session {
    path: PathBuf,
    image_path: PathBuf,
    source: ImageSource,
    device_identity: DeviceIdentity,
    containers: Vec<Container>,
    modified: bool,
}

impl Session {
    /// Opens `path` — a raw disk image, or `archive.zip[/entry]` — with the
    /// built-in image-format catalog and the stated default cache bound
    /// ([`crate::DEFAULT_CACHE_BYTES`]).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_cache(path, crate::DEFAULT_CACHE_BYTES)
    }

    /// Opens `path` with a caller-declared session cache bound (P27):
    /// at most `cache_bytes` of the image stays resident, rounded up to
    /// whole 64 KiB extents — the read-ahead unit — with one extent as
    /// the floor. The bound narrows the working set; it never refuses
    /// service.
    pub fn open_with_cache(path: impl AsRef<Path>, cache_bytes: u64) -> Result<Self> {
        Self::open_full(path, cache_bytes)
    }

    fn open_full(path: impl AsRef<Path>, cache_bytes: u64) -> Result<Self> {
        let resolved = archive::resolve_image(path.as_ref(), cache_bytes)?;

        let containers = resolved
            .archive_layers
            .into_iter()
            .map(container_from_layer)
            .collect();

        Ok(Self {
            path: resolved.source_path,
            image_path: resolved.image_path,
            source: resolved.source,
            device_identity: DeviceIdentity::first(),
            containers,
            modified: false,
        })
    }

    /// Which P7 mode the open obtained on the source file: `ReadWrite`
    /// normally, `ReadOnly` when the file or media denies us write
    /// permission (writes are still denied to every other process).
    pub fn access_mode(&self) -> AccessMode {
        self.source.mode()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn image_path(&self) -> &Path {
        &self.image_path
    }

    /// The resolved image's size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.source.len()
    }

    /// Reads `buf` from the resolved image at `offset` — the bounded
    /// access form (P27): the image streams from its backing
    /// through the session cache, and no operation requires it resident
    /// whole.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.source.read_at(offset, buf)
    }

    /// Parses the HDOS directory from the session's image. HDOS images
    /// are bounded small by their formats, so the whole volume is read
    /// through the cache; an image past the bound is refused by size,
    /// never loaded (P27).
    pub fn list_hdos_files(&self) -> Result<Vec<HdosFile>> {
        let bytes = self.source.bytes_bounded(HDOS_IMAGE_BOUND, "hdos")?;
        crate::hdos::list_hdos_files(&bytes)
    }

    /// Copies one HDOS file's bytes out of the session's image, under
    /// the same size bound as [`Session::list_hdos_files`].
    pub fn read_hdos_file(&self, name: &str) -> Result<Vec<u8>> {
        let bytes = self.source.bytes_bounded(HDOS_IMAGE_BOUND, "hdos")?;
        crate::hdos::read_hdos_file(&bytes, name)
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn close(self) {}

    /// Test-only helper to simulate an in-session modification.
    #[doc(hidden)]
    pub fn mark_modified_for_test(&mut self) {
        self.modified = true;
    }

    fn containers_with(&self, extra: Vec<Container>) -> Vec<Container> {
        let mut result = self.containers.clone();
        result.extend(extra);
        result
    }

    /// Identifies the image's container layers and probable filesystem.
    /// Probes read bounded evidence — a leading prefix, the length, and the
    /// name — never the whole image (P27).
    pub fn identify(&self) -> Identification {
        let prefix = self.source.prefix(512).unwrap_or_default();
        let input = ProbeInput {
            len: self.source.len(),
            prefix: &prefix,
            path: Some(&self.image_path),
        };
        let result = adapters::image_catalog().identify(&input);
        let current_bytes = self.source.len();

        let mut archive_evidence = Vec::new();
        for existing in &self.containers {
            if let ContainerLayout::Archive(layout) = &existing.layout {
                archive_evidence.push(format!(
                    "loaded '{}' from {} archive '{}'",
                    layout.entry_name,
                    existing.id,
                    layout.path.display()
                ));
            }
        }

        let (found, confidence, mut evidence) = match result {
            ImageIdentification::Unknown { evidence } => {
                archive_evidence.extend(evidence);
                return Identification {
                    containers: self.containers_with(vec![
                        unknown_image(SizeInformation {
                            current_bytes: Some(current_bytes),
                            expected_bytes: None,
                        }),
                        unknown_filesystem(),
                    ]),
                    modified: self.modified,
                    evidence: archive_evidence,
                };
            }
            ImageIdentification::Match(found) => (found.adapter, found.confidence, found.evidence),
            ImageIdentification::Invalid {
                descriptor,
                confidence,
                evidence,
                category: _,
                reason,
            } => {
                let mut all = evidence;
                all.push(format!(
                    "recognized '{}' but refused it: {reason}",
                    descriptor.id
                ));
                archive_evidence.extend(all);
                let expected = descriptor.disk.map(|disk| disk.expected_size());
                let image = Container {
                    kind: ContainerKind::Image,
                    id: descriptor.id.to_owned(),
                    name: descriptor.name.to_owned(),
                    confidence,
                    known: true,
                    size: SizeInformation {
                        current_bytes: Some(current_bytes),
                        expected_bytes: expected,
                    },
                    layout: ContainerLayout::Image(ImageLayout {
                        payload_offset_bytes: Some(0),
                        payload_length_bytes: Some(current_bytes),
                    }),
                };
                let mut extra = vec![image];
                if let Some(media) = physical_media_from_descriptor(descriptor, current_bytes) {
                    extra.push(media);
                }
                extra.push(unknown_filesystem());
                return Identification {
                    containers: self.containers_with(extra),
                    modified: self.modified,
                    evidence: archive_evidence,
                };
            }
        };
        let descriptor = found.descriptor();
        archive_evidence.append(&mut evidence);
        archive_evidence.push(format!(
            "image format '{}' declares authoritative {} layer",
            descriptor.id,
            descriptor.authoritative_layer.name()
        ));
        archive_evidence.push(format!(
            "device {} has active {} layer",
            self.device_identity.value(),
            descriptor.initial_active_layer.name()
        ));

        let expected_bytes = descriptor.disk.map(|disk| disk.expected_size());
        let image = Container {
            kind: ContainerKind::Image,
            id: descriptor.id.to_owned(),
            name: descriptor.name.to_owned(),
            confidence,
            known: true,
            size: SizeInformation {
                current_bytes: Some(current_bytes),
                expected_bytes,
            },
            layout: ContainerLayout::Image(ImageLayout {
                payload_offset_bytes: Some(0),
                payload_length_bytes: Some(current_bytes),
            }),
        };

        let mut extra = vec![image];
        if let Some(media) = physical_media_from_descriptor(descriptor, current_bytes) {
            extra.push(media);
        }
        match found.identify_filesystems(&self.source, &mut archive_evidence) {
            Ok(filesystems) if !filesystems.is_empty() => {
                for filesystem in filesystems {
                    archive_evidence.extend(filesystem.evidence);
                    archive_evidence.push(format!(
                        "filesystem '{}' is a decoded view of device {}",
                        filesystem.id,
                        self.device_identity.value()
                    ));
                    extra.push(Container {
                        kind: ContainerKind::Filesystem,
                        id: filesystem.id,
                        name: filesystem.name,
                        confidence: filesystem.confidence,
                        known: true,
                        size: SizeInformation {
                            current_bytes: Some(filesystem.length),
                            expected_bytes: expected_bytes.filter(|_| filesystem.offset == 0),
                        },
                        layout: ContainerLayout::Filesystem(FilesystemLayout {
                            offset_bytes: Some(filesystem.offset),
                            length_bytes: Some(filesystem.length),
                        }),
                    });
                }
            }
            Ok(_) => extra.push(unknown_filesystem()),
            Err(error) => {
                archive_evidence.push(format!("{} contents not walked: {error}", descriptor.id));
                extra.push(unknown_filesystem());
            }
        }

        Identification {
            containers: self.containers_with(extra),
            modified: self.modified,
            evidence: archive_evidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_image_path(name: &str, extension: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("{name}-{}-{nonce}.{extension}", std::process::id()))
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).expect("write temp image");
    }

    #[test]
    fn session_loads_file_and_identifies_container() {
        let path = temp_image_path("session-container", "h8d");
        write_file(&path, &vec![0u8; 102_400]);

        let session = Session::open(&path).expect("session opens");
        let identification = session.identify();

        assert!(!session.is_modified());
        assert!(!identification.modified);
        assert_eq!(session.size_bytes(), 102_400);
        let mut probe = [0u8; 16];
        session.read_at(102_384, &mut probe).expect("bounded read");
        assert_eq!(probe, [0u8; 16]);
        assert_eq!(identification.containers.len(), 3);

        let image = &identification.containers[0];
        assert_eq!(image.kind, ContainerKind::Image);
        assert_eq!(image.id, "h8d");
        assert_eq!(image.name, "Heathkit H8 H17 disk image");
        assert_eq!(image.size.current_bytes, Some(102_400));
        assert_eq!(image.size.expected_bytes, Some(102_400));

        let media = &identification.containers[1];
        assert_eq!(media.kind, ContainerKind::PhysicalMedia);
        let ContainerLayout::PhysicalMedia(PhysicalMediaLayout::Disk(disk)) = &media.layout else {
            panic!("expected disk layout, found {:?}", media.layout);
        };
        assert_eq!(disk.cylinders, Some(40));
        assert_eq!(disk.sides, Some(1));
        assert_eq!(
            disk.sectors,
            SectorLayout::Fixed {
                sectors_per_track: 10
            }
        );

        let filesystem = identification
            .containers
            .last()
            .expect("filesystem container");
        assert_eq!(filesystem.kind, ContainerKind::Filesystem);
        assert_eq!(filesystem.id, "unknown");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn session_identifies_filesystem_after_container() {
        let path = temp_image_path("session-filesystem", "h8d");
        let mut bytes = vec![0u8; 102_400];
        bytes[128..132].copy_from_slice(b"HDOS");
        write_file(&path, &bytes);

        let session = Session::open(&path).expect("session opens");
        let identification = session.identify();

        let image = &identification.containers[0];
        assert_eq!(image.kind, ContainerKind::Image);
        assert_eq!(image.id, "h8d");

        let filesystem = identification
            .containers
            .last()
            .expect("filesystem container");
        assert_eq!(filesystem.kind, ContainerKind::Filesystem);
        assert_eq!(filesystem.id, "hdos");
        assert_eq!(filesystem.name, "Heath Disk Operating System");
        assert_eq!(filesystem.size.current_bytes, Some(102_400));
        let ContainerLayout::Filesystem(layout) = &filesystem.layout else {
            panic!("expected filesystem layout, found {:?}", filesystem.layout);
        };
        assert_eq!(layout.offset_bytes, Some(0));
        assert_eq!(layout.length_bytes, Some(102_400));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_declared_cache_bound_streams_identification() {
        let path = temp_image_path("session-tiny-bound", "h8d");
        let mut bytes = vec![0u8; 102_400];
        bytes[128..132].copy_from_slice(b"HDOS");
        write_file(&path, &bytes);

        // A one-extent working set (P27's declared bound at its floor):
        // identification still walks every layer correctly.
        let session = Session::open_with_cache(&path, 1).expect("session opens");
        let identification = session.identify();
        assert_eq!(identification.containers[0].id, "h8d");
        assert_eq!(
            identification.containers.last().expect("filesystem").id,
            "hdos"
        );
        let mut probe = [0u8; 4];
        session.read_at(128, &mut probe).expect("reads");
        assert_eq!(&probe, b"HDOS");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sequential_reads_stream_correctly_beside_the_prefetcher() {
        let path = temp_image_path("session-sequential", "img");
        let bytes: Vec<u8> = (0..409_600u32).map(|n| (n % 241) as u8).collect();
        write_file(&path, &bytes);

        // A four-extent bound under a sequential scan: the predictive
        // reader races ahead while eviction churns behind, and the
        // results must be identical to an unbounded, unthreaded read.
        let session = Session::open_with_cache(&path, 4 * 64 * 1024).expect("opens");
        let mut out = vec![0u8; bytes.len()];
        let chunk = 64 * 1024;
        for start in (0..bytes.len()).step_by(chunk) {
            let end = (start + chunk).min(bytes.len());
            session
                .read_at(start as u64, &mut out[start..end])
                .expect("reads");
        }
        assert_eq!(out, bytes);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn session_reports_unknown_container_and_filesystem() {
        let path = temp_image_path("session-unknown", "bin");
        write_file(&path, &[0u8; 10]);

        let session = Session::open(&path).expect("session opens");
        let identification = session.identify();

        assert_eq!(identification.containers.len(), 2);
        let image = &identification.containers[0];
        assert_eq!(image.kind, ContainerKind::Unknown);
        assert_eq!(image.id, "unknown");
        assert_eq!(image.name, "Unknown container format");
        assert!(!image.known);
        assert_eq!(image.size.current_bytes, Some(10));
        assert_eq!(image.size.expected_bytes, None);
        assert_eq!(image.layout, ContainerLayout::Unknown);

        let filesystem = identification
            .containers
            .last()
            .expect("filesystem container");
        assert_eq!(filesystem.kind, ContainerKind::Filesystem);
        assert_eq!(filesystem.id, "unknown");
        assert_eq!(filesystem.name, "Unknown filesystem");
        assert!(!filesystem.known);
        assert_eq!(filesystem.size.current_bytes, None);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn session_identification_reports_internal_modification_state() {
        let path = temp_image_path("session-modified", "h8d");
        write_file(&path, &vec![0u8; 102_400]);

        let mut session = Session::open(&path).expect("session opens");
        assert!(!session.identify().modified);

        session.mark_modified_for_test();

        assert!(session.is_modified());
        assert!(session.identify().modified);

        std::fs::remove_file(&path).ok();
    }
}
