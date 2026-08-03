// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Internal adapter vocabulary and the built-in image-format catalog.
//!
//! Descriptors and executable behavior are deliberately inseparable: a
//! catalog contains adapters, never declarations that central code must
//! interpret. Each probe is bounded to the supplied prefix and source
//! metadata (P27).

use std::path::Path;

use crate::source::{ImageSource, SourceDevice};
use crate::device::{Device, FileDevice};
use crate::disk::DiskFormat;
use crate::error::{Error, ErrorCategory, Result};
use crate::fat::FatVolume;
use crate::filesystem;
use crate::mbr;
use crate::qcow2::{QCOW2_MAGIC, Qcow2, SUPPORTED_VERSION_CEILING};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageLayer {
    Chs,
    Block,
}

impl ImageLayer {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Chs => "CHS",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveLayer {
    Chs,
    Block,
}

impl ActiveLayer {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Chs => "CHS",
            Self::Block => "block",
        }
    }
}

/// Opaque identity assigned by one loaded composition (P21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceIdentity(u64);

impl DeviceIdentity {
    pub(crate) const fn first() -> Self {
        Self(1)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiskDescriptor {
    pub(crate) media_kind: &'static str,
    pub(crate) sector_size: u64,
    pub(crate) cylinders: u32,
    pub(crate) sides: u32,
    pub(crate) sectors_per_track: u32,
}

impl DiskDescriptor {
    pub(crate) const fn expected_size(self) -> u64 {
        self.sector_size * self.cylinders as u64 * self.sides as u64 * self.sectors_per_track as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageFormatDescriptor {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) extensions: &'static [&'static str],
    pub(crate) authoritative_layer: ImageLayer,
    pub(crate) initial_active_layer: ActiveLayer,
    pub(crate) media_kind: Option<&'static str>,
    pub(crate) disk: Option<DiskDescriptor>,
}

pub(crate) struct ProbeInput<'a> {
    pub(crate) len: u64,
    pub(crate) prefix: &'a [u8],
    pub(crate) path: Option<&'a Path>,
}

impl ProbeInput<'_> {
    pub(crate) fn extension(&self) -> Option<String> {
        self.path
            .and_then(Path::extension)
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
            .filter(|extension| !extension.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeResult {
    NoMatch,
    Match {
        confidence: u8,
        evidence: Vec<String>,
    },
    Invalid {
        confidence: u8,
        evidence: Vec<String>,
        category: ErrorCategory,
        reason: String,
    },
}

impl ProbeResult {
    fn confidence(&self) -> u8 {
        match self {
            Self::NoMatch => 0,
            Self::Match { confidence, .. } | Self::Invalid { confidence, .. } => *confidence,
        }
    }
}

pub(crate) trait OpenedImage: Device + std::fmt::Debug + Send + Sync {
    fn device_mut(&mut self) -> &mut dyn Device;
    fn host_mut(&mut self) -> &mut FileDevice;
    fn cache_snapshot(&self) -> Option<Vec<u64>>;
    fn restore_cache(&mut self, snapshot: Option<Vec<u64>>);
    fn format(&self) -> DiskFormat;
    fn presented_size(&self) -> u64;
}

#[derive(Debug)]
struct RawImage(FileDevice);

impl Device for RawImage {
    fn len(&self) -> u64 {
        self.0.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.0.read_at(offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.0.write_at(offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        self.0.flush()
    }
}

impl OpenedImage for RawImage {
    fn device_mut(&mut self) -> &mut dyn Device {
        self
    }

    fn host_mut(&mut self) -> &mut FileDevice {
        &mut self.0
    }

    fn cache_snapshot(&self) -> Option<Vec<u64>> {
        None
    }

    fn restore_cache(&mut self, _snapshot: Option<Vec<u64>>) {}

    fn format(&self) -> DiskFormat {
        DiskFormat::Raw
    }

    fn presented_size(&self) -> u64 {
        self.0.len()
    }
}

#[derive(Debug)]
struct Qcow2Image(Qcow2<FileDevice>);

impl Device for Qcow2Image {
    fn len(&self) -> u64 {
        self.0.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.0.read_at(offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.0.write_at(offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        self.0.flush()
    }
}

impl OpenedImage for Qcow2Image {
    fn device_mut(&mut self) -> &mut dyn Device {
        self
    }

    fn host_mut(&mut self) -> &mut FileDevice {
        self.0.host_mut()
    }

    fn cache_snapshot(&self) -> Option<Vec<u64>> {
        Some(self.0.l1_snapshot())
    }

    fn restore_cache(&mut self, snapshot: Option<Vec<u64>>) {
        if let Some(l1) = snapshot {
            self.0.restore_l1(l1);
        }
    }

    fn format(&self) -> DiskFormat {
        DiskFormat::Qcow2 {
            version: self.0.header().version,
        }
    }

    fn presented_size(&self) -> u64 {
        self.0.header().virtual_size
    }
}

#[derive(Debug)]
pub(crate) struct DetectedFilesystem {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) confidence: u8,
    pub(crate) offset: u64,
    pub(crate) length: u64,
    pub(crate) evidence: Vec<String>,
}

pub(crate) trait ImageFormatAdapter: Sync {
    fn descriptor(&self) -> &'static ImageFormatDescriptor;
    fn probe(&self, input: &ProbeInput<'_>) -> ProbeResult;
    fn identify_filesystems(
        &self,
        _source: &ImageSource,
        _evidence: &mut Vec<String>,
    ) -> Result<Vec<DetectedFilesystem>> {
        Ok(Vec::new())
    }
    fn open_disk(&self, file: FileDevice, path: &Path) -> Result<Box<dyn OpenedImage>>;
}

pub(crate) struct ImageMatch<'a> {
    pub(crate) adapter: &'a dyn ImageFormatAdapter,
    pub(crate) confidence: u8,
    pub(crate) evidence: Vec<String>,
}

pub(crate) enum ImageIdentification<'a> {
    Unknown {
        evidence: Vec<String>,
    },
    Match(ImageMatch<'a>),
    Invalid {
        descriptor: &'static ImageFormatDescriptor,
        confidence: u8,
        evidence: Vec<String>,
        category: ErrorCategory,
        reason: String,
    },
}

pub(crate) struct ImageCatalog<'a> {
    adapters: &'a [&'a dyn ImageFormatAdapter],
}

impl<'a> ImageCatalog<'a> {
    pub(crate) const fn new(adapters: &'a [&'a dyn ImageFormatAdapter]) -> Self {
        Self { adapters }
    }

    pub(crate) fn identify(&self, input: &ProbeInput<'_>) -> ImageIdentification<'a> {
        let mut strongest = 0;
        let mut candidates = Vec::new();
        for adapter in self.adapters {
            let result = adapter.probe(input);
            let confidence = result.confidence();
            if confidence == 0 || confidence < strongest {
                continue;
            }
            if confidence > strongest {
                strongest = confidence;
                candidates.clear();
            }
            candidates.push((*adapter, result));
        }

        if candidates.is_empty() {
            return ImageIdentification::Unknown {
                evidence: vec!["no image-format signatures found".to_owned()],
            };
        }

        if candidates.len() > 1 {
            let mut evidence = vec![format!(
                "image-format recognition is ambiguous at confidence {strongest}"
            )];
            for (adapter, result) in candidates {
                evidence.push(format!("competing format '{}':", adapter.descriptor().id));
                match result {
                    ProbeResult::Match {
                        evidence: observations,
                        ..
                    }
                    | ProbeResult::Invalid {
                        evidence: observations,
                        ..
                    } => {
                        evidence.extend(observations.into_iter().map(|item| format!("  {item}")));
                    }
                    ProbeResult::NoMatch => unreachable!(),
                }
            }
            return ImageIdentification::Unknown { evidence };
        }

        let (adapter, result) = candidates.pop().expect("one candidate");
        match result {
            ProbeResult::Match {
                confidence,
                evidence,
            } => ImageIdentification::Match(ImageMatch {
                adapter,
                confidence,
                evidence,
            }),
            ProbeResult::Invalid {
                confidence,
                evidence,
                category,
                reason,
            } => ImageIdentification::Invalid {
                descriptor: adapter.descriptor(),
                confidence,
                evidence,
                category,
                reason,
            },
            ProbeResult::NoMatch => unreachable!(),
        }
    }

    pub(crate) fn open_disk(
        &self,
        mut file: FileDevice,
        path: &Path,
    ) -> Result<(Box<dyn OpenedImage>, &'static ImageFormatDescriptor)> {
        let mut prefix = vec![0u8; file.len().min(512) as usize];
        if !prefix.is_empty() {
            file.read_at(0, &mut prefix)?;
        }
        let input = ProbeInput {
            len: file.len(),
            prefix: &prefix,
            path: Some(path),
        };
        match self.identify(&input) {
            ImageIdentification::Match(found) => {
                let descriptor = found.adapter.descriptor();
                Ok((found.adapter.open_disk(file, path)?, descriptor))
            }
            ImageIdentification::Invalid {
                descriptor,
                category,
                reason,
                ..
            } => Err(Error::categorized_image(category, descriptor.id, reason)),
            ImageIdentification::Unknown { .. } => {
                let adapter = &RAW_ADAPTER as &dyn ImageFormatAdapter;
                Ok((adapter.open_disk(file, path)?, adapter.descriptor()))
            }
        }
    }
}

pub(crate) struct H8dAdapter;

pub(crate) static H8D_ADAPTER: H8dAdapter = H8dAdapter;

static H8D_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
    id: "h8d",
    name: "Heathkit H8 H17 disk image",
    extensions: &["h8d"],
    authoritative_layer: ImageLayer::Chs,
    initial_active_layer: ActiveLayer::Chs,
    media_kind: Some("floppy"),
    disk: Some(DiskDescriptor {
        media_kind: "floppy",
        sector_size: 256,
        cylinders: 40,
        sides: 1,
        sectors_per_track: 10,
    }),
};

impl ImageFormatAdapter for H8dAdapter {
    fn descriptor(&self) -> &'static ImageFormatDescriptor {
        &H8D_DESCRIPTOR
    }

    fn probe(&self, input: &ProbeInput<'_>) -> ProbeResult {
        let expected = H8D_DESCRIPTOR.disk.expect("H8D geometry").expected_size();
        let extension = input.extension();
        let extension_matches = extension
            .as_deref()
            .is_some_and(|extension| H8D_DESCRIPTOR.extensions.contains(&extension));
        if input.len != expected {
            return if extension_matches {
                ProbeResult::Invalid {
                    confidence: 20,
                    evidence: vec![format!("matched file extension '.{}'", extension.unwrap())],
                    category: ErrorCategory::InvalidImage,
                    reason: format!("expected {expected} bytes, found {}", input.len),
                }
            } else {
                ProbeResult::NoMatch
            };
        }

        let mut confidence = 80;
        let mut evidence = vec![format!("matched expected size of {expected} bytes")];
        if extension_matches {
            confidence = 100;
            evidence.push(format!("matched file extension '.{}'", extension.unwrap()));
        }
        ProbeResult::Match {
            confidence,
            evidence,
        }
    }

    fn identify_filesystems(
        &self,
        source: &ImageSource,
        _evidence: &mut Vec<String>,
    ) -> Result<Vec<DetectedFilesystem>> {
        let expected = H8D_DESCRIPTOR.disk.expect("H8D geometry").expected_size();
        let mut volume = SourceDevice(source);
        let found = filesystem::detect(&mut volume)?;
        Ok(match (found.filesystem_id, found.filesystem_name) {
            (Some(id), Some(name)) => vec![DetectedFilesystem {
                id: id.to_owned(),
                name: name.to_owned(),
                confidence: found.confidence,
                offset: 0,
                length: expected,
                evidence: found.evidence,
            }],
            _ => {
                _evidence.extend(found.evidence);
                Vec::new()
            }
        })
    }

    fn open_disk(&self, file: FileDevice, _path: &Path) -> Result<Box<dyn OpenedImage>> {
        Ok(Box::new(RawImage(file)))
    }
}

pub(crate) struct Qcow2Adapter;

pub(crate) static QCOW2_ADAPTER: Qcow2Adapter = Qcow2Adapter;

static QCOW2_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
    id: "qcow2",
    name: "QEMU copy-on-write disk image",
    extensions: &["qcow2", "qcow"],
    authoritative_layer: ImageLayer::Block,
    initial_active_layer: ActiveLayer::Block,
    media_kind: Some("hard_disk"),
    disk: None,
};

impl ImageFormatAdapter for Qcow2Adapter {
    fn descriptor(&self) -> &'static ImageFormatDescriptor {
        &QCOW2_DESCRIPTOR
    }

