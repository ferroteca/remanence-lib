// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Python bindings for the Remanence disk image analysis library.
//!
//! The module mirrors the Rust crate's public surface: `Archive` lists
//! what a supported archive holds (`.zip`, `.7z`), and `Disk` opens a
//! disk image (optionally an entry inside one of those archives) under
//! one P7 claim that serves both of the medium's planes —
//! `Disk.identify()` reports the detected container layers over the
//! image's own bytes, while `Disk.inspect()` and the volume-scoped file
//! verbs work over the disk a format adapter presents above them.
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

fn kind_str(kind: remanence::ContainerKind) -> &'static str {
    match kind {
        remanence::ContainerKind::Archive => "archive",
        remanence::ContainerKind::Image => "image",
        remanence::ContainerKind::PhysicalMedia => "physical-media",
        remanence::ContainerKind::Filesystem => "filesystem",
        remanence::ContainerKind::Unknown => "unknown",
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
    pub path: String,
    pub entry_name: String,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: Option<u64>,
}

/// Where the payload sits inside a raw image container.
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

/// Physical disk geometry derived from a container format.
///
/// `sector_layout` is `"unknown"`, `"fixed"`, or `"variable"`;
/// `sectors_per_track` is set for fixed layouts and `tracks` for variable ones.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DiskLayout {
    pub media_kind: Option<String>,
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
            media_kind: layout.media_kind.clone(),
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

/// One detected container layer.
///
/// `kind` and `layout_kind` are `"archive"`, `"image"`, `"physical-media"`,
/// `"filesystem"`, or `"unknown"`. `layout` is the matching layout object —
/// `ArchiveLayout`, `ImageLayout`, `DiskLayout`, `FilesystemLayout` — or
/// `None` when no layout details are known.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct Container {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub confidence: u8,
    pub known: bool,
    pub size: SizeInformation,
    pub layout_kind: String,
    pub layout: Option<Py<PyAny>>,
}

impl Container {
    fn new(py: Python<'_>, container: &remanence::Container) -> PyResult<Self> {
        let (layout_kind, layout) = match &container.layout {
            remanence::ContainerLayout::Unknown => ("unknown", None),
            remanence::ContainerLayout::Archive(layout) => (
                "archive",
                Some(
                    Py::new(
                        py,
                        ArchiveLayout {
                            path: layout.path.display().to_string(),
                            entry_name: layout.entry_name.clone(),
                            compressed_size: layout.compressed_size,
                            uncompressed_size: layout.uncompressed_size,
                        },
                    )?
                    .into_any(),
                ),
            ),
            remanence::ContainerLayout::Image(layout) => (
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
            remanence::ContainerLayout::PhysicalMedia(layout) => match layout {
                remanence::PhysicalMediaLayout::Unknown => ("physical-media", None),
                remanence::PhysicalMediaLayout::Disk(disk) => (
                    "physical-media",
                    Some(Py::new(py, DiskLayout::new(disk))?.into_any()),
                ),
            },
            remanence::ContainerLayout::Filesystem(layout) => (
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
            kind: kind_str(container.kind).to_owned(),
            id: container.id.clone(),
            name: container.name.clone(),
            confidence: container.confidence,
            known: container.known,
            size: SizeInformation {
                current_bytes: container.size.current_bytes,
                expected_bytes: container.size.expected_bytes,
            },
            layout_kind: layout_kind.to_owned(),
            layout,
        })
    }
}

/// The result of identifying a session's image.
#[pyclass(frozen, get_all, module = "remanence")]
pub struct Identification {
    pub containers: Vec<Container>,
    pub modified: bool,
    pub evidence: Vec<String>,
}

/// One file listed in an HDOS directory.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct HdosFile {
    pub name: String,
    pub extension: String,
    pub size_sectors: u32,
    pub modified_date: u16,
    pub flags: u8,
}

#[pymethods]
impl HdosFile {
    /// `"NAME.EXT"` or `"NAME"` when the extension is empty.
    #[getter]
    fn display_name(&self) -> String {
        self.as_core().display_name()
    }

    /// Size in bytes (`size_sectors * 256`).
    #[getter]
    fn size_bytes(&self) -> u64 {
        self.as_core().size_bytes()
    }

    /// HDOS flag letters (subset of `"SLWC"`), possibly empty.
    #[getter]
    fn flags_string(&self) -> String {
        self.as_core().flags_string()
    }

