// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Python bindings for the Remanence disk image analysis library.
//!
//! The module mirrors the Rust crate's public surface: a `Session` holds
//! `Machine`s, a machine holds `StorageDevice`s, and a device is the one
//! handle for a slot and the medium in it. Attaching a disk image
//! (optionally an entry inside a `.zip` or `.7z` archive) takes one P7
//! claim serving both of the medium's planes — `StorageDevice.identify()`
//! reports the layers of the artifact's nesting over the image's own
//! bytes, while `StorageDevice.inspect()` works over the disk a format
//! adapter presents above them. **File access lives on one node**:
//! `StorageDevice.filesystem()` resolves device to volume to filesystem
//! where every seam has one supported answer, `StorageDevice.volume(id)`
//! selects where several exist, and the verbs live on the `Filesystem`
//! that resolver hands back. `Archive` lists what a supported archive
//! holds.
//! Failures raise `RemanenceError`, which carries a stable `category`
//! saying how to behave and, where the refusal came from an enumerated rule
//! set such as the DOS 8.3 namespace's, a stable `rule` naming which rule
//! the input broke.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

create_exception!(
    remanence,
    RemanenceError,
    PyException,
    "Raised when the remanence library reports an error; `category` and \
     `rule` are stable. `category` says how to behave and is always set; \
     `rule` names which rule of an enumerated set the input broke — the DOS \
     8.3 namespace's rules are the set the file verbs draw on — and is None \
     where the refusal belongs to no such set."
);

/// A refusal raised by the binding itself. It belongs to no format's rule
/// set, so it carries no rule identity, and that absence is the ordinary
/// case rather than an omission.
fn categorized_py_err(category: remanence::ErrorCategory, message: impl Into<String>) -> PyErr {
    py_err(category, None, message)
}

fn py_err(
    category: remanence::ErrorCategory,
    rule: Option<remanence::RuleIdentity>,
    message: impl Into<String>,
) -> PyErr {
    let error = RemanenceError::new_err(message.into());
    Python::attach(|py| {
        let value = error.value(py);
        value
            .setattr("category", category.as_str())
            .expect("RemanenceError instances accept attributes");
        value
            .setattr("rule", rule)
            .expect("RemanenceError instances accept attributes");
    });
    error
}

fn to_py_err(error: remanence::Error) -> PyErr {
    py_err(error.category(), error.rule(), error.to_string())
}

fn kind_str(kind: remanence::LayerKind) -> &'static str {
    match kind {
        remanence::LayerKind::Archive => "archive",
        remanence::LayerKind::Image => "image",
        remanence::LayerKind::PhysicalMedia => "physical-media",
        remanence::LayerKind::Filesystem => "filesystem",
        remanence::LayerKind::Unknown => "unknown",
    }
}

/// Current and expected byte sizes, when known.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct SizeInformation {
    pub current_bytes: Option<u64>,
    pub expected_bytes: Option<u64>,
}

/// Where the image bytes came from inside an archive.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ArchiveLayout {
    /// Where the archive sits, where its own handle could be named.
    pub path: Option<String>,
    pub entry_name: String,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: Option<u64>,
}

/// Where the payload sits inside a raw image layer.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ImageLayout {
    pub payload_offset_bytes: Option<u64>,
    pub payload_length_bytes: Option<u64>,
}

/// Per-track sector geometry for variable layouts.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct TrackSectorLayout {
    pub cylinder: u32,
    pub side: u32,
    pub sectors: u32,
    pub sector_size: Option<u64>,
}

/// Physical disk geometry derived from an image format.
///
/// `sector_layout` is `"unknown"`, `"fixed"`, or `"variable"`;
/// `sectors_per_track` is set for fixed layouts and `tracks` for variable ones.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DiskLayout {
    pub media_type: String,
    pub sector_size: Option<u64>,
    pub cylinders: Option<u32>,
    pub sides: Option<u32>,
    pub sector_layout: String,
    pub sectors_per_track: Option<u32>,
    pub tracks: Vec<TrackSectorLayout>,
    pub total_sectors: Option<u64>,
}

impl DiskLayout {
    fn new(layout: &remanence::DiskLayout) -> Self {
        let (sector_layout, sectors_per_track, tracks) = match &layout.sectors {
            remanence::SectorLayout::Unknown => ("unknown", None, Vec::new()),
            remanence::SectorLayout::Fixed { sectors_per_track } => {
                ("fixed", Some(*sectors_per_track), Vec::new())
            }
            remanence::SectorLayout::Variable { tracks } => (
                "variable",
                None,
                tracks
                    .iter()
                    .map(|track| TrackSectorLayout {
                        cylinder: track.cylinder,
                        side: track.side,
                        sectors: track.sectors,
                        sector_size: track.sector_size,
                    })
                    .collect(),
            ),
        };

        Self {
            media_type: layout.media_type.clone(),
            sector_size: layout.sector_size,
            cylinders: layout.cylinders,
            sides: layout.sides,
            sector_layout: sector_layout.to_owned(),
            sectors_per_track,
            tracks,
            total_sectors: layout.total_sectors,
        }
    }
}

/// Where a filesystem sits inside the image.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct FilesystemLayout {
    pub offset_bytes: Option<u64>,
    pub length_bytes: Option<u64>,
}

/// One recognized layer of an artifact's nesting.
///
/// `kind` and `layout_kind` are `"archive"`, `"image"`, `"physical-media"`,
/// `"filesystem"`, or `"unknown"`. `layout` is the matching layout object —
/// `ArchiveLayout`, `ImageLayout`, `DiskLayout`, `FilesystemLayout` — or
/// `None` when no layout details are known.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct Layer {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub confidence: u8,
    pub known: bool,
    pub size: SizeInformation,
    pub layout_kind: String,
    pub layout: Option<Py<PyAny>>,
}

impl Layer {
    fn new(py: Python<'_>, layer: &remanence::Layer) -> PyResult<Self> {
        let (layout_kind, layout) = match &layer.layout {
            remanence::LayerLayout::Unknown => ("unknown", None),
            remanence::LayerLayout::Archive(layout) => (
                "archive",
                Some(
                    Py::new(
                        py,
                        ArchiveLayout {
                            path: layout
                                .path
                                .as_ref()
                                .map(|path| path.display().to_string()),
                            entry_name: layout.entry_name.clone(),
                            compressed_size: layout.compressed_size,
                            uncompressed_size: layout.uncompressed_size,
                        },
                    )?
                    .into_any(),
                ),
            ),
            remanence::LayerLayout::Image(layout) => (
                "image",
                Some(
                    Py::new(
                        py,
                        ImageLayout {
                            payload_offset_bytes: layout.payload_offset_bytes,
                            payload_length_bytes: layout.payload_length_bytes,
                        },
                    )?
                    .into_any(),
                ),
            ),
            remanence::LayerLayout::PhysicalMedia(layout) => match layout {
                remanence::PhysicalMediaLayout::Unknown => ("physical-media", None),
                remanence::PhysicalMediaLayout::Disk(disk) => (
                    "physical-media",
                    Some(Py::new(py, DiskLayout::new(disk))?.into_any()),
                ),
            },
            remanence::LayerLayout::Filesystem(layout) => (
                "filesystem",
                Some(
                    Py::new(
                        py,
                        FilesystemLayout {
                            offset_bytes: layout.offset_bytes,
                            length_bytes: layout.length_bytes,
                        },
                    )?
                    .into_any(),
                ),
            ),
        };

        Ok(Self {
            kind: kind_str(layer.kind).to_owned(),
            id: layer.id.clone(),
            name: layer.name.clone(),
            confidence: layer.confidence,
            known: layer.known,
            size: SizeInformation {
                current_bytes: layer.size.current_bytes,
                expected_bytes: layer.size.expected_bytes,
            },
            layout_kind: layout_kind.to_owned(),
            layout,
        })
    }
}

/// The result of identifying a session's image.
#[pyclass(frozen, get_all, module = "remanence")]
pub struct Identification {
    pub layers: Vec<Layer>,
    pub modified: bool,
    pub evidence: Vec<String>,
}

/// An open analysis session over one disk image.
/// The one addressed device the image adapter supplied. `id` is scoped to
/// this open (P21), unlike the layout-derived identities below.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DeviceInfo {
    pub id: u64,
    pub image_format: String,
    pub media_type: String,
    pub length_bytes: u64,
    pub authoritative_layer: String,
    pub active_layer: String,
}

/// A recognized partition schema on the device (P16). At most one, and it
/// agrees with the report's `content`.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct PartitionSchemaInfo {
    pub kind: String,
    pub evidence: Vec<String>,
    pub issues: Vec<String>,
}

/// One region a partition schema declares (P16). Every declared region is
/// reported, including one whose type this release declines to read, and a
/// region carrying an issue keeps its place so nothing behind it renumbers.
///
/// `declared_type_reading` says what the type value *declares*, in a
/// sentence fit to quote in a refusal, and is present whether or not
/// `claimed` is true. It describes the declaration, never the content.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct RegionInfo {
    /// Opaque, library-owned. Pass it back; never parse or build one.
    pub id: u64,
    pub declared_number: u32,
    /// How the schema places this region in its own vocabulary: for MBR,
    /// `"primary"` for one of the four slots and `"logical"` for an entry
    /// on the extended chain. A different axis from `role`: the extended
    /// partition is a primary slot whose role is structural.
    pub declared_placement: String,
    /// `"data"` or `"structure"`.
    pub role: String,
    pub declared_type: u8,
    pub declared_type_reading: String,
    pub claimed: bool,
    pub start_bytes: u64,
    pub length_bytes: u64,
    pub issue_category: Option<String>,
    pub issue: Option<String>,
}

/// One volume actually composed (P17). Filesystem recognition neither
/// creates a volume nor erases one: a volume whose filesystem is
/// unrecognized stays here, with the refusal at the filesystem seam.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct VolumeInfo {
    /// Opaque, library-owned. Pass it back; never parse or build one.
    pub id: u64,
    /// `"whole-device"` or `"regions"`.
    pub origin: String,
    /// The identities of the regions composed, empty for a whole-device
    /// volume.
    pub origin_regions: Vec<u64>,
    pub start_bytes: u64,
    pub length_bytes: u64,
    pub evidence: Vec<String>,
    pub issues: Vec<String>,
}

/// One source's own reading of a volume label, kept beside the answer as
/// evidence (P4). `stored` is `None` where the format gives this volume no
/// such field at all — a third state distinct from a field that is present
/// and blank (`""`).
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct LabelReading {
    /// The source, in the recognizing filesystem's own vocabulary:
    /// `"root-directory-entry"` or `"boot-record-field"` for FAT.
    pub source: String,
    pub stored: Option<String>,
}

/// A recognized volume's label, answered whole: `name` is the label, or
/// `None` for a volume that has none. The format's own spelling of
/// unlabeled is resolved where the format is known, so no consumer that
/// displays a drive compares strings to find that out, and nothing outside
/// `readings` may become a label.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct VolumeLabel {
    pub name: Option<String>,
    /// Which source decided the answer, `None` only where the volume
    /// carries no such source at all. A source that exists and says
    /// unlabeled is named here beside a `name` of `None`.
    pub answered_by: Option<String>,
    /// Every source read, in the order the filesystem's policy consults
    /// them.
    pub readings: Vec<LabelReading>,
}

fn volume_label(label: &remanence::VolumeLabel) -> VolumeLabel {
    VolumeLabel {
        name: label.name.clone(),
        answered_by: label.answered_by.clone(),
        readings: label
            .readings
            .iter()
            .map(|reading| LabelReading {
                source: reading.source.clone(),
                stored: reading.stored.clone(),
            })
            .collect(),
    }
}

/// What filesystem recognition found on one volume (P18). A record exists
/// wherever recognition was attempted, so a refusal has a home at the seam
/// that owns it: a refused attempt carries `kind = None` and says why in
/// `issues`, and the volume stands either way.
///
/// The geometry here is what the filesystem's own structures declare; it
/// manufactures no physical drive.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct FilesystemInfo {
    /// Opaque, library-owned. Pass it back; never parse or build one.
    pub id: u64,
    /// The identity of the volume this recognition was attempted on.
    pub volume: u64,
    pub kind: Option<String>,
    /// The label answer, or `None` where recognition was refused — the
    /// absence of a *filesystem*, never of a label.
    pub label: Option<VolumeLabel>,
    pub cluster_bytes: Option<u64>,
    pub cluster_count: Option<u64>,
    pub sectors_per_track: Option<u16>,
    pub heads: Option<u16>,
    pub cylinders: Option<u64>,
    pub evidence: Vec<String>,
    pub issues: Vec<String>,
}

/// The complete layered inspection of one disk.
///
/// `content` is a stated outcome — `"blank"`, `"schema"`,
/// `"direct-volume"`, or `"unknown-nonblank"` — rather than something to
/// reconstruct from which lists came back empty. List order is for stable
/// presentation and never supplies identity: every relationship is
/// traversed by the opaque identity the report issued.
#[pyclass(frozen, module = "remanence")]
pub struct DiskReport {
    #[pyo3(get)]
    pub device: DeviceInfo,
    #[pyo3(get)]
    pub content: String,
    /// Why no adapter claimed the content, for `"unknown-nonblank"` only.
    #[pyo3(get)]
    pub content_evidence: Option<String>,
    #[pyo3(get)]
    pub partition_schema: Option<PartitionSchemaInfo>,
    #[pyo3(get)]
    pub regions: Vec<RegionInfo>,
    #[pyo3(get)]
    pub volumes: Vec<VolumeInfo>,
    #[pyo3(get)]
    pub filesystems: Vec<FilesystemInfo>,
    /// The report as the core issued it, kept so a `DosMachine` can be
    /// asserted over the report a caller already holds rather than over a
    /// flattened copy of it. Not part of the Python surface.
    source: remanence::DiskReport,
}

#[pymethods]
impl DiskReport {
    /// The region this identity names, or `None`.
    fn region(&self, id: u64) -> Option<RegionInfo> {
        self.regions.iter().find(|r| r.id == id).cloned()
    }

    /// The volume this identity names, or `None`.
    fn volume(&self, id: u64) -> Option<VolumeInfo> {
        self.volumes.iter().find(|v| v.id == id).cloned()
    }

    /// The filesystem recognized on `volume`, or `None` where none was.
    /// Absence is an answer: the volume still exists.
    fn filesystem_on(&self, volume: u64) -> Option<FilesystemInfo> {
        self.filesystems
            .iter()
            .find(|f| f.volume == volume)
            .cloned()
    }

    /// How many volumes were composed, whatever was recognized on them.
    fn composed_volume_count(&self) -> usize {
        self.volumes.len()
    }

    /// How many volumes carry a filesystem the host actually read.
    /// Deliberately distinct from `composed_volume_count`: an unrecognized
    /// volume stays in the report rather than vanishing to keep one number
    /// correct.
    fn readable_filesystem_volume_count(&self) -> usize {
        self.filesystems
            .iter()
            .filter(|f| f.kind.is_some() && f.issues.is_empty())
            .count()
    }
}

/// One claimed DOS drive-letter assignment rule (P3).
///
/// The variants of DOS differ in exactly one place — what becomes of a
/// second primary DOS partition on one disk — and each rule is a claim
/// about the variants it names, not about every DOS that shipped.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DosAssignmentRule {
    /// The stable name, and what `DosMachine.compose` takes.
    pub name: String,
    /// What the rule says, fit to show a user beside the mapping.
    pub reading: String,
}

#[pymethods]
impl DosAssignmentRule {
    fn __repr__(&self) -> String {
        format!("DosAssignmentRule(name={:?})", self.name)
    }
}

/// One entry in the device-family catalog: what a machine's slots can be
/// (P32).
///
/// A family entry is as concrete as the machine fact it asserts, and
/// states what it is a kind of. **Interior names classify; only concrete
/// entries instantiate** — `Session.add_device` refuses the rest by name.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DeviceFamily {
    /// The stable spelling, and what `Session.add_device` takes.
    pub id: String,
    /// The name, fit to show a user beside the slot it fills.
    pub name: String,
    /// Where this entry's declaration came from.
    pub provenance: String,
    /// What this family is a kind of; `None` for the root of the lineage.
    pub kind_of: Option<String>,
    /// Whether a device of this family can exist in a machine.
    pub is_concrete: bool,
    /// The family half of every attachment identity in it — `"hdd"` for
    /// `"hdd0"`. `None` for an interior name, which names no slot.
    pub slot_prefix: Option<String>,
    /// The media types a device of this family accepts.
    pub accepted_media: Vec<String>,
    /// The drive profile this family claims as its recording path, or
    /// `None` where it claims none — ordinary, not deficient.
    pub flux_path: Option<String>,
}

#[pymethods]
impl DeviceFamily {
    fn __repr__(&self) -> String {
        format!("DeviceFamily(id={:?})", self.id)
    }
}

/// Every format a load may declare, by its stable spelling — the set a
/// declaration is checked against (P3). A word that names a kind rather
/// than one catalog entry is not among them and is refused by name.
#[pyfunction]
fn formats() -> Vec<(String, String)> {
    remanence::Format::ALL
        .iter()
        .map(|format| (format.id().to_owned(), format.name().to_owned()))
        .collect()
}

/// Every storage-device family this release enrols, interior names of the
/// lineage among them.
#[pyfunction]
fn device_families() -> Vec<DeviceFamily> {
    remanence::DeviceFamily::enrolled()
        .into_iter()
        .map(|family| DeviceFamily {
            id: family.id().to_owned(),
            name: family.name().to_owned(),
            provenance: family.provenance().to_owned(),
            kind_of: family.kind_of().map(|parent| parent.id().to_owned()),
            is_concrete: family.is_concrete(),
            slot_prefix: family.slot_prefix().map(str::to_owned),
            accepted_media: family
                .accepted_media()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            flux_path: family.flux_path().map(str::to_owned),
        })
        .collect()
}

/// Every DOS drive-letter assignment rule this release claims.
#[pyfunction]
fn dos_assignment_rules() -> Vec<DosAssignmentRule> {
    remanence::DosAssignmentRule::CLAIMED
        .iter()
        .map(|rule| DosAssignmentRule {
            name: rule.name().to_owned(),
            reading: rule.reading().to_owned(),
        })
        .collect()
}

/// One drive letter and what it names.
///
/// `outcome` is `"volume"`, `"declared-device"`, `"phantom"` or
/// `"undetermined"`. A `"volume"` names it by the opaque identity its own
/// inspection report issued — the value passed back into a file verb — and
/// an `"undetermined"` letter says in `reason` why the claimed rules could
/// not settle it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DriveMapping {
    /// The letter, without its colon.
    pub letter: String,
    pub outcome: String,
    /// `"floppy"`, `"fixed-disk"` or `"cd-rom"`, where the outcome names a
    /// device.
    pub device_kind: Option<String>,
    /// The slot or attachment order the caller asserted for it.
    pub device_index: Option<u32>,
    /// Opaque, library-owned. Pass it back; never parse or build one.
    pub volume: Option<u64>,
    /// The letter a phantom drive stands for.
    pub phantom_of: Option<String>,
    pub reason: Option<String>,
}

#[pymethods]
impl DriveMapping {
    fn __repr__(&self) -> String {
        format!(
            "DriveMapping(letter={:?}, outcome={:?})",
            self.letter, self.outcome
        )
    }
}

