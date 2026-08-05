// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use std::path::{Path, PathBuf};

use crate::adapters::{
    self, DeviceIdentity, ImageFormatDescriptor, ImageIdentification, ProbeInput,
};
use crate::source::{ArchiveLayer, ImageSource};

/// What role a recognized layer plays in the artifact's nesting.
///
/// This is a different axis from the P13 authoritative layer and the P23
/// active layer a device reports: those name which representation an
/// artifact records and which one carries the session's mutable truth,
/// while these name what was recognized at each level of the nesting an
/// artifact was reached through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
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

/// Where the payload sits inside a raw image.
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

/// Physical disk geometry derived from an image format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskLayout {
    /// The media type the image format names for the medium it holds
    /// state for — an entry in the media-type catalog (P14), not a
    /// free-form word. What the recording is sits in the fields below
    /// it; what the medium is stays with the type.
    pub media_type: String,
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
                media_type: descriptor.media.id.to_owned(),
                sector_size: None,
                cylinders: None,
                sides: None,
                sectors: SectorLayout::Unknown,
                total_sectors: None,
            };
        };
        Self {
            media_type: descriptor.media.id.to_owned(),
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

/// Layout details specific to each layer kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerLayout {
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

/// One recognized layer of the artifact's nesting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub kind: LayerKind,
    pub id: String,
    pub name: String,
    pub confidence: u8,
    pub known: bool,
    pub size: SizeInformation,
    pub layout: LayerLayout,
}

/// The result of identifying a session's image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identification {
    pub layers: Vec<Layer>,
    pub modified: bool,
    pub evidence: Vec<String>,
}

fn unknown_image(size: SizeInformation) -> Layer {
    Layer {
        kind: LayerKind::Unknown,
        id: "unknown".to_owned(),
        name: "Unknown image format".to_owned(),
        confidence: 0,
        known: false,
        size,
        layout: LayerLayout::Unknown,
    }
}

fn unknown_filesystem() -> Layer {
    Layer {
        kind: LayerKind::Filesystem,
        id: "unknown".to_owned(),
        name: "Unknown filesystem".to_owned(),
        confidence: 0,
        known: false,
        size: SizeInformation::unknown(),
        layout: LayerLayout::Filesystem(FilesystemLayout::unknown()),
    }
}

/// The medium layer of an identification: the media type the recognized
/// image format names, said in the catalog's own words.
///
/// Central code reads an id and a name off the profile and interprets
/// neither. There was a `match` on a media-kind string here once, which
/// is exactly the string-named rule P12 keeps out of orchestration and
/// the reason P14's catalog is declarative.
fn physical_media_from_descriptor(
    descriptor: &ImageFormatDescriptor,
    current_bytes: u64,
) -> Layer {
    let media = descriptor.media;
    let expected_bytes = descriptor.disk.map(|disk| disk.expected_size());
    Layer {
        kind: LayerKind::PhysicalMedia,
        id: media.id.to_owned(),
        name: media.name.to_owned(),
        confidence: 100,
        known: true,
        size: SizeInformation {
            current_bytes: Some(current_bytes),
            expected_bytes,
        },
        layout: LayerLayout::PhysicalMedia(PhysicalMediaLayout::Disk(
            DiskLayout::from_descriptor(descriptor),
        )),
    }
}

pub(crate) fn layer_from_archive(layer: ArchiveLayer) -> Layer {
    let layout = ArchiveLayout {
        path: layer.path,
        entry_name: layer.entry_name,
        compressed_size: layer.compressed_size,
        uncompressed_size: layer.uncompressed_size,
    };
    Layer {
        kind: LayerKind::Archive,
        id: layer.id,
        name: layer.name,
        confidence: 100,
        known: true,
        size: SizeInformation {
            current_bytes: layer.archive_size,
            expected_bytes: None,
        },
        layout: LayerLayout::Archive(layout),
    }
}

fn layers_with(layers: &[Layer], extra: Vec<Layer>) -> Vec<Layer> {
    let mut result = layers.to_vec();
    result.extend(extra);
    result
}