    /// HDOS catalog date, e.g. `"09-May-78"`, or `"No-Date"`.
    #[getter]
    fn modified_date_string(&self) -> String {
        self.as_core().modified_date_string()
    }

    fn __repr__(&self) -> String {
        format!(
            "HdosFile(name={:?}, extension={:?}, size_sectors={}, modified_date={}, flags={})",
            self.name, self.extension, self.size_sectors, self.modified_date, self.flags
        )
    }
}

impl HdosFile {
    fn new(file: &remanence::HdosFile) -> Self {
        Self {
            name: file.name.clone(),
            extension: file.extension.clone(),
            size_sectors: file.size_sectors,
            modified_date: file.modified_date,
            flags: file.flags,
        }
    }

    fn as_core(&self) -> remanence::HdosFile {
        remanence::HdosFile {
            name: self.name.clone(),
            extension: self.extension.clone(),
            size_sectors: self.size_sectors,
            modified_date: self.modified_date,
            flags: self.flags,
        }
    }
}

/// An open analysis session over one disk image.
/// The one addressed device the image adapter supplied. `id` is scoped to
/// this open (P21), unlike the layout-derived identities below.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DeviceInfo {
    pub id: u64,
    pub image_format: String,
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
    /// container is a primary slot whose role is structural.
    pub declared_placement: String,
    /// `"data"` or `"container"`.
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

        Ok(DriveMap {
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
        })
    }

    fn __repr__(&self) -> String {
        format!("DosMachine(devices={})", self.devices.len())
    }
}

/// One FAT directory entry; `kind` is `"file"` or `"directory"`.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct FatEntry {
    pub name: String,
    pub kind: String,
    pub size_bytes: u64,
}

impl FatEntry {
    fn new(entry: &remanence::FatEntry) -> Self {
        Self {
            name: entry.name.clone(),
            kind: match entry.kind {
                remanence::FatEntryKind::File => "file".to_owned(),
                remanence::FatEntryKind::Directory => "directory".to_owned(),
            },
            size_bytes: entry.size_bytes,
        }
    }
}

fn mode_str(mode: remanence::AccessMode) -> &'static str {
    match mode {
        remanence::AccessMode::ReadWrite => "read-write",
        remanence::AccessMode::ReadOnly => "read-only",
    }
}

/// An open session: the machine scope, holding a set of family-typed
/// storage devices (P32).
///
/// There is no separate machine object — a session *is* the scope within
/// which device identity is resolved.
#[pyclass(module = "remanence")]
pub struct Session {
    inner: Arc<Mutex<remanence::Session>>,
}