/// The mapping a rule established over asserted machine facts.
///
/// A letter absent from `mappings` is a letter the machine had no drive
/// at, which is different from a letter that exists and could not be
/// settled — that one is present and `"undetermined"`.
#[pyclass(frozen, get_all, module = "remanence")]
pub struct DriveMap {
    /// The rules applied: one where the caller stated the variant, and
    /// every claimed rule where it did not.
    pub applied_rules: Vec<String>,
    pub mappings: Vec<DriveMapping>,
    /// The asserted facts and the applied rules, travelling with the
    /// answer. **This is not evidence**: nothing here was read off a disk.
    pub provenance: Vec<String>,
}

impl DriveMap {
    /// Mirrors one composed mapping. Both composers — the asserted
    /// machine and the machine that reads its own device set — answer
    /// with the same records, because they answered with the same core
    /// type.
    fn new(map: &remanence::DriveMap) -> Self {
        Self {
            applied_rules: map
                .applied_rules
                .iter()
                .map(|rule| rule.name().to_owned())
                .collect(),
            mappings: map
                .mappings
                .iter()
                .map(|mapping| {
                    let (device, volume, phantom_of, reason) = match &mapping.outcome {
                        remanence::LetterOutcome::Volume { device, volume } => {
                            (Some(*device), Some(volume.value()), None, None)
                        }
                        remanence::LetterOutcome::DeclaredDevice { device } => {
                            (Some(*device), None, None, None)
                        }
                        remanence::LetterOutcome::Phantom { of } => {
                            (None, None, Some(of.to_string()), None)
                        }
                        remanence::LetterOutcome::Undetermined { reason } => {
                            (None, None, None, Some(reason.clone()))
                        }
                    };
                    DriveMapping {
                        letter: mapping.letter.to_string(),
                        outcome: mapping.outcome.name().to_owned(),
                        device_kind: device.map(|device| device.kind().to_owned()),
                        device_index: device.map(remanence::MachineDevice::index),
                        volume,
                        phantom_of,
                        reason,
                    }
                })
                .collect(),
            provenance: map.provenance.clone(),
        }
    }
}

#[pymethods]
impl DriveMap {
    /// What this letter names, or `None` where the machine had no drive at
    /// it. `"C"` and `"c"` ask the same question.
    fn letter(&self, letter: &str) -> Option<DriveMapping> {
        let wanted = letter.trim_end_matches(':').to_uppercase();
        self.mappings
            .iter()
            .find(|mapping| mapping.letter == wanted)
            .cloned()
    }

    /// How many letters the rules established — the count that excludes
    /// every undetermined one.
    fn established_count(&self) -> usize {
        self.mappings
            .iter()
            .filter(|mapping| mapping.outcome != "undetermined")
            .count()
    }

    fn __repr__(&self) -> String {
        format!(
            "DriveMap(applied_rules={:?}, letters={})",
            self.applied_rules,
            self.mappings.len()
        )
    }
}

enum AssertedDevice {
    Floppy {
        slot: u32,
        report: remanence::DiskReport,
    },
    FixedDisk {
        order: u32,
        report: remanence::DiskReport,
    },
    CdRom {
        order: u32,
        driver_letter: Option<char>,
    },
}

/// The machine facts a caller asserts, and the composer that maps DOS
/// drive letters over them.
///
/// A DOS machine persists no drive-letter map: its letters were assigned
/// at boot by a rule over the machine's own configuration, and nothing on
/// the disks records the result. So this composes the mapping from one
/// named assignment rule and the facts asserted here — medium, slot and
/// attachment order — over the inspection reports the caller already
/// holds. It opens no artifact, and every report stays the caller's.
#[pyclass(module = "remanence")]
pub struct DosMachine {
    devices: Vec<AssertedDevice>,
    conditions: Vec<remanence::ResidentCondition>,
}

impl DosMachine {
    /// Rebuilds the core machine over the stored facts. Every rule about
    /// what may be asserted lives in the core, so the assertions are
    /// replayed through it rather than re-checked here.
    fn build(&self) -> remanence::Result<remanence::DosMachine<'_>> {
        let mut machine = remanence::DosMachine::new();
        for device in &self.devices {
            match device {
                AssertedDevice::Floppy { slot, report } => machine.assert_floppy(*slot, report)?,
                AssertedDevice::FixedDisk { order, report } => {
                    machine.assert_fixed_disk(*order, report)?
                }
                AssertedDevice::CdRom {
                    order,
                    driver_letter,
                } => machine.assert_cdrom(*order, *driver_letter)?,
            }
        }
        for condition in &self.conditions {
            machine.declare(*condition);
        }
        Ok(machine)
    }

    /// Adds a fact and keeps it only if the core accepts it, so a refused
    /// assertion never half-lands.
    fn assert_device(&mut self, device: AssertedDevice) -> PyResult<()> {
        self.devices.push(device);
        match self.build() {
            Ok(_) => Ok(()),
            Err(error) => {
                self.devices.pop();
                Err(to_py_err(error))
            }
        }
    }
}

#[pymethods]
impl DosMachine {
    /// A machine with nothing asserted about it yet.
    #[new]
    fn new() -> Self {
        Self {
            devices: Vec::new(),
            conditions: Vec::new(),
        }
    }

    /// Asserts that the medium `report` inspects occupies floppy slot
    /// `slot` — 0 being `A:`. DOS letters two floppy slots, so a slot
    /// above 1 is refused by name, as is a slot already asserted.
    fn assert_floppy(&mut self, slot: u32, report: &DiskReport) -> PyResult<()> {
        self.assert_device(AssertedDevice::Floppy {
            slot,
            report: report.source.clone(),
        })
    }

    /// Asserts that the medium `report` inspects is the fixed disk
    /// attached at `order` — 0 being the first attached, which is the
    /// order DOS assigned letters in.
    fn assert_fixed_disk(&mut self, order: u32, report: &DiskReport) -> PyResult<()> {
        self.assert_device(AssertedDevice::FixedDisk {
            order,
            report: report.source.clone(),
        })
    }

    /// Asserts a CD-ROM drive at attachment order `order`. `letter` is
    /// where the caller declares the resident driver placed it; nothing on
    /// the disks records that, so an undeclared CD-ROM takes no letter
    /// rather than a guessed one.
    #[pyo3(signature = (order, *, letter = None))]
    fn assert_cdrom(&mut self, order: u32, letter: Option<&str>) -> PyResult<()> {
        let driver_letter = match letter {
            None => None,
            Some(letter) => {
                let mut letters = letter.chars();
                match (letters.next(), letters.next()) {
                    (Some(letter), None) => Some(letter),
                    _ => {
                        return Err(categorized_py_err(
                            remanence::ErrorCategory::Unsupported,
                            &format!(
                                "'{letter}' is not a drive letter; one is a \
                                 single letter A through Z"
                            ),
                        ));
                    }
                }
            }
        };
        self.assert_device(AssertedDevice::CdRom {
            order,
            driver_letter,
        })
    }

    /// Declares a runtime condition outside every claimed rule, by its
    /// stable spelling: `"lastdrive=<letter>"`, `"subst"`, `"join"`,
    /// `"assign"`, `"block-device-driver"`, `"network-redirector"`. The
    /// letters it could have changed come back undetermined.
    fn declare_condition(&mut self, condition: &str) -> PyResult<()> {
        let condition = remanence::ResidentCondition::parse(condition).map_err(to_py_err)?;
        if !self.conditions.contains(&condition) {
            self.conditions.push(condition);
        }
        Ok(())
    }

    /// Composes the mapping. `rule` names the variant the machine ran —
    /// one of `dos_assignment_rules()` — or is `None` where the caller
    /// states none, in which case every claimed rule is applied and a
    /// letter they disagree on comes back undetermined rather than settled
    /// by choosing the most common one.
    #[pyo3(signature = (rule = None))]
    fn compose(&self, rule: Option<&str>) -> PyResult<DriveMap> {
        let rule = rule
            .map(remanence::DosAssignmentRule::from_name)
            .transpose()
            .map_err(to_py_err)?;
        let map = self
            .build()
            .and_then(|machine| machine.compose(rule))
            .map_err(to_py_err)?;
        Ok(DriveMap::new(&map))
    }

    fn __repr__(&self) -> String {
        format!("DosMachine(devices={})", self.devices.len())
    }
}

/// A fact the recognizing filesystem declares about an entry that this
/// vocabulary has no named field for, in that filesystem's own spelling.
///
/// Nothing is normalized on the way through, so an HDOS catalog date
/// keeps HDOS's reading of it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct EntryFact {
    pub key: String,
    pub value: String,
}

#[pymethods]
impl EntryFact {
    fn __repr__(&self) -> String {
        format!("EntryFact(key={:?}, value={:?})", self.key, self.value)
    }
}

/// One entry of a namespace; `kind` is `"file"` or `"directory"`.
///
/// `declared` carries what the recognizing filesystem states beyond the
/// fields above, in its own spelling and order. A filesystem whose format
/// records none declares none.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub kind: String,
    pub size_bytes: u64,
    pub declared: Vec<EntryFact>,
}

#[pymethods]
impl Entry {
    fn __repr__(&self) -> String {
        format!(
            "Entry(name={:?}, kind={:?}, size_bytes={})",
            self.name, self.kind, self.size_bytes
        )
    }
}

impl Entry {
    fn new(entry: &remanence::Entry) -> Self {
        Self {
            name: entry.name.clone(),
            kind: entry.kind.name().to_owned(),
            size_bytes: entry.size_bytes,
            declared: entry
                .declared
                .iter()
                .map(|fact| EntryFact {
                    key: fact.key.clone(),
                    value: fact.value.clone(),
                })
                .collect(),
        }
    }
}

fn access_intent(writable: bool) -> remanence::AccessIntent {
    if writable {
        remanence::AccessIntent::Write
    } else {
        remanence::AccessIntent::Read
    }
}

fn mode_str(mode: remanence::AccessMode) -> &'static str {
    match mode {
        remanence::AccessMode::ReadWrite => "read-write",
        remanence::AccessMode::ReadOnly => "read-only",
    }
}

/// What one open established about the evidence beneath it (P28).
///
/// `outcome` is `"verified"` or `"degraded"`; the third outcome,
/// `"refused"`, arrives as a `RemanenceError` carrying the same condition
/// as its `rule`, so no open medium ever reports it. A degraded medium is
/// read-only for its whole life, states the shortfall in `evidence`, and
/// answers only for the extents in `readable` — an operation needing what
/// is missing is refused by name rather than clipped or zero-filled.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct Assurance {
    pub outcome: String,
    /// `"source-truncated"` or `"evidence-conflict"`, or None where
    /// nothing narrowed this session. It is the same identity a withheld
    /// operation's refusal carries as its `rule`.
    pub condition: Option<String>,
    /// Why, in the order the observations were made.
    pub evidence: Vec<String>,
    /// The exact extents of the medium that read, as half-open
    /// `(start, end)` byte pairs.
    pub readable: Vec<(u64, u64)>,
    /// The access this session actually has: `"read-write"` or
    /// `"read-only"`.
    pub access: String,
    /// Whose open this medium's P7 claim is: `"library-opened"` where
    /// the library opened the artifact and holds the denial itself, or
    /// `"caller-opened"` where the caller handed a handle over and what
    /// it affords is the whole of what the session has.
    pub claim: String,
    /// The size the interpretation declares, where one declares a size.
    pub declared_bytes: Option<u64>,
    /// The size the source actually holds.
    pub observed_bytes: Option<u64>,
    /// The first byte the source does not hold, where the session is
    /// bounded short of its declaration.
    pub first_unavailable_byte: Option<u64>,
}

impl Assurance {
    fn new(assurance: &remanence::Assurance) -> Self {
        Self {
            outcome: assurance.outcome.as_str().to_owned(),
            condition: assurance
                .condition
                .map(|condition| condition.as_str().to_owned()),
            evidence: assurance.evidence.clone(),
            readable: assurance
                .readable
                .iter()
                .map(|range| (range.start, range.end))
                .collect(),
            access: mode_str(assurance.access).to_owned(),
            claim: assurance.claim.as_str().to_owned(),
            declared_bytes: assurance.declared_bytes,
            observed_bytes: assurance.observed_bytes,
            first_unavailable_byte: assurance.first_unavailable_byte,
        }
    }
}

#[pymethods]
impl Assurance {
    fn __repr__(&self) -> String {
        format!(
            "Assurance(outcome={:?}, condition={:?})",
            self.outcome, self.condition
        )
    }
}

/// Every assurance condition this release claims (P3), so a caller can
/// hold every identity it may meet without waiting to meet one.
#[pyfunction]
fn assurance_conditions() -> Vec<String> {
    remanence::AssuranceCondition::ALL
        .iter()
        .map(|condition| condition.as_str().to_owned())
        .collect()
}

/// Identifies the artifact at `path` — a disk image, or
/// an archive — under the caller's declared intent, and answers
/// with what it is and where it could go.
///
/// It is on no handle at all: no session and no machine, because it
/// consults catalogs and evidence rather than configuration, and it
/// mutates nothing. The claim it takes is held by the returned
/// `Discovery` until that is consumed or dropped, so `writable=True`
/// claims the artifact exclusively and raises here when that claim
/// cannot be secured, never falling back.
#[pyfunction]
#[pyo3(signature = (path, *, writable, cache_bytes = None))]
fn discover_media(path: PathBuf, writable: bool, cache_bytes: Option<u64>) -> PyResult<Discovery> {
    let intent = access_intent(writable);
    let discovered = match cache_bytes {
        Some(cache_bytes) => remanence::discover_media_with_cache(path, intent, cache_bytes),
        None => remanence::discover_media(path, intent),
    };
    Ok(Discovery {
        inner: Mutex::new(Some(discovered.map_err(to_py_err)?)),
    })
}

/// What one artifact turned out to be, and the claim under which that
/// was established.
///
/// It is a handle rather than a record because it holds two things a
/// record could not: the claim on the artifact, and the work the
/// recognition already did. `StorageDevice.load_discovery` **consumes**
/// it — the state moves into the device, so nothing runs twice and the
/// claim never lapses between the question and the load — and every
/// attribute below raises by name once it has been.
#[pyclass(module = "remanence")]
pub struct Discovery {
    /// `None` once a load has consumed it.
    inner: Mutex<Option<remanence::Discovery>>,
}

impl Discovery {
    /// A discovery over one already minted by the core — the nested
    /// journey, where a file view named the artifact.
    fn over(discovery: remanence::Discovery) -> Self {
        Self {
            inner: Mutex::new(Some(discovery)),
        }
    }

    /// Reads one fact off the discovery, or refuses by name where a load
    /// has already taken it.
    fn read<T>(&self, read: impl FnOnce(&remanence::Discovery) -> T) -> PyResult<T> {
        let discovery = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match discovery.as_ref() {
            Some(discovery) => Ok(read(discovery)),
            None => Err(categorized_py_err(
                remanence::ErrorCategory::NotFound,
                "this discovery was consumed by a load; ask again with \
                 discover_media",
            )),
        }
    }

    /// Takes the discovery for the load that consumes it.
    fn take(&self) -> PyResult<remanence::Discovery> {
        let mut discovery = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        discovery.take().ok_or_else(|| {
            categorized_py_err(
                remanence::ErrorCategory::NotFound,
                "this discovery was consumed by a load; ask again with \
                 discover_media",
            )
        })
    }
}

#[pymethods]
impl Discovery {
    /// The artifact claimed — the archive itself for an image discovered
    /// inside one.
    #[getter]
    fn path(&self) -> PyResult<Option<String>> {
        self.read(|discovery| discovery.path().map(str::to_owned))
    }

    /// The resolved image — the entry name for an image inside an
    /// archive, else the source's own name. `None` where the artifact
    /// was reached through a handle this host cannot name.
    #[getter]
    fn image_path(&self) -> PyResult<Option<String>> {
        self.read(|discovery| {
            discovery
                .image_path()
                .map(|path| path.display().to_string())
        })
    }

    /// The image format's stable spelling — `"h8d"`, `"qcow2"`,
    /// `"vdi"`, `"raw"`.
    #[getter]
    fn image_format(&self) -> PyResult<String> {
        self.read(|discovery| discovery.image_format().to_owned())
    }

    /// The image format's name, fit to show a user.
    #[getter]
    fn image_format_name(&self) -> PyResult<String> {
        self.read(|discovery| discovery.image_format_name().to_owned())
    }

    /// `"raw"`, `"qcow2"` or `"vdi"` — the image container format, as
    /// `StorageDevice.format` reports it. A medium that is no disk
    /// image — an archive — refuses by name; its grammar is
    /// `image_format`.
    #[getter]
    fn format(&self) -> PyResult<&'static str> {
        let format = self.read(remanence::Discovery::format)?.map_err(to_py_err)?;
        Ok(match format {
            remanence::DiskFormat::Raw => "raw",
            remanence::DiskFormat::Qcow2 { .. } => "qcow2",
            remanence::DiskFormat::Vdi { .. } => "vdi",
        })
    }

    /// The **exact medium**, by the media-type catalog's stable
    /// spelling. The image-format adapter that loaded the state named
    /// it; nothing here guessed.
    #[getter]
    fn media_type(&self) -> PyResult<String> {
        self.read(|discovery| discovery.media_type().to_owned())
    }

    /// The medium's name, fit to show a user beside the drive it goes
    /// in.
    #[getter]
    fn media_type_name(&self) -> PyResult<String> {
        self.read(|discovery| discovery.media_type_name().to_owned())
    }

    /// Every concrete device family served this medium, by stable
    /// spelling — the answer to "where could this go?", derived from the
    /// families' own declarations. Empty means no drive this release
    /// claims takes it.
    #[getter]
    fn device_families(&self) -> PyResult<Vec<String>> {
        self.read(|discovery| {
            discovery
                .accepting_families()
                .iter()
                .map(|family| family.id().to_owned())
                .collect()
        })
    }

    /// The device family the **image format** declares for the disks it
    /// records — the answer to "where did this come from?" — or `None`
    /// where it declares none.
    ///
    /// `None` is ordinary: a raw image says nothing about its machine,
    /// and the caller then states the drive itself in the two acts.
    #[getter]
    fn default_device(&self) -> PyResult<Option<String>> {
        self.read(|discovery| {
            discovery
                .default_device()
                .map(|family| family.id().to_owned())
        })
    }

    /// The resolved image's own size in bytes — the raw plane, distinct
    /// from `size`.
    #[getter]
    fn image_size_bytes(&self) -> PyResult<u64> {
        self.read(remanence::Discovery::image_size_bytes)
    }

    /// The presented disk's size in bytes (the guest-visible size for
    /// qcow2).
    #[getter]
    fn size(&self) -> PyResult<u64> {
        self.read(remanence::Discovery::size)?.map_err(to_py_err)
    }

    /// `"read-write"` or `"read-only"` — the **effective** mode this
    /// discovery established, which a load consuming it inherits.
    #[getter]
    fn mode(&self) -> PyResult<&'static str> {
        self.read(|discovery| mode_str(discovery.mode()))
    }

    /// What this discovery established about the evidence beneath the
    /// medium, before anything is read.
    #[getter]
    fn assurance(&self) -> PyResult<Assurance> {
        self.read(|discovery| Assurance::new(discovery.assurance()))
    }

    /// Identifies the artifact's nesting layers and probable
    /// filesystem — the same reading `StorageDevice.identify` gives once
    /// a medium is loaded.
    fn identify(&self, py: Python<'_>) -> PyResult<Identification> {
        let identification = self.read(remanence::Discovery::identify)?;
        let layers = identification
            .layers
            .iter()
            .map(|layer| Layer::new(py, layer))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Identification {
            layers,
            modified: identification.modified,
            evidence: identification.evidence,
        })
    }

    fn __repr__(&self) -> String {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.as_ref() {
            Some(discovery) => format!(
                "Discovery(path={:?}, media_type={:?}, default_device={})",
                discovery.path(),
                discovery.media_type(),
                match discovery.default_device() {
                    Some(family) => format!("{:?}", family.id()),
                    None => "None".to_owned(),
                }
            ),
            None => "Discovery(consumed)".to_owned(),
        }
    }
}