    fn probe(&self, input: &ProbeInput<'_>) -> ProbeResult {
        if !input.prefix.starts_with(&QCOW2_MAGIC) {
            return ProbeResult::NoMatch;
        }
        let evidence = vec!["matched 4-byte qcow2 magic signature".to_owned()];
        if input.prefix.len() < 8 {
            return ProbeResult::Invalid {
                confidence: 100,
                evidence,
                category: ErrorCategory::InvalidImage,
                reason: "file too small for a qcow2 version field".to_owned(),
            };
        }
        // P8's version gate is first once the magic and version field exist.
        let version = u32::from_be_bytes(input.prefix[4..8].try_into().expect("four bytes"));
        if version < 2 {
            return ProbeResult::Invalid {
                confidence: 100,
                evidence,
                category: ErrorCategory::Unsupported,
                reason: format!("unsupported qcow2 version {version}"),
            };
        }
        if version > SUPPORTED_VERSION_CEILING {
            return ProbeResult::Invalid {
                confidence: 100,
                evidence,
                category: ErrorCategory::Unsupported,
                reason: format!(
                    "qcow2 version {version} is newer than this release supports                      (ceiling: version {SUPPORTED_VERSION_CEILING}); refusing to touch it"
                ),
            };
        }
        if input.len < 72 {
            return ProbeResult::Invalid {
                confidence: 100,
                evidence,
                category: ErrorCategory::InvalidImage,
                reason: "file too small for a qcow2 header".to_owned(),
            };
        }
        ProbeResult::Match {
            confidence: 100,
            evidence: vec![
                "matched 4-byte qcow2 magic signature".to_owned(),
                format!("recognized qcow2 version {version}"),
            ],
        }
    }