/// Identifies a medium's nesting layers and probable filesystem.
/// Probes read bounded evidence — a leading prefix, the length, and the
/// name — never the whole image (P27).
///
/// This is the raw plane's verb (F43): it works over the medium's own
/// bytes, above the same claim the presented disk is opened on, and is
/// reached through [`crate::StorageDevice`].
pub(crate) fn identify_medium(
    source: &ImageSource,
    image_path: &Path,
    layers: &[Layer],
    device_identity: DeviceIdentity,
    modified: bool,
) -> Identification {
    {
        let prefix = source.prefix(512).unwrap_or_default();
        let input = ProbeInput {
            len: source.len(),
            prefix: &prefix,
            path: Some(image_path),
        };
        let result = adapters::image_catalog().identify(&input);
        let current_bytes = source.len();

        let mut archive_evidence = Vec::new();
        for existing in layers {
            if let LayerLayout::Archive(layout) = &existing.layout {
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
                    layers: layers_with(layers, vec![
                        unknown_image(SizeInformation {
                            current_bytes: Some(current_bytes),
                            expected_bytes: None,
                        }),
                        unknown_filesystem(),
                    ]),
                    modified: modified,
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
                let image = Layer {
                    kind: LayerKind::Image,
                    id: descriptor.id.to_owned(),
                    name: descriptor.name.to_owned(),
                    confidence,
                    known: true,
                    size: SizeInformation {
                        current_bytes: Some(current_bytes),
                        expected_bytes: expected,
                    },
                    layout: LayerLayout::Image(ImageLayout {
                        payload_offset_bytes: Some(0),
                        payload_length_bytes: Some(current_bytes),
                    }),
                };
                let extra = vec![
                    image,
                    physical_media_from_descriptor(descriptor, current_bytes),
                    unknown_filesystem(),
                ];
                return Identification {
                    layers: layers_with(layers, extra),
                    modified: modified,
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
            device_identity.value(),
            descriptor.initial_active_layer.name()
        ));

        let expected_bytes = descriptor.disk.map(|disk| disk.expected_size());
        let image = Layer {
            kind: LayerKind::Image,
            id: descriptor.id.to_owned(),
            name: descriptor.name.to_owned(),
            confidence,
            known: true,
            size: SizeInformation {
                current_bytes: Some(current_bytes),
                expected_bytes,
            },
            layout: LayerLayout::Image(ImageLayout {
                payload_offset_bytes: Some(0),
                payload_length_bytes: Some(current_bytes),
            }),
        };

        let mut extra = vec![
            image,
            physical_media_from_descriptor(descriptor, current_bytes),
        ];
        match found.identify_filesystems(&source, &mut archive_evidence) {
            Ok(filesystems) if !filesystems.is_empty() => {
                for filesystem in filesystems {
                    archive_evidence.extend(filesystem.evidence);
                    archive_evidence.push(format!(
                        "filesystem '{}' is a decoded view of device {}",
                        filesystem.id,
                        device_identity.value()
                    ));
                    extra.push(Layer {
                        kind: LayerKind::Filesystem,
                        id: filesystem.id,
                        name: filesystem.name,
                        confidence: filesystem.confidence,
                        known: true,
                        size: SizeInformation {
                            current_bytes: Some(filesystem.length),
                            expected_bytes: expected_bytes.filter(|_| filesystem.offset == 0),
                        },
                        layout: LayerLayout::Filesystem(FilesystemLayout {
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
            layers: layers_with(layers, extra),
            modified: modified,
            evidence: archive_evidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::AccessIntent;

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
    fn session_loads_file_and_identifies_the_image_layer() {
        let path = temp_image_path("session-layer", "h8d");
        write_file(&path, &vec![0u8; 102_400]);

        let disk = crate::disk::MediaState::open(&path, AccessIntent::Read).expect("disk opens");
        let identification = disk.identify();

        assert!(!disk.is_modified());
        assert!(!identification.modified);
        assert_eq!(disk.image_size_bytes(), 102_400);
        let mut probe = [0u8; 16];
        disk.read_at(102_384, &mut probe).expect("bounded read");
        assert_eq!(probe, [0u8; 16]);
        assert_eq!(identification.layers.len(), 3);

        let image = &identification.layers[0];
        assert_eq!(image.kind, LayerKind::Image);
        assert_eq!(image.id, "h8d");
        assert_eq!(image.name, "Heathkit H8 H17 disk image");
        assert_eq!(image.size.current_bytes, Some(102_400));
        assert_eq!(image.size.expected_bytes, Some(102_400));

        let media = &identification.layers[1];
        assert_eq!(media.kind, LayerKind::PhysicalMedia);
        let LayerLayout::PhysicalMedia(PhysicalMediaLayout::Disk(disk)) = &media.layout else {
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
            .layers
            .last()
            .expect("filesystem layer");
        assert_eq!(filesystem.kind, LayerKind::Filesystem);
        assert_eq!(filesystem.id, "unknown");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn session_identifies_filesystem_after_the_image_layer() {
        let path = temp_image_path("session-filesystem", "h8d");
        let mut bytes = vec![0u8; 102_400];
        bytes[128..132].copy_from_slice(b"HDOS");
        write_file(&path, &bytes);

        let disk = crate::disk::MediaState::open(&path, AccessIntent::Read).expect("disk opens");
        let identification = disk.identify();

        let image = &identification.layers[0];
        assert_eq!(image.kind, LayerKind::Image);
        assert_eq!(image.id, "h8d");

        let filesystem = identification
            .layers
            .last()
            .expect("filesystem layer");
        assert_eq!(filesystem.kind, LayerKind::Filesystem);
        assert_eq!(filesystem.id, "hdos");
        assert_eq!(filesystem.name, "Heath Disk Operating System");
        assert_eq!(filesystem.size.current_bytes, Some(102_400));
        let LayerLayout::Filesystem(layout) = &filesystem.layout else {
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
        let disk = crate::disk::MediaState::open_with_cache(&path, AccessIntent::Read, 1).expect("disk opens");
        let identification = disk.identify();
        assert_eq!(identification.layers[0].id, "h8d");
        assert_eq!(
            identification.layers.last().expect("filesystem").id,
            "hdos"
        );
        let mut probe = [0u8; 4];
        disk.read_at(128, &mut probe).expect("reads");
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
        let disk = crate::disk::MediaState::open_with_cache(&path, AccessIntent::Read, 4 * 64 * 1024).expect("opens");
        let mut out = vec![0u8; bytes.len()];
        let chunk = 64 * 1024;
        for start in (0..bytes.len()).step_by(chunk) {
            let end = (start + chunk).min(bytes.len());
            disk.read_at(start as u64, &mut out[start..end])
                .expect("reads");
        }
        assert_eq!(out, bytes);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn session_reports_unknown_image_and_filesystem() {
        let path = temp_image_path("session-unknown", "bin");
        write_file(&path, &[0u8; 10]);

        let disk = crate::disk::MediaState::open(&path, AccessIntent::Read).expect("disk opens");
        let identification = disk.identify();

        assert_eq!(identification.layers.len(), 2);
        let image = &identification.layers[0];
        assert_eq!(image.kind, LayerKind::Unknown);
        assert_eq!(image.id, "unknown");
        assert_eq!(image.name, "Unknown image format");
        assert!(!image.known);
        assert_eq!(image.size.current_bytes, Some(10));
        assert_eq!(image.size.expected_bytes, None);
        assert_eq!(image.layout, LayerLayout::Unknown);

        let filesystem = identification
            .layers
            .last()
            .expect("filesystem layer");
        assert_eq!(filesystem.kind, LayerKind::Filesystem);
        assert_eq!(filesystem.id, "unknown");
        assert_eq!(filesystem.name, "Unknown filesystem");
        assert!(!filesystem.known);
        assert_eq!(filesystem.size.current_bytes, None);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn identification_reports_the_real_modification_state() {
        let path = temp_image_path("session-modified", "h8d");
        write_file(&path, &vec![0u8; 102_400]);

        // Before F43 this was a `mark_modified_for_test` flag, because an
        // identification session had no writes to report. The merged
        // surface has real ones, so the reported state is the session
        // cache's own and the flag is gone. The modified-after-write half
        // lives in `tests/disk.rs`, where a volume exists to write to.
        let disk = crate::disk::MediaState::open(&path, AccessIntent::Read).expect("disk opens");
        assert!(!disk.is_modified());
        assert_eq!(disk.identify().modified, disk.is_modified());

        std::fs::remove_file(&path).ok();
    }
}