/// An open session: the claim and cache scope, holding the machines
/// within it (P32).
///
/// A session holds machines; a machine holds devices. The attach verbs
/// here are the session's **anonymous machine** — the one whose identity
/// is null, which every session has exactly one of, and which behaves as
/// any other machine in every respect.
#[pyclass(module = "remanence")]
pub struct Session {
    inner: Arc<Mutex<remanence::Session>>,
}

#[pymethods]
impl Session {
    /// A session holding nothing but its anonymous machine. Machines and
    /// devices are added over its life; neither set is fixed at open.
    #[new]
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(remanence::Session::new())),
        }
    }

    /// Adds a machine carrying `identity` and returns it.
    ///
    /// An identity already in use is refused by name rather than
    /// resolving to the machine holding it, and the empty identity is
    /// refused too — the machine with no identity is this session's
    /// anonymous one, and there is exactly one of it.
    fn add_machine(&self, identity: &str) -> PyResult<Machine> {
        self.lock().add_machine(identity).map_err(to_py_err)?;
        Ok(Machine {
            session: Arc::clone(&self.inner),
            identity: Some(identity.to_owned()),
        })
    }

    /// Every machine identity in the session, in the order the machines
    /// were added; `None` is the anonymous machine's.
    #[getter]
    fn machines(&self) -> Vec<Option<String>> {
        self.lock()
            .machines()
            .iter()
            .map(|machine| machine.identity().map(str::to_owned))
            .collect()
    }

    /// The machine carrying `identity`, or the anonymous machine when
    /// `identity` is `None` — the anonymous machine being exactly the
    /// one whose identity is null.
    ///
    /// **`None` where the session holds no machine of that identity** —
    /// absence is an answer, not a manufactured error, and asking never
    /// creates one. The anonymous machine always answers, a session
    /// having exactly one of it.
    #[pyo3(signature = (identity = None))]
    fn machine(&self, identity: Option<&str>) -> Option<Machine> {
        if let Some(identity) = identity {
            self.lock().machine(identity)?;
        }
        Some(Machine {
            session: Arc::clone(&self.inner),
            identity: identity.map(str::to_owned),
        })
    }

    /// Adds a device of `family` (a family's stable spelling, such as
    /// `"hard-disk"`) to the session's anonymous machine, taking the
    /// lowest free slot of that family, and returns it — empty, until
    /// `StorageDevice.load_media` puts a medium in it.
    ///
    /// `slot` chooses the slot, never the name; a slot already taken is
    /// refused rather than displaced. A family this release does not
    /// claim, and an interior name of the lineage which classifies rather
    /// than instantiates, are both refused by name.
    #[pyo3(signature = (family, *, slot = None))]
    fn add_device(&self, family: &str, slot: Option<u32>) -> PyResult<StorageDevice> {
        let family = remanence::DeviceFamily::from_id(family).map_err(to_py_err)?;
        let mut session = self.lock();
        let added = match slot {
            Some(slot) => session.add_device_at(family, slot),
            None => session.add_device(family),
        };
        let attachment = added.map_err(to_py_err)?.attachment();
        drop(session);
        Ok(StorageDevice {
            session: Arc::clone(&self.inner),
            machine: None,
            attachment,
        })
    }

    /// Loads the caller's own opened artifact as the `format` they
    /// declare it to be, and returns the medium — **linked to nothing**.
    ///
    /// `source` is an open file: anything with a `fileno()` — the object
    /// `open(path, "rb")` returns — or a raw descriptor. The descriptor
    /// is **duplicated**, so closing the Python file afterwards leaves
    /// the medium's claim intact and the library closes its own copy
    /// when the medium is released.
    ///
    /// **Whoever opens owns the lock.** That open is the claim: the
    /// library checks it for exactly one thing — may it write through
    /// it? — honours the answer exactly, and never supplements it with a
    /// lock of its own. A name is recovered from the handle for location
    /// alone, under an identity check; a handle this host cannot name
    /// serves everything but the commit journal and a backing chain's
    /// parent, and refuses those two by name.
    ///
    /// The declaration is checked by that one format's own adapter and
    /// refused by name where the evidence cannot bear it. `format` is a
    /// stable spelling from `formats()`.
    #[pyo3(signature = (source, format, *, cache_bytes = None))]
    fn load_media(
        &self,
        source: &Bound<'_, PyAny>,
        format: &str,
        cache_bytes: Option<u64>,
    ) -> PyResult<Medium> {
        let format = remanence::Format::from_id(format).map_err(to_py_err)?;
        let file = duplicated_file(source)?;
        let mut session = self.lock();
        let loaded = match cache_bytes {
            Some(cache_bytes) => session.load_media_with_cache(file, format, cache_bytes),
            None => session.load_media(file, format),
        };
        let id = loaded.map_err(to_py_err)?.id();
        drop(session);
        Ok(Medium {
            session: Arc::clone(&self.inner),
            id,
        })
    }

    /// Loads the medium a `Discovery` already opened into this session's
    /// pool, **consuming the discovery**.
    ///
    /// This is the load that runs nothing twice: the discovery holds the
    /// claim taken when the artifact was identified and the work that
    /// identification did, and both move into the pool, so nothing can
    /// change the artifact between the question and the load. The
    /// intent, the cache bound and the assurance are the ones the
    /// discovery established.
    ///
    /// **The discovery is consumed either way** — a refused load
    /// releases its claim with it. Asking again is `discover_media`.
    fn load_discovery(&self, discovery: &Discovery) -> PyResult<Medium> {
        let discovered = discovery.take()?;
        let mut session = self.lock();
        let id = session
            .load_discovery(discovered)
            .map_err(to_py_err)?
            .id();
        drop(session);
        Ok(Medium {
            session: Arc::clone(&self.inner),
            id,
        })
    }

    /// Every medium identity this session holds, in the order they were
    /// loaded.
    #[getter]
    fn media(&self) -> Vec<u64> {
        self.lock()
            .media()
            .iter()
            .map(|id| id.value())
            .collect()
    }

    /// The medium `media_id` names, or **`None`** — absence is an
    /// answer, not a manufactured error.
    fn medium(&self, media_id: u64) -> Option<Medium> {
        let id = remanence::MediaId::from_value(media_id);
        self.lock().medium(id).map(|_| Medium {
            session: Arc::clone(&self.inner),
            id,
        })
    }

    /// **The one state-destroying verb.** It severs the medium's link if
    /// a device holds it, ends the claim, and discards everything
    /// uncommitted.
    ///
    /// Releasing is not a commit and never becomes one — buffered
    /// changes go with the medium, so a caller who wants them commits
    /// first.
    fn release_media(&self, media_id: u64) -> PyResult<()> {
        self.lock()
            .release_media(remanence::MediaId::from_value(media_id))
            .map_err(to_py_err)
    }

    /// Releases a machine and everything it configures, **taking no
    /// state with it**: every device is ejected first — severing, so each
    /// medium stays pooled — then the devices go, then the machine.
    fn release_machine(&self, identity: &str) -> PyResult<()> {
        self.lock().release_machine(identity).map_err(to_py_err)
    }

    /// Adds a device for the artifact at `path` to the session's
    /// anonymous machine, as `Machine.add_device_for` does there.
    #[pyo3(signature = (path, *, writable, cache_bytes = None))]
    fn add_device_for(
        &self,
        path: PathBuf,
        writable: bool,
        cache_bytes: Option<u64>,
    ) -> PyResult<StorageDevice> {
        let intent = access_intent(writable);
        let mut session = self.lock();
        let added = match cache_bytes {
            Some(cache_bytes) => session.add_device_for_with_cache(path, intent, cache_bytes),
            None => session.add_device_for(path, intent),
        };
        let attachment = added.map_err(to_py_err)?.attachment();
        drop(session);
        Ok(StorageDevice {
            session: Arc::clone(&self.inner),
            machine: None,
            attachment,
        })
    }

    /// Releases the device at `attachment` from the anonymous machine,
    /// as `Machine.release_device` does there.
    fn release_device(&self, attachment: &str) -> PyResult<()> {
        let attachment = remanence::AttachmentId::parse(attachment).map_err(to_py_err)?;
        self.lock().release_device(attachment).map_err(to_py_err)
    }

    /// The anonymous machine's attachment identities, in slot-fill
    /// order.
    #[getter]
    fn devices(&self) -> Vec<String> {
        self.lock()
            .attachments()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// The device at `attachment` in the session's anonymous machine —
    /// `Machine.device` reaches a named machine's. The session owns it;
    /// the returned object stays valid until that device is released.
    ///
    /// **`None` where nothing is attached there** — absence is an
    /// answer. An `attachment` that names no claimed slot at all is a
    /// different matter and raises, because that is a refusal rather
    /// than an empty slot.
    fn device(&self, attachment: &str) -> PyResult<Option<StorageDevice>> {
        let attachment = remanence::AttachmentId::parse(attachment).map_err(to_py_err)?;
        if self.lock().device(attachment).is_none() {
            return Ok(None);
        }
        Ok(Some(StorageDevice {
            session: Arc::clone(&self.inner),
            machine: None,
            attachment,
        }))
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exception_type: Bound<'_, PyAny>,
        _exception: Bound<'_, PyAny>,
        _traceback: Bound<'_, PyAny>,
    ) -> bool {
        *self.lock() = remanence::Session::new();
        false
    }
}

impl Session {
    fn lock(&self) -> MutexGuard<'_, remanence::Session> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// One machine in a session: a set of family-typed storage devices,
/// their attachment identities, and the order they were attached in.
///
/// The device set is the machine's own. Two machines in one session may
/// each hold an `"hdd0"`, and neither can reach the other's.
#[pyclass(module = "remanence")]
pub struct Machine {
    session: Arc<Mutex<remanence::Session>>,
    /// `None` for the session's anonymous machine.
    identity: Option<String>,
}

impl Machine {
    /// The machine this handle names, or a refusal where the session no
    /// longer holds it.
    ///
    /// The lookup answers with absence; this is the demand written over
    /// it, because a handle whose machine was released is a stale handle
    /// rather than a question about an identity.
    fn get<'a>(
        &self,
        session: &'a mut MutexGuard<'_, remanence::Session>,
    ) -> PyResult<remanence::MachineView<'a>> {
        match &self.identity {
            Some(identity) => session.machine_mut(identity).ok_or_else(|| {
                categorized_py_err(
                    remanence::ErrorCategory::NotFound,
                    format!("this session holds no machine identified '{identity}'"),
                )
            }),
            None => Ok(session.anonymous_mut()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, remanence::Session> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[pymethods]
impl Machine {
    /// This machine's identity, or `None` where it is the session's
    /// anonymous machine.
    #[getter]
    fn identity(&self) -> Option<String> {
        self.identity.clone()
    }

    /// Adds a device of `family` to this machine, taking the lowest free
    /// slot of that family — or `slot`, where the caller chooses one —
    /// and returns it, empty.
    #[pyo3(signature = (family, *, slot = None))]
    fn add_device(&self, family: &str, slot: Option<u32>) -> PyResult<StorageDevice> {
        let family = remanence::DeviceFamily::from_id(family).map_err(to_py_err)?;
        let mut session = self.lock();
        let mut machine = self.get(&mut session)?;
        let added = match slot {
            Some(slot) => machine.add_device_at(family, slot),
            None => machine.add_device(family),
        };
        let attachment = added.map_err(to_py_err)?.attachment();
        drop(session);
        Ok(StorageDevice {
            session: Arc::clone(&self.session),
            machine: self.identity.clone(),
            attachment,
        })
    }

    /// Adds a device of the artifact's **format-declared default
    /// family**, loads the medium at `path` into it, and returns that
    /// device.
    ///
    /// It is the one convenience over discovery, and it composes the two
    /// acts without changing the access path: one claim is held from the
    /// question to the load, and the device it answers with is an
    /// ordinary device in this machine's own set — a fresh one, never a
    /// slot already there.
    ///
    /// **A format that declares no default raises by name**, toward the
    /// two explicit acts (`add_device` then `StorageDevice.load_media`),
    /// naming the families the medium could go in. A refused call leaves
    /// no device behind.
    #[pyo3(signature = (path, *, writable, cache_bytes = None))]
    fn add_device_for(
        &self,
        path: PathBuf,
        writable: bool,
        cache_bytes: Option<u64>,
    ) -> PyResult<StorageDevice> {
        let intent = access_intent(writable);
        let mut session = self.lock();
        let mut machine = self.get(&mut session)?;
        let added = match cache_bytes {
            Some(cache_bytes) => machine.add_device_for_with_cache(path, intent, cache_bytes),
            None => machine.add_device_for(path, intent),
        };
        let attachment = added.map_err(to_py_err)?.attachment();
        drop(session);
        Ok(StorageDevice {
            session: Arc::clone(&self.session),
            machine: self.identity.clone(),
            attachment,
        })
    }

    /// Releases the device at `attachment` from this machine, **ejecting
    /// first**: the link is severed and the medium stays in the session's
    /// pool, claim and buffered changes intact. The slot is freed.
    ///
    /// Destroying a medium's state is `Session.release_media`, and it is
    /// the one verb that does. An `attachment` this machine holds no
    /// device at raises — a release names what resolves to nothing,
    /// unlike `device`, which answers `None`.
    fn release_device(&self, attachment: &str) -> PyResult<()> {
        let attachment = remanence::AttachmentId::parse(attachment).map_err(to_py_err)?;
        let mut session = self.lock();
        self.get(&mut session)?
            .release_device(attachment)
            .map_err(to_py_err)
    }

    /// Composes this machine's DOS drive-letter mapping from its **own
    /// device set**, reading attachment order from the order its devices
    /// were added rather than from an assertion (P32, P35).
    ///
    /// `rule` is a claimed rule's name, or `None` to apply every claimed
    /// rule and leave a letter they disagree on undetermined. Families no
    /// claimed rule letters are passed over by family, and the mapping's
    /// provenance says which.
    #[pyo3(signature = (rule = None))]
    fn compose_dos_letters(&self, rule: Option<&str>) -> PyResult<DriveMap> {
        let rule = rule
            .map(remanence::DosAssignmentRule::from_name)
            .transpose()
            .map_err(to_py_err)?;
        let mut session = self.lock();
        let map = self
            .get(&mut session)?
            .compose_dos_letters(rule, &[])
            .map_err(to_py_err)?;
        Ok(DriveMap::new(&map))
    }

    /// This machine's attachment identities, in slot-fill order — the
    /// attachment order a namespace composer reads.
    #[getter]
    fn devices(&self) -> PyResult<Vec<String>> {
        let mut session = self.lock();
        Ok(self
            .get(&mut session)?
            .attachments()
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    /// The device at `attachment` in this machine, or **`None` where
    /// this machine has no device there** — another machine's `"hdd0"`
    /// is not this one's. The session owns it; the returned object stays
    /// valid until that device is released.
    fn device(&self, attachment: &str) -> PyResult<Option<StorageDevice>> {
        let attachment = remanence::AttachmentId::parse(attachment).map_err(to_py_err)?;
        let mut session = self.lock();
        if self.get(&mut session)?.device(attachment).is_none() {
            return Ok(None);
        }
        Ok(Some(StorageDevice {
            session: Arc::clone(&self.session),
            machine: self.identity.clone(),
            attachment,
        }))
    }
}

/// One storage device — the durable slot and its family, and the link to
/// whichever pooled medium currently occupies it.
///
/// **The device is the slot, not the disk**: every content verb lives on
/// `Medium`, and this carries `insert`, `eject` and `medium` — the one
/// edge between a machine's configuration and the session's state.
#[pyclass(module = "remanence")]
pub struct StorageDevice {
    session: Arc<Mutex<remanence::Session>>,
    /// `None` for the session's anonymous machine.
    machine: Option<String>,
    attachment: remanence::AttachmentId,
}

/// A borrow of the session with one device selected.
///
/// It dereferences to the device, so every reader below reads as though
/// it held the device directly while the session stays the owner. The
/// device is re-resolved on each borrow, so a removed one refuses
/// rather than reaching freed state.
struct DeviceGuard<'a> {
    session: MutexGuard<'a, remanence::Session>,
    machine: Option<String>,
    attachment: remanence::AttachmentId,
}

impl std::ops::Deref for DeviceGuard<'_> {
    type Target = remanence::StorageDevice;

    fn deref(&self) -> &Self::Target {
        let machine = match &self.machine {
            Some(identity) => self.session.machine(identity),
            None => Some(self.session.anonymous()),
        };
        machine
            .and_then(|machine| machine.device(self.attachment))
            .expect("the device was present when this guard was taken")
    }
}

/// The same borrow with the session's media pool beside it — what the
/// edge verbs need, configuration being the machine's and state the
/// session's.
struct DeviceViewGuard<'a> {
    session: MutexGuard<'a, remanence::Session>,
    machine: Option<String>,
    attachment: remanence::AttachmentId,
}

impl DeviceViewGuard<'_> {
    fn view(&mut self) -> remanence::DeviceView<'_> {
        let machine = match &self.machine {
            Some(identity) => self.session.machine_mut(identity),
            None => Some(self.session.anonymous_mut()),
        };
        machine
            .and_then(|machine| machine.into_device(self.attachment))
            .expect("the device was present when this guard was taken")
    }
}

impl StorageDevice {
    fn present(&self) -> PyResult<MutexGuard<'_, remanence::Session>> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let present = match &self.machine {
            Some(identity) => session
                .machine(identity)
                .is_some_and(|machine| machine.device(self.attachment).is_some()),
            None => session.device(self.attachment).is_some(),
        };
        if !present {
            return Err(categorized_py_err(
                remanence::ErrorCategory::NotFound,
                "this device was released",
            ));
        }
        Ok(session)
    }

    fn get(&mut self) -> PyResult<DeviceGuard<'_>> {
        Ok(DeviceGuard {
            session: self.present()?,
            machine: self.machine.clone(),
            attachment: self.attachment,
        })
    }

    fn view(&mut self) -> PyResult<DeviceViewGuard<'_>> {
        Ok(DeviceViewGuard {
            session: self.present()?,
            machine: self.machine.clone(),
            attachment: self.attachment,
        })
    }
}

#[pymethods]
impl StorageDevice {
    /// This device's attachment identity — `"hdd0"` and the like.
    #[getter]
    fn attachment(&self) -> String {
        self.attachment.to_string()
    }

    /// The device family this slot belongs to, by its stable spelling.
    #[getter]
    fn family(&self) -> String {
        self.attachment.family().id().to_owned()
    }

    /// Whether a medium currently occupies the slot.
    #[getter]
    fn is_occupied(&mut self) -> PyResult<bool> {
        Ok(self.get()?.is_occupied())
    }

    /// The identity of the medium in this slot, or `None` while it is
    /// empty — absence being an answer rather than a manufactured error.
    #[getter]
    fn media_id(&mut self) -> PyResult<Option<u64>> {
        Ok(self.get()?.media_id().map(|id| id.value()))
    }

    /// The medium in this slot, or `None` while it is empty.
    #[getter]
    fn medium(&mut self) -> PyResult<Option<Medium>> {
        let id = self.get()?.media_id();
        Ok(id.map(|id| Medium {
            session: Arc::clone(&self.session),
            id,
        }))
    }