    fn identify_filesystems(
        &self,
        source: &ImageSource,
        evidence: &mut Vec<String>,
    ) -> Result<Vec<DetectedFilesystem>> {
        let mut qcow2 = Qcow2::open(SourceDevice(source))?;
        let header = qcow2.header().clone();
        evidence.push(format!(
            "qcow2 version {}, virtual size {} bytes",
            header.version, header.virtual_size
        ));
        let spans: Vec<(Option<u32>, u64, u64)> = match crate::partition::discover(&mut qcow2)? {
            mbr::Discovery::Blank => {
                evidence.push("virtual disk is blank (sector 0 all zero)".to_owned());
                Vec::new()
            }
            mbr::Discovery::BareVolume => {
                let volume = crate::volume::direct(crate::volume::AddressedRegion {
                    device: DeviceIdentity::first(),
                    offset: 0,
                    length: header.virtual_size,
                });
                vec![(None, volume.offset, volume.length)]
            }
            // Identification refuses guest content no adapter claims, as
            // it always has. Stating it as an outcome instead is the
            // layered report's change and belongs to that surface only.
            mbr::Discovery::UnknownNonblank { evidence: reason } => {
                return Err(mbr::unknown_nonblank(&reason));
            }
            mbr::Discovery::Partitioned(partitions) => {
                evidence.push(format!(
                    "found {} partition(s) in the virtual disk",
                    partitions.len()
                ));
                partitions
                    .into_iter()
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

        let mut found = Vec::new();
        for (partition, offset, length) in spans {
            let Ok(volume) = FatVolume::open(&mut qcow2, offset) else {
                continue;
            };
            let info = volume.recognized(&mut qcow2)?;
            let kind = info.kind.name();
            let name = match &info.label {
                Some(label) => format!("{kind} volume '{label}'"),
                None => format!("{kind} volume"),
            };
            let observation = match (&info.label, partition) {
                (Some(label), Some(number)) => {
                    format!("{kind} volume '{label}' in partition {number}")
                }
                (Some(label), None) => format!("{kind} volume '{label}'"),
                (None, Some(number)) => format!("{kind} volume in partition {number}"),
                (None, None) => format!("{kind} volume"),
            };
            found.push(DetectedFilesystem {
                id: kind.to_ascii_lowercase(),
                name,
                confidence: 100,
                offset,
                length,
                evidence: vec![observation],
            });
        }
        Ok(found)
    }

    fn open_disk(&self, file: FileDevice, path: &Path) -> Result<Box<dyn OpenedImage>> {
        Ok(Box::new(Qcow2Image(crate::qcow2::open_chain(file, path)?)))
    }
}

struct RawAdapter;

static RAW_ADAPTER: RawAdapter = RawAdapter;
static RAW_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
    id: "raw",
    name: "Raw disk image",
    extensions: &[],
    authoritative_layer: ImageLayer::Block,
    initial_active_layer: ActiveLayer::Block,
    media_kind: None,
    disk: None,
};

impl ImageFormatAdapter for RawAdapter {
    fn descriptor(&self) -> &'static ImageFormatDescriptor {
        &RAW_DESCRIPTOR
    }

