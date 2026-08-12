// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Internal adapter vocabulary and the built-in image-format catalog.
//!
//! Descriptors and executable behavior are deliberately inseparable: a
//! catalog contains adapters, never declarations that central code must
//! interpret. Each probe is bounded to the supplied prefix and source
//! metadata (P27).

use std::path::Path;

use crate::device::{Device, MediumDevice};
use crate::device_type::{DeviceType, FloppyDrive, HardDrive};
use crate::disk::DiskFormat;
use crate::error::{Error, ErrorCategory, Result};
use crate::fat::FatVolume;
use crate::filesystem_catalog;
use crate::mbr;
use crate::media::Format;
use crate::media_profile::{FLEXIBLE_5_25_HARD_10, LOGICAL_BLOCK_512, MediaProfile};
use crate::qcow2::{QCOW2_MAGIC, Qcow2, SUPPORTED_VERSION_CEILING};
use crate::source::{Evidence, SourceDevice};
use crate::vdi::{
    SIGNATURE_AT as VDI_SIGNATURE_AT, SUPPORTED_MAJOR as VDI_SUPPORTED_MAJOR,
    SUPPORTED_MINOR_CEILING as VDI_SUPPORTED_MINOR_CEILING, VDI_SIGNATURE,
    VERSION_AT as VDI_VERSION_AT, Vdi,
};

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

/// The recording geometry an image format declares for every image it
/// claims.
///
/// These are facts about the *recording*, not about the medium: what the
/// medium is remains the media profile the descriptor names, and the
/// two are kept apart because one disk carries different recordings
/// (P14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiskDescriptor {
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

/// The hard-drive types a block image is declared to record: the two the
/// MBR scheme's own adapter reads.
///
/// `gpt-hd` is enumerated in the device catalog and absent here, because
/// no adapter in this release reads a GPT — which is what makes
/// declaring it a named refusal rather than a silent reading of the
/// wrong table. The [`Format`] catalog reads this same list, so the
/// adapter's claim is stated once.
pub(crate) static RECORDED_HARD_DRIVES: [DeviceType; 2] = [
    DeviceType::HardDrive(HardDrive::MbrSector),
    DeviceType::HardDrive(HardDrive::MbrBlock),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageFormatDescriptor {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) extensions: &'static [&'static str],
    pub(crate) authoritative_layer: ImageLayer,
    pub(crate) initial_active_layer: ActiveLayer,
    /// The article an image of this format holds state for (P14).
    /// An adapter loads and saves media state, so it is the adapter
    /// that names the medium; it never restates the medium's own
    /// passive facts inline, which is how one disk comes to be
    /// described two ways by two formats.
    pub(crate) media: &'static MediaProfile,
    /// The device types this format's images are recorded by (P12).
    ///
    /// It is a **recording-side** fact and belongs here rather than on
    /// the article, which cannot honestly hold it: a ten-sector
    /// hard-sectored 5.25-inch disk is the article of more than one
    /// machine's drive, while an H8D records a Heathkit one.
    ///
    /// **One entry means the format carries it bare** — a declaration
    /// needs no second word, and a discovery over such an artifact
    /// knows what recorded it. Several mean the load declares which, and
    /// a type absent from this list is a named refusal at the load even
    /// where the class is right. Empty is an archive grammar, which
    /// records no device at all.
    ///
    /// Which devices a medium *could* go in is a different question
    /// entirely, derived by asking the device catalog what is served the
    /// article ([`DeviceType::accepting`]).
    pub(crate) devices: &'static [DeviceType],
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
    fn host_mut(&mut self) -> &mut MediumDevice;
    fn cache_snapshot(&self) -> Option<Vec<u64>>;
    fn restore_cache(&mut self, snapshot: Option<Vec<u64>>);
    fn format(&self) -> DiskFormat;
    fn presented_size(&self) -> u64;
}