    /// Links the pooled medium `media_id` into this slot.
    ///
    /// **A device accepts only the media its family is served**, and a
    /// medium belonging in another drive is refused naming both sides. An
    /// identity the pool does not hold, a slot already occupied, and a
    /// medium another slot already holds are each refused by name.
    fn insert(&mut self, media_id: u64) -> PyResult<()> {
        let id = remanence::MediaId::from_value(media_id);
        self.view()?.view().insert(id).map_err(to_py_err)
    }

    /// **Severs the link and nothing more**: the device stays in its
    /// machine and the medium stays in the session's pool, its claim,
    /// its assurance and everything buffered intact.
    ///
    /// Ejecting is not a commit point and never becomes one. Destroying a
    /// medium's state is `Session.release_media`, and it is the one verb
    /// that does. Answers with the identity that left the slot.
    fn eject(&mut self) -> PyResult<u64> {
        Ok(self.view()?.view().eject().map_err(to_py_err)?.value())
    }
}

/// One loaded medium: the content handle, pool-owned and holdable.
///
/// Every content verb lives here. A medium answers whether or not a
/// device links it — a disk mastered out of an archive answers before any
/// machine exists to seat it — and the verbs a namespace-native medium
/// has no space for refuse by name.
#[pyclass(module = "remanence")]
pub struct Medium {
    session: Arc<Mutex<remanence::Session>>,
    id: remanence::MediaId,
}

/// A borrow of the session with one medium selected.
///
/// It dereferences to the medium, so every verb below reads as though it
/// held the medium directly while the session stays the owner. The medium
/// is re-resolved on each borrow, so a released one refuses rather than
/// reaching freed state.
struct MediumGuard<'a> {
    session: MutexGuard<'a, remanence::Session>,
    id: remanence::MediaId,
}

impl std::ops::Deref for MediumGuard<'_> {
    type Target = remanence::Medium;

    fn deref(&self) -> &Self::Target {
        self.session
            .medium(self.id)
            .expect("the medium was present when this guard was taken")
    }
}

impl std::ops::DerefMut for MediumGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.session
            .medium_mut(self.id)
            .expect("the medium was present when this guard was taken")
    }
}

impl Medium {
    fn get(&self) -> PyResult<MediumGuard<'_>> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if session.medium(self.id).is_none() {
            return Err(categorized_py_err(
                remanence::ErrorCategory::NotFound,
                "this medium was released",
            ));
        }
        Ok(MediumGuard {
            session,
            id: self.id,
        })
    }
}

#[pymethods]
impl Medium {
    /// This medium's identity in its session's pool.
    #[getter]
    fn id(&self) -> u64 {
        self.id.value()
    }

    /// Whether a device currently links this medium. An unlinked medium
    /// is ordinary rather than idle: it is loaded, claimed, and
    /// answering.
    #[getter]
    fn is_linked(&self) -> PyResult<bool> {
        Ok(self.get()?.is_linked())
    }

    /// The media type this medium is, by the catalog's stable spelling.
    #[getter]
    fn media_type(&self) -> PyResult<&'static str> {
        Ok(self.get()?.media_type())
    }

    /// The artifact the medium was loaded from (the archive itself for an
    /// image loaded out of one), or **`None` where the caller's handle
    /// has no recoverable name** — a name serves location alone.
    #[getter]
    fn path(&self) -> PyResult<Option<String>> {
        Ok(self.get()?.path().map(str::to_owned))
    }

    /// The resolved image path (the entry name for archive inputs), or
    /// `None` as above.
    #[getter]
    fn image_path(&self) -> PyResult<Option<String>> {
        Ok(self.get()?.image_path().map(|path| path.display().to_string()))
    }

    /// The resolved image's own size in bytes — the raw plane. Distinct
    /// from `size`, which is the presented disk's size; for a qcow2 the
    /// two differ.
    #[getter]
    fn image_size_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.image_size_bytes())
    }

    /// Reads `length` bytes of the resolved image at `offset` — the
    /// bounded access form: the image streams from its backing and is
    /// never resident whole.
    fn read_at<'py>(
        &self,
        py: Python<'py>,
        offset: u64,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut buffer = vec![0u8; length];
        self.get()?.read_at(offset, &mut buffer).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Identifies the artifact's nesting layers and probable filesystem.
    fn identify(&self, py: Python<'_>) -> PyResult<Identification> {
        let identification = self.get()?.identify();
        let layers = identification
            .layers
            .iter()
            .map(|layer| Layer::new(py, layer))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Identification {
            layers,
            modified: identification.modified,
            evidence: identification.evidence,
        })
    }

    /// `"read-write"` or `"read-only"` — the **effective** mode: what the
    /// claim afforded where the evidence supports it, and read-only where
    /// it does not. `assurance` says why.
    #[getter]
    fn mode(&self) -> PyResult<&'static str> {
        Ok(mode_str(self.get()?.mode()))
    }

    /// What this open established about the evidence beneath it,
    /// available before anything is read: the outcome, the condition
    /// where one narrowed the session, the ordered evidence, the exact
    /// extents that read, the access the evidence permits, and whose open
    /// the claim is.
    #[getter]
    fn assurance(&self) -> PyResult<Assurance> {
        Ok(Assurance::new(self.get()?.assurance()))
    }

    /// `"raw"`, `"qcow2"` or `"vdi"`.
    #[getter]
    fn format(&self) -> PyResult<&'static str> {
        Ok(match self.get()?.format().map_err(to_py_err)? {
            remanence::DiskFormat::Raw => "raw",
            remanence::DiskFormat::Qcow2 { .. } => "qcow2",
            remanence::DiskFormat::Vdi { .. } => "vdi",
        })
    }

    /// The qcow2 version, or `None` for an image of any other format.
    #[getter]
    fn qcow2_version(&self) -> PyResult<Option<u32>> {
        Ok(match self.get()?.format().map_err(to_py_err)? {
            remanence::DiskFormat::Qcow2 { version } => Some(version),
            remanence::DiskFormat::Raw | remanence::DiskFormat::Vdi { .. } => None,
        })
    }

    /// The VDI version as a `(major, minor)` pair, or `None` for an image
    /// of any other format.
    #[getter]
    fn vdi_version(&self) -> PyResult<Option<(u32, u32)>> {
        Ok(match self.get()?.format().map_err(to_py_err)? {
            remanence::DiskFormat::Vdi { major, minor } => Some((major, minor)),
            remanence::DiskFormat::Raw | remanence::DiskFormat::Qcow2 { .. } => None,
        })
    }

    /// The virtual disk size in bytes.
    #[getter]
    fn size(&self) -> PyResult<u64> {
        self.get()?.size().map_err(to_py_err)
    }

    /// Whether uncommitted changes exist.
    #[getter]
    fn is_modified(&self) -> PyResult<bool> {
        Ok(self.get()?.is_modified())
    }

    /// The layered inspection of this disk: the block-active device, what
    /// its leading structure turned out to be, any recognized partition
    /// schema, every declared region, every volume actually composed, and
    /// every filesystem recognition attempted on one.
    ///
    /// Each fact stays at the seam that owns it, and a failure at one seam
    /// neither erases a record another seam owns nor renumbers what
    /// follows. Content no adapter claims is an outcome here rather than a
    /// refusal; an image that cannot be *read* still raises.
    fn inspect(&self) -> PyResult<DiskReport> {
        let report = self.get()?.inspect().map_err(to_py_err)?;
        Ok(disk_report(report))
    }

    /// The filesystem this medium resolves to.
    ///
    /// The walk medium to volume to filesystem is transparent where every
    /// seam has exactly one supported answer, and raises naming the
    /// candidates where one does not. A volume bearing no filesystem is a
    /// named absence, not an empty listing. **The medium carries no file
    /// verbs of its own** — it may be asked what it resolves to, and may
    /// not be told to act as something it isn't.
    fn filesystem(&self) -> PyResult<StorageSpace> {
        let (kind, volume, start_bytes, length_bytes) = {
            let mut medium = self.get()?;
            let space = medium.filesystem().map_err(to_py_err)?;
            (
                space.kind().map_err(to_py_err)?.to_owned(),
                space.volume_id(),
                space.start_bytes().unwrap_or(0),
                space.length_bytes().unwrap_or(0),
            )
        };
        Ok(StorageSpace {
            session: Some(Arc::clone(&self.session)),
            media: Some(self.id),
            sectors: None,
            volume: volume.map(remanence::VolumeId::value),
            start_bytes,
            length_bytes,
            kind: Some(kind),
        })
    }

    /// One volume of this medium, by the identity the inspection report
    /// issued for it — the selector where several namespaces exist.
    fn volume(&self, volume_id: u64) -> PyResult<StorageSpace> {
        let (start_bytes, length_bytes, kind) = {
            let mut medium = self.get()?;
            let space = medium
                .volume(remanence::VolumeId::from_value(volume_id))
                .map_err(to_py_err)?;
            // A volume bearing no namespace is an ordinary volume, so the
            // absence travels on the space rather than failing here.
            let kind = space.kind().ok().map(str::to_owned);
            (
                space.start_bytes().unwrap_or(0),
                space.length_bytes().unwrap_or(0),
                kind,
            )
        };
        Ok(StorageSpace {
            session: Some(Arc::clone(&self.session)),
            media: Some(self.id),
            sectors: None,
            volume: Some(volume_id),
            start_bytes,
            length_bytes,
            kind,
        })
    }

    /// The commit point: everything buffered reaches the image, flushed.
    /// The commit is durable (P9): a private recovery journal is armed
    /// before the first byte of the file changes, so an interruption at
    /// any point leaves state the next open reconciles to wholly the old
    /// image or wholly the committed new one.
    ///
    /// The journal lands **beside the artifact**, so a medium whose source
    /// handle has no recoverable name refuses here by name rather than
    /// committing without it.
    fn commit(&self) -> PyResult<()> {
        self.get()?.commit().map_err(to_py_err)
    }

    /// Discards everything buffered; the image is untouched.
    fn rollback(&self) -> PyResult<()> {
        self.get()?.rollback().map_err(to_py_err)
    }
}

/// The layered inspection report, in the module's own records.
fn disk_report(report: remanence::DiskReport) -> DiskReport {
    let issues = |issues: &[remanence::Error]| -> Vec<String> {
        issues.iter().map(|issue| issue.to_string()).collect()
    };
    DiskReport {
        device: DeviceInfo {
            id: report.device.id,
            image_format: report.device.image_format.clone(),
            media_type: report.device.media_type.clone(),
            length_bytes: report.device.length_bytes,
            authoritative_layer: report.device.authoritative_layer.clone(),
            active_layer: report.device.active_layer.clone(),
        },
        content: report.content.name().to_owned(),
        content_evidence: match &report.content {
            remanence::DiskContent::UnknownNonblank { evidence } => Some(evidence.clone()),
            _ => None,
        },
        partition_schema: report
            .partition_schema
            .as_ref()
            .map(|schema| PartitionSchemaInfo {
                kind: schema.kind.clone(),
                evidence: schema.evidence.clone(),
                issues: issues(&schema.issues),
            }),
        regions: report
            .regions
            .iter()
            .map(|region| RegionInfo {
                id: region.id.value(),
                declared_number: region.declared_number,
                declared_placement: region.declared_placement.clone(),
                role: region.role.name().to_owned(),
                declared_type: region.declared_type,
                declared_type_reading: region.declared_type_reading.clone(),
                claimed: region.claimed,
                start_bytes: region.start_bytes,
                length_bytes: region.length_bytes,
                issue_category: region
                    .issue
                    .as_ref()
                    .map(|issue| issue.category().as_str().to_owned()),
                issue: region.issue.as_ref().map(|issue| issue.to_string()),
            })
            .collect(),
        volumes: report
            .volumes
            .iter()
            .map(|volume| VolumeInfo {
                id: volume.id.value(),
                origin: match &volume.origin {
                    remanence::VolumeOrigin::WholeDevice => "whole-device".to_owned(),
                    remanence::VolumeOrigin::Regions(_) => "regions".to_owned(),
                },
                origin_regions: match &volume.origin {
                    remanence::VolumeOrigin::WholeDevice => Vec::new(),
                    remanence::VolumeOrigin::Regions(regions) => {
                        regions.iter().map(|region| region.value()).collect()
                    }
                },
                start_bytes: volume.start_bytes,
                length_bytes: volume.length_bytes,
                evidence: volume.evidence.clone(),
                issues: issues(&volume.issues),
            })
            .collect(),
        filesystems: report
            .filesystems
            .iter()
            .map(|filesystem| FilesystemInfo {
                id: filesystem.id.value(),
                volume: filesystem.volume.value(),
                kind: filesystem.kind.clone(),
                label: filesystem.label.as_ref().map(volume_label),
                cluster_bytes: filesystem.cluster_bytes,
                cluster_count: filesystem.cluster_count,
                sectors_per_track: filesystem.declared_geometry.sectors_per_track,
                heads: filesystem.declared_geometry.heads,
                cylinders: filesystem.declared_geometry.cylinders,
                evidence: filesystem.evidence.clone(),
                issues: issues(&filesystem.issues),
            })
            .collect(),
        source: report,
    }
}

/// The caller's own open, duplicated so the library's claim survives the
/// Python object being closed.
///
/// `source` is anything with a `fileno()` — the object `open(path, "rb")`
/// returns — or a raw descriptor. Duplicating is what makes the two
/// lifetimes independent: the caller closes theirs when they like, and
/// the library closes its own when the medium is released.
fn duplicated_file(source: &Bound<'_, PyAny>) -> PyResult<std::fs::File> {
    let raw: i32 = match source.call_method0("fileno") {
        Ok(fileno) => fileno.extract()?,
        Err(_) => source.extract().map_err(|_| {
            categorized_py_err(
                remanence::ErrorCategory::Io,
                "the source must be an open file or a file descriptor",
            )
        })?,
    };
    duplicate_descriptor(raw)
}

#[cfg(windows)]
fn duplicate_descriptor(raw: i32) -> PyResult<std::fs::File> {
    use std::os::windows::io::{FromRawHandle, RawHandle};
    unsafe extern "C" {
        fn _get_osfhandle(fd: i32) -> isize;
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn DuplicateHandle(
            source_process: *mut std::ffi::c_void,
            source: *mut std::ffi::c_void,
            target_process: *mut std::ffi::c_void,
            target: *mut *mut std::ffi::c_void,
            desired_access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
    }
    const DUPLICATE_SAME_ACCESS: u32 = 0x2;
    let handle = unsafe { _get_osfhandle(raw) };
    if handle == -1 || handle == 0 {
        return Err(categorized_py_err(
            remanence::ErrorCategory::Io,
            "the source is not an open file",
        ));
    }
    let mut copy: *mut std::ffi::c_void = std::ptr::null_mut();
    let ok = unsafe {
        let process = GetCurrentProcess();
        DuplicateHandle(
            process,
            handle as *mut std::ffi::c_void,
            process,
            &raw mut copy,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(categorized_py_err(
            remanence::ErrorCategory::Io,
            "cannot duplicate the source handle",
        ));
    }
    Ok(unsafe { std::fs::File::from_raw_handle(copy as RawHandle) })
}

#[cfg(not(windows))]
fn duplicate_descriptor(raw: i32) -> PyResult<std::fs::File> {
    use std::os::fd::FromRawFd;
    unsafe extern "C" {
        fn dup(fd: i32) -> i32;
    }
    let copy = unsafe { dup(raw) };
    if copy < 0 {
        return Err(categorized_py_err(
            remanence::ErrorCategory::Io,
            "cannot duplicate the source descriptor",
        ));
    }
    Ok(unsafe { std::fs::File::from_raw_fd(copy) })
}

/// Resolves the named filesystem and runs `action` over it.
///
/// Every verb below passes through here, so the refusals a caller meets
/// are the library's own — the resolver's where the walk had no single
/// answer, the namespace's where it did — and a medium that has left
/// answers by name rather than through state that is gone.
fn with_filesystem<T>(
    session: Option<&Arc<Mutex<remanence::Session>>>,
    media: Option<remanence::MediaId>,
    volume: Option<u64>,
    sectors: Option<&Arc<remanence::C1541Sectors>>,
    action: impl FnOnce(&mut remanence::StorageSpace<'_>) -> remanence::Result<T>,
) -> PyResult<T> {
    // A namespace presented over a sector layer re-resolves from that
    // layer, exactly as a medium-backed one re-resolves from its medium.
    if let Some(sectors) = sectors {
        let mut space = sectors.filesystem().map_err(to_py_err)?;
        return action(&mut space).map_err(to_py_err);
    }
    let (Some(session), Some(media)) = (session, media) else {
        return Err(categorized_py_err(
            remanence::ErrorCategory::NotFound,
            "this namespace names no medium to be resolved through",
        ));
    };
    let mut guard = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(medium) = guard.medium_mut(media) else {
        return Err(categorized_py_err(
            remanence::ErrorCategory::NotFound,
            "the medium this namespace reads was released",
        ));
    };
    let mut space = match volume {
        Some(id) => medium
            .volume(remanence::VolumeId::from_value(id))
            .map_err(to_py_err)?,
        None => medium.filesystem().map_err(to_py_err)?,
    };
    action(&mut space).map_err(to_py_err)
}

/// The namespace node: the one type that carries file verbs.
///
/// It is a view over its provider's state, never an instance: every verb
/// reads or writes the state beneath it, mutations project into the
/// active layer, and nothing here holds a listing that could go stale.
#[pyclass(module = "remanence")]
pub struct StorageSpace {
    /// The session the medium bearing this namespace lives in, or
    /// `None` where no medium composed it.
    session: Option<Arc<Mutex<remanence::Session>>>,
    /// The medium this namespace was resolved through, or `None` where
    /// no medium composed it at all.
    media: Option<remanence::MediaId>,
    /// The sector layer it is presented over, where the flux family
    /// composed it: that family is reached through its own types rather
    /// than through a device, and the node still carries the file verbs.
    sectors: Option<Arc<remanence::C1541Sectors>>,
    /// The volume that composed it, where it has the addressable
    /// vantage. `None` where the medium bears its namespace directly.
    volume: Option<u64>,
    start_bytes: u64,
    length_bytes: u64,
    /// The namespace kind, where it has the namespace vantage.
    kind: Option<String>,
}

#[pymethods]
impl StorageSpace {
    /// Whether this space has the addressable vantage — an extent to
    /// read and write by position.
    #[getter]
    fn is_addressable(&self) -> bool {
        self.volume.is_some()
    }

    /// The identity of the volume that composed this space, or `None`
    /// where the medium bears its namespace directly.
    #[getter]
    fn volume_id(&self) -> Option<u64> {
        self.volume
    }

    /// Where this space starts in the presented disk, or `None` where it
    /// has no addressable vantage.
    #[getter]
    fn start_bytes(&self) -> Option<u64> {
        self.volume.map(|_| self.start_bytes)
    }

    /// How far this space runs, or `None` where it has no addressable
    /// vantage.
    #[getter]
    fn length_bytes(&self) -> Option<u64> {
        self.volume.map(|_| self.length_bytes)
    }

    /// Reads `length` bytes at `offset` **within this space**, not within
    /// the medium — the vantage that reaches a boot record, allocation
    /// metadata, or the extents a filesystem calls free. A read past this
    /// space's own end is refused by name.
    fn read_at(&self, py: Python<'_>, offset: u64, length: usize) -> PyResult<Py<PyBytes>> {
        let mut buf = vec![0_u8; length];
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |space| space.read_at(offset, &mut buf),
        )?;
        Ok(PyBytes::new(py, &buf).unbind())
    }

    /// Writes `data` at `offset` within this space, buffered until
    /// `commit` like every other write.
    fn write_at(&self, offset: u64, data: &[u8]) -> PyResult<()> {
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |space| space.write_at(offset, data),
        )
    }

    /// Whether this space has the namespace vantage — files to name.
    /// False for a volume bearing no filesystem, which is an ordinary
    /// volume.
    #[getter]
    fn has_namespace(&self) -> bool {
        self.kind.is_some()
    }

    /// The filesystem kind in its stable spelling — `"FAT12"`, `"hdos"` —
    /// or `None` where this space bears no namespace. It is data on the
    /// handle, never a type of its own.
    #[getter]
    fn kind(&self) -> Option<String> {
        self.kind.clone()
    }

    /// The label the recognizing filesystem read, answered whole — the
    /// name, which source decided it, and every source it consulted.
    ///
    /// `None` where the namespace's format carries no such field at all,
    /// which is a different fact from a field that is present and blank;
    /// the readings say which it was.
    fn label(&self) -> PyResult<Option<VolumeLabel>> {
        Ok(with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.label(),
        )?
        .as_ref()
        .map(volume_label))
    }

    /// What recognized this namespace, in human-readable terms — a
    /// verdict without the observations that produced it is not an
    /// answer.
    fn evidence(&self) -> PyResult<Vec<String>> {
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.evidence(),
        )
    }

    /// Lists a directory (`""` is the root, `"A/B"` descends).
    #[pyo3(signature = (path = ""))]
    fn entries(&self, path: &str) -> PyResult<Vec<Entry>> {
        let entries = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.entries(path),
        )?;
        Ok(entries.iter().map(Entry::new).collect())
    }

    /// Answers one path with its entry, or `None` when nothing exists
    /// there — a missing leaf, a missing parent, or a parent that is a
    /// file alike. Absence is an answer, distinguished from failure,
    /// which raises.
    fn stat(&self, path: &str) -> PyResult<Option<Entry>> {
        let entry = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.stat(path),
        )?;
        Ok(entry.as_ref().map(Entry::new))
    }

    /// The file at `path`.
    ///
    /// This is where absence stops being an answer: `stat` asks whether
    /// something is there, and this asks for the file, so nothing and a
    /// directory both raise by name.
    fn get_file(&self, path: &str) -> PyResult<File> {
        let entry = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| Ok(filesystem.get_file(path)?.entry().clone()),
        )?;
        Ok(File {
            session: self.session.clone(),
            media: self.media,
            sectors: self.sectors.clone(),
            volume: self.volume,
            path: path.to_owned(),
            entry: Entry::new(&entry),
        })
    }

    /// Copies a file's bytes out — the whole-value convenience beside
    /// `File.read_at`.
    fn read_file<'py>(&self, py: Python<'py>, path: &str) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.read_file(path),
        )?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Writes a file. An existing file is overwritten — shorter or
    /// longer, its old clusters released and reclaimed — while an
    /// existing directory is refused. Buffered until
    /// `StorageDevice.commit()`.
    fn write_file(&self, path: &str, contents: &[u8]) -> PyResult<()> {
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.write_file(path, contents),
        )
    }

    /// Sets a file's size, creating it when absent — `truncate`-shaped:
    /// kept bytes preserved in place, a grown region reads as zeros.
    /// Buffered until commit.
    fn resize_file(&self, path: &str, size: u64) -> PyResult<()> {
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.resize_file(path, size),
        )
    }

    /// Ensures a directory exists: missing parents are created, and a
    /// path that already leads to one succeeds unchanged. Buffered until
    /// commit.
    fn make_directory(&self, path: &str) -> PyResult<()> {
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.make_directory(path),
        )
    }

    fn __repr__(&self) -> String {
        match self.volume {
            Some(volume) => format!("Filesystem(kind={:?}, volume_id={volume})", self.kind),
            None => format!("Filesystem(kind={:?}, volume_id=None)", self.kind),
        }
    }
}