    fn probe(&self, _input: &ProbeInput<'_>) -> ProbeResult {
        ProbeResult::NoMatch
    }

    fn open_disk(&self, file: FileDevice, _path: &Path) -> Result<Box<dyn OpenedImage>> {
        Ok(Box::new(RawImage(file)))
    }
}

static BUILT_IN_IMAGE_ADAPTERS: [&dyn ImageFormatAdapter; 2] = [&H8D_ADAPTER, &QCOW2_ADAPTER];

pub(crate) fn image_catalog() -> ImageCatalog<'static> {
    ImageCatalog::new(&BUILT_IN_IMAGE_ADAPTERS)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SameProbe(&'static ImageFormatDescriptor);

    impl ImageFormatAdapter for SameProbe {
        fn descriptor(&self) -> &'static ImageFormatDescriptor {
            self.0
        }

        fn probe(&self, _input: &ProbeInput<'_>) -> ProbeResult {
            ProbeResult::Match {
                confidence: 50,
                evidence: vec![format!("{} says yes", self.0.id)],
            }
        }

        fn open_disk(&self, file: FileDevice, _path: &Path) -> Result<Box<dyn OpenedImage>> {
            Ok(Box::new(RawImage(file)))
        }
    }

    static TEST_A_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
        id: "test-a",
        name: "Test A",
        extensions: &[],
        authoritative_layer: ImageLayer::Block,
        initial_active_layer: ActiveLayer::Block,
        media_kind: None,
        disk: None,
    };
    static TEST_B_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
        id: "test-b",
        name: "Test B",
        extensions: &[],
        authoritative_layer: ImageLayer::Block,
        initial_active_layer: ActiveLayer::Block,
        media_kind: None,
        disk: None,
    };
    static TEST_A: SameProbe = SameProbe(&TEST_A_DESCRIPTOR);
    static TEST_B: SameProbe = SameProbe(&TEST_B_DESCRIPTOR);

    #[test]
    fn a_test_adapter_is_enrolled_without_changing_orchestration() {
        let adapters: [&dyn ImageFormatAdapter; 1] = [&TEST_A];
        let result = ImageCatalog::new(&adapters).identify(&ProbeInput {
            len: 0,
            prefix: &[],
            path: None,
        });
        let ImageIdentification::Match(found) = result else {
            panic!("test adapter was not selected");
        };
        assert_eq!(found.adapter.descriptor().id, "test-a");
    }

    #[test]
    fn equal_strongest_matches_remain_unknown_and_name_the_competitors() {
        let adapters: [&dyn ImageFormatAdapter; 2] = [&TEST_A, &TEST_B];
        let result = ImageCatalog::new(&adapters).identify(&ProbeInput {
            len: 0,
            prefix: &[],
            path: None,
        });
        let ImageIdentification::Unknown { evidence } = result else {
            panic!("tie must remain unknown");
        };
        let evidence = evidence.join("\n");
        assert!(evidence.contains("test-a"));
        assert!(evidence.contains("test-b"));
    }

    #[test]
    fn a_recognized_invalid_qcow2_keeps_its_refusal() {
        let bytes = [b'Q', b'F', b'I', 0xfb, 0, 0, 0, 9];
        let result = image_catalog().identify(&ProbeInput {
            len: bytes.len() as u64,
            prefix: &bytes,
            path: Some(Path::new("broken.qcow2")),
        });
        let ImageIdentification::Invalid {
            descriptor, reason, ..
        } = result
        else {
            panic!("recognized-invalid input must not become unknown");
        };
        assert_eq!(descriptor.id, "qcow2");
        assert!(
            reason.contains("too small")
                || reason.contains("unsupported")
                || reason.contains("newer")
        );
    }
}
