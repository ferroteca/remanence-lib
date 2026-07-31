// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use std::path::{Path, PathBuf};

use crate::archive::{self, ArchiveLayer, SourceClaim};
use crate::container;
use crate::device::{AccessMode, SliceDevice};
use crate::error::Result;
use crate::fat::FatVolume;
use crate::filesystem;
use crate::image::DiskImage;
use crate::mbr;
use crate::qcow2::Qcow2;
use crate::registry::{ContainerFormat, FormatRegistry};

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

fn to_u32(value: Option<usize>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

impl DiskLayout {
    pub fn from_container(container: &ContainerFormat) -> Self {
        let sector_size = container.sector_size.map(|value| value as u64);
        let cylinders = to_u32(container.cylinders_or_tracks());
        let sides = to_u32(Some(container.sides_value().unwrap_or(1)));

        let sectors = match to_u32(container.sectors_per_track) {
            Some(sectors_per_track) => SectorLayout::Fixed { sectors_per_track },
            None => SectorLayout::Unknown,
        };

        let total_sectors = match (&sectors, cylinders, sides) {
            (SectorLayout::Fixed { sectors_per_track }, Some(cylinders), Some(sides)) => {
                Some(u64::from(*sectors_per_track) * u64::from(cylinders) * u64::from(sides))
            }
            _ => None,
        };

        Self {
            media_kind: container.media_kind.clone(),
            sector_size,
            cylinders,
            sides,
            sectors,
            total_sectors,
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

fn physical_media_from_container(
    container: &ContainerFormat,
    layout: DiskLayout,
    current_bytes: u64,
) -> Container {
    let media_kind = container
        .media_kind
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let name = match media_kind.as_str() {
        "floppy" => "Floppy disk",
        "hard_disk" => "Hard disk",
        _ => "Unknown physical media",
    };

    let expected_bytes = container.expected_size().map(|expected| expected as u64);

    Container {
        kind: ContainerKind::PhysicalMedia,
        id: media_kind,
        name: name.to_owned(),
        confidence: 100,
        known: true,
        size: SizeInformation {
            current_bytes: Some(current_bytes),
            expected_bytes,
        },
        layout: ContainerLayout::PhysicalMedia(PhysicalMediaLayout::Disk(layout)),
    }
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
/// every other process — from open until it is dropped.
#[derive(Debug)]
pub struct Session {
    path: PathBuf,
    image_path: PathBuf,
    bytes: Vec<u8>,
    registry: FormatRegistry,
    containers: Vec<Container>,
    modified: bool,
    claim: SourceClaim,
}

impl Session {
    /// Opens `path` — a raw disk image, or `archive.zip[/entry]` — with the
    /// default format registry.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_registry(path, crate::default_format_registry()?)
    }

    pub fn open_with_registry(path: impl AsRef<Path>, registry: FormatRegistry) -> Result<Self> {
        let resolved = archive::resolve_image(path.as_ref())?;

        let containers = resolved
            .archive_layers
            .into_iter()
            .map(container_from_layer)
            .collect();

        Ok(Self {
            path: resolved.source_path,
            image_path: resolved.image_path,
            bytes: resolved.bytes,
            registry,
            containers,
            modified: false,
            claim: resolved.claim,
        })
    }

    /// Which P7 mode the open obtained on the source file: `ReadWrite`
    /// normally, `ReadOnly` when the file or media denies us write
    /// permission (writes are still denied to every other process).
    pub fn access_mode(&self) -> AccessMode {
        self.claim.mode
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn image_path(&self) -> &Path {
        &self.image_path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn registry(&self) -> &FormatRegistry {
        &self.registry
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
    pub fn identify(&self) -> Identification {
        let container = container::detect(&self.bytes, Some(&self.image_path), &self.registry);
        let mut evidence = container.evidence;

        for existing in &self.containers {
            if let ContainerLayout::Archive(layout) = &existing.layout {
                evidence.insert(
                    0,
                    format!(
                        "loaded '{}' from {} archive '{}'",
                        layout.entry_name,
                        existing.id,
                        layout.path.display()
                    ),
                );
            }
        }

        let current_bytes = self.bytes.len() as u64;

        let Some(container_id) = container.container_id else {
            let image_container = unknown_image(SizeInformation {
                current_bytes: Some(current_bytes),
                expected_bytes: None,
            });
            return Identification {
                containers: self.containers_with(vec![image_container, unknown_filesystem()]),
                modified: self.modified,
                evidence,
            };
        };

        let Some(format) = self.registry.container(&container_id) else {
            evidence.push(format!("unknown container '{container_id}'"));
            let image_container = Container {
                kind: ContainerKind::Image,
                id: container_id,
                name: container
                    .container_name
                    .unwrap_or_else(|| "Unknown container format".to_owned()),
                confidence: container.confidence,
                known: true,
                size: SizeInformation {
                    current_bytes: Some(current_bytes),
                    expected_bytes: None,
                },
                layout: ContainerLayout::Unknown,
            };
            return Identification {
                containers: self.containers_with(vec![image_container, unknown_filesystem()]),
                modified: self.modified,
                evidence,
            };
        };

        let expected_bytes = format.expected_size().map(|expected| expected as u64);

        let image_container = Container {
            kind: ContainerKind::Image,
            id: container_id.clone(),
            name: container
                .container_name
                .unwrap_or_else(|| format.name.clone()),
            confidence: container.confidence,
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

        let physical_media = physical_media_from_container(
            format,
            DiskLayout::from_container(format),
            current_bytes,
        );

        let image = match DiskImage::from_bytes(format, self.bytes.clone()) {
            Ok(image) => image,
            Err(_) => {
                evidence.push(format!("invalid container '{container_id}'"));
                return Identification {
                    containers: self.containers_with(vec![
                        image_container,
                        physical_media,
                        unknown_filesystem(),
                    ]),
                    modified: self.modified,
                    evidence,
                };
            }
        };

        let filesystem = filesystem::detect(&image, &self.registry);
        evidence.extend(filesystem.evidence);

        let filesystem_container = match (filesystem.filesystem_id, filesystem.filesystem_name) {
            (Some(id), Some(name)) => Container {
                kind: ContainerKind::Filesystem,
                id,
                name,
                confidence: filesystem.confidence,
                known: true,
                size: SizeInformation {
                    current_bytes: Some(current_bytes),
                    expected_bytes,
                },
                layout: ContainerLayout::Filesystem(FilesystemLayout {
                    offset_bytes: Some(0),
                    length_bytes: Some(current_bytes),
                }),
            },
            _ => unknown_filesystem(),
        };

        // A qcow2 container gets its virtual-disk layers reported too
        // (pledged U5): the partitions inside the virtual disk and the
        // FAT volumes inside those, through the same evidence model.
        let mut extra = vec![image_container, physical_media];
        if container_id == "qcow2" {
            match self.qcow2_layers(&mut evidence) {
                Ok(mut volumes) => extra.append(&mut volumes),
                Err(error) => evidence.push(format!("qcow2 virtual disk not walked: {error}")),
            }
        }
        extra.push(filesystem_container);

        Identification {
            containers: self.containers_with(extra),
            modified: self.modified,
            evidence,
        }
    }

    /// Walks a qcow2 session's virtual disk — header, partitions, FAT
    /// volumes — and returns one Filesystem container per volume read.
    fn qcow2_layers(&self, evidence: &mut Vec<String>) -> Result<Vec<Container>> {
        let mut qcow2 = Qcow2::open(SliceDevice::new(&self.bytes))?;
        let header = qcow2.header().clone();
        evidence.push(format!(
            "qcow2 version {}, virtual size {} bytes",
            header.version, header.virtual_size
        ));

        let mut containers = Vec::new();
        let spans: Vec<(Option<u32>, u64, u64)> = match mbr::discover(&mut qcow2)? {
            mbr::Discovery::Blank => {
                evidence.push("virtual disk is blank (sector 0 all zero)".to_owned());
                Vec::new()
            }
            mbr::Discovery::BareVolume => vec![(None, 0, header.virtual_size)],
            mbr::Discovery::Partitioned(partitions) => {
                if !partitions.is_empty() {
                    evidence.push(format!(
                        "found {} partition(s) in the virtual disk",
                        partitions.len()
                    ));
                }
                partitions
                    .iter()
                    .filter(|partition| !mbr::is_extended(partition.type_byte))
                    .map(|partition| {
                        (
                            Some(partition.number),
                            partition.start_bytes,
                            partition.length_bytes,
                        )
                    })
                    .collect()
            }
        };

        for (partition, offset, length) in spans {
            let Ok(volume) = FatVolume::open(&mut qcow2, offset) else {
                continue;
            };
            let id = partition.map_or_else(
                || "superfloppy:0".to_owned(),
                |number| format!("partition:{number}"),
            );
            let info = volume.info(&mut qcow2, id, partition, length)?;
            let kind_name = info.kind.name();
            evidence.push(match (&info.label, partition) {
                (Some(label), Some(number)) => {
                    format!("{kind_name} volume '{label}' in partition {number}")
                }
                (Some(label), None) => format!("{kind_name} volume '{label}'"),
                (None, Some(number)) => {
                    format!("{kind_name} volume in partition {number}")
                }
                (None, None) => format!("{kind_name} volume"),
            });
            containers.push(Container {
                kind: ContainerKind::Filesystem,
                id: kind_name.to_ascii_lowercase(),
                name: match &info.label {
                    Some(label) => format!("{kind_name} volume '{label}'"),
                    None => format!("{kind_name} volume"),
                },
                confidence: 100,
                known: true,
                size: SizeInformation {
                    current_bytes: Some(info.length_bytes),
                    expected_bytes: None,
                },
                layout: ContainerLayout::Filesystem(FilesystemLayout {
                    offset_bytes: Some(info.offset_bytes),
                    length_bytes: Some(info.length_bytes),
                }),
            });
        }

        Ok(containers)
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
        assert_eq!(session.bytes().len(), 102_400);
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