/// One file, named by the filesystem that holds it.
///
/// It is never an instance: the bytes stay where they are and this offers
/// the two ways of reaching them — `read_at`, the bounded streamed form,
/// and `bytes()`, the whole-value convenience beside it.
#[pyclass(module = "remanence")]
pub struct File {
    session: Option<Arc<Mutex<remanence::Session>>>,
    media: Option<remanence::MediaId>,
    /// As on `StorageSpace`: the sector layer a flux-family namespace is
    /// presented over, where no device composed it.
    sectors: Option<Arc<remanence::C1541Sectors>>,
    volume: Option<u64>,
    path: String,
    entry: Entry,
}

#[pymethods]
impl File {
    /// The path this file was reached by.
    #[getter]
    fn path(&self) -> String {
        self.path.clone()
    }

    /// The name as the filesystem stores it, which is not always the
    /// spelling the caller asked by.
    #[getter]
    fn name(&self) -> String {
        self.entry.name.clone()
    }

    /// What the filesystem claims this file's size is.
    #[getter]
    fn size_bytes(&self) -> u64 {
        self.entry.size_bytes
    }

    /// This file's entry, declared facts included.
    #[getter]
    fn entry(&self) -> Entry {
        self.entry.clone()
    }

    /// Opens this file as an artifact of its own, returning the
    /// `Discovery` a device loads it from.
    ///
    /// **Recursion is the same journey again.** An entry recognized as
    /// an image is not read through the namespace that names it: it is
    /// loaded into a device of its own — in a machine of its own where
    /// one is being reconstructed. The claim is the one the archive
    /// already holds, so nothing is re-opened.
    ///
    /// This release mints a discovery from an **archive entry**; a file
    /// on a volume-backed filesystem is refused by name.
    fn discover(&self) -> PyResult<Discovery> {
        let path = self.path.clone();
        let discovery = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.get_file(&path)?.discover(),
        )?;
        Ok(Discovery::over(discovery))
    }

    /// The whole file, copied out.
    fn bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let path = self.path.clone();
        let bytes = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.get_file(&path)?.bytes(),
        )?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Exactly `length` bytes at `offset` — the streamed form,
    /// `os.pread`-shaped. The span must lie within the file.
    fn read_at<'py>(
        &self,
        py: Python<'py>,
        offset: u64,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let path = self.path.clone();
        let mut buffer = vec![0u8; length];
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.get_file(&path)?.read_at(offset, &mut buffer),
        )?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Writes `data` at `offset` in place — the streamed form,
    /// `os.pwrite`-shaped. The span must lie within the file's current
    /// size; `Filesystem.resize_file` is what changes it. Buffered until
    /// commit.
    fn write_at(&self, offset: u64, data: &[u8]) -> PyResult<()> {
        let path = self.path.clone();
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.volume,
            self.sectors.as_ref(),
            |filesystem| filesystem.get_file(&path)?.write_at(offset, data),
        )
    }

    fn __repr__(&self) -> String {
        format!(
            "File(path={:?}, size_bytes={})",
            self.path, self.entry.size_bytes
        )
    }
}

/// A capture's declared timing basis: an exact count of ticks per
/// second, as a ratio, because the common capture clocks are not exactly
/// representable any other way.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct TimeBaseReport {
    pub ticks_per_second_numerator: u64,
    pub ticks_per_second_denominator: u64,
}

#[pymethods]
impl TimeBaseReport {
    fn __repr__(&self) -> String {
        format!(
            "TimeBaseReport({}/{} Hz)",
            self.ticks_per_second_numerator, self.ticks_per_second_denominator
        )
    }
}

/// A source's own drive-step position, held exactly. Sources step in
/// fractions, so this is a ratio and never a rounded whole number.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct StepPosition {
    pub numerator: u64,
    pub denominator: u64,
}

#[pymethods]
impl StepPosition {
    fn __repr__(&self) -> String {
        format!("StepPosition({}/{})", self.numerator, self.denominator)
    }
}

/// Something qualified about a member, recorded rather than repaired.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct CaptureIssue {
    /// The adapter's stable spelling for this kind of issue.
    pub code: String,
    pub detail: String,
}

#[pymethods]
impl CaptureIssue {
    fn __repr__(&self) -> String {
        format!("CaptureIssue(code={:?})", self.code)
    }
}

/// One circular observation bounded out of a capture run.
///
/// It reports the observation's shape, never its pulses: the evidence
/// stays behind this surface.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ObservationReport {
    /// Its place in this location's source-record order — not a rank,
    /// and no claim that it is a good or complete revolution.
    pub ordinal: u64,
    /// The declared circumference, in the capture's own ticks.
    pub span_ticks: u64,
    pub transitions: u64,
    pub markers: u64,
}

#[pymethods]
impl ObservationReport {
    fn __repr__(&self) -> String {
        format!(
            "ObservationReport(ordinal={}, span_ticks={}, transitions={})",
            self.ordinal, self.span_ticks, self.transitions
        )
    }
}

/// One source transfer, as the set holds it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct CaptureRunReport {
    pub ordinal: u64,
    pub transitions: u64,
    /// The last transition's tick: the extent of what was recorded, not
    /// a circumference. A run states no period.
    pub extent_ticks: u64,
    pub markers: u64,
    pub index_markers: u64,
    /// The result the capture tool declared for this transfer, where it
    /// declared one. Zero is a clean read.
    pub transfer_result: Option<u32>,
    /// Transitions recorded before the first index and after the last:
    /// evidence bounding into circular observations does not consume.
    pub transitions_before_first_index: u64,
    pub transitions_after_last_index: u64,
    pub observations: Vec<ObservationReport>,
}

#[pymethods]
impl CaptureRunReport {
    fn __repr__(&self) -> String {
        format!(
            "CaptureRunReport(ordinal={}, transitions={}, index_markers={})",
            self.ordinal, self.transitions, self.index_markers
        )
    }
}

/// One member of the set, and everything read out of it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct CaptureSetMember {
    /// The catalog's own identity for this member.
    pub entry_name: String,
    pub entry_bytes: u64,
    pub position: StepPosition,
    /// The head that captured this position. `None` is a source that
    /// numbers no head, which is a different fact from head zero.
    pub head: Option<u64>,
    pub runs: Vec<CaptureRunReport>,
    pub issues: Vec<CaptureIssue>,
}

#[pymethods]
impl CaptureSetMember {
    fn __repr__(&self) -> String {
        format!(
            "CaptureSetMember(entry_name={:?}, position={}/{}, head={})",
            self.entry_name,
            self.position.numerator,
            self.position.denominator,
            self.head
                .map_or_else(|| "None".to_owned(), |head| head.to_string())
        )
    }
}

/// The set as the adapter recognized it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct CaptureSetReport {
    pub format_id: String,
    pub format_name: String,
    pub time_base: TimeBaseReport,
    pub members: Vec<CaptureSetMember>,
    /// How the set was recognized, in human-readable terms.
    pub evidence: Vec<String>,
}

#[pymethods]
impl CaptureSetReport {
    fn __repr__(&self) -> String {
        format!(
            "CaptureSetReport(format_id={:?}, members={})",
            self.format_id,
            self.members.len()
        )
    }
}

impl CaptureSetReport {
    fn new(report: &remanence::CaptureSetReport) -> Self {
        Self {
            format_id: report.format_id.clone(),
            format_name: report.format_name.clone(),
            time_base: TimeBaseReport {
                ticks_per_second_numerator: report.time_base.ticks_per_second_numerator,
                ticks_per_second_denominator: report.time_base.ticks_per_second_denominator,
            },
            members: report
                .members
                .iter()
                .map(|member| CaptureSetMember {
                    entry_name: member.entry_name.clone(),
                    entry_bytes: member.entry_bytes,
                    position: StepPosition {
                        numerator: member.position.numerator,
                        denominator: member.position.denominator,
                    },
                    head: member.head,
                    runs: member
                        .runs
                        .iter()
                        .map(|run| CaptureRunReport {
                            ordinal: run.ordinal,
                            transitions: run.transitions,
                            extent_ticks: run.extent_ticks,
                            markers: run.markers,
                            index_markers: run.index_markers,
                            transfer_result: run.transfer_result,
                            transitions_before_first_index: run.transitions_before_first_index,
                            transitions_after_last_index: run.transitions_after_last_index,
                            observations: run
                                .observations
                                .iter()
                                .map(|observation| ObservationReport {
                                    ordinal: observation.ordinal,
                                    span_ticks: observation.span_ticks,
                                    transitions: observation.transitions,
                                    markers: observation.markers,
                                })
                                .collect(),
                        })
                        .collect(),
                    issues: member
                        .issues
                        .iter()
                        .map(|issue| CaptureIssue {
                            code: issue.code.clone(),
                            detail: issue.detail.clone(),
                        })
                        .collect(),
                })
                .collect(),
            evidence: report.evidence.clone(),
        }
    }
}

/// One KryoFlux capture set, opened from a catalog subtree.
///
/// A capture of a disk is one stream file per head per drive-step
/// position, and the logical capture is all of them together. Opening
/// claims the archive — writes denied to every other process — decodes
/// every member once into private session storage, and holds the claim
/// until the object is closed or dropped. An incomplete, duplicate,
/// contradictory, or unrelated member refuses the whole set by name.
#[pyclass(module = "remanence")]
pub struct CaptureSet {
    inner: Option<remanence::CaptureSet>,
}

impl CaptureSet {
    fn get(&self) -> PyResult<&remanence::CaptureSet> {
        self.inner.as_ref().ok_or_else(|| {
            categorized_py_err(remanence::ErrorCategory::Io, "capture set is closed")
        })
    }
}

#[pymethods]
impl CaptureSet {
    /// Opens the capture set held by `path` — an archive this library
    /// reads, optionally followed by the subtree inside it that holds
    /// the members. `cache_bytes` declares the session working set; the
    /// bound narrows what stays resident and never refuses service.
    #[new]
    #[pyo3(signature = (path, *, cache_bytes = None))]
    fn new(path: PathBuf, cache_bytes: Option<u64>) -> PyResult<Self> {
        let opened = match cache_bytes {
            Some(cache_bytes) => remanence::CaptureSet::open_with_cache(path, cache_bytes),
            None => remanence::CaptureSet::open(path),
        };
        opened
            .map(|inner| Self { inner: Some(inner) })
            .map_err(to_py_err)
    }

    /// The path the set was opened from.
    #[getter]
    fn path(&self) -> PyResult<String> {
        Ok(self.get()?.path().display().to_string())
    }

    /// The subtree inside the archive the members were read from, or
    /// `None` when the whole archive is the set.
    #[getter]
    fn subtree(&self) -> PyResult<Option<String>> {
        Ok(self.get()?.subtree().map(str::to_owned))
    }

    /// The capture format's stable identifier: `"kryoflux"`.
    #[getter]
    fn format_id(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_id())
    }

    /// The capture format's human-readable name.
    #[getter]
    fn format_name(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_name())
    }

    /// The archive grammar the members were read through.
    #[getter]
    fn archive_format_id(&self) -> PyResult<&'static str> {
        Ok(self.get()?.archive_format_id())
    }

    /// `"read-write"` or `"read-only"`: which mode the deny-write claim
    /// on the archive file was obtained in.
    #[getter]
    fn access_mode(&self) -> PyResult<&'static str> {
        Ok(mode_str(self.get()?.access_mode()))
    }

    /// How many bytes of private session storage the decoded capture
    /// occupies.
    #[getter]
    fn backing_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.backing_bytes())
    }

    /// How much of that backing is currently resident. The capture is
    /// never held whole.
    #[getter]
    fn resident_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.resident_bytes())
    }

    /// The set as the adapter recognized it: its members, their catalog
    /// identities, positions and heads, the transfers read out of them,
    /// and the evidence behind the recognition.
    fn inspect(&self) -> PyResult<CaptureSetReport> {
        Ok(CaptureSetReport::new(self.get()?.inspect()))
    }

    /// Recognizes the drive family this capture belongs to.
    ///
    /// Every enrolled profile is consulted and what claims the capture
    /// is ranked, never resolved by catalog order; a capture no profile
    /// claims is a named refusal, and a lone enrolled profile never wins
    /// by being the only one. The verdict carries the observations that
    /// produced its confidence, because a confidence figure on its own
    /// is not an answer.
    fn recognize(&self) -> PyResult<Recognition> {
        self.get()?
            .recognize()
            .map(|recognition| Recognition::new(&recognition))
            .map_err(to_py_err)
    }

    /// Recognizes the capture against one named profile, whether or not
    /// it would have won the ranking.
    fn recognize_as(&self, profile_id: &str) -> PyResult<Recognition> {
        self.get()?
            .recognize_as(profile_id)
            .map(|recognition| Recognition::new(&recognition))
            .map_err(to_py_err)
    }

    /// Plans the gap-first reconstruction of this capture into one
    /// remanence image under a declared policy.
    ///
    /// Nothing is written and nothing is mutated: the plan computes the
    /// whole reduction — every revolution of every location aligned by
    /// gap correspondence, the cell lattice measured from the intervals
    /// themselves, the angles integrated gap-first, coherence decided
    /// per transition, and the fat track merged under measured
    /// agreement — and enumerates everything the image cannot carry in
    /// the capture's own terms.
    fn plan_reconstruction(
        &self,
        policy: &ReconstructionPolicy,
    ) -> PyResult<ReconstructionPlan> {
        let plan = self
            .get()?
            .plan_reconstruction(&policy.to_core()?)
            .map_err(to_py_err)?;
        let report = ReconstructionReport::new(plan.report());
        Ok(ReconstructionPlan {
            inner: Some(plan),
            report,
        })
    }

    /// Releases the claim on the archive file and discards the private
    /// session storage the capture decoded into.
    fn close(&mut self) {
        self.inner = None;
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exception_type: Bound<'_, PyAny>,
        _exception: Bound<'_, PyAny>,
        _traceback: Bound<'_, PyAny>,
    ) -> bool {
        self.inner = None;
        false
    }
}

/// One zone as a profile declares it, and what the capture recovered of
/// it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ZoneClaim {
    pub first_location: u64,
    pub last_location: u64,
    /// What the family claims one location in this zone holds.
    pub records_declared: u32,
    pub locations_declared: u64,
    pub locations_claimed: u64,
    /// The cell this zone claims, in thousandths of a reference cycle.
    pub nominal_cell_millicycles: u64,
}

#[pymethods]
impl ZoneClaim {
    fn __repr__(&self) -> String {
        format!(
            "ZoneClaim({}-{}, {} records, {}/{} claimed)",
            self.first_location,
            self.last_location,
            self.records_declared,
            self.locations_claimed,
            self.locations_declared
        )
    }
}

/// What the probe found at one source position.
///
/// Every field is an observation, not a conclusion: a count, a density,
/// an angle, an absence. Nothing here names a sector or reads a byte.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct LocationVerdict {
    /// The member this position was read from.
    pub artifact: String,
    pub position: StepPosition,
    pub head: Option<u64>,
    /// The family location this position addresses, where the family's
    /// addressing covers it at all.
    pub family_location: Option<u64>,
    pub zone: Option<u32>,
    pub records: u32,
    /// The bit distance between record starts, where it repeats.
    pub record_bits: Option<u64>,
    /// How far that spacing departs from its own median. Zero is a
    /// spacing that repeats to the bit.
    pub record_bits_deviation: u64,
    /// The one departure from it, as an angle in reference-clock cycles.
    pub seam_cycles: Option<u64>,
    /// The derived cell projected onto the family's nominal rotation,
    /// in thousandths of a reference cycle, beside what the zone claims.
    pub cell_millicycles: Option<u64>,
    pub nominal_cell_millicycles: Option<u64>,
    /// How much of the interval population classified, per thousand.
    pub resolved_permille: u32,
    pub observations: u32,
    pub observations_agreeing: u32,
    /// The adjacent position holding the same content, where one does.
    /// Reported, never resolved.
    pub duplicate_of: Option<StepPosition>,
    pub claimed: bool,
    /// Why this position was not claimed, in the profile's own terms.
    pub refusal: Option<String>,
}