#[pymethods]
impl Session {
    /// A session with no devices. Devices are attached and detached over
    /// its life; the set is not fixed at open.
    #[new]
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(remanence::Session::new())),
        }
    }

    /// Attaches the medium at `path` — a raw disk image, or
    /// `archive[/entry]` — to a new device in the lowest free slot of
    /// its family, returning the attachment identity it took (`"hdd0"`).
    /// `writable=True` claims the medium exclusively and fails at the
    /// open when that claim cannot be secured, never by falling back.
    #[pyo3(signature = (path, *, writable, cache_bytes = None))]
    fn attach(
        &self,
        path: PathBuf,
        writable: bool,
        cache_bytes: Option<u64>,
    ) -> PyResult<String> {
        let intent = if writable {
            remanence::AccessIntent::Write
        } else {
            remanence::AccessIntent::Read
        };
        let mut session = self.lock();
        let attachment = match cache_bytes {
            Some(cache_bytes) => session.attach_with_cache(path, intent, cache_bytes),
            None => session.attach(path, intent),
        }
        .map_err(to_py_err)?;
        Ok(attachment.to_string())
    }

    /// Attaches the medium at `path` to the slot `attachment` names
    /// (such as `"hdd1"`). The caller chooses the slot, never the name.
    /// An occupied slot is refused rather than displaced, and a family
    /// this release does not claim is refused by name.
    #[pyo3(signature = (attachment, path, *, writable, cache_bytes = None))]
    fn attach_at(
        &self,
        attachment: &str,
        path: PathBuf,
        writable: bool,
        cache_bytes: Option<u64>,
    ) -> PyResult<String> {
        let intent = if writable {
            remanence::AccessIntent::Write
        } else {
            remanence::AccessIntent::Read
        };
        let attachment = remanence::AttachmentId::parse(attachment).map_err(to_py_err)?;
        let mut session = self.lock();
        match cache_bytes {
            Some(cache_bytes) => session.attach_at_with_cache(
                attachment.family(),
                attachment.index(),
                path,
                intent,
                cache_bytes,
            ),
            None => session.attach_at(attachment.family(), attachment.index(), path, intent),
        }
        .map_err(to_py_err)?;
        Ok(attachment.to_string())
    }

    /// Detaches the device at `attachment`, releasing its medium's claim
    /// and freeing the slot.
    fn detach(&self, attachment: &str) -> PyResult<()> {
        let attachment = remanence::AttachmentId::parse(attachment).map_err(to_py_err)?;
        self.lock().detach(attachment).map_err(to_py_err)
    }

    /// The attachment identities currently in use, in slot-fill order.
    #[getter]
    fn devices(&self) -> Vec<String> {
        self.lock()
            .attachments()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// The medium in the device at `attachment`. The session owns it;
    /// the returned object stays valid until that device is detached.
    fn medium(&self, attachment: &str) -> PyResult<Disk> {
        let attachment = remanence::AttachmentId::parse(attachment).map_err(to_py_err)?;
        self.lock().medium(attachment).map_err(to_py_err)?;
        Ok(Disk {
            session: Arc::clone(&self.inner),
            attachment,
        })
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

/// The medium in one storage device, reached through the session that
/// holds it. Nothing is reachable except through a device (P32).
#[pyclass(module = "remanence")]
pub struct Disk {
    session: Arc<Mutex<remanence::Session>>,
    attachment: remanence::AttachmentId,
}

/// A borrow of the session with one medium selected.
///
/// It dereferences to the medium, so every verb below reads as though it
/// held the disk directly while the session stays the owner. The medium
/// is re-resolved on each borrow, so a detached device refuses rather
/// than reaching freed state.
struct MediumGuard<'a> {
    session: MutexGuard<'a, remanence::Session>,
    attachment: remanence::AttachmentId,
}

impl std::ops::Deref for MediumGuard<'_> {
    type Target = remanence::Disk;

    fn deref(&self) -> &Self::Target {
        self.session
            .device(self.attachment)
            .and_then(remanence::StorageDevice::medium)
            .expect("the device was present when this guard was taken")
    }
}

impl std::ops::DerefMut for MediumGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.session
            .medium(self.attachment)
            .expect("the device was present when this guard was taken")
    }
}

impl Disk {
    fn get(&mut self) -> PyResult<MediumGuard<'_>> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if session.device(self.attachment).is_none() {
            return Err(categorized_py_err(
                remanence::ErrorCategory::NotFound,
                "the device holding this medium was detached",
            ));
        }
        Ok(MediumGuard {
            session,
            attachment: self.attachment,
        })
    }
}

#[pymethods]
impl Disk {
    /// The artifact the disk was opened from (the archive path for
    /// archive inputs).
    #[getter]
    fn path(&mut self) -> PyResult<String> {
        Ok(self.get()?.path().to_owned())
    }

    /// The resolved image path (the entry name for archive inputs).
    #[getter]
    fn image_path(&mut self) -> PyResult<String> {
        Ok(self.get()?.image_path().display().to_string())
    }

    /// The resolved image's own size in bytes — the raw plane. Distinct
    /// from `size`, which is the presented disk's size; for a qcow2 the
    /// two differ.
    #[getter]
    fn image_size_bytes(&mut self) -> PyResult<u64> {
        Ok(self.get()?.image_size_bytes())
    }