#[derive(Debug)]
struct RawImage(MediumDevice);

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

    fn host_mut(&mut self) -> &mut MediumDevice {
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
struct Qcow2Image(Qcow2<MediumDevice>);

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

    fn host_mut(&mut self) -> &mut MediumDevice {
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
struct VdiImage(Vdi<MediumDevice>);

impl Device for VdiImage {
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

impl OpenedImage for VdiImage {
    fn device_mut(&mut self) -> &mut dyn Device {
        self
    }

    fn host_mut(&mut self) -> &mut MediumDevice {
        self.0.host_mut()
    }

    /// The driver holds no mapping of its own: a VDI's block map and its
    /// allocation count live in the file, so a commit that does not land
    /// has nothing here to put back.
    fn cache_snapshot(&self) -> Option<Vec<u64>> {
        None
    }

    fn restore_cache(&mut self, _snapshot: Option<Vec<u64>>) {}

    fn format(&self) -> DiskFormat {
        DiskFormat::Vdi {
            major: self.0.header().major,
            minor: self.0.header().minor,
        }
    }

    fn presented_size(&self) -> u64 {
        self.0.header().disk_size
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
        _source: &dyn Evidence,
        _evidence: &mut Vec<String>,
    ) -> Result<Vec<DetectedFilesystem>> {
        Ok(Vec::new())
    }

    /// Whether the evidence bears a caller's **declaration** that this
    /// is what the artifact is.
    ///
    /// It is the same reading the probe performs, asked the other way
    /// round: the probe picks among formats, and this checks one that
    /// was named. The default derives it from the probe, so a format
    /// with a signature refuses a declaration its evidence contradicts
    /// without stating the rule twice; a format whose claim is
    /// structural overrides it.
    fn verify(&self, input: &ProbeInput<'_>, named: &str) -> Result<()> {
        let descriptor = self.descriptor();
        match self.probe(input) {
            ProbeResult::Match { .. } => Ok(()),
            ProbeResult::Invalid {
                category, reason, ..
            } => Err(Error::categorized_image(category, descriptor.id, reason)),
            ProbeResult::NoMatch => Err(Error::invalid_image(
                descriptor.id,
                format!(
                    "{named} was declared '{}' and the evidence does not bear                      it: nothing in the artifact matches what a {} records",
                    descriptor.id, descriptor.name
                ),
            )),
        }
    }

    fn open_disk(&self, file: MediumDevice, path: Option<&Path>) -> Result<Box<dyn OpenedImage>>;
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
        mut file: MediumDevice,
        path: Option<&Path>,
    ) -> Result<(Box<dyn OpenedImage>, &'static ImageFormatDescriptor)> {
        let mut prefix = vec![0u8; file.len().min(512) as usize];
        if !prefix.is_empty() {
            file.read_at(0, &mut prefix)?;
        }
        let input = ProbeInput {
            len: file.len(),
            prefix: &prefix,
            path,
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
    // The H17's ten records to a track are the ten sector holes the
    // medium itself carries; the medium says how the revolution is
    // divided, and this format says what was recorded in each division.
    media: &FLEXIBLE_5_25_HARD_10,
    // The format is written against the published H17 conventions, so
    // the drive it records is stated rather than inferred: the same
    // article served a North Star MDS, and the medium cannot tell the
    // two apart.
    devices: &[DeviceType::Floppy(FloppyDrive::HeathH17)],
    disk: Some(DiskDescriptor {
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
        source: &dyn Evidence,
        _evidence: &mut Vec<String>,
    ) -> Result<Vec<DetectedFilesystem>> {
        let expected = H8D_DESCRIPTOR.disk.expect("H8D geometry").expected_size();
        let mut volume = SourceDevice(source);
        let found = filesystem_catalog::detect(&mut volume)?;
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

    fn open_disk(&self, file: MediumDevice, _path: Option<&Path>) -> Result<Box<dyn OpenedImage>> {
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
    media: &LOGICAL_BLOCK_512,
    // The format exists to record one machine's hard disk, which is the
    // declaration: a virtual machine's qcow2 is its drive, and the
    // logical-block medium inside it says nothing about that.
    devices: &RECORDED_HARD_DRIVES,
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
        source: &dyn Evidence,
        evidence: &mut Vec<String>,
    ) -> Result<Vec<DetectedFilesystem>> {
        let mut qcow2 = Qcow2::open(SourceDevice(source))?;
        let header = qcow2.header().clone();
        evidence.push(format!(
            "qcow2 version {}, virtual size {} bytes",
            header.version, header.virtual_size
        ));
        volumes_of(&mut qcow2, header.virtual_size, evidence)
    }

    fn open_disk(&self, file: MediumDevice, path: Option<&Path>) -> Result<Box<dyn OpenedImage>> {
        Ok(Box::new(Qcow2Image(crate::qcow2::open_chain(file, path)?)))
    }
}

/// Walks the volumes a block-family virtual disk composes, and the
/// filesystem recognized on each. This is a mechanism two image formats
/// demonstrate rather than one format's rule (P12): it takes a presented
/// device and its size, knows nothing about which image format produced
/// them, and leaves every format-specific observation to the adapter that
/// called it.
fn volumes_of(
    device: &mut dyn Device,
    virtual_size: u64,
    evidence: &mut Vec<String>,
) -> Result<Vec<DetectedFilesystem>> {
    let spans: Vec<(Option<u32>, u64, u64)> = match mbr::discover(device)? {
        mbr::Discovery::Blank => {
            evidence.push("virtual disk is blank (sector 0 all zero)".to_owned());
            Vec::new()
        }
        mbr::Discovery::BareVolume => {
            let volume = crate::volume::direct(crate::volume::AddressedRegion {
                device: DeviceIdentity::first(),
                offset: 0,
                length: virtual_size,
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
        let Ok(volume) = FatVolume::open(device, offset) else {
            continue;
        };
        let info = volume.recognized(device)?;
        let kind = info.kind.name();
        let name = match &info.label.name {
            Some(label) => format!("{kind} volume '{label}'"),
            None => format!("{kind} volume"),
        };
        let observation = match (&info.label.name, partition) {
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

pub(crate) struct VdiAdapter;

pub(crate) static VDI_ADAPTER: VdiAdapter = VdiAdapter;

static VDI_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
    id: "vdi",
    name: "VirtualBox disk image",
    extensions: &["vdi"],
    authoritative_layer: ImageLayer::Block,
    initial_active_layer: ActiveLayer::Block,
    media: &LOGICAL_BLOCK_512,
    // As qcow2: the format records a virtual machine's hard disk.
    devices: &RECORDED_HARD_DRIVES,
    disk: None,
};

impl ImageFormatAdapter for VdiAdapter {
    fn descriptor(&self) -> &'static ImageFormatDescriptor {
        &VDI_DESCRIPTOR
    }

    fn probe(&self, input: &ProbeInput<'_>) -> ProbeResult {
        // The signature sits past the format's creator line, so a prefix
        // too short to hold it has not been recognized either way.
        if input.prefix.len() < VDI_VERSION_AT + 4 {
            return ProbeResult::NoMatch;
        }
        if input.prefix[VDI_SIGNATURE_AT..VDI_SIGNATURE_AT + 4] != VDI_SIGNATURE {
            return ProbeResult::NoMatch;
        }
        let evidence = vec![format!(
            "matched 4-byte VDI signature at offset {VDI_SIGNATURE_AT}"
        )];
        // P8's version gate is first once the signature and version field
        // exist.
        let (major, minor) = crate::vdi::version(input.prefix);
        if major < VDI_SUPPORTED_MAJOR {
            return ProbeResult::Invalid {
                confidence: 100,
                evidence,
                category: ErrorCategory::Unsupported,
                reason: format!(
                    "unsupported VDI version {major}.{minor} (versions \
                     {VDI_SUPPORTED_MAJOR}.0 and \
                     {VDI_SUPPORTED_MAJOR}.{VDI_SUPPORTED_MINOR_CEILING} are supported)"
                ),
            };
        }
        if major > VDI_SUPPORTED_MAJOR || minor > VDI_SUPPORTED_MINOR_CEILING {
            return ProbeResult::Invalid {
                confidence: 100,
                evidence,
                category: ErrorCategory::Unsupported,
                reason: format!(
                    "VDI version {major}.{minor} is newer than this release \
                     supports (ceiling: version \
                     {VDI_SUPPORTED_MAJOR}.{VDI_SUPPORTED_MINOR_CEILING}); \
                     refusing to touch it"
                ),
            };
        }
        ProbeResult::Match {
            confidence: 100,
            evidence: vec![
                format!("matched 4-byte VDI signature at offset {VDI_SIGNATURE_AT}"),
                format!("recognized VDI version {major}.{minor}"),
            ],
        }
    }

    /// Identification is the artifact's own layers, and a differencing
    /// image identifies as the VDI it is (U5): the header is evidence
    /// before the chain is anyone's business, and walking the volumes
    /// inside one would need the parent, which only an open through a
    /// path can reach. The refusal that says so is what the session
    /// reports as contents not walked.
    fn identify_filesystems(
        &self,
        source: &dyn Evidence,
        evidence: &mut Vec<String>,
    ) -> Result<Vec<DetectedFilesystem>> {
        let mut device = SourceDevice(source);
        let header = crate::vdi::VdiHeader::parse(&mut device)?;
        evidence.push(format!(
            "VDI version {}.{}, {} image, virtual size {} bytes in {}-byte blocks",
            header.major,
            header.minor,
            header.image_type.name(),
            header.disk_size,
            header.block_size
        ));
        if header.image_type == crate::vdi::VdiImageType::Differencing {
            evidence.push(format!("declares parent image {}", header.parent_id));
        }
        let mut vdi = Vdi::open(device)?;
        volumes_of(&mut vdi, header.disk_size, evidence)
    }

    /// The whole differencing chain composes here (U6), which is why this
    /// adapter takes the path: the format names a parent by identity and
    /// never by path, so the file holding that identity is searched for
    /// relative to this one.
    fn open_disk(&self, file: MediumDevice, path: Option<&Path>) -> Result<Box<dyn OpenedImage>> {
        Ok(Box::new(VdiImage(crate::vdi::open_chain(file, path)?)))
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
    media: &LOGICAL_BLOCK_512,
    // A raw image is bytes and nothing else: it records no ecosystem, so
    // it declares no drive. This is the ordinary shape of `None`, not a
    // gap waiting to be filled in.
    devices: &RECORDED_HARD_DRIVES,
    disk: None,
};

impl ImageFormatAdapter for RawAdapter {
    fn descriptor(&self) -> &'static ImageFormatDescriptor {
        &RAW_DESCRIPTOR
    }

    fn probe(&self, _input: &ProbeInput<'_>) -> ProbeResult {
        ProbeResult::NoMatch
    }

    /// **Raw declares nothing, so nothing in an artifact can contradict
    /// it.** The reading is the bytes as they are, which every artifact
    /// bears; a caller who declares it is asking for exactly that, and
    /// the refusal that still applies — an artifact whose own adapter
    /// declares another family (P13) — belongs one seam up, where every
    /// declaration passes it.
    fn verify(&self, _input: &ProbeInput<'_>, _named: &str) -> Result<()> {
        Ok(())
    }

    fn open_disk(&self, file: MediumDevice, _path: Option<&Path>) -> Result<Box<dyn OpenedImage>> {
        Ok(Box::new(RawImage(file)))
    }
}

static BUILT_IN_IMAGE_ADAPTERS: [&dyn ImageFormatAdapter; 3] =
    [&H8D_ADAPTER, &QCOW2_ADAPTER, &VDI_ADAPTER];

pub(crate) fn image_catalog() -> ImageCatalog<'static> {
    ImageCatalog::new(&BUILT_IN_IMAGE_ADAPTERS)
}

/// The one adapter a declared block format names.
///
/// The enumeration is exhaustive over the block formats, so a format
/// this release does not read fails to compile rather than falling
/// through to a guess. The archive grammars are reached through their
/// own catalog: their vantage is a namespace, and no image adapter
/// opens one.
fn adapter_for(format: Format) -> &'static dyn ImageFormatAdapter {
    match format {
        Format::Raw { .. } => &RAW_ADAPTER,
        Format::Qcow2 { .. } => &QCOW2_ADAPTER,
        Format::Vdi { .. } => &VDI_ADAPTER,
        Format::H8d => &H8D_ADAPTER,
        Format::Zip | Format::SevenZip => {
            unreachable!("an archive grammar is opened by its own catalog")
        }
        Format::KryoFlux { .. } | Format::P64 => {
            unreachable!("a flux format builds a flux medium, and no image adapter opens one")
        }
    }
}

/// Opens `file` as the format the caller **declared**, checked by that
/// one adapter.
///
/// This is the declared reading and it does no probing: exactly one
/// adapter is consulted, it is asked whether the evidence bears the
/// declaration, and a refusal names both what was declared and what was
/// found. Nothing falls through to a second answer — picking among
/// formats is [`discover_media`](crate::discover_media)'s question, and
/// answering it here would be discovery wearing a declaration's clothes.
pub(crate) fn open_declared(
    format: Format,
    mut file: MediumDevice,
    path: Option<&Path>,
    named: &str,
) -> Result<(Box<dyn OpenedImage>, &'static ImageFormatDescriptor)> {
    let adapter = adapter_for(format);
    let mut prefix = vec![0u8; file.len().min(512) as usize];
    if !prefix.is_empty() {
        file.read_at(0, &mut prefix)?;
    }
    adapter.verify(
        &ProbeInput {
            len: file.len(),
            prefix: &prefix,
            path,
        },
        named,
    )?;
    Ok((adapter.open_disk(file, path)?, adapter.descriptor()))
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

        fn open_disk(
            &self,
            file: MediumDevice,
            _path: Option<&Path>,
        ) -> Result<Box<dyn OpenedImage>> {
            Ok(Box::new(RawImage(file)))
        }
    }

    static TEST_A_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
        id: "test-a",
        name: "Test A",
        extensions: &[],
        authoritative_layer: ImageLayer::Block,
        initial_active_layer: ActiveLayer::Block,
        media: &LOGICAL_BLOCK_512,
        devices: &[],
        disk: None,
    };
    static TEST_B_DESCRIPTOR: ImageFormatDescriptor = ImageFormatDescriptor {
        id: "test-b",
        name: "Test B",
        extensions: &[],
        authoritative_layer: ImageLayer::Block,
        initial_active_layer: ActiveLayer::Block,
        media: &LOGICAL_BLOCK_512,
        devices: &[],
        disk: None,
    };
    static TEST_A: SameProbe = SameProbe(&TEST_A_DESCRIPTOR);
    static TEST_B: SameProbe = SameProbe(&TEST_B_DESCRIPTOR);

    #[test]
    fn a_recording_follows_the_mediums_own_division_without_restating_it() {
        // P14: the medium says how a revolution is divided, and this
        // format says what was recorded in each division. The H17's ten
        // records to a track are the medium's ten sector holes, and the
        // two are checked against each other rather than one being
        // derived from the other — a format free to disagree with the
        // article it records on would be describing a different disk.
        let holes = H8D_DESCRIPTOR
            .media
            .flexible_magnetic()
            .expect("hard-sectored flexible media")
            .sectoring
            .sector_holes();
        assert_eq!(holes, 10);
        assert_eq!(
            H8D_DESCRIPTOR.disk.expect("H8D geometry").sectors_per_track,
            holes,
        );

        // And the block-active formats name the article their state
        // belongs to just as squarely: a virtual disk is logical-block
        // media, which is where "hard disk" stopped being said — that
        // is the device type that recorded it, not the article.
        for descriptor in [&QCOW2_DESCRIPTOR, &VDI_DESCRIPTOR, &RAW_DESCRIPTOR] {
            assert_eq!(descriptor.media.id, "logical-block-512");
            assert!(
                descriptor.media.flexible_magnetic().is_none(),
                "a logical-block medium answers no flexible-media question"
            );
        }
    }

    #[test]
    fn a_recorded_device_is_one_the_article_could_go_in() {
        // A format's recorded types are its own declaration, but they
        // are not free of the device catalog: a format naming a drive
        // its own article would never be served is describing two
        // different disks, and every load of it would be a lie.
        for descriptor in [
            &H8D_DESCRIPTOR,
            &QCOW2_DESCRIPTOR,
            &VDI_DESCRIPTOR,
            &RAW_DESCRIPTOR,
        ] {
            assert!(
                !descriptor.devices.is_empty(),
                "{} records no device, and only an archive grammar does",
                descriptor.id
            );
            for device in descriptor.devices {
                assert_eq!(
                    device.article_profile().id,
                    descriptor.media.id,
                    "{} says it records a {}, which is served {} rather than \
                     the {} the format loads",
                    descriptor.id,
                    device.id(),
                    device.article_profile().id,
                    descriptor.media.id
                );
                assert!(
                    DeviceType::accepting(descriptor.media).contains(device),
                    "a recorded device is one the device catalog derives from \
                     its own declarations, never a second answer"
                );
            }
        }

        // The two shapes the feature rests on, checked rather than
        // described: the format that records one ecosystem's disk
        // carries it bare, and the format that records a class leaves
        // the type to the caller.
        assert_eq!(
            H8D_DESCRIPTOR.devices,
            &[DeviceType::Floppy(FloppyDrive::HeathH17)]
        );
        assert_eq!(RAW_DESCRIPTOR.devices.len(), 2);
        assert!(
            !RAW_DESCRIPTOR
                .devices
                .contains(&DeviceType::HardDrive(HardDrive::Gpt)),
            "no adapter records a GPT drive, so declaring one refuses"
        );
    }

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