#[pymethods]
impl LocationVerdict {
    fn __repr__(&self) -> String {
        format!(
            "LocationVerdict(position={}/{}, claimed={}, records={})",
            self.position.numerator, self.position.denominator, self.claimed, self.records
        )
    }
}

/// One profile's answer, with the observations that produced it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ProfileVerdict {
    pub profile_id: String,
    pub profile_name: String,
    pub profile_version: u32,
    /// Bounded and comparable, 0 to 100. Never an answer on its own.
    pub confidence: u8,
    pub locations_claimed: u32,
    pub locations_declared: u64,
    pub zones: Vec<ZoneClaim>,
    pub locations: Vec<LocationVerdict>,
    pub evidence: Vec<String>,
}

#[pymethods]
impl ProfileVerdict {
    fn __repr__(&self) -> String {
        format!(
            "ProfileVerdict(profile_id={:?}, confidence={}, {}/{} locations)",
            self.profile_id, self.confidence, self.locations_claimed, self.locations_declared
        )
    }
}

/// What the enrolled profiles made of one capture, ranked.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct Recognition {
    /// Highest confidence first. Several profiles may claim one
    /// capture, and the ranking is reported rather than resolved.
    pub verdicts: Vec<ProfileVerdict>,
    /// The profile the caller pinned, where one was pinned.
    pub pinned: Option<String>,
    pub evidence: Vec<String>,
}

#[pymethods]
impl Recognition {
    fn __repr__(&self) -> String {
        format!(
            "Recognition(verdicts={}, pinned={})",
            self.verdicts.len(),
            self.pinned
                .as_deref()
                .map_or_else(|| "None".to_owned(), |pinned| format!("{pinned:?}"))
        )
    }
}

impl Recognition {
    fn new(recognition: &remanence::Recognition) -> Self {
        Self {
            verdicts: recognition
                .verdicts
                .iter()
                .map(|verdict| ProfileVerdict {
                    profile_id: verdict.profile_id.clone(),
                    profile_name: verdict.profile_name.clone(),
                    profile_version: verdict.profile_version,
                    confidence: verdict.confidence,
                    locations_claimed: verdict.locations_claimed,
                    locations_declared: verdict.locations_declared,
                    zones: verdict
                        .zones
                        .iter()
                        .map(|zone| ZoneClaim {
                            first_location: zone.first_location,
                            last_location: zone.last_location,
                            records_declared: zone.records_declared,
                            locations_declared: zone.locations_declared,
                            locations_claimed: zone.locations_claimed,
                            nominal_cell_millicycles: zone.nominal_cell_millicycles,
                        })
                        .collect(),
                    locations: verdict
                        .locations
                        .iter()
                        .map(|location| LocationVerdict {
                            artifact: location.artifact.clone(),
                            position: StepPosition {
                                numerator: location.position.numerator,
                                denominator: location.position.denominator,
                            },
                            head: location.head,
                            family_location: location.family_location,
                            zone: location.zone,
                            records: location.records,
                            record_bits: location.record_bits,
                            record_bits_deviation: location.record_bits_deviation,
                            seam_cycles: location.seam_cycles,
                            cell_millicycles: location.cell_millicycles,
                            nominal_cell_millicycles: location.nominal_cell_millicycles,
                            resolved_permille: location.resolved_permille,
                            observations: location.observations,
                            observations_agreeing: location.observations_agreeing,
                            duplicate_of: location.duplicate_of.map(|of| StepPosition {
                                numerator: of.numerator,
                                denominator: of.denominator,
                            }),
                            claimed: location.claimed,
                            refusal: location.refusal.clone(),
                        })
                        .collect(),
                    evidence: verdict.evidence.clone(),
                })
                .collect(),
            pinned: recognition.pinned.clone(),
            evidence: recognition.evidence.clone(),
        }
    }
}

/// One thing the destination will not carry, in the source's own terms.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DeclaredLoss {
    /// The profile's stable spelling for this kind of loss.
    pub code: String,
    pub detail: String,
    /// How much of it there is, in whatever the detail counts.
    pub count: u64,
}

#[pymethods]
impl DeclaredLoss {
    fn __repr__(&self) -> String {
        format!("DeclaredLoss(code={:?}, count={})", self.code, self.count)
    }
}

/// One location the bitstream holds, and what the channel resolved
/// there.
///
/// Every field is an observation: a count, a cell, a run, a remainder.
/// Nothing here names a byte.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct BitstreamLocation {
    /// The family half-track this addresses, as an exact ratio.
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    /// Which declared density zone supplied the cell.
    pub zone: u32,
    /// The cell, in reference-clock cycles, exactly.
    pub cell_cycles_numerator: u64,
    pub cell_cycles_denominator: u64,
    pub cells: u64,
    pub one_bits: u64,
    /// Bits the medium recorded, and bits a declared rule resolved.
    /// They sum to `cells`.
    pub recorded_bits: u64,
    pub resolved_bits: u64,
    pub short_cells: u64,
    pub longest_zero_run: u64,
    /// What is left of the circle after the last whole cell, over
    /// `cell_cycles_denominator`.
    pub wrap_slack_numerator: u64,
}

#[pymethods]
impl BitstreamLocation {
    fn __repr__(&self) -> String {
        format!(
            "BitstreamLocation(half_track={}/{}, zone={}, cells={})",
            self.half_track_numerator, self.half_track_denominator, self.zone, self.cells
        )
    }
}

/// What one medium-to-bitstream transition produced.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct BitstreamReport {
    pub profile_id: String,
    pub profile_name: String,
    pub profile_version: u32,
    pub reference_clock_hz: u64,
    pub cycles_per_rotation: u64,
    pub locations: Vec<BitstreamLocation>,
    /// Everything the bitstream does not carry of the medium beneath it.
    pub declared_loss: Vec<DeclaredLoss>,
    /// The channel that produced it and the policy that produced the
    /// medium, in that order.
    pub evidence: Vec<String>,
}

#[pymethods]
impl BitstreamReport {
    fn __repr__(&self) -> String {
        format!(
            "BitstreamReport(locations={}, declared_loss={})",
            self.locations.len(),
            self.declared_loss.len()
        )
    }
}

impl BitstreamReport {
    fn new(report: &remanence::BitstreamReport) -> Self {
        Self {
            profile_id: report.profile_id.clone(),
            profile_name: report.profile_name.clone(),
            profile_version: report.profile_version,
            reference_clock_hz: report.reference_clock_hz,
            cycles_per_rotation: report.cycles_per_rotation,
            locations: report
                .locations
                .iter()
                .map(|location| BitstreamLocation {
                    half_track_numerator: location.half_track_numerator,
                    half_track_denominator: location.half_track_denominator,
                    surface: location.surface,
                    zone: location.zone,
                    cell_cycles_numerator: location.cell_cycles_numerator,
                    cell_cycles_denominator: location.cell_cycles_denominator,
                    cells: location.cells,
                    one_bits: location.one_bits,
                    recorded_bits: location.recorded_bits,
                    resolved_bits: location.resolved_bits,
                    short_cells: location.short_cells,
                    longest_zero_run: location.longest_zero_run,
                    wrap_slack_numerator: location.wrap_slack_numerator,
                })
                .collect(),
            declared_loss: report
                .declared_loss
                .iter()
                .map(|loss| DeclaredLoss {
                    code: loss.code.clone(),
                    detail: loss.detail.clone(),
                    count: loss.count,
                })
                .collect(),
            evidence: report.evidence.clone(),
        }
    }
}

/// One location the bytestream holds.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct BytestreamLocation {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    pub bytes: u64,
    pub resolved_bytes: u64,
    /// Groups holding a pattern the family's table does not assign.
    pub unassigned_groups: u64,
    /// How many times framing was established on the family's landmark.
    /// It says where bytes begin and nothing about what they are.
    pub alignments: u64,
    pub longest_landmark_bits: u64,
    pub unframed_bits: u64,
}

#[pymethods]
impl BytestreamLocation {
    fn __repr__(&self) -> String {
        format!(
            "BytestreamLocation(half_track={}/{}, bytes={})",
            self.half_track_numerator, self.half_track_denominator, self.bytes
        )
    }
}

/// What one bitstream-to-bytestream transition produced.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct BytestreamReport {
    pub profile_id: String,
    pub codec_id: String,
    pub codec_name: String,
    pub symbol_bits: u32,
    pub data_bits: u32,
    pub symbols_per_byte: u32,
    pub locations: Vec<BytestreamLocation>,
    pub declared_loss: Vec<DeclaredLoss>,
    pub evidence: Vec<String>,
}

#[pymethods]
impl BytestreamReport {
    fn __repr__(&self) -> String {
        format!(
            "BytestreamReport(codec_id={:?}, locations={})",
            self.codec_id,
            self.locations.len()
        )
    }
}

impl BytestreamReport {
    fn new(report: &remanence::BytestreamReport) -> Self {
        Self {
            profile_id: report.profile_id.clone(),
            codec_id: report.codec_id.clone(),
            codec_name: report.codec_name.clone(),
            symbol_bits: report.symbol_bits,
            data_bits: report.data_bits,
            symbols_per_byte: report.symbols_per_byte,
            locations: report
                .locations
                .iter()
                .map(|location| BytestreamLocation {
                    half_track_numerator: location.half_track_numerator,
                    half_track_denominator: location.half_track_denominator,
                    surface: location.surface,
                    bytes: location.bytes,
                    resolved_bytes: location.resolved_bytes,
                    unassigned_groups: location.unassigned_groups,
                    alignments: location.alignments,
                    longest_landmark_bits: location.longest_landmark_bits,
                    unframed_bits: location.unframed_bits,
                })
                .collect(),
            declared_loss: report
                .declared_loss
                .iter()
                .map(|loss| DeclaredLoss {
                    code: loss.code.clone(),
                    detail: loss.detail.clone(),
                    count: loss.count,
                })
                .collect(),
            evidence: report.evidence.clone(),
        }
    }
}

/// The complete declared policy for one medium-to-bitstream transition.
///
/// Every argument is keyword-only and required, deliberately: each is a
/// decision about evidence, and a channel that arrived at one by
/// construction rather than by declaration is what the drive profile
/// exists to prevent. `density` is `"declared"` or `"fixed"` with
/// `density_zone`; `unzoned` is `"refuse"` or `"omit"`; `weak_pulse` is
/// `"declared"` with `weak_pulse_detected` or `"seeded"`.
#[pyclass(frozen, get_all, from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ReadChannelPolicy {
    pub density: String,
    pub density_zone: u32,
    pub unzoned: String,
    pub weak_pulse: String,
    pub weak_pulse_detected: bool,
    /// What makes any stochastic element of the channel reproducible.
    pub seed: u64,
}

#[pymethods]
impl ReadChannelPolicy {
    #[new]
    #[pyo3(signature = (
        *,
        density,
        unzoned,
        weak_pulse,
        seed,
        density_zone = 0,
        weak_pulse_detected = false,
    ))]
    fn new(
        density: String,
        unzoned: String,
        weak_pulse: String,
        seed: u64,
        density_zone: u32,
        weak_pulse_detected: bool,
    ) -> Self {
        Self {
            density,
            density_zone,
            unzoned,
            weak_pulse,
            weak_pulse_detected,
            seed,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ReadChannelPolicy(density={:?}, unzoned={:?}, weak_pulse={:?})",
            self.density, self.unzoned, self.weak_pulse
        )
    }
}

impl ReadChannelPolicy {
    fn to_core(&self) -> PyResult<remanence::ReadChannelPolicy> {
        Ok(remanence::ReadChannelPolicy {
            density: match self.density.as_str() {
                "declared" => remanence::DensityPolicy::Declared,
                "fixed" => remanence::DensityPolicy::Fixed {
                    zone: self.density_zone,
                },
                other => return Err(unadmitted("density", other, &["declared", "fixed"])),
            },
            unzoned: match self.unzoned.as_str() {
                "refuse" => remanence::UnzonedPolicy::Refuse,
                "omit" => remanence::UnzonedPolicy::Omit,
                other => return Err(unadmitted("unzoned", other, &["refuse", "omit"])),
            },
            weak_pulse: match self.weak_pulse.as_str() {
                "declared" => remanence::WeakPulsePolicy::Declared {
                    detected: self.weak_pulse_detected,
                },
                "seeded" => remanence::WeakPulsePolicy::Seeded,
                other => return Err(unadmitted("weak pulse", other, &["declared", "seeded"])),
            },
            seed: self.seed,
        })
    }
}

/// The complete declared policy for one bitstream-to-bytestream
/// transition. `alignment` is `"landmark"` or `"origin"`;
/// `unassigned_symbol` is `"refuse"` or `"declare-loss"`.
#[pyclass(frozen, get_all, from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct GcrCodecPolicy {
    pub alignment: String,
    pub unassigned_symbol: String,
}

#[pymethods]
impl GcrCodecPolicy {
    #[new]
    #[pyo3(signature = (*, alignment, unassigned_symbol))]
    fn new(alignment: String, unassigned_symbol: String) -> Self {
        Self {
            alignment,
            unassigned_symbol,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "GcrCodecPolicy(alignment={:?}, unassigned_symbol={:?})",
            self.alignment, self.unassigned_symbol
        )
    }
}

impl GcrCodecPolicy {
    fn to_core(&self) -> PyResult<remanence::GcrCodecPolicy> {
        Ok(remanence::GcrCodecPolicy {
            alignment: match self.alignment.as_str() {
                "landmark" => remanence::AlignmentPolicy::Landmark,
                "origin" => remanence::AlignmentPolicy::Origin,
                other => return Err(unadmitted("alignment", other, &["landmark", "origin"])),
            },
            unassigned_symbol: match self.unassigned_symbol.as_str() {
                "refuse" => remanence::UnassignedSymbolPolicy::Refuse,
                "declare-loss" => remanence::UnassignedSymbolPolicy::DeclareLoss,
                other => {
                    return Err(unadmitted(
                        "unassigned symbol",
                        other,
                        &["refuse", "declare-loss"],
                    ));
                }
            },
        })
    }
}

/// A policy spelling this library does not admit.
fn unadmitted(what: &str, given: &str, admitted: &[&str]) -> PyErr {
    categorized_py_err(
        remanence::ErrorCategory::Unsupported,
        format!(
            "{what} policy {given:?} is not one this library admits; it takes one of \
             {admitted:?}"
        ),
    )
}

/// A hardware bitstream, held in the session. The bits stay behind this
/// surface: what a hardware presentation makes of them is that seam's
/// business.
#[pyclass(module = "remanence")]
pub struct C1541Bitstream {
    inner: remanence::C1541Bitstream,
    report: BitstreamReport,
}

impl C1541Bitstream {
    fn produce(inner: remanence::C1541Bitstream) -> PyResult<Self> {
        let report = BitstreamReport::new(inner.inspect());
        Ok(Self { inner, report })
    }
}

#[pymethods]
impl C1541Bitstream {
    /// The transition that produced this bitstream, and everything it
    /// does not carry of the medium beneath it.
    fn inspect(&self) -> BitstreamReport {
        self.report.clone()
    }

    /// How many locations the bitstream claims.
    #[getter]
    fn locations(&self) -> u64 {
        self.inner.locations()
    }

    #[getter]
    fn backing_bytes(&self) -> u64 {
        self.inner.backing_bytes()
    }

    #[getter]
    fn resident_bytes(&self) -> u64 {
        self.inner.resident_bytes()
    }

    /// Materializes the family's encoded bytestream from this bitstream
    /// under its declared group code. The bitstream is untouched.
    #[pyo3(signature = (policy, *, cache_bytes = None))]
    fn materialize_c1541_bytestream(
        &self,
        policy: &GcrCodecPolicy,
        cache_bytes: Option<u64>,
    ) -> PyResult<C1541Bytestream> {
        let inner = self
            .inner
            .materialize_c1541_bytestream(
                policy.to_core()?,
                cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES),
            )
            .map_err(to_py_err)?;
        let report = BytestreamReport::new(inner.inspect());
        Ok(C1541Bytestream { inner, report })
    }

    fn __repr__(&self) -> String {
        format!("C1541Bitstream(locations={})", self.inner.locations())
    }
}

/// An encoded bytestream, held in the session.
#[pyclass(module = "remanence")]
pub struct C1541Bytestream {
    inner: remanence::C1541Bytestream,
    report: BytestreamReport,
}

#[pymethods]
impl C1541Bytestream {
    fn inspect(&self) -> BytestreamReport {
        self.report.clone()
    }

    #[getter]
    fn locations(&self) -> u64 {
        self.inner.locations()
    }

    #[getter]
    fn backing_bytes(&self) -> u64 {
        self.inner.backing_bytes()
    }

    #[getter]
    fn resident_bytes(&self) -> u64 {
        self.inner.resident_bytes()
    }

    /// Recognizes the recording's own sectors out of this bytestream,
    /// under the family's declared record grammar. The bytestream is
    /// untouched.
    #[pyo3(signature = (policy, *, cache_bytes = None))]
    fn recognize_c1541_sectors(
        &self,
        policy: &SectorPolicy,
        cache_bytes: Option<u64>,
    ) -> PyResult<C1541Sectors> {
        let inner = self
            .inner
            .recognize_c1541_sectors(
                policy.to_core()?,
                cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES),
            )
            .map_err(to_py_err)?;
        let report = SectorReport::new(inner.inspect());
        Ok(C1541Sectors {
            inner: Arc::new(inner),
            report,
        })
    }

    fn __repr__(&self) -> String {
        format!("C1541Bytestream(locations={})", self.inner.locations())
    }
}

/// The complete declared policy for one bytestream-to-sector
/// recognition. Both fields are `"refuse"` or `"declare-loss"`.
#[pyclass(frozen, get_all, from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct SectorPolicy {
    pub checksum_failure: String,
    pub unpaired_record: String,
}

#[pymethods]
impl SectorPolicy {
    #[new]
    #[pyo3(signature = (*, checksum_failure, unpaired_record))]
    fn new(checksum_failure: String, unpaired_record: String) -> Self {
        Self {
            checksum_failure,
            unpaired_record,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SectorPolicy(checksum_failure={:?}, unpaired_record={:?})",
            self.checksum_failure, self.unpaired_record
        )
    }
}

impl SectorPolicy {
    fn to_core(&self) -> PyResult<remanence::SectorPolicy> {
        Ok(remanence::SectorPolicy {
            checksum_failure: match self.checksum_failure.as_str() {
                "refuse" => remanence::ChecksumFailurePolicy::Refuse,
                "declare-loss" => remanence::ChecksumFailurePolicy::DeclareLoss,
                other => {
                    return Err(unadmitted(
                        "checksum failure",
                        other,
                        &["refuse", "declare-loss"],
                    ));
                }
            },
            unpaired_record: match self.unpaired_record.as_str() {
                "refuse" => remanence::UnpairedRecordPolicy::Refuse,
                "declare-loss" => remanence::UnpairedRecordPolicy::DeclareLoss,
                other => {
                    return Err(unadmitted(
                        "unpaired record",
                        other,
                        &["refuse", "declare-loss"],
                    ));
                }
            },
        })
    }
}

/// One location the sector layer read, and what it found there.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct SectorLocation {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    /// What the family's density map claims one location in this zone
    /// holds, where a declared zone covers it at all.
    pub records_declared: Option<u32>,
    pub headers: u64,
    pub records: u64,
    pub readable: u64,
    pub failed_checksum: u64,
    pub runs_without_a_record: u64,
}

#[pymethods]
impl SectorLocation {
    fn __repr__(&self) -> String {
        format!(
            "SectorLocation(half_track={}/{}, readable={}/{})",
            self.half_track_numerator,
            self.half_track_denominator,
            self.readable,
            self.headers
        )
    }
}