    /// Reads `length` bytes of the resolved image at `offset` — the
    /// bounded access form: the image streams from its backing and is
    /// never resident whole.
    fn read_at<'py>(
        &mut self,
        py: Python<'py>,
        offset: u64,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut buffer = vec![0u8; length];
        self.get()?.read_at(offset, &mut buffer).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Identifies the image's container layers and probable filesystem.
    fn identify(&mut self, py: Python<'_>) -> PyResult<Identification> {
        let identification = self.get()?.identify();
        let containers = identification
            .containers
            .iter()
            .map(|container| Container::new(py, container))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Identification {
            containers,
            modified: identification.modified,
            evidence: identification.evidence,
        })
    }

    /// Parses the HDOS directory from the disk's image.
    fn list_hdos_files(&mut self) -> PyResult<Vec<HdosFile>> {
        self.get()?
            .list_hdos_files()
            .map(|files| files.iter().map(HdosFile::new).collect())
            .map_err(to_py_err)
    }

    /// Reads a cataloged HDOS file's contents out of the disk's image.
    fn read_hdos_file<'py>(
        &mut self,
        py: Python<'py>,
        name: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.get()?.read_hdos_file(name).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// `"read-write"` or `"read-only"` — an echo of the declared intent.
    #[getter]
    fn mode(&mut self) -> PyResult<&'static str> {
        Ok(mode_str(self.get()?.mode()))
    }

    /// `"raw"`, `"qcow2"` or `"vdi"`.
    #[getter]
    fn format(&mut self) -> PyResult<&'static str> {
        Ok(match self.get()?.format() {
            remanence::DiskFormat::Raw => "raw",
            remanence::DiskFormat::Qcow2 { .. } => "qcow2",
            remanence::DiskFormat::Vdi { .. } => "vdi",
        })
    }

    /// The qcow2 version, or `None` for an image of any other format.
    #[getter]
    fn qcow2_version(&mut self) -> PyResult<Option<u32>> {
        Ok(match self.get()?.format() {
            remanence::DiskFormat::Qcow2 { version } => Some(version),
            remanence::DiskFormat::Raw | remanence::DiskFormat::Vdi { .. } => None,
        })
    }

    /// The VDI version as a `(major, minor)` pair, or `None` for an image
    /// of any other format.
    #[getter]
    fn vdi_version(&mut self) -> PyResult<Option<(u32, u32)>> {
        Ok(match self.get()?.format() {
            remanence::DiskFormat::Vdi { major, minor } => Some((major, minor)),
            remanence::DiskFormat::Raw | remanence::DiskFormat::Qcow2 { .. } => None,
        })
    }

    /// The virtual disk size in bytes.
    #[getter]
    fn size(&mut self) -> PyResult<u64> {
        Ok(self.get()?.size())
    }

    /// Whether uncommitted changes exist.
    #[getter]
    fn is_modified(&mut self) -> PyResult<bool> {
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
    fn inspect(&mut self) -> PyResult<DiskReport> {
        let report = self.get()?.inspect().map_err(to_py_err)?;
        let issues = |issues: &[remanence::Error]| -> Vec<String> {
            issues.iter().map(|issue| issue.to_string()).collect()
        };
        Ok(DiskReport {
            device: DeviceInfo {
                id: report.device.id,
                image_format: report.device.image_format.clone(),
                length_bytes: report.device.length_bytes,
                authoritative_layer: report.device.authoritative_layer.clone(),
                active_layer: report.device.active_layer.clone(),
            },
            content: report.content.name().to_owned(),
            content_evidence: match &report.content {
                remanence::DiskContent::UnknownNonblank { evidence } => Some(evidence.clone()),
                _ => None,
            },
            partition_schema: report.partition_schema.as_ref().map(|schema| {
                PartitionSchemaInfo {
                    kind: schema.kind.clone(),
                    evidence: schema.evidence.clone(),
                    issues: issues(&schema.issues),
                }
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
        })
    }

    /// Lists a directory in `volume_id` ("" = root, "A/B" descends).
    #[pyo3(signature = (volume_id, path = ""))]
    fn entries(&mut self, volume_id: u64, path: &str) -> PyResult<Vec<FatEntry>> {
        Ok(self
            .get()?
            .entries(remanence::VolumeId::from_value(volume_id), path)
            .map_err(to_py_err)?
            .iter()
            .map(FatEntry::new)
            .collect())
    }

    /// Answers one path in `volume_id` with its entry, or `None` when
    /// nothing exists at that path — a missing leaf, a missing parent,
    /// or a parent that is a file alike. Absence is an answer,
    /// distinguished from failure, which raises.
    fn stat(&mut self, volume_id: u64, path: &str) -> PyResult<Option<FatEntry>> {
        Ok(self
            .get()?
            .stat(remanence::VolumeId::from_value(volume_id), path)
            .map_err(to_py_err)?
            .as_ref()
            .map(FatEntry::new))
    }

    /// Copies a file's bytes out of `volume_id`.
    fn read_file<'py>(
        &mut self,
        py: Python<'py>,
        volume_id: u64,
        path: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.get()?.read_file(remanence::VolumeId::from_value(volume_id), path).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Reads part of a file — the streamed form, `os.pread`-shaped:
    /// exactly `length` bytes at `offset`, which must lie within the
    /// file.
    fn read_file_at<'py>(
        &mut self,
        py: Python<'py>,
        volume_id: u64,
        path: &str,
        offset: u64,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut buffer = vec![0u8; length];
        self.get()?
            .read_file_at(remanence::VolumeId::from_value(volume_id), path, offset, &mut buffer)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Writes a file into `volume_id`. An existing file is overwritten —
    /// shorter or longer, its old clusters released and reclaimed —
    /// while an existing directory is refused. Buffered until
    /// `commit()`.
    fn write_file(&mut self, volume_id: u64, path: &str, contents: &[u8]) -> PyResult<()> {
        self.get()?
            .write_file(remanence::VolumeId::from_value(volume_id), path, contents)
            .map_err(to_py_err)
    }

    /// Sets a file's size, creating it when absent — `truncate`-shaped:
    /// kept bytes preserved in place, a grown region reads as zeros.
    /// With `write_file_at` this is the streamed replacement for
    /// `write_file`. Buffered until `commit()`.
    fn resize_file(&mut self, volume_id: u64, path: &str, size: u64) -> PyResult<()> {
        self.get()?
            .resize_file(remanence::VolumeId::from_value(volume_id), path, size)
            .map_err(to_py_err)
    }

    /// Writes part of a file in place — the streamed form,
    /// `os.pwrite`-shaped: the span must lie within the file's current
    /// size (resize first to change it). Buffered until `commit()`.
    fn write_file_at(
        &mut self,
        volume_id: u64,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> PyResult<()> {
        self.get()?
            .write_file_at(remanence::VolumeId::from_value(volume_id), path, offset, data)
            .map_err(to_py_err)
    }

    /// Ensures a directory exists in `volume_id`: missing parents are
    /// created, and a path that already leads to a directory succeeds
    /// unchanged. Buffered until `commit()`.
    fn make_directory(&mut self, volume_id: u64, path: &str) -> PyResult<()> {
        self.get()?
            .make_directory(remanence::VolumeId::from_value(volume_id), path)
            .map_err(to_py_err)
    }

    /// The commit point: everything buffered reaches the image, flushed.
    /// The commit is durable (P9): a private recovery journal is armed
    /// before the first byte of the file changes, so an interruption at
    /// any point leaves state the next open reconciles to wholly the
    /// old image or wholly the committed new one.
    fn commit(&mut self) -> PyResult<()> {
        self.get()?.commit().map_err(to_py_err)
    }

    /// Discards everything buffered; the image is untouched.
    fn rollback(&mut self) -> PyResult<()> {
        self.get()?.rollback();
        Ok(())
    }

    /// The attachment identity of the device holding this medium.
    #[getter]
    fn attachment(&self) -> String {
        self.attachment.to_string()
    }
}

/// One entry an archive holds.
///
/// `compressed_size` is `None` where the grammar attributes no packed
/// size to a single entry — a member of a solid 7z folder is compressed
/// together with its neighbours, so no share of the packed bytes is its
/// own.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ArchiveEntry {
    /// The entry's path inside the archive, `/`-separated.
    pub name: String,
    pub is_dir: bool,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: u64,
}

#[pymethods]
impl ArchiveEntry {
    fn __repr__(&self) -> String {
        format!(
            "ArchiveEntry(name={:?}, is_dir={}, uncompressed_size={})",
            self.name, self.is_dir, self.uncompressed_size
        )
    }
}

impl ArchiveEntry {
    fn new(entry: &remanence::ArchiveEntry) -> Self {
        Self {
            name: entry.name.clone(),
            is_dir: entry.is_dir,
            compressed_size: entry.compressed_size,
            uncompressed_size: entry.uncompressed_size,
        }
    }
}

/// An archive's entries, read under the claim this listing holds.
///
/// Opening claims the archive file — writes denied to every other
/// process — until the object is closed or dropped. Only the archive's
/// own index is read; entry data is never touched.
#[pyclass(module = "remanence")]
pub struct Archive {
    inner: Option<remanence::Archive>,
}

impl Archive {
    fn get(&self) -> PyResult<&remanence::Archive> {
        self.inner
            .as_ref()
            .ok_or_else(|| categorized_py_err(remanence::ErrorCategory::Io, "archive is closed"))
    }
}

#[pymethods]
impl Archive {
    /// Opens the archive at `path`. A path naming no archive format this
    /// library reads is refused by name, never guessed at.
    #[new]
    fn new(path: PathBuf) -> PyResult<Self> {
        remanence::Archive::open(path)
            .map(|inner| Self { inner: Some(inner) })
            .map_err(to_py_err)
    }

    /// The path the archive was opened from.
    #[getter]
    fn path(&self) -> PyResult<String> {
        Ok(self.get()?.path().display().to_string())
    }

    /// The archive format's stable identifier: `"zip"` or `"7z"`.
    #[getter]
    fn format_id(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_id())
    }

    /// The archive format's human-readable name.
    #[getter]
    fn format_name(&self) -> PyResult<&'static str> {
        Ok(self.get()?.format_name())
    }

    /// `"read-write"` or `"read-only"`: which mode the deny-write claim
    /// on the archive file was obtained in.
    #[getter]
    fn access_mode(&self) -> PyResult<&'static str> {
        Ok(mode_str(self.get()?.access_mode()))
    }

    /// The archive file's own size in bytes.
    #[getter]
    fn size_bytes(&self) -> PyResult<u64> {
        Ok(self.get()?.size_bytes())
    }

    /// Every entry the archive holds, in the archive's own order.
    #[getter]
    fn entries(&self) -> PyResult<Vec<ArchiveEntry>> {
        Ok(self.get()?.entries().iter().map(ArchiveEntry::new).collect())
    }

    /// Releases the claim on the archive file.
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

    /// Plans the reduction of this capture to one 1541 flux medium
    /// under a declared policy.
    ///
    /// Nothing is written and nothing is mutated. A reduction the
    /// policy does not name is a refusal rather than a default, so the
    /// plan either accounts for the whole capture or does not exist.
    fn plan_c1541_mastering(&self, policy: &MasteringPolicy) -> PyResult<MasteringPlan> {
        let plan = self
            .get()?
            .plan_c1541_mastering(policy.to_core()?)
            .map_err(to_py_err)?;
        let report = MasteringPlanReport::new(plan.report());
        Ok(MasteringPlan {
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

/// One half-track the medium will hold.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct MasteredLocation {
    pub source_position: StepPosition,
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub observation_ordinal: u64,
    pub pulses: u64,
    pub strong_pulses: u64,
    pub weak_pulses: u64,
    /// Where the circle was given its start, in reference-clock cycles.
    pub origin_cycles: u64,
    pub seam_cycles: Option<u64>,
}

#[pymethods]
impl MasteredLocation {
    fn __repr__(&self) -> String {
        format!(
            "MasteredLocation(half_track={}/{}, pulses={})",
            self.half_track_numerator, self.half_track_denominator, self.pulses
        )
    }
}

/// The whole transformation, computed and written nowhere.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct MasteringPlanReport {
    pub profile_id: String,
    pub reference_clock_hz: u64,
    pub cycles_per_rotation: u64,
    pub origin_rule: String,
    pub locations: Vec<MasteredLocation>,
    /// Everything the destination will not carry. A count is not an
    /// account, so each entry says what was lost and in what terms.
    pub declared_loss: Vec<DeclaredLoss>,
    /// The policy that produced this plan, stated in full.
    pub evidence: Vec<String>,
}

#[pymethods]
impl MasteringPlanReport {
    fn __repr__(&self) -> String {
        format!(
            "MasteringPlanReport(locations={}, declared_loss={})",
            self.locations.len(),
            self.declared_loss.len()
        )
    }
}

impl MasteringPlanReport {
    fn new(report: &remanence::MasteringPlanReport) -> Self {
        Self {
            profile_id: report.profile_id.clone(),
            reference_clock_hz: report.reference_clock_hz,
            cycles_per_rotation: report.cycles_per_rotation,
            origin_rule: report.origin_rule.clone(),
            locations: report
                .locations
                .iter()
                .map(|location| MasteredLocation {
                    source_position: StepPosition {
                        numerator: location.source_position.numerator,
                        denominator: location.source_position.denominator,
                    },
                    half_track_numerator: location.half_track_numerator,
                    half_track_denominator: location.half_track_denominator,
                    observation_ordinal: location.observation_ordinal,
                    pulses: location.pulses,
                    strong_pulses: location.strong_pulses,
                    weak_pulses: location.weak_pulses,
                    origin_cycles: location.origin_cycles,
                    seam_cycles: location.seam_cycles,
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

/// The complete declared policy for one reduction.
///
/// Every argument is keyword-only and required, deliberately: each is a
/// decision about evidence, and a reduction that arrived at one by
/// construction rather than by declaration is what P29 forbids.
/// `duplicate` is `"declared"`, `"admit-as-observed"` or `"omit"`;
/// `projection` is `"refuse"` or `"declare-loss"`; `pulse_strength` is
/// `"declared"` with `strength_state` or `"from-agreement"` with
/// `strength_window_cycles`; `origin` is `"declared"` or `"angle"` with
/// `origin_cycles`.
#[pyclass(frozen, get_all, from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct MasteringPolicy {
    pub side: u64,
    pub observation_ordinal: u64,
    pub duplicate: String,
    pub projection: String,
    pub pulse_strength: String,
    pub strength_state: u32,
    pub strength_window_cycles: u64,
    pub origin: String,
    pub origin_cycles: u64,
    pub seed: u64,
}

#[pymethods]
impl MasteringPolicy {
    #[new]
    #[pyo3(signature = (
        *,
        side,
        observation_ordinal,
        duplicate,
        projection,
        pulse_strength,
        origin,
        seed,
        strength_state = 0,
        strength_window_cycles = 0,
        origin_cycles = 0,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        side: u64,
        observation_ordinal: u64,
        duplicate: String,
        projection: String,
        pulse_strength: String,
        origin: String,
        seed: u64,
        strength_state: u32,
        strength_window_cycles: u64,
        origin_cycles: u64,
    ) -> Self {
        Self {
            side,
            observation_ordinal,
            duplicate,
            projection,
            pulse_strength,
            strength_state,
            strength_window_cycles,
            origin,
            origin_cycles,
            seed,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "MasteringPolicy(side={}, observation_ordinal={}, duplicate={:?})",
            self.side, self.observation_ordinal, self.duplicate
        )
    }
}

impl MasteringPolicy {
    fn to_core(&self) -> PyResult<remanence::MasteringPolicy> {
        let named = |what: &str, given: &str, admitted: &[&str]| {
            categorized_py_err(
                remanence::ErrorCategory::Unsupported,
                format!(
                    "{what} policy {given:?} is not one this library admits; it takes \
                     one of {admitted:?}"
                ),
            )
        };
        Ok(remanence::MasteringPolicy {
            side: self.side,
            observation: remanence::ObservationPolicy::Selected {
                ordinal: self.observation_ordinal,
            },
            duplicate: match self.duplicate.as_str() {
                "declared" => remanence::DuplicatePolicy::Declared,
                "admit-as-observed" => remanence::DuplicatePolicy::AdmitAsObserved,
                "omit" => remanence::DuplicatePolicy::Omit,
                other => {
                    return Err(named(
                        "duplicate",
                        other,
                        &["declared", "admit-as-observed", "omit"],
                    ));
                }
            },
            projection: match self.projection.as_str() {
                "refuse" => remanence::ProjectionPolicy::Refuse,
                "declare-loss" => remanence::ProjectionPolicy::DeclareLoss,
                other => return Err(named("projection", other, &["refuse", "declare-loss"])),
            },
            pulse_strength: match self.pulse_strength.as_str() {
                "declared" => remanence::PulseStrengthPolicy::Declared {
                    state: self.strength_state,
                },
                "from-agreement" => remanence::PulseStrengthPolicy::FromAgreement {
                    window_cycles: self.strength_window_cycles,
                },
                other => {
                    return Err(named(
                        "pulse strength",
                        other,
                        &["declared", "from-agreement"],
                    ));
                }
            },
            origin: match self.origin.as_str() {
                "declared" => remanence::OriginPolicy::Declared,
                "angle" => remanence::OriginPolicy::Angle {
                    cycles: self.origin_cycles,
                },
                other => return Err(named("origin", other, &["declared", "angle"])),
            },
            seed: self.seed,
        })
    }
}

/// A planned reduction: everything computed, nothing written.
#[pyclass(module = "remanence")]
pub struct MasteringPlan {
    inner: Option<remanence::MasteringPlan>,
    report: MasteringPlanReport,
}

#[pymethods]
impl MasteringPlan {
    /// What the transformation will do, and everything it will not
    /// carry. Read before executing: executing adds nothing to it.
    fn report(&self) -> MasteringPlanReport {
        self.report.clone()
    }

    /// Produces the medium. The sources are untouched.
    #[pyo3(signature = (*, cache_bytes = None))]
    fn execute(&mut self, cache_bytes: Option<u64>) -> PyResult<MasteredMedium> {
        let plan = self.inner.take().ok_or_else(|| {
            categorized_py_err(
                remanence::ErrorCategory::Io,
                "plan has already been executed",
            )
        })?;
        let medium = plan
            .execute(cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES))
            .map_err(to_py_err)?;
        Ok(MasteredMedium {
            report: self.report.clone(),
            inner: medium,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "MasteringPlan(locations={}, declared_loss={})",
            self.report.locations.len(),
            self.report.declared_loss.len()
        )
    }
}

/// A mastered medium, held in the session. The pulses stay behind this
/// surface.
#[pyclass(module = "remanence")]
pub struct MasteredMedium {
    inner: remanence::MasteredMedium,
    report: MasteringPlanReport,
}

#[pymethods]
impl MasteredMedium {
    /// The plan this medium was produced from, unchanged.
    fn plan(&self) -> MasteringPlanReport {
        self.report.clone()
    }

    /// How many locations the medium claims.
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

    /// What a P64 will and will not carry of this medium, computed and
    /// written nowhere.
    ///
    /// Read it before writing: the write adds nothing to the account. A
    /// medium the container's claim cannot encode is refused here rather
    /// than approximated into it.
    fn describe_p64(&self) -> PyResult<P64Report> {
        self.inner
            .describe_p64()
            .map(|report| P64Report::new(&report))
            .map_err(to_py_err)
    }

    /// Writes this medium into a new P64 image at `path`, and reports
    /// what the container carried and what it did not.
    ///
    /// The medium is untouched. An existing destination is a named
    /// refusal rather than an overwrite, and an interruption leaves the
    /// destination absent rather than half an artifact.
    fn write_p64(&self, path: PathBuf) -> PyResult<P64Report> {
        self.inner
            .write_p64(path)
            .map(|report| P64Report::new(&report))
            .map_err(to_py_err)
    }

    fn __repr__(&self) -> String {
        format!("MasteredMedium(locations={})", self.inner.locations())
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

/// Parses the HDOS directory from raw image bytes.
#[pyfunction]
fn list_hdos_files(image: Vec<u8>) -> PyResult<Vec<HdosFile>> {
    remanence::list_hdos_files(&image)
        .map(|files| files.iter().map(HdosFile::new).collect())
        .map_err(to_py_err)
}

/// Reads a cataloged HDOS file's contents out of raw image bytes.
#[pyfunction]
fn read_hdos_file<'py>(
    py: Python<'py>,
    image: Vec<u8>,
    name: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = remanence::read_hdos_file(&image, name).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &bytes))
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
    m.add_class::<Archive>()?;
    m.add_class::<ArchiveEntry>()?;
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
    m.add_class::<MasteringPolicy>()?;
    m.add_class::<MasteringPlan>()?;
    m.add_class::<MasteringPlanReport>()?;
    m.add_class::<MasteredMedium>()?;
    m.add_class::<P64Image>()?;
    m.add_class::<P64Report>()?;
    m.add_class::<P64HalfTrack>()?;
    m.add_class::<MasteredLocation>()?;
    m.add_class::<DeclaredLoss>()?;
    m.add_class::<Identification>()?;
    m.add_class::<Container>()?;
    m.add_class::<SizeInformation>()?;
    m.add_class::<ArchiveLayout>()?;
    m.add_class::<ImageLayout>()?;
    m.add_class::<DiskLayout>()?;
    m.add_class::<TrackSectorLayout>()?;
    m.add_class::<FilesystemLayout>()?;
    m.add_class::<HdosFile>()?;
    m.add_class::<Session>()?;
    m.add_class::<Disk>()?;
    m.add_class::<DiskReport>()?;
    m.add_class::<DeviceInfo>()?;
    m.add_class::<PartitionSchemaInfo>()?;
    m.add_class::<RegionInfo>()?;
    m.add_class::<VolumeInfo>()?;
    m.add_class::<FilesystemInfo>()?;
    m.add_class::<VolumeLabel>()?;
    m.add_class::<LabelReading>()?;
    m.add_class::<FatEntry>()?;
    m.add_class::<DosMachine>()?;
    m.add_class::<DriveMap>()?;
    m.add_class::<DriveMapping>()?;
    m.add_class::<DosAssignmentRule>()?;
    m.add_function(wrap_pyfunction!(dos_assignment_rules, m)?)?;
    m.add_function(wrap_pyfunction!(list_hdos_files, m)?)?;
    m.add_function(wrap_pyfunction!(read_hdos_file, m)?)?;
    Ok(())
}