/// One record the recognition read, and the evidence for every claim it
/// makes. `rule` and `refusal` are None for a claim that reads.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct SectorClaim {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    pub at_bit: u64,
    pub track: u8,
    pub sector: u8,
    pub id_high: u8,
    pub id_low: u8,
    pub header_checksum_stated: u8,
    pub header_checksum_computed: u8,
    pub has_data: bool,
    pub data_at_bit: u64,
    pub data_checksum_stated: u8,
    pub data_checksum_computed: u8,
    pub unresolved_bytes: u64,
    pub within_declaration: bool,
    pub readable: bool,
    pub rule: Option<String>,
    pub refusal: Option<String>,
}

#[pymethods]
impl SectorClaim {
    fn __repr__(&self) -> String {
        format!(
            "SectorClaim(track={}, sector={}, readable={})",
            self.track, self.sector, self.readable
        )
    }
}

/// One address more than one readable claim states.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ContestedAddress {
    pub track: u8,
    pub sector: u8,
    pub readable_claims: u32,
}

#[pymethods]
impl ContestedAddress {
    fn __repr__(&self) -> String {
        format!(
            "ContestedAddress(track={}, sector={}, readable_claims={})",
            self.track, self.sector, self.readable_claims
        )
    }
}

/// What one bytestream-to-sector recognition produced.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct SectorReport {
    pub profile_id: String,
    pub grammar_id: String,
    pub grammar_name: String,
    pub payload_bytes: u32,
    pub locations: Vec<SectorLocation>,
    pub claims: Vec<SectorClaim>,
    /// Addresses more than one readable claim states. Reported rather
    /// than resolved; whether those claims agree is decided on a read.
    pub contested: Vec<ContestedAddress>,
    pub declared_loss: Vec<DeclaredLoss>,
    pub evidence: Vec<String>,
}

#[pymethods]
impl SectorReport {
    fn __repr__(&self) -> String {
        format!(
            "SectorReport(grammar_id={:?}, claims={})",
            self.grammar_id,
            self.claims.len()
        )
    }
}

impl SectorReport {
    fn new(report: &remanence::SectorReport) -> Self {
        Self {
            profile_id: report.profile_id.clone(),
            grammar_id: report.grammar_id.clone(),
            grammar_name: report.grammar_name.clone(),
            payload_bytes: report.payload_bytes,
            locations: report
                .locations
                .iter()
                .map(|location| SectorLocation {
                    half_track_numerator: location.half_track_numerator,
                    half_track_denominator: location.half_track_denominator,
                    surface: location.surface,
                    records_declared: location.records_declared,
                    headers: location.headers,
                    records: location.records,
                    readable: location.readable,
                    failed_checksum: location.failed_checksum,
                    runs_without_a_record: location.runs_without_a_record,
                })
                .collect(),
            claims: report
                .claims
                .iter()
                .map(|claim| SectorClaim {
                    half_track_numerator: claim.half_track_numerator,
                    half_track_denominator: claim.half_track_denominator,
                    surface: claim.surface,
                    at_bit: claim.at_bit,
                    track: claim.track,
                    sector: claim.sector,
                    id_high: claim.id_high,
                    id_low: claim.id_low,
                    header_checksum_stated: claim.header_checksum_stated,
                    header_checksum_computed: claim.header_checksum_computed,
                    has_data: claim.has_data,
                    data_at_bit: claim.data_at_bit,
                    data_checksum_stated: claim.data_checksum_stated,
                    data_checksum_computed: claim.data_checksum_computed,
                    unresolved_bytes: claim.unresolved_bytes,
                    within_declaration: claim.within_declaration,
                    readable: claim.readable,
                    rule: claim.rule.clone(),
                    refusal: claim.refusal.clone(),
                })
                .collect(),
            contested: report
                .contested
                .iter()
                .map(|contested| ContestedAddress {
                    track: contested.track,
                    sector: contested.sector,
                    readable_claims: contested.readable_claims,
                })
                .collect(),
            declared_loss: report
                .declared_loss
                .iter()
                .map(|loss| DeclaredLoss {
                    code: loss.code.clone(),
                    detail: loss.detail.clone(),
                    count: loss.count,
                })
                .collect(),
            evidence: report.evidence.clone(),
        }
    }
}

/// The recording's own sectors, held in the session. The payloads stay
/// behind this surface and are read by the address the recording states
/// for them.
#[pyclass(module = "remanence")]
pub struct C1541Sectors {
    inner: Arc<remanence::C1541Sectors>,
    report: SectorReport,
}

#[pymethods]
impl C1541Sectors {
    /// The recognition that produced these sectors, every claim's
    /// evidence, and everything the layer does not carry of the
    /// bytestream beneath it.
    fn inspect(&self) -> SectorReport {
        self.report.clone()
    }

    /// Reads one sector by the address the recording states for it.
    ///
    /// It answers only where the recording is unambiguous: one readable
    /// claim, or several holding the same bytes. Every other outcome
    /// raises, with `rule` naming which rule of the sector layer's set
    /// stands in the way. Nothing is repaired and no block is filled in.
    fn read_sector<'py>(
        &self,
        py: Python<'py>,
        track: u8,
        sector: u8,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let payload = self.inner.read_sector(track, sector).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &payload))
    }

    /// The filesystem this recording bears, where it bears one.
    ///
    /// **The sector layer carries no file verbs of its own** — it may be
    /// asked what it resolves to, and may not be told to act as a
    /// namespace it is not. The answer is the same `StorageSpace` a
    /// device resolves to, and the protected and the blank raise naming
    /// the seam that ran out of answers.
    fn filesystem(&self) -> PyResult<StorageSpace> {
        let kind = self
            .inner
            .filesystem()
            .and_then(|space| space.kind().map(str::to_owned))
            .map_err(to_py_err)?;
        Ok(StorageSpace {
            session: None,
            media: None,
            sectors: Some(Arc::clone(&self.inner)),
            volume: None,
            start_bytes: 0,
            length_bytes: 0,
            kind: Some(kind),
        })
    }

    /// How many records the recognition read.
    #[getter]
    fn claims(&self) -> u64 {
        self.inner.claims()
    }

    /// How many locations it read them out of.
    #[getter]
    fn locations(&self) -> u64 {
        self.inner.locations()
    }

    #[getter]
    fn backing_bytes(&self) -> u64 {
        self.inner.backing_bytes()
    }

    #[getter]
    fn resident_bytes(&self) -> u64 {
        self.inner.resident_bytes()
    }

    fn __repr__(&self) -> String {
        format!("C1541Sectors(claims={})", self.inner.claims())
    }
}

/// One half-track a P64 holds, in the container's addressing and the
/// family's both.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct P64HalfTrack {
    /// The container's own index byte, side bit included.
    pub index: u8,
    pub side: u64,
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub pulses: u64,
    /// Pulses that always trigger, that sometimes do, and that never do.
    pub strong_pulses: u64,
    pub weak_pulses: u64,
    pub absent_pulses: u64,
}

#[pymethods]
impl P64HalfTrack {
    fn __repr__(&self) -> String {
        format!(
            "P64HalfTrack(index={}, half_track={}/{}, pulses={})",
            self.index, self.half_track_numerator, self.half_track_denominator, self.pulses
        )
    }
}

/// A P64 container as the adapter reads or writes one.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct P64Report {
    pub format_id: String,
    pub format_name: String,
    /// The container's declared format version.
    pub version: u32,
    pub write_protected: bool,
    pub double_sided: bool,
    /// The drive profile the container's own signature names, and the
    /// frame that profile declares.
    pub profile_id: String,
    pub reference_clock_hz: u64,
    pub cycles_per_rotation: u64,
    pub half_tracks: Vec<P64HalfTrack>,
    /// What the crossing does not carry, in the source's own terms. A
    /// count is not an account, so each entry says what it was.
    pub declared_loss: Vec<DeclaredLoss>,
    /// How the container was recognized and what the adapter claims of
    /// it.
    pub evidence: Vec<String>,
}

#[pymethods]
impl P64Report {
    fn __repr__(&self) -> String {
        format!(
            "P64Report(half_tracks={}, declared_loss={})",
            self.half_tracks.len(),
            self.declared_loss.len()
        )
    }
}

impl P64Report {
    fn new(report: &remanence::P64Report) -> Self {
        Self {
            format_id: report.format_id.clone(),
            format_name: report.format_name.clone(),
            version: report.version,
            write_protected: report.write_protected,
            double_sided: report.double_sided,
            profile_id: report.profile_id.clone(),
            reference_clock_hz: report.reference_clock_hz,
            cycles_per_rotation: report.cycles_per_rotation,
            half_tracks: report
                .half_tracks
                .iter()
                .map(|track| P64HalfTrack {
                    index: track.index,
                    side: track.side,
                    half_track_numerator: track.half_track_numerator,
                    half_track_denominator: track.half_track_denominator,
                    pulses: track.pulses,
                    strong_pulses: track.strong_pulses,
                    weak_pulses: track.weak_pulses,
                    absent_pulses: track.absent_pulses,
                })
                .collect(),
            declared_loss: report
                .declared_loss
                .iter()
                .map(|loss| DeclaredLoss {
                    code: loss.code.clone(),
                    detail: loss.detail.clone(),
                    count: loss.count,
                })
                .collect(),
            evidence: report.evidence.clone(),
        }
    }
}

/// One P64 image, opened and read.
///
/// Opening claims the file — writes denied to every other process —
/// decodes every half-track once into private session storage, and holds
/// the claim until the image is closed or collected.
#[pyclass(module = "remanence")]
pub struct P64Image {
    inner: Option<remanence::P64Image>,
    report: P64Report,
}

#[pymethods]
impl P64Image {
    /// Opens the P64 image at `path`. The version is checked before
    /// anything else is touched, and a version, flag bit, or chunk
    /// signature past this release's claim is refused by name.
    #[new]
    #[pyo3(signature = (path, *, cache_bytes = None))]
    fn new(path: PathBuf, cache_bytes: Option<u64>) -> PyResult<Self> {
        let image = match cache_bytes {
            Some(cache_bytes) => remanence::P64Image::open_with_cache(path, cache_bytes),
            None => remanence::P64Image::open(path),
        }
        .map_err(to_py_err)?;
        let report = P64Report::new(image.inspect());
        Ok(Self {
            inner: Some(image),
            report,
        })
    }

    /// The container as the adapter read it.
    fn inspect(&self) -> P64Report {
        self.report.clone()
    }

    /// The path the image was opened from.
    #[getter]
    fn path(&self) -> PyResult<String> {
        Ok(self.get()?.path().to_string_lossy().into_owned())
    }

    #[getter]
    fn format_id(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_id())
    }

    #[getter]
    fn format_name(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_name())
    }

    /// How many bytes of private session storage the decoded medium
    /// occupies, and how much of that is currently resident.
    #[getter]
    fn backing_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.backing_bytes())
    }

    #[getter]
    fn resident_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.resident_bytes())
    }

    /// Materializes the family's hardware bitstream from the medium
    /// this container holds at rest.
    ///
    /// The container is read and nothing else. What the bitstream says
    /// about how its medium came to exist is what the container said:
    /// that it was found already a medium and this library derived none
    /// of it.
    #[pyo3(signature = (policy, *, cache_bytes = None))]
    fn materialize_c1541_bitstream(
        &self,
        policy: &ReadChannelPolicy,
        cache_bytes: Option<u64>,
    ) -> PyResult<C1541Bitstream> {
        C1541Bitstream::produce(
            self.get()?
                .materialize_c1541_bitstream(
                    policy.to_core()?,
                    cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES),
                )
                .map_err(to_py_err)?,
        )
    }

    /// Releases the claim on the file and discards the private session
    /// storage the medium decoded into.
    fn close(&mut self) {
        self.inner = None;
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exception_type: Bound<'_, PyAny>,
        _exception: Bound<'_, PyAny>,
        _traceback: Bound<'_, PyAny>,
    ) -> bool {
        self.inner = None;
        false
    }

    fn __repr__(&self) -> String {
        format!(
            "P64Image(half_tracks={})",
            self.report.half_tracks.len()
        )
    }
}

impl P64Image {
    fn get(&self) -> PyResult<&remanence::P64Image> {
        self.inner
            .as_ref()
            .ok_or_else(|| categorized_py_err(remanence::ErrorCategory::Io, "image is closed"))
    }
}

// ---------------------------------------------------------------------------
// The remanence image: the flux family's physical stratum, and the
// `.remanence` artifact it is read from and written to. The model
// beneath the root — orbits' points, magnetization, write geometry —
// does not cross this boundary; what crosses is the image's shape.

/// One index hole, as the image holds it: an exact fraction of a turn
/// for the centre and another for the extent. Nothing radial is stored.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone, PartialEq, Eq)]
pub struct RemanenceHole {
    pub center_numerator: u64,
    pub center_denominator: u64,
    pub extent_numerator: u64,
    pub extent_denominator: u64,
}

#[pymethods]
impl RemanenceHole {
    fn __repr__(&self) -> String {
        format!(
            "RemanenceHole(center={}/{}, extent={}/{})",
            self.center_numerator,
            self.center_denominator,
            self.extent_numerator,
            self.extent_denominator
        )
    }
}

/// One orbit's identity and shape — never its points.
///
/// The points are the model beneath this root and stay there: a whole
/// side carries millions of them, and what a reader of the image needs
/// is where the orbit sits and how much it holds.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone, PartialEq, Eq)]
pub struct RemanenceOrbit {
    pub surface: u64,
    /// The centre radius of the recorded band, in whole microns — a
    /// fact about the disk, never the step index of whichever
    /// instrument found it.
    pub radius_microns: u64,
    /// Every point the orbit holds, coherent or not.
    pub points: u64,
    /// How many of them carry a sense a reversal can be drawn from.
    pub coherent_points: u64,
    /// How many spans the image declines to read. Genuine
    /// indeterminacy, recorded rather than repaired into a guess.
    pub unaligned_spans: u64,
}

#[pymethods]
impl RemanenceOrbit {
    fn __repr__(&self) -> String {
        format!(
            "RemanenceOrbit(surface={}, radius_microns={}, points={})",
            self.surface, self.radius_microns, self.points
        )
    }
}

/// A remanence image as it stands: the physical facts of one disk.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct RemanenceImageReport {
    /// The medium's shape in the model's own spelling: `"8-inch"`,
    /// `"5.25-inch"` or `"3.5-inch"`.
    pub form_factor: String,
    /// The angular unit every angle in the image is stated over — a
    /// unit rather than a measurement, so equality is exact.
    pub angular_divisions: u64,
    pub holes: Vec<RemanenceHole>,
    /// The surfaces carrying orbits, ascending.
    pub surfaces: Vec<u64>,
    /// Every orbit, ordered by surface then radius.
    pub orbits: Vec<RemanenceOrbit>,
    /// How the image came to be known, in human-readable terms.
    pub provenance: Vec<String>,
}

#[pymethods]
impl RemanenceImageReport {
    fn __repr__(&self) -> String {
        format!(
            "RemanenceImageReport(form_factor={:?}, orbits={})",
            self.form_factor,
            self.orbits.len()
        )
    }
}

impl RemanenceImageReport {
    fn new(report: &remanence::RemanenceImageReport) -> Self {
        Self {
            form_factor: report.form_factor.clone(),
            angular_divisions: report.angular_divisions,
            holes: report
                .holes
                .iter()
                .map(|hole| RemanenceHole {
                    center_numerator: hole.center_numerator,
                    center_denominator: hole.center_denominator,
                    extent_numerator: hole.extent_numerator,
                    extent_denominator: hole.extent_denominator,
                })
                .collect(),
            surfaces: report.surfaces.clone(),
            orbits: report
                .orbits
                .iter()
                .map(|orbit| RemanenceOrbit {
                    surface: orbit.surface,
                    radius_microns: orbit.radius_microns,
                    points: orbit.points,
                    coherent_points: orbit.coherent_points,
                    unaligned_spans: orbit.unaligned_spans,
                })
                .collect(),
            provenance: report.provenance.clone(),
        }
    }
}

/// What writing an image into a `.remanence` artifact carried.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct RemanenceWriteReport {
    /// Where the artifact was written.
    pub path: String,
    /// The artifact's size on storage.
    pub artifact_bytes: u64,
    pub orbits: u64,
    /// Every point across every orbit the artifact carries.
    pub points: u64,
    /// What the destination did not carry. Empty for this format,
    /// always: the remanence artifact is the model's own, so it carries
    /// every fact the image holds. An empty account is the claim, not a
    /// missing one.
    pub declared_loss: Vec<DeclaredLoss>,
}

#[pymethods]
impl RemanenceWriteReport {
    fn __repr__(&self) -> String {
        format!(
            "RemanenceWriteReport(path={:?}, orbits={}, points={})",
            self.path, self.orbits, self.points
        )
    }
}

/// The complete declared policy for one gap-first reconstruction.
///
/// There is no default: a reduction the policy does not name is a
/// refusal. `recordings` is `"measured"` — a position records where its
/// revolutions resolve the same transitions, which is the count-spread
/// discriminator reading the evidence — or `"declared"`, in which case
/// `declared_positions` is the caller's own assertion and is checked to
/// exist before it is honoured.
#[pyclass(frozen, get_all, from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ReconstructionPolicy {
    /// Which recorded side the image is reconstructed from. Sides are
    /// never merged or averaged.
    pub side: u64,
    pub recordings: String,
    pub declared_positions: Vec<u64>,
}

#[pymethods]
impl ReconstructionPolicy {
    #[new]
    #[pyo3(signature = (*, side, recordings, declared_positions = Vec::new()))]
    fn new(side: u64, recordings: String, declared_positions: Vec<u64>) -> Self {
        Self {
            side,
            recordings,
            declared_positions,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ReconstructionPolicy(side={}, recordings={:?})",
            self.side, self.recordings
        )
    }
}

impl ReconstructionPolicy {
    fn to_core(&self) -> PyResult<remanence::ReconstructionPolicy> {
        Ok(remanence::ReconstructionPolicy {
            side: self.side,
            recordings: match self.recordings.as_str() {
                "measured" => remanence::RecordingSelection::Measured,
                "declared" => {
                    remanence::RecordingSelection::Declared(self.declared_positions.clone())
                }
                other => {
                    return Err(unadmitted(
                        "recording selection",
                        other,
                        &["measured", "declared"],
                    ));
                }
            },
        })
    }
}

/// One reconstructed orbit, as the plan reports it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ReconstructedOrbit {
    /// The instrument position the orbit was read at — capture
    /// provenance, not a fact of the medium.
    pub position: u64,
    /// Where the orbit is: the rig's radius at that position.
    pub radius_microns: u64,
    pub revolutions: u32,
    /// Raw transitions per revolution, before alignment.
    pub transition_counts: Vec<u32>,
    /// The count-spread discriminator, in permille of the largest.
    pub count_spread_permille: u32,
    pub points: u64,
    pub coherent_points: u64,
    pub unaligned_spans: u64,
    /// The cell the closed revolution implies, in millidivisions.
    pub implied_cell_millidivisions: u64,
    /// Intervals kept off the lattice: the medium holding what the
    /// crystal did not write.
    pub off_lattice: u32,
    /// Whether the fat-track merge admitted this orbit into the image.
    pub admitted: bool,
}

#[pymethods]
impl ReconstructedOrbit {
    fn __repr__(&self) -> String {
        format!(
            "ReconstructedOrbit(position={}, radius_microns={}, points={}, admitted={})",
            self.position,
            self.radius_microns,
            self.points,
            if self.admitted { "True" } else { "False" }
        )
    }
}

/// What the reconstruction will produce, computed whole before anything
/// is written.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ReconstructionReport {
    pub format_id: String,
    pub side: u64,
    /// Every position the capture holds on the side.
    pub swept_positions: u32,
    /// The positions the policy's selection names as recordings.
    pub recorded_positions: Vec<u64>,
    pub orbits: Vec<ReconstructedOrbit>,
    /// What the image cannot carry of the capture. A count is not an
    /// account, so each entry says what it was.
    pub declared_loss: Vec<DeclaredLoss>,
    /// What the reduction claims of what it did.
    pub evidence: Vec<String>,
}

#[pymethods]
impl ReconstructionReport {
    fn __repr__(&self) -> String {
        format!(
            "ReconstructionReport(side={}, orbits={}, declared_loss={})",
            self.side,
            self.orbits.len(),
            self.declared_loss.len()
        )
    }
}

impl ReconstructionReport {
    fn new(report: &remanence::ReconstructionReport) -> Self {
        Self {
            format_id: report.format_id.to_owned(),
            side: report.side,
            swept_positions: report.swept_positions,
            recorded_positions: report.recorded_positions.clone(),
            orbits: report
                .orbits
                .iter()
                .map(|orbit| ReconstructedOrbit {
                    position: orbit.position,
                    radius_microns: orbit.radius_microns,
                    revolutions: orbit.revolutions,
                    transition_counts: orbit.transition_counts.clone(),
                    count_spread_permille: orbit.count_spread_permille,
                    points: orbit.points,
                    coherent_points: orbit.coherent_points,
                    unaligned_spans: orbit.unaligned_spans,
                    implied_cell_millidivisions: orbit.implied_cell_millidivisions,
                    off_lattice: orbit.off_lattice,
                    admitted: orbit.admitted,
                })
                .collect(),
            declared_loss: declared_loss(&report.declared_loss),
            evidence: report.evidence.clone(),
        }
    }
}

/// A planned reduction: everything computed, nothing written.
#[pyclass(module = "remanence")]
pub struct ReconstructionPlan {
    inner: Option<remanence::ReconstructionPlan>,
    report: ReconstructionReport,
}

#[pymethods]
impl ReconstructionPlan {
    /// What the reduction will produce, and everything the image will
    /// not carry. Read before executing: executing adds nothing to it.
    fn report(&self) -> ReconstructionReport {
        self.report.clone()
    }

    /// Produces the remanence image. The capture is untouched, and at
    /// most `cache_bytes` of the image's points stay resident.
    ///
    /// What comes back is the family's ordinary `RemanenceImage` — the
    /// same handle a `.remanence` artifact opens to — carrying this
    /// reduction's declared policy and evidence as its provenance.
    #[pyo3(signature = (*, cache_bytes = None))]
    fn execute(&mut self, cache_bytes: Option<u64>) -> PyResult<RemanenceImage> {
        let plan = self.inner.take().ok_or_else(|| {
            categorized_py_err(
                remanence::ErrorCategory::Io,
                "plan has already been executed",
            )
        })?;
        let image = plan
            .execute(cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES))
            .map_err(to_py_err)?;
        let report = RemanenceImageReport::new(&image.inspect());
        Ok(RemanenceImage {
            inner: Some(image),
            report,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "ReconstructionPlan(side={}, orbits={})",
            self.report.side,
            self.report.orbits.len()
        )
    }
}

/// One CBM DOS block, by the address the recording states for it.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct D64Block {
    pub track: u8,
    pub sector: u8,
}

#[pymethods]
impl D64Block {
    fn __repr__(&self) -> String {
        format!("D64Block(track={}, sector={})", self.track, self.sector)
    }
}

/// What a d64 rendition carried, or will carry, of one image.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct D64Report {
    /// Where the artifact was written, or `None` for a rendition
    /// computed and not written.
    pub path: Option<String>,
    /// What the artifact occupies on storage: 683 blocks, and the error
    /// map beside them wherever the disk is incomplete.
    pub artifact_bytes: u64,
    pub blocks_read: u32,
    /// What the CBM DOS grid defines, which is 683 whatever was read.
    pub blocks_defined: u32,
    /// Sectors whose header or data failed its own checksum — recorded
    /// and left out, never repaired.
    pub failed_checksums: u32,
    /// Every block the recording did not yield, in grid order. The
    /// artifact's error map says the same thing in the format's own
    /// spelling.
    pub missing: Vec<D64Block>,
    /// What the destination did not carry, in the image's own terms.
    pub declared_loss: Vec<DeclaredLoss>,
}

#[pymethods]
impl D64Report {
    fn __repr__(&self) -> String {
        format!(
            "D64Report(blocks_read={}/{}, missing={})",
            self.blocks_read,
            self.blocks_defined,
            self.missing.len()
        )
    }
}

impl D64Report {
    fn new(report: &remanence::D64Report) -> Self {
        Self {
            path: report.path.clone(),
            artifact_bytes: report.artifact_bytes,
            blocks_read: report.blocks_read,
            blocks_defined: report.blocks_defined,
            failed_checksums: report.failed_checksums,
            missing: report
                .missing
                .iter()
                .map(|block| D64Block {
                    track: block.track,
                    sector: block.sector,
                })
                .collect(),
            declared_loss: declared_loss(&report.declared_loss),
        }
    }
}

/// One half-track slot a g64 carries.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct G64HalfTrack {
    /// The g64's own slot: 0 is track 1, and the odd indices are the
    /// half-tracks between the whole ones.
    pub index: u64,
    /// How many channel bits the slot carries.
    pub bits: u64,
    /// Which of the 1541's four rates it was packed at.
    pub speed_zone: u8,
    /// Whether the orbit was clocked at its zone's nominal cell because
    /// its own measured figure was not a recording's.
    pub clocked_at_nominal: bool,
}

#[pymethods]
impl G64HalfTrack {
    fn __repr__(&self) -> String {
        format!(
            "G64HalfTrack(index={}, bits={}, speed_zone={})",
            self.index, self.bits, self.speed_zone
        )
    }
}

/// What a g64 rendition carried, or will carry, of one image.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct G64Report {
    /// Where the artifact was written, or `None` for a rendition
    /// computed and not written.
    pub path: Option<String>,
    /// What the artifact occupies on storage.
    pub artifact_bytes: u64,
    /// Every slot the artifact carries, ascending.
    pub half_tracks: Vec<G64HalfTrack>,
    /// What the destination did not carry, in the image's own terms.
    pub declared_loss: Vec<DeclaredLoss>,
}

#[pymethods]
impl G64Report {
    fn __repr__(&self) -> String {
        format!(
            "G64Report(half_tracks={}, declared_loss={})",
            self.half_tracks.len(),
            self.declared_loss.len()
        )
    }
}

impl G64Report {
    fn new(report: &remanence::G64Report) -> Self {
        Self {
            path: report.path.clone(),
            artifact_bytes: report.artifact_bytes,
            half_tracks: report
                .half_tracks
                .iter()
                .map(|half_track| G64HalfTrack {
                    index: half_track.index,
                    bits: half_track.bits,
                    speed_zone: half_track.speed_zone,
                    clocked_at_nominal: half_track.clocked_at_nominal,
                })
                .collect(),
            declared_loss: declared_loss(&report.declared_loss),
        }
    }
}

fn declared_loss(account: &[remanence::DeclaredLoss]) -> Vec<DeclaredLoss> {
    account
        .iter()
        .map(|loss| DeclaredLoss {
            code: loss.code.clone(),
            detail: loss.detail.clone(),
            count: loss.count,
        })
        .collect()
}

/// One remanence image, opened from a `.remanence` artifact.
///
/// The image is the flux family's physical stratum — what the medium
/// holds, stated as facts of the surfaces, distinct from any capture of
/// them. Opening claims the file — writes denied to every other process
/// — decodes the whole image once into private session storage, and
/// holds the claim until the image is closed or collected.
#[pyclass(module = "remanence")]
pub struct RemanenceImage {
    inner: Option<remanence::RemanenceImage>,
    report: RemanenceImageReport,
}

#[pymethods]
impl RemanenceImage {
    /// Opens the `.remanence` artifact at `path`. The magic, the binary
    /// sentinel and the layout version are checked before anything else
    /// is believed, and a version past this release's claim is refused
    /// by name. `cache_bytes` declares the session working set; the
    /// bound narrows what stays resident and never refuses service.
    #[new]
    #[pyo3(signature = (path, *, cache_bytes = None))]
    fn new(path: PathBuf, cache_bytes: Option<u64>) -> PyResult<Self> {
        let image = match cache_bytes {
            Some(cache_bytes) => remanence::RemanenceImage::open_with_cache(path, cache_bytes),
            None => remanence::RemanenceImage::open(path),
        }
        .map_err(to_py_err)?;
        let report = RemanenceImageReport::new(&image.inspect());
        Ok(Self {
            inner: Some(image),
            report,
        })
    }

    /// The image as it stands: its shape, its holes, and every orbit's
    /// identity and counts.
    fn inspect(&self) -> RemanenceImageReport {
        self.report.clone()
    }

    /// The artifact the image was opened from.
    #[getter]
    fn path(&self) -> PyResult<Option<String>> {
        Ok(self
            .get()?
            .path()
            .map(|path| path.to_string_lossy().into_owned()))
    }

    /// The artifact format's stable identifier: `"remanence"`.
    #[getter]
    fn format_id(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_id())
    }

    /// That format's human-readable name.
    #[getter]
    fn format_name(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_name())
    }

    /// `"read-write"` or `"read-only"`: which mode the deny-write claim
    /// on the artifact was obtained in.
    #[getter]
    fn access_mode(&self) -> PyResult<Option<&'static str>> {
        Ok(self.get()?.access_mode().map(mode_str))
    }

    /// How many bytes of private session storage the decoded points
    /// occupy.
    #[getter]
    fn backing_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.backing_bytes())
    }

    /// How much of that backing is currently resident. The points are
    /// never held whole.
    #[getter]
    fn resident_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.resident_bytes())
    }

    /// Writes this image into a new `.remanence` artifact at `path`,
    /// and reports what the artifact carried.
    ///
    /// The image is untouched. An existing destination is a named
    /// refusal rather than an overwrite, and an interruption leaves the
    /// destination absent rather than half an artifact. The bytes are
    /// deterministic — the same image spells the same artifact, every
    /// time.
    fn write(&self, path: PathBuf) -> PyResult<RemanenceWriteReport> {
        let written = self.get()?.write(path).map_err(to_py_err)?;
        Ok(RemanenceWriteReport {
            path: written.path.clone(),
            artifact_bytes: written.artifact_bytes,
            orbits: written.orbits,
            points: written.points,
            declared_loss: written
                .declared_loss
                .iter()
                .map(|loss| DeclaredLoss {
                    code: loss.code.clone(),
                    detail: loss.detail.clone(),
                    count: loss.count,
                })
                .collect(),
        })
    }

    /// Computes the d64 this image renders to, writing nothing. Read it
    /// before writing: the write adds nothing to the account.
    fn describe_d64(&self) -> PyResult<D64Report> {
        self.get()?
            .describe_d64()
            .map(|report| D64Report::new(&report))
            .map_err(to_py_err)
    }

    /// Writes this image into a new d64 at `path` and reports what the
    /// artifact carried.
    ///
    /// The recording's own sectors are read by the family's group code
    /// and laid into the CBM DOS 683-block grid, addressed by the
    /// header's own track and sector. Nothing is repaired and nothing is
    /// rejected, and an incomplete disk carries the error map — this
    /// rendition's declared-loss account made flesh. An existing
    /// destination is a named refusal rather than an overwrite.
    fn write_d64(&self, path: PathBuf) -> PyResult<D64Report> {
        self.get()?
            .write_d64(path)
            .map(|report| D64Report::new(&report))
            .map_err(to_py_err)
    }

    /// Computes the g64 this image renders to, writing nothing.
    fn describe_g64(&self) -> PyResult<G64Report> {
        self.get()?
            .describe_g64()
            .map(|report| G64Report::new(&report))
            .map_err(to_py_err)
    }

    /// Writes this image into a new g64 at `path` and reports what the
    /// artifact carried.
    ///
    /// Every on-grid orbit is clocked at its measured cell — or at its
    /// zone's nominal where the measured figure is not a recording's —
    /// and packed under the `GCR-1541` grammar, one speed zone per
    /// half-track. An existing destination is a named refusal rather
    /// than an overwrite.
    fn write_g64(&self, path: PathBuf) -> PyResult<G64Report> {
        self.get()?
            .write_g64(path)
            .map(|report| G64Report::new(&report))
            .map_err(to_py_err)
    }

    /// Computes what a p64 will and will not carry of this image,
    /// writing nothing.
    fn describe_p64(&self) -> PyResult<P64Report> {
        self.get()?
            .describe_p64()
            .map(|report| P64Report::new(&report))
            .map_err(to_py_err)
    }

    /// Writes this image into a new p64 at `path` and reports what the
    /// container carried.
    ///
    /// One multiply carries an angle to a cycle over the coherent points
    /// only, and an orbit with no pulse is skipped rather than written
    /// empty: an absent half-track claims never-written, where an empty
    /// chunk would claim formatted-then-erased.
    fn write_p64(&self, path: PathBuf) -> PyResult<P64Report> {
        self.get()?
            .write_p64(path)
            .map(|report| P64Report::new(&report))
            .map_err(to_py_err)
    }

    /// Materializes the family's hardware bitstream from what this image
    /// holds, under declared mechanics and read-channel rules.
    ///
    /// The image carries no clock — a cell length is a property of a
    /// recording, recoverable from the image, never a field of it — so
    /// the ladder stands on the served projection of it rather than on
    /// the image directly. The image is untouched and stays exactly what
    /// it was: the bitstream is separate session state, carrying the
    /// image's own provenance beneath the channel that produced it.
    /// There is no way back down.
    #[pyo3(signature = (policy, *, cache_bytes = None))]
    fn materialize_c1541_bitstream(
        &self,
        policy: &ReadChannelPolicy,
        cache_bytes: Option<u64>,
    ) -> PyResult<C1541Bitstream> {
        C1541Bitstream::produce(
            self.get()?
                .materialize_c1541_bitstream(
                    policy.to_core()?,
                    cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES),
                )
                .map_err(to_py_err)?,
        )
    }

    /// Releases the claim on the artifact and discards the private
    /// session storage its points decoded into.
    fn close(&mut self) {
        self.inner = None;
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exception_type: Bound<'_, PyAny>,
        _exception: Bound<'_, PyAny>,
        _traceback: Bound<'_, PyAny>,
    ) -> bool {
        self.inner = None;
        false
    }

    fn __repr__(&self) -> String {
        format!(
            "RemanenceImage(form_factor={:?}, orbits={})",
            self.report.form_factor,
            self.report.orbits.len()
        )
    }
}

impl RemanenceImage {
    fn get(&self) -> PyResult<&remanence::RemanenceImage> {
        self.inner
            .as_ref()
            .ok_or_else(|| categorized_py_err(remanence::ErrorCategory::Io, "image is closed"))
    }
}

#[pymodule(name = "remanence")]
fn remanence_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // The distribution version (pyproject.toml) governs; the crate version is
    // only a fallback for uninstalled contexts.
    let version = m
        .py()
        .import("importlib.metadata")
        .and_then(|metadata| metadata.call_method1("version", ("remanence",)))
        .and_then(|version| version.extract::<String>())
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned());
    m.add("__version__", version)?;
    m.add("DEFAULT_CACHE_BYTES", remanence::DEFAULT_CACHE_BYTES)?;
    m.add("RemanenceError", m.py().get_type::<RemanenceError>())?;
    m.add_class::<CaptureSet>()?;
    m.add_class::<CaptureSetReport>()?;
    m.add_class::<CaptureSetMember>()?;
    m.add_class::<CaptureRunReport>()?;
    m.add_class::<ObservationReport>()?;
    m.add_class::<CaptureIssue>()?;
    m.add_class::<StepPosition>()?;
    m.add_class::<TimeBaseReport>()?;
    m.add_class::<Recognition>()?;
    m.add_class::<ProfileVerdict>()?;
    m.add_class::<LocationVerdict>()?;
    m.add_class::<ZoneClaim>()?;
    m.add_class::<ReadChannelPolicy>()?;
    m.add_class::<GcrCodecPolicy>()?;
    m.add_class::<C1541Bitstream>()?;
    m.add_class::<C1541Bytestream>()?;
    m.add_class::<BitstreamReport>()?;
    m.add_class::<BitstreamLocation>()?;
    m.add_class::<BytestreamReport>()?;
    m.add_class::<BytestreamLocation>()?;
    m.add_class::<SectorPolicy>()?;
    m.add_class::<C1541Sectors>()?;
    m.add_class::<SectorReport>()?;
    m.add_class::<SectorLocation>()?;
    m.add_class::<SectorClaim>()?;
    m.add_class::<ContestedAddress>()?;
    m.add_class::<P64Image>()?;
    m.add_class::<RemanenceImage>()?;
    m.add_class::<RemanenceImageReport>()?;
    m.add_class::<RemanenceHole>()?;
    m.add_class::<RemanenceOrbit>()?;
    m.add_class::<RemanenceWriteReport>()?;
    m.add_class::<ReconstructionPolicy>()?;
    m.add_class::<ReconstructionPlan>()?;
    m.add_class::<ReconstructionReport>()?;
    m.add_class::<ReconstructedOrbit>()?;
    m.add_class::<D64Report>()?;
    m.add_class::<D64Block>()?;
    m.add_class::<G64Report>()?;
    m.add_class::<G64HalfTrack>()?;
    m.add_class::<P64Report>()?;
    m.add_class::<P64HalfTrack>()?;
    m.add_class::<DeclaredLoss>()?;
    m.add_class::<Identification>()?;
    m.add_class::<Layer>()?;
    m.add_class::<SizeInformation>()?;
    m.add_class::<ArchiveLayout>()?;
    m.add_class::<ImageLayout>()?;
    m.add_class::<DiskLayout>()?;
    m.add_class::<TrackSectorLayout>()?;
    m.add_class::<FilesystemLayout>()?;
    m.add_class::<Discovery>()?;
    m.add_class::<Session>()?;
    m.add_class::<Machine>()?;
    m.add_class::<StorageDevice>()?;
    m.add_class::<Medium>()?;
    m.add_class::<DeviceFamily>()?;
    m.add_class::<Assurance>()?;
    m.add_class::<DiskReport>()?;
    m.add_class::<DeviceInfo>()?;
    m.add_class::<PartitionSchemaInfo>()?;
    m.add_class::<RegionInfo>()?;
    m.add_class::<VolumeInfo>()?;
    m.add_class::<FilesystemInfo>()?;
    m.add_class::<VolumeLabel>()?;
    m.add_class::<LabelReading>()?;
    m.add_class::<Entry>()?;
    m.add_class::<EntryFact>()?;
    m.add_class::<StorageSpace>()?;
    m.add_class::<File>()?;
    m.add_class::<DosMachine>()?;
    m.add_class::<DriveMap>()?;
    m.add_class::<DriveMapping>()?;
    m.add_class::<DosAssignmentRule>()?;
    m.add_function(wrap_pyfunction!(assurance_conditions, m)?)?;
    m.add_function(wrap_pyfunction!(device_families, m)?)?;
    m.add_function(wrap_pyfunction!(discover_media, m)?)?;
    m.add_function(wrap_pyfunction!(dos_assignment_rules, m)?)?;
    m.add_function(wrap_pyfunction!(formats, m)?)?;
    Ok(())
}
