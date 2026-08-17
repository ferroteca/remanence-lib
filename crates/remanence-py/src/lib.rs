// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Python bindings for the Remanence disk image analysis library.
//!
//! The module mirrors the Rust crate's public surface: a `Session` holds
//! devices, which are configuration, and media, which are
//! state — and the **medium is the content handle**.
//! `Session.load_media(source, format)` takes the caller's own open
//! file — or a list of them, or a `FileSource` taken from an archive
//! medium's namespace, or a list of those — and one declared format,
//! checked by that format's own adapter, and answers with a `Medium`
//! linked to nothing;
//! `StorageDevice.insert(media_id)` seats it in a drive and
//! `StorageDevice.eject()` severs, taking nothing away.
//!
//! Every content verb answers on the medium: `Medium.identify()` reports
//! the layers of the artifact's nesting over the image's own bytes, while
//! `Medium.inspect()` works over the disk a format adapter presents above
//! them. **A medium's content is reached through a partition** (P19):
//! `Medium.partitions()` is the pool the load established and
//! `Medium.partition(ordinal)` takes the scheme's own ordinal — MBR entry
//! 1 is `1`, and `0` is the direct partition a medium recording no scheme
//! bears. The two vantage doors sit on that record: `Partition.volume()`
//! asks by position, `Partition.filesystem()` asks by name, and
//! `Partition.filesystem_as(id)` is the caller's own reading where
//! nothing determines one. **Both doors hand out the same
//! `StorageSpace`**, so which one was opened changes nothing about what
//! comes back, and the file verbs live on that node and nowhere else.
//!
//! **Geometry is discovered, and the sector verbs address in it.**
//! `Medium.geometry` is what the sources beneath the medium stated about
//! the recording's coordinates — the format's own declaration, a FAT boot
//! record's recorded heads and sectors-per-track, the partition table's
//! end tuples, arithmetic over the content's extent — each reading kept
//! with where it came from, and `"undetermined"` where two of them
//! disagree. `Medium.read_sector(cylinder, head, sector)` and
//! `Medium.write_sector(...)` address in what that established, on the
//! device types whose `addressing` is `"sector"`, and raise by name
//! everywhere else; a write buffers until `Medium.commit()` like every
//! other write, and **nothing is ever declared onto a medium that
//! exists**.
//!
//! Failures raise `remanence.Error`, which carries a stable `category`
//! saying how to behave and, where the refusal came from an enumerated rule
//! set such as the DOS 8.3 namespace's, a stable `rule` naming which rule
//! the input broke.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyList};

create_exception!(
    remanence,
    Error,
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
    let error = Error::new_err(message.into());
    Python::attach(|py| {
        let value = error.value(py);
        value
            .setattr("category", category.as_str())
            .expect("Error instances accept attributes");
        value
            .setattr("rule", rule)
            .expect("Error instances accept attributes");
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
    /// The article the image format names for the medium it holds state
    /// for — the physical substrate (P14).
    pub article: String,
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
            article: layout.article.clone(),
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
                            path: layout.path.as_ref().map(|path| path.display().to_string()),
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
    /// The article of the medium attached here (P14) — the substrate.
    pub article: String,
    /// The device its content was recorded by, or `None` where no device
    /// recorded it.
    pub device_type: Option<String>,
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

/// One device a machine's slot may hold: a device type from the catalog
/// (P14), or the archive receiver.
///
/// A device type names the device a medium's content is assumed recorded
/// by, enumerated in two levels — the class, then the concrete type
/// within it. **The receiver is no device type**: an archive was
/// recorded by no device, so its `device_type`, `class` and `article`
/// are all `None`, exactly as a medium recorded by nothing answers.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct DeviceSlot {
    /// The stable spelling, and what `Session.add_device` takes —
    /// `"c1541"`, `"mbr-block-hd"`, or `"archive"`.
    pub id: String,
    /// The name, fit to show a user beside the slot it fills.
    pub name: String,
    /// The recording device type, or `None` for the archive receiver.
    pub device_type: Option<String>,
    /// `"floppy"`, `"hard-drive"` or `"optical"` — the catalog's first
    /// level. `None` for the receiver. Spelled `device_class` because
    /// `class` is a Python keyword and would be unreachable as an
    /// attribute.
    pub device_class: Option<String>,
    /// Where this device type's declaration came from. `None` for the
    /// receiver, which declares no recording.
    pub provenance: Option<String>,
    /// The article this device is served (P14), by stable spelling.
    pub article: Option<String>,
    /// The bay half of every attachment identity in it — `"hdd"` for
    /// `"hdd0"`. Several device types share one where the machine does.
    pub slot_prefix: String,
    /// The drive profile this device claims as its recording path, or
    /// `None` where it claims none — ordinary, not deficient.
    pub flux_path: Option<String>,
    /// The partition scheme this device's spec lays its content out
    /// under — the hard-drive specs carry it. `None` for the schemeless
    /// types, whose media bear the direct partition.
    pub scheme: Option<String>,
    /// `"sector"` or `"block"` — how this device type addresses its
    /// recording. Every device type declares one; `None` for the archive
    /// receiver, which is no device type at all. A `"sector"` type is one
    /// whose medium answers `read_sector` and `write_sector`, in the
    /// coordinates that medium's own geometry established.
    pub addressing: Option<String>,
}

#[pymethods]
impl DeviceSlot {
    fn __repr__(&self) -> String {
        format!("DeviceSlot(id={:?})", self.id)
    }
}

/// Every format a load may declare (P3): its stable spelling, its name,
/// the device types its adapter records, whether its declaration
/// carries a block size, and whether it reads a collection of sources.
///
/// A format that records exactly one device type carries it bare, so a
/// load of it needs no `device` argument; one that records several needs
/// the caller to name which, and a type absent from its list is refused
/// by name even where the class is right. A word that names a kind
/// rather than one catalog entry is not among these at all.
///
/// The last flag is `takes_collection`: a format bearing it reads one
/// disk spread over many streams — a list of sources — and every other
/// claimed format reads one artifact.
#[pyfunction]
fn formats() -> Vec<(String, String, Vec<String>, bool, bool)> {
    remanence::Format::claimed()
        .iter()
        .map(|claim| {
            (
                claim.id().to_owned(),
                claim.name().to_owned(),
                claim
                    .devices()
                    .iter()
                    .map(|device| device.id().to_owned())
                    .collect(),
                claim.takes_block_bytes(),
                claim.takes_collection(),
            )
        })
        .collect()
}

/// Every kind of blank medium this release authors (P3): its stable
/// spelling, its name, the article a medium of it is, and whether its
/// declaration carries the recording's coordinates.
///
/// **Authorship is the third fact class.** Evidence is discovered onto
/// media and declarations are configured onto machines; `new_media`
/// creates one whole, and what the author states becomes the medium's
/// original facts. The **blank article kinds** each name one article of
/// the catalog and create that manufactured substrate with nothing
/// recorded on it; `chs-disk` is the kind whose facts *are* coordinates,
/// and it is the one whose flag here is true.
#[pyfunction]
fn new_media_kinds() -> Vec<(String, String, String, bool)> {
    remanence::NewMedia::claimed()
        .iter()
        .map(|claim| {
            (
                claim.id().to_owned(),
                claim.name().to_owned(),
                claim.article().to_owned(),
                claim.takes_geometry(),
            )
        })
        .collect()
}

/// Every device a machine's slot may hold: one per device type this
/// release claims (P14), plus the archive receiver.
#[pyfunction]
fn device_slots() -> Vec<DeviceSlot> {
    remanence::DeviceSlot::claimed()
        .into_iter()
        .map(|slot| DeviceSlot {
            id: slot.id().to_owned(),
            name: slot.name().to_owned(),
            device_type: slot.device_type().map(|device| device.id().to_owned()),
            device_class: slot.device_type().map(|device| device.class().to_owned()),
            provenance: slot
                .device_type()
                .map(|device| device.provenance().to_owned()),
            article: slot.device_type().map(|device| device.article().to_owned()),
            slot_prefix: slot.slot_prefix().to_owned(),
            flux_path: slot
                .device_type()
                .and_then(remanence::DeviceType::flux_path)
                .map(str::to_owned),
            scheme: slot
                .device_type()
                .and_then(remanence::DeviceType::scheme)
                .map(str::to_owned),
            addressing: slot
                .device_type()
                .map(|device| device.addressing().to_owned()),
        })
        .collect()
}

/// Every partition-layout scheme this release reads, by its stable
/// spelling and the name fit to show a user — so a caller can hold every
/// identity it may meet without waiting to meet one (P3).
#[pyfunction]
fn partition_schemes() -> Vec<(String, String)> {
    remanence::PartitionScheme::ALL
        .iter()
        .map(|scheme| (scheme.id().to_owned(), scheme.name().to_owned()))
        .collect()
}

/// Every partition type a declaration may name, by its stable spelling
/// and the name fit to show a user — so a caller can hold every identity
/// it may meet without waiting to meet one (P3). It is what
/// `Partition.check_type` weighs the recorded byte against.
#[pyfunction]
fn partition_types() -> Vec<(String, String)> {
    remanence::PartitionType::ALL
        .iter()
        .map(|declared| (declared.id().to_owned(), declared.name().to_owned()))
        .collect()
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
/// `"refused"`, arrives as a `remanence.Error` carrying the same condition
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
    /// the library opened the artifact and holds the denial itself,
    /// `"caller-opened"` where the caller handed a handle over and what
    /// it affords is the whole of what the session has, or `"authored"`
    /// where nobody opened anything — the medium was created whole by
    /// its author and there is no artifact for a claim to be over.
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

/// One source's own statement about a recording's coordinates.
///
/// A reading states the parts its source actually carries and leaves the
/// rest `None` — a boot record records heads and sectors-per-track and
/// says nothing about how many cylinders the drive had. Nothing is
/// filled in from a neighbour.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct GeometryReading {
    /// `"format-declaration"`, `"boot-record"`, `"partition-table"` or
    /// `"extent-arithmetic"`.
    pub source: String,
    /// Where in the artifact it was taken, in words fit to show a user.
    pub at: String,
    pub cylinders: Option<u32>,
    pub heads: Option<u32>,
    pub sectors_per_track: Option<u32>,
    pub sector_bytes: Option<u64>,
    /// What the source states, in its own terms.
    pub detail: String,
}

#[pymethods]
impl GeometryReading {
    fn __repr__(&self) -> String {
        format!(
            "GeometryReading(source={:?}, at={:?})",
            self.source, self.at
        )
    }
}

/// A medium's geometry as the evidence left it: what was settled, what
/// the sources contradict each other about, and every reading taken.
///
/// `state` is `"determined"`, `"undetermined"` or `"unstated"`. The
/// coordinates are present only in the first — cylinders and heads
/// number from zero and sectors from one, which is the recording's own
/// convention — and the other two are different facts kept apart:
/// `"undetermined"` means two sources state different values and neither
/// settles it, `"unstated"` that nothing states one at all.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct Geometry {
    pub state: String,
    pub cylinders: Option<u32>,
    pub heads: Option<u32>,
    pub sectors_per_track: Option<u32>,
    pub sector_bytes: Option<u64>,
    /// One sentence per part of the coordinates the sources contradict
    /// each other about, each naming both readings.
    pub conflicts: Vec<String>,
    /// Which parts no source settled, named the way the refusals name
    /// them. Empty for a determined geometry.
    pub unsettled: Vec<String>,
    /// Every reading taken, in the order the sources were read, whether
    /// or not they agreed.
    pub readings: Vec<GeometryReading>,
}

impl Geometry {
    fn new(geometry: &remanence::Geometry) -> Self {
        let determined = geometry.determined();
        Self {
            state: geometry.state().as_str().to_owned(),
            cylinders: determined.map(|coordinates| coordinates.cylinders),
            heads: determined.map(|coordinates| coordinates.heads),
            sectors_per_track: determined.map(|coordinates| coordinates.sectors_per_track),
            sector_bytes: determined.map(|coordinates| coordinates.sector_bytes),
            conflicts: geometry.conflicts().to_vec(),
            unsettled: geometry
                .unsettled()
                .iter()
                .map(|part| (*part).to_owned())
                .collect(),
            readings: geometry
                .readings()
                .iter()
                .map(|reading| GeometryReading {
                    source: reading.source.as_str().to_owned(),
                    at: reading.at.clone(),
                    cylinders: reading.cylinders,
                    heads: reading.heads,
                    sectors_per_track: reading.sectors_per_track,
                    sector_bytes: reading.sector_bytes,
                    detail: reading.detail.clone(),
                })
                .collect(),
        }
    }
}

#[pymethods]
impl Geometry {
    fn __repr__(&self) -> String {
        match (
            self.cylinders,
            self.heads,
            self.sectors_per_track,
            self.sector_bytes,
        ) {
            (Some(cylinders), Some(heads), Some(sectors_per_track), Some(sector_bytes)) => format!(
                "Geometry(state={:?}, cylinders={cylinders}, heads={heads}, \
                 sectors_per_track={sectors_per_track}, \
                 sector_bytes={sector_bytes})",
                self.state
            ),
            _ => format!(
                "Geometry(state={:?}, unsettled={:?})",
                self.state, self.unsettled
            ),
        }
    }
}

/// Every source this release reads a geometry out of (P3), so a caller
/// can hold every identity it may meet without waiting to meet one.
#[pyfunction]
fn geometry_sources() -> Vec<String> {
    remanence::GeometrySource::ALL
        .iter()
        .map(|source| source.as_str().to_owned())
        .collect()
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
///
/// **Nothing is created**: no medium, no session cache, no spilled
/// backing. A cache bound is the load's declaration, so there is no
/// `cache_bytes` here — it is stated at `Session.load_discovery`, where
/// the medium comes into existence.
#[pyfunction]
#[pyo3(signature = (path, *, writable))]
fn discover_media(path: PathBuf, writable: bool) -> PyResult<Discovery> {
    let discovered = remanence::discover_media(path, access_intent(writable));
    Ok(Discovery {
        inner: Mutex::new(Some(discovered.map_err(to_py_err)?)),
    })
}

/// What one artifact turned out to be, and the claim under which that
/// was established.
///
/// It is a handle rather than a record because it holds two things a
/// record could not: the claim on the artifact, and the work the
/// recognition already did. `Session.load_discovery` **consumes**
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
    /// `Medium.format` reports it. A medium that is no disk
    /// image — an archive — refuses by name; its grammar is
    /// `image_format`.
    #[getter]
    fn format(&self) -> PyResult<&'static str> {
        let format = self
            .read(remanence::Discovery::format)?
            .map_err(to_py_err)?;
        Ok(match format {
            remanence::DiskFormat::Raw => "raw",
            remanence::DiskFormat::Qcow2 { .. } => "qcow2",
            remanence::DiskFormat::Vdi { .. } => "vdi",
            remanence::DiskFormat::Imd => "imd",
        })
    }

    /// The **exact article**, by the catalog's stable spelling. The
    /// image-format adapter that loaded the state named it; nothing here
    /// guessed.
    #[getter]
    fn article(&self) -> PyResult<String> {
        self.read(|discovery| discovery.article().to_owned())
    }

    /// The article's name, fit to show a user beside the drive it goes
    /// in.
    #[getter]
    fn article_name(&self) -> PyResult<String> {
        self.read(|discovery| discovery.article_name().to_owned())
    }

    /// Every device served this article, by stable spelling — the answer
    /// to "where could this go?", derived from the device catalog's own
    /// declarations. Empty means no device this release claims takes it,
    /// which is an archive's honest answer.
    #[getter]
    fn accepting_devices(&self) -> PyResult<Vec<String>> {
        self.read(|discovery| {
            discovery
                .accepting_devices()
                .iter()
                .map(|device| device.id().to_owned())
                .collect()
        })
    }

    /// The device this artifact's content was recorded by — the answer
    /// to "what wrote it?" — or `None` where the format records several
    /// types and nothing in the artifact says which.
    ///
    /// `None` is honest rather than deficient, and it is also where a
    /// load of the discovery refuses: the caller declares the type at
    /// `Session.load_media` instead, choosing from `device_types`.
    #[getter]
    fn device_type(&self) -> PyResult<Option<String>> {
        self.read(|discovery| discovery.device_type().map(|device| device.id().to_owned()))
    }

    /// Every device type the recognizing format records — one where it
    /// carries the type bare, several where a load declares which, and
    /// none for an archive grammar.
    #[getter]
    fn device_types(&self) -> PyResult<Vec<String>> {
        self.read(|discovery| {
            discovery
                .device_types()
                .iter()
                .map(|device| device.id().to_owned())
                .collect()
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
    /// filesystem — the same reading `Medium.identify` gives once
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
                "Discovery(path={:?}, article={:?}, device_type={})",
                discovery.path(),
                discovery.article(),
                match discovery.device_type() {
                    Some(device) => format!("{:?}", device.id()),
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

    /// Adds a device of `device` (a stable spelling from
    /// `device_slots()` — a device type such as `"mbr-block-hd"`, or
    /// `"archive"`) to the session, taking the
    /// lowest free slot of that bay, and returns it — empty, until
    /// `StorageDevice.insert` puts a medium in it.
    ///
    /// `slot` chooses the slot, never the name; a slot already taken is
    /// refused rather than displaced, whatever device would fill it. A
    /// device this release does not claim is refused by name.
    #[pyo3(signature = (device, *, slot = None))]
    fn add_device(&self, device: &str, slot: Option<u32>) -> PyResult<StorageDevice> {
        let device = remanence::DeviceSlot::from_id(device).map_err(to_py_err)?;
        let mut session = self.lock();
        let added = match slot {
            Some(slot) => session.add_device_at(device, slot),
            None => session.add_device(device),
        };
        let attachment = added.map_err(to_py_err)?.attachment();
        drop(session);
        Ok(StorageDevice {
            session: Arc::clone(&self.inner),
            attachment,
        })
    }

    /// Loads the caller's own opened artifact — or a declared collection
    /// of sources — as the `format` they declare it to be, and returns
    /// the medium — **linked to nothing**.
    ///
    /// `source` arrives in one of four shapes. One open file: anything
    /// with a `fileno()` — the object `open(path, "rb")` returns — or a
    /// raw descriptor; the descriptor is **duplicated**, so closing the
    /// Python file afterwards leaves the medium's claim intact and the
    /// library closes its own copy when the medium is released. A list
    /// of open files: the collection shape, for a format that reads one
    /// disk spread over many streams. A `FileSource` taken from an
    /// archive medium's namespace, or a list of those: the load
    /// **consumes** each — the source moves into the load, exactly as
    /// `load_discovery` consumes a `Discovery`, whether or not the load
    /// is refused. **A format declares which shape it reads** —
    /// `formats()` says which take a collection — and a shape the
    /// format does not read is refused by name.
    ///
    /// **Whoever opens owns the lock.** That open is the claim: the
    /// library checks it for exactly one thing — may it write through
    /// it? — honours the answer exactly, and never supplements it with a
    /// lock of its own. A name is recovered from the handle for location
    /// alone, under an identity check; a handle this host cannot name
    /// serves everything but the commit journal and a backing chain's
    /// parent, and refuses those two by name. A `FileSource` rides the
    /// claim of the medium it came from, so nothing is opened at all.
    ///
    /// The declaration is checked by that one format's own adapter and
    /// refused by name where the evidence cannot bear it. `format` is a
    /// stable spelling from `formats()`.
    ///
    /// **The declaration carries the device that recorded the content.**
    /// `device` is a stable spelling from that format's own recorded
    /// list, and may be left out where the format records exactly one
    /// type and so carries it bare. `block_bytes` is the raw reading's
    /// declared addressable unit, which every format recording its own
    /// refuses as a second answer about one disk.
    #[pyo3(signature = (source, format, *, device = None, block_bytes = None, cache_bytes = None))]
    fn load_media(
        &self,
        source: &Bound<'_, PyAny>,
        format: &str,
        device: Option<&str>,
        block_bytes: Option<u64>,
        cache_bytes: Option<u64>,
    ) -> PyResult<Medium> {
        let device = match device {
            Some(device) => Some(remanence::DeviceType::from_id(device).map_err(to_py_err)?),
            None => None,
        };
        let format = remanence::Format::declared(format, device, block_bytes).map_err(to_py_err)?;
        let source = media_source(source)?;
        let mut session = self.lock();
        let loaded = match cache_bytes {
            Some(cache_bytes) => session.load_media_with_cache(source, format, cache_bytes),
            None => session.load_media(source, format),
        };
        let id = loaded.map_err(to_py_err)?.id();
        drop(session);
        Ok(Medium {
            session: Arc::clone(&self.inner),
            id,
        })
    }

    /// Creates blank media whole — **authorship, the third fact class** —
    /// and returns the medium, **linked to nothing**.
    ///
    /// Nothing is discovered and nothing is opened, because there is no
    /// artifact: the author declares one enumerated `kind` (a stable
    /// spelling from `new_media_kinds()`), and the facts that declaration
    /// states become the medium's original facts — carried from creation
    /// as its `assurance` provenance and, where the kind states
    /// coordinates, as its `geometry`, whose one reading is `authorship`.
    ///
    /// `cylinders`, `heads`, `sectors_per_track` and `sector_bytes` are
    /// the author's own coordinates, for the kind whose claim takes them;
    /// a blank article kind takes none and refuses them by name.
    /// Coordinates that address nothing — a zero in any part, or a
    /// product no medium could hold — are refused when they are stated,
    /// which is the one moment authorship offers to check them.
    ///
    /// **An authored blank assumes no device**: `device_type` is `None`,
    /// so no drive takes one and `StorageDevice.insert` refuses by name.
    /// It is session-backed until an explicit encode gives it an
    /// artifact, and `Medium.commit` is the ordinary commit point over
    /// it.
    #[pyo3(signature = (
        kind, *, cylinders = None, heads = None, sectors_per_track = None,
        sector_bytes = None, cache_bytes = None
    ))]
    fn new_media(
        &self,
        kind: &str,
        cylinders: Option<u32>,
        heads: Option<u32>,
        sectors_per_track: Option<u32>,
        sector_bytes: Option<u64>,
        cache_bytes: Option<u64>,
    ) -> PyResult<Medium> {
        let stated = cylinders.is_some()
            || heads.is_some()
            || sectors_per_track.is_some()
            || sector_bytes.is_some();
        let geometry = stated.then(|| remanence::RecordingGeometry {
            cylinders: cylinders.unwrap_or(0),
            heads: heads.unwrap_or(0),
            sectors_per_track: sectors_per_track.unwrap_or(0),
            sector_bytes: sector_bytes.unwrap_or(0),
        });
        let kind = remanence::NewMedia::declared(kind, geometry).map_err(to_py_err)?;
        let mut session = self.lock();
        let created = match cache_bytes {
            Some(cache_bytes) => session.new_media_with_cache(kind, cache_bytes),
            None => session.new_media(kind),
        };
        let id = created.map_err(to_py_err)?.id();
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
    /// identification did, and the medium is built over that claim, so
    /// nothing can change the artifact between the question and the
    /// load. The intent and the assurance are the ones the discovery
    /// established; the **cache bound is declared here**, because this
    /// is where the medium comes into existence — discovery built
    /// nothing, so it had nothing to bound.
    ///
    /// **The discovery is consumed either way** — a refused load
    /// releases its claim with it. Asking again is `discover_media`.
    #[pyo3(signature = (discovery, *, cache_bytes = None))]
    fn load_discovery(&self, discovery: &Discovery, cache_bytes: Option<u64>) -> PyResult<Medium> {
        let discovered = discovery.take()?;
        let mut session = self.lock();
        let loaded = match cache_bytes {
            Some(cache_bytes) => session.load_discovery_with_cache(discovered, cache_bytes),
            None => session.load_discovery(discovered),
        };
        let id = loaded.map_err(to_py_err)?.id();
        drop(session);
        Ok(Medium {
            session: Arc::clone(&self.inner),
            id,
        })
    }

    /// `Session.load_discovery` under the caller's own declaration of
    /// the device that recorded the artifact — the `_as` door, for a
    /// format that records several device types and so asserts none.
    ///
    /// `device` is a stable spelling from `Discovery.device_types`, and
    /// one the recognizing format's adapter records; anything else
    /// raises by name. The discovery is consumed either way, and the
    /// cache bound is declared here as it is at the plain door.
    #[pyo3(signature = (discovery, device, *, cache_bytes = None))]
    fn load_discovery_as(
        &self,
        discovery: &Discovery,
        device: &str,
        cache_bytes: Option<u64>,
    ) -> PyResult<Medium> {
        let device = remanence::DeviceType::from_id(device).map_err(to_py_err)?;
        let taken = discovery.take()?;
        let mut session = self.lock();
        let loaded = match cache_bytes {
            Some(cache_bytes) => session.load_discovery_as_with_cache(taken, device, cache_bytes),
            None => session.load_discovery_as(taken, device),
        };
        let id = loaded.map_err(to_py_err)?.id();
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
        self.lock().media().iter().map(|id| id.value()).collect()
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

    /// Adds a device for the artifact at `path` — one of the device type
    /// the artifact's format records — loads the medium into it, and
    /// returns it. A format recording several types is refused by name.
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

/// One storage device — the durable slot and its family, and the link to
/// whichever pooled medium currently occupies it.
///
/// **The device is the slot, not the disk**: every content verb lives on
/// `Medium`, and this carries `insert`, `eject` and `medium` — the one
/// edge between configuration and state.
#[pyclass(module = "remanence")]
pub struct StorageDevice {
    session: Arc<Mutex<remanence::Session>>,
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
    attachment: remanence::AttachmentId,
}

impl std::ops::Deref for DeviceGuard<'_> {
    type Target = remanence::StorageDevice;

    fn deref(&self) -> &Self::Target {
        self.session
            .device(self.attachment)
            .expect("the device was present when this guard was taken")
    }
}

/// The same borrow with the session's media pool beside it — what the
/// edge verbs need, since linking is the one act crossing configuration
/// into state.
struct DeviceViewGuard<'a> {
    session: MutexGuard<'a, remanence::Session>,
    attachment: remanence::AttachmentId,
}

impl DeviceViewGuard<'_> {
    fn view(&mut self) -> remanence::DeviceView<'_> {
        self.session
            .device_mut(self.attachment)
            .expect("the device was present when this guard was taken")
    }
}

impl StorageDevice {
    fn present(&self) -> PyResult<MutexGuard<'_, remanence::Session>> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if session.device(self.attachment).is_none() {
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
            attachment: self.attachment,
        })
    }

    fn view(&mut self) -> PyResult<DeviceViewGuard<'_>> {
        Ok(DeviceViewGuard {
            session: self.present()?,
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

    /// The bay this slot belongs to, by its prefix — `"hdd"` for
    /// `"hdd0"`.
    #[getter]
    fn slot_prefix(&self) -> String {
        self.attachment.prefix().to_owned()
    }

    /// What this device is, by stable spelling — a device type's own, or
    /// `"archive"` for the receiver.
    #[getter]
    fn slot(&mut self) -> PyResult<String> {
        Ok(self.get()?.slot().id().to_owned())
    }

    /// The recording device type this slot is typed by, or `None` for
    /// the archive receiver — which records nothing, as the archive it
    /// holds was recorded by nothing.
    #[getter]
    fn device_type(&mut self) -> PyResult<Option<String>> {
        Ok(self
            .get()?
            .device_type()
            .map(|device| device.id().to_owned()))
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
    /// **The check is device-type equality**: a medium carries the
    /// device its content was recorded by, a slot is typed by the device
    /// that fills it, and a medium belonging in another drive is refused
    /// naming both sides. An
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

    /// The article this medium is, by the catalog's stable spelling —
    /// the physical substrate.
    #[getter]
    fn article(&self) -> PyResult<&'static str> {
        Ok(self.get()?.article())
    }

    /// The device this medium's content was recorded by, by the device
    /// catalog's stable spelling — or `None` where no device recorded
    /// it, which is an archive's and an authored blank's honest answer
    /// rather than a gap.
    #[getter]
    fn device_type(&self) -> PyResult<Option<String>> {
        Ok(self
            .get()?
            .device_type()
            .map(|device| device.id().to_owned()))
    }

    /// The authored kind this medium was created as, by
    /// `new_media_kinds()`'s stable spelling — or **`None` where it was
    /// loaded from an artifact instead**.
    ///
    /// It is the third fact class showing on the surface: a medium says
    /// what it was *made* as exactly where an author made it, and says
    /// nothing where evidence is what it has.
    #[getter]
    fn authored_as(&self) -> PyResult<Option<&'static str>> {
        Ok(self.get()?.authored_as().map(remanence::NewMedia::id))
    }

    /// The artifact the medium was loaded from (the archive itself for an
    /// image loaded out of one), or **`None` where the caller's handle
    /// has no recoverable name** — a name serves location alone. An
    /// authored medium answers `None` because it has no artifact at all.
    #[getter]
    fn path(&self) -> PyResult<Option<String>> {
        Ok(self.get()?.path().map(str::to_owned))
    }

    /// The resolved image path (the entry name for archive inputs), or
    /// `None` as above.
    #[getter]
    fn image_path(&self) -> PyResult<Option<String>> {
        Ok(self
            .get()?
            .image_path()
            .map(|path| path.display().to_string()))
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
        self.get()?
            .read_at(offset, &mut buffer)
            .map_err(to_py_err)?;
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
            remanence::DiskFormat::Imd => "imd",
        })
    }

    /// The qcow2 version, or `None` for an image of any other format.
    #[getter]
    fn qcow2_version(&self) -> PyResult<Option<u32>> {
        Ok(match self.get()?.format().map_err(to_py_err)? {
            remanence::DiskFormat::Qcow2 { version } => Some(version),
            remanence::DiskFormat::Raw
            | remanence::DiskFormat::Imd
            | remanence::DiskFormat::Vdi { .. } => None,
        })
    }

    /// The VDI version as a `(major, minor)` pair, or `None` for an image
    /// of any other format.
    #[getter]
    fn vdi_version(&self) -> PyResult<Option<(u32, u32)>> {
        Ok(match self.get()?.format().map_err(to_py_err)? {
            remanence::DiskFormat::Vdi { major, minor } => Some((major, minor)),
            remanence::DiskFormat::Raw
            | remanence::DiskFormat::Imd
            | remanence::DiskFormat::Qcow2 { .. } => None,
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

    /// The recording's coordinates as the evidence beneath this medium
    /// left them: what was settled, what the sources contradict each
    /// other about, and every reading taken.
    ///
    /// It was established when the medium was loaded and is evidence from
    /// then on — nothing re-reads a boot record behind a caller, and
    /// **nothing is ever declared onto it**.
    #[getter]
    fn geometry(&self) -> PyResult<Geometry> {
        Ok(Geometry::new(self.get()?.geometry()))
    }

    /// Reads one whole sector in the recording's own coordinates.
    ///
    /// Cylinders and heads number from zero and **sectors from one**,
    /// which is the recording's convention rather than this library's.
    ///
    /// It answers on a sector-addressed recording whose geometry the
    /// evidence established and raises by name otherwise, the `rule` on
    /// the error naming which: `"not-sector-addressed"`,
    /// `"geometry-unstated"`, `"geometry-undetermined"`,
    /// `"outside-geometry"` or `"partial-sector"`.
    fn read_sector<'py>(
        &self,
        py: Python<'py>,
        cylinder: u32,
        head: u32,
        sector: u32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut medium = self.get()?;
        let length = medium
            .geometry()
            .determined()
            .map_or(0, |coordinates| coordinates.sector_bytes);
        // A zero-length buffer is what a medium with no settled sector
        // size gets, and the refusal it earns is the one that says so —
        // never a short read dressed up as an answer.
        let mut buffer = vec![0u8; length as usize];
        medium
            .read_sector(cylinder, head, sector, &mut buffer)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Writes one whole sector in the recording's own coordinates,
    /// **buffered until `commit`** like every other write, under the same
    /// rules `read_sector` answers by.
    fn write_sector(&self, cylinder: u32, head: u32, sector: u32, data: &[u8]) -> PyResult<()> {
        self.get()?
            .write_sector(cylinder, head, sector, data)
            .map_err(to_py_err)
    }

    /// The family's hardware bitstream over this medium's recording,
    /// materialized once under the profile's declared mechanics and
    /// read-channel rules and answered from then on.
    ///
    /// **It takes no arguments because the type carries the rules**:
    /// being a medium of a declared family *means* clocking through that
    /// family's channel, and what was used travels into the result's
    /// account. The rung names no family, and the one behind it is the
    /// medium's own. It
    /// answers where the device type's profile bears flux, and raises by
    /// name everywhere else — a block medium's recording is presented by
    /// its format adapter, and the two families are disjoint.
    fn bitstream(&self) -> PyResult<Bitstream> {
        let report = {
            let mut medium = self.get()?;
            BitstreamReport::new(medium.bitstream().map_err(to_py_err)?.inspect())
        };
        Ok(Bitstream {
            provider: BitstreamProvider::Medium {
                session: Arc::clone(&self.session),
                id: self.id,
            },
            report,
        })
    }

    /// The family's encoded bytestream — the byte sequence the declared
    /// group code makes of the bitstream — materialized once under the
    /// same rule and answered from then on.
    ///
    /// The framed bytes of one location are read through
    /// `Bytestream.location`; no byte here is a header, a sector or
    /// a file, and the layers that assign those sit above.
    fn bytestream(&self) -> PyResult<Bytestream> {
        let report = {
            let mut medium = self.get()?;
            BytestreamReport::new(medium.bytestream().map_err(to_py_err)?.inspect())
        };
        Ok(Bytestream {
            provider: BytestreamProvider::Medium {
                session: Arc::clone(&self.session),
                id: self.id,
            },
            report,
        })
    }

    /// The scheme this medium's content is laid out under — `"mbr"` — or
    /// `None` where it records none and the direct partition stands.
    ///
    /// It is the evidence answer, and the direct partition leaves it
    /// unchanged: a medium that recorded no table still says so here.
    #[getter]
    fn partition_scheme(&self) -> PyResult<Option<String>> {
        Ok(self
            .get()?
            .partition_scheme()
            .map(|scheme| scheme.id().to_owned()))
    }

    /// Every partition in the pool, in the scheme's own order.
    ///
    /// The pool was established when the medium was loaded and is
    /// evidence from then on — nothing here re-reads a table, and nothing
    /// is discovered on demand. A medium recording no scheme answers with
    /// exactly one: the direct partition, the library's own composition of
    /// the whole content, which says so in its `provenance`.
    fn partitions(&self) -> PyResult<Vec<Partition>> {
        Ok(self
            .get()?
            .partitions()
            .iter()
            .map(|partition| {
                partition_record(
                    partition,
                    Some(Arc::clone(&self.session)),
                    Some(self.id),
                    None,
                )
            })
            .collect())
    }

    /// The partition the scheme's own ordinal names, or `None` — absence
    /// being an answer, with nothing manufactured to report it.
    ///
    /// The ordinals are the scheme's own: MBR entry 1 is `1`, and the
    /// direct partition is `0`. **The content of a medium is reached
    /// through here** — the vantage doors on the answer hand out the one
    /// `StorageSpace` that partition composes, and the file verbs live on
    /// that node and nowhere else (P19). **The medium carries no file
    /// verbs of its own**: it may be asked what it holds, and may not be
    /// told to act as something it isn't.
    fn partition(&self, ordinal: u32) -> PyResult<Option<Partition>> {
        Ok(self
            .get()?
            .partitions()
            .iter()
            .find(|partition| partition.ordinal() == ordinal)
            .map(|partition| {
                partition_record(
                    partition,
                    Some(Arc::clone(&self.session)),
                    Some(self.id),
                    None,
                )
            }))
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

/// One partition of a medium's evidence pool (P16, P19).
///
/// It carries what the scheme declared — the ordinal it was declared at,
/// its raw type value beside a reading of what that value declares, the
/// boot flag, its placement and its role (U4) — and what the library
/// composed over it. The pool was established when the medium was loaded,
/// so nothing here was probed for on demand, and a partition carrying an
/// `issue` keeps its number rather than being dropped, so the partitions
/// behind it never renumber.
///
/// The **direct partition** is the one member the library composes rather
/// than reads. It stands at ordinal `0` where the medium records no
/// scheme, it declares no type, and its account is `provenance` rather
/// than `evidence` — a composition act stated as one, never offered as
/// something the medium said.
///
/// **The doors are here rather than on a handle of their own.** `volume()`
/// asks by position, `filesystem()` asks by name, and both hand out the
/// same `StorageSpace`, so which one was opened changes nothing about what
/// comes back (D26). Each re-resolves through whatever provides the
/// partition, so a released medium refuses by name rather than reaching
/// state that is gone.
#[pyclass(frozen, module = "remanence")]
pub struct Partition {
    /// The scheme's own ordinal — MBR entry 1 is `1` — or `0` for the
    /// direct partition, which no scheme numbered.
    #[pyo3(get)]
    pub ordinal: u32,
    /// Whether this is the direct partition, the library's own
    /// composition of the whole content.
    #[pyo3(get)]
    pub is_direct: bool,
    /// Whether the scheme flags this partition active, as it records it.
    /// The direct partition is flagged by nothing and answers `False`.
    #[pyo3(get)]
    pub active: bool,
    /// The type value exactly as the scheme records it, or `None` for the
    /// direct partition, which records none.
    #[pyo3(get)]
    pub type_byte: Option<u8>,
    /// What that value *declares*, in a sentence fit to quote in a refusal
    /// a user reads — present whether or not this release reads the type,
    /// because the partition a caller most needs explained is the one the
    /// library declines to read. It describes the declaration, never the
    /// content.
    #[pyo3(get)]
    pub type_reading: Option<String>,
    /// Whether this release reads the declared type.
    #[pyo3(get)]
    pub is_claimed: bool,
    /// How the scheme places this partition, in its own vocabulary:
    /// `"primary"` for one of MBR's four slots, `"logical"` for an entry
    /// on the extended chain, `"direct"` for the direct partition. A
    /// different axis from `role`, and neither implies the other.
    #[pyo3(get)]
    pub placement: String,
    /// `"data"` or `"structure"`.
    #[pyo3(get)]
    pub role: String,
    /// Where this partition starts in the presented content, or `None`
    /// where it has no addressed extent at all — the direct partition over
    /// content whose native vantage is a namespace.
    #[pyo3(get)]
    pub start_bytes: Option<u64>,
    /// How far it runs, under the same rule.
    #[pyo3(get)]
    pub length_bytes: Option<u64>,
    /// Whether the addressable vantage opens — whether `volume()` answers.
    #[pyo3(get)]
    pub is_addressable: bool,
    /// Whether the namespace vantage opens — whether `filesystem()`
    /// answers. Where it does not, the namespace is declared with
    /// `filesystem_as`.
    #[pyo3(get)]
    pub bears_namespace: bool,
    /// The identity the inspection report issued for the volume composed
    /// over this partition, or `None` where it composed none. Opaque and
    /// stable across opens of an unchanged layout (P21, U4).
    #[pyo3(get)]
    pub volume_id: Option<u64>,
    /// The category of the refusal that keeps this partition in the pool
    /// when its type is outside the claim or its chain could not be
    /// followed, where one does.
    #[pyo3(get)]
    pub issue_category: Option<String>,
    /// That refusal in words. A partition carrying one is still a
    /// partition, and still keeps its place.
    #[pyo3(get)]
    pub issue: Option<String>,
    /// What the scheme's adapter read to declare this partition (P4).
    /// Empty for the direct partition, which read nothing and states so
    /// through `provenance` instead.
    #[pyo3(get)]
    pub evidence: Vec<String>,
    /// The direct partition's account of itself: what the library composed
    /// and why. Present for the direct partition and `None` for every
    /// partition a scheme declared, which is the whole of the distinction.
    #[pyo3(get)]
    pub provenance: Option<String>,
    /// What this record re-resolves through — the session and medium whose
    /// pool holds it, or the recording's own record layer that composed
    /// it. Not part of the Python surface.
    session: Option<Arc<Mutex<remanence::Session>>>,
    media: Option<remanence::MediaId>,
    sectors: Option<Arc<remanence::C1541Sectors>>,
    /// The FM or MFM record layer, where that is what composed it. Never
    /// set alongside `sectors`: a recording belongs to one family.
    ibm_sectors: Option<Arc<remanence::IbmSectors>>,
}

/// One partition of a pool, in the module's own record, keyed to whatever
/// provides it so its doors re-resolve rather than hold state.
fn partition_record(
    partition: &remanence::Partition,
    session: Option<Arc<Mutex<remanence::Session>>>,
    media: Option<remanence::MediaId>,
    sectors: Option<Arc<remanence::C1541Sectors>>,
) -> Partition {
    Partition {
        ordinal: partition.ordinal(),
        is_direct: partition.is_direct(),
        active: partition.active(),
        type_byte: partition.type_byte(),
        type_reading: partition.type_reading().map(str::to_owned),
        is_claimed: partition.is_claimed(),
        placement: partition.placement().to_owned(),
        role: partition.role().name().to_owned(),
        start_bytes: partition.start_bytes(),
        length_bytes: partition.length_bytes(),
        is_addressable: partition.is_addressable(),
        bears_namespace: partition.bears_namespace(),
        volume_id: partition.volume_id().map(remanence::VolumeId::value),
        issue_category: partition
            .issue()
            .map(|issue| issue.category().as_str().to_owned()),
        issue: partition.issue().map(|issue| issue.to_string()),
        evidence: partition.evidence().to_vec(),
        provenance: partition.provenance().map(str::to_owned),
        session,
        media,
        sectors,
        ibm_sectors: None,
    }
}

/// The direct partition over an FM or MFM recording's records, which
/// unlike a CBM DOS recording's composes an addressed extent (D62).
fn partition_over_ibm_sectors(sectors: Arc<remanence::IbmSectors>) -> Partition {
    let mut holder = sectors
        .partition()
        .expect("the caller composed it once already");
    let mut record = partition_record(holder.view().partition(), None, None, None);
    record.ibm_sectors = Some(sectors);
    record
}

impl Partition {
    /// Resolves this partition afresh and runs `action` over the borrow
    /// that holds it and its provider at once.
    ///
    /// The record holds no borrow and no copy of the pool: the ordinal is
    /// the whole of the key, and a medium that has left answers by name
    /// rather than through state that is gone.
    fn with_view<T>(
        &self,
        action: impl FnOnce(remanence::PartitionView<'_>) -> PyResult<T>,
    ) -> PyResult<T> {
        // The direct partition over a recording's own record layer
        // re-resolves from that layer, exactly as a pooled one re-resolves
        // from its medium (P13).
        if let Some(sectors) = &self.sectors {
            return action(sectors.partition());
        }
        // An FM or MFM recording composes an addressed extent instead
        // (D62), and the holder is what owns it for the borrow's life.
        if let Some(sectors) = &self.ibm_sectors {
            let mut holder = sectors.partition().map_err(to_py_err)?;
            return action(holder.view());
        }
        let (Some(session), Some(media)) = (&self.session, self.media) else {
            return Err(categorized_py_err(
                remanence::ErrorCategory::NotFound,
                "this partition names no medium to be resolved through",
            ));
        };
        let mut guard = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(medium) = guard.medium_mut(media) else {
            return Err(categorized_py_err(
                remanence::ErrorCategory::NotFound,
                "the medium whose pool holds this partition was released",
            ));
        };
        let Some(view) = medium.partition(self.ordinal) else {
            return Err(categorized_py_err(
                remanence::ErrorCategory::NotFound,
                format!(
                    "the medium whose pool held partition {} no longer holds it",
                    self.ordinal
                ),
            ));
        };
        action(view)
    }

    /// The one node a door hands out, in the module's own record.
    ///
    /// The space is keyed to this partition and to the declaration it was
    /// opened under, never to state read out of it, so every verb on it
    /// re-resolves exactly the way this did.
    fn compose(
        &self,
        space: &remanence::StorageSpace<'_>,
        declared: Option<String>,
    ) -> StorageSpace {
        StorageSpace {
            session: self.session.clone(),
            media: self.media,
            sectors: self.sectors.clone(),
            ordinal: self.sectors.is_none().then_some(self.ordinal),
            declared,
            volume_id: space.volume_id().map(remanence::VolumeId::value),
            start_bytes: space.start_bytes(),
            length_bytes: space.length_bytes(),
            // A space bearing no namespace is an ordinary volume, so the
            // absence travels on the space rather than failing here.
            kind: space.kind().ok().map(str::to_owned),
        }
    }
}

#[pymethods]
impl Partition {
    /// The caller's own reading of the type, checked against the value the
    /// scheme recorded: `"dos-primary"` or `"dos-extended"`.
    ///
    /// The declaration is the caller's and the check is the library's. A
    /// reading the recorded byte does not bear raises naming both sides;
    /// the direct partition — which records no type — raises by name
    /// rather than accepting a reading of nothing; and a spelling this
    /// release does not declare raises naming what it does (P3).
    fn check_type(&self, type_id: &str) -> PyResult<()> {
        let declared = remanence::PartitionType::from_id(type_id).map_err(to_py_err)?;
        self.with_view(|view| view.check_type(declared).map_err(to_py_err))
    }

    /// The addressable vantage: the space this partition composes, read
    /// and written **by position within the partition's own extent**.
    ///
    /// `None` where the partition composes no extent — a structural
    /// region, a type this release will not read, and the direct partition
    /// over content whose native vantage is a namespace. The answer is a
    /// lookup: the extent was settled when the pool was established, and
    /// nothing is probed for here.
    fn volume(&self) -> PyResult<Option<StorageSpace>> {
        self.with_view(|view| Ok(view.volume().map(|space| self.compose(&space, None))))
    }

    /// The namespace vantage: the same space, reached by the names it
    /// holds.
    ///
    /// `None` where nothing determines a namespace over this partition —
    /// where the declared type determines none, or where no type is
    /// declared at all. That is the honest absence P19 requires rather
    /// than a failure to read one, and `filesystem_as` is where a caller
    /// who knows says so.
    fn filesystem(&self) -> PyResult<Option<StorageSpace>> {
        self.with_view(|view| Ok(view.filesystem().map(|space| self.compose(&space, None))))
    }

    /// The declared reading, where no partition type determines one:
    /// `"fat"`, `"hdos"`, `"cpm"`, a `"cpm-*"` layout, or `"cbmdos"`.
    ///
    /// **The reading is the caller's and the check is the library's.** The
    /// adapter the declaration names is the one that reads it, and it
    /// reads the evidence to verify the declaration rather than to pick
    /// one — a declaration the content cannot bear raises from that
    /// adapter, by name, with `rule` naming which rule stood in the way. A
    /// spelling this release does not read raises naming what it does
    /// (P3).
    fn filesystem_as(&self, id: &str) -> PyResult<StorageSpace> {
        self.with_view(|view| {
            let space = view.filesystem_as(id).map_err(to_py_err)?;
            Ok(self.compose(&space, Some(id.to_owned())))
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "Partition(ordinal={}, placement={:?}, role={:?})",
            self.ordinal, self.placement, self.role
        )
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
            article: report.device.article.clone(),
            device_type: report.device.device_type.clone(),
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

/// The load's source, in whichever of its four shapes arrived: one open
/// file, a list of them, one `FileSource` taken from an archive medium's
/// namespace, or a list of those.
///
/// **A collection is one kind of source throughout** — its first member
/// says which kind, and a member of the other kind is refused by name. A
/// `FileSource` is consumed on the way through, whether or not the load
/// is then refused, exactly as a `Discovery` is.
fn media_source(source: &Bound<'_, PyAny>) -> PyResult<remanence::MediaSource> {
    if let Ok(entry) = source.cast::<FileSource>() {
        return Ok(entry.borrow().take()?.into());
    }
    let Ok(list) = source.cast::<PyList>() else {
        return Ok(duplicated_file(source)?.into());
    };
    let leading_entry = list
        .get_item(0)
        .is_ok_and(|first| first.cast::<FileSource>().is_ok());
    if leading_entry {
        let mut entries = Vec::with_capacity(list.len());
        for (at, item) in list.iter().enumerate() {
            let entry = item.cast::<FileSource>().map_err(|_| {
                categorized_py_err(
                    remanence::ErrorCategory::Unsupported,
                    format!(
                        "a collection is one kind of source throughout, and \
                         member {at} is no FileSource like the members before \
                         it"
                    ),
                )
            })?;
            entries.push(entry.borrow().take()?);
        }
        return Ok(entries.into());
    }
    let mut handles = Vec::with_capacity(list.len());
    for (at, item) in list.iter().enumerate() {
        if item.cast::<FileSource>().is_ok() {
            return Err(categorized_py_err(
                remanence::ErrorCategory::Unsupported,
                format!(
                    "a collection is one kind of source throughout, and \
                     member {at} is a FileSource among opened files"
                ),
            ));
        }
        handles.push(duplicated_file(&item)?);
    }
    Ok(handles.into())
}

/// Re-opens the space one partition composes and runs `action` over it.
///
/// Every verb below passes through here, so the refusals a caller meets
/// are the library's own — the partition seam's where the pool holds no
/// such ordinal, the namespace's where a declaration could not be borne —
/// and a medium that has left answers by name rather than through state
/// that is gone. The key is the ordinal and the declaration made under it
/// and nothing else: the pool was established at the load and is the same
/// pool on every call, so re-resolving reaches the same partition it
/// reached before.
fn with_filesystem<T>(
    session: Option<&Arc<Mutex<remanence::Session>>>,
    media: Option<remanence::MediaId>,
    sectors: Option<&Arc<remanence::C1541Sectors>>,
    ordinal: Option<u32>,
    declared: Option<&str>,
    action: impl FnOnce(&mut remanence::StorageSpace<'_>) -> remanence::Result<T>,
) -> PyResult<T> {
    // A space composed over a recording's own record layer re-resolves
    // from that layer, exactly as a medium-backed one re-resolves from its
    // medium (P13).
    if let Some(sectors) = sectors {
        let mut space = open_vantage(sectors.partition(), declared)?;
        return action(&mut space).map_err(to_py_err);
    }
    let (Some(session), Some(media)) = (session, media) else {
        return Err(categorized_py_err(
            remanence::ErrorCategory::NotFound,
            "this space names no medium to be resolved through",
        ));
    };
    let Some(ordinal) = ordinal else {
        return Err(categorized_py_err(
            remanence::ErrorCategory::NotFound,
            "this space names no partition to be reached through",
        ));
    };
    let mut guard = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(medium) = guard.medium_mut(media) else {
        return Err(categorized_py_err(
            remanence::ErrorCategory::NotFound,
            "the medium this space reads was released",
        ));
    };
    let Some(view) = medium.partition(ordinal) else {
        return Err(categorized_py_err(
            remanence::ErrorCategory::NotFound,
            format!("the medium this space reads holds no partition {ordinal}"),
        ));
    };
    let mut space = open_vantage(view, declared)?;
    action(&mut space).map_err(to_py_err)
}

/// The vantage a space was opened by, opened the same way again.
///
/// A declaration is honoured first, because it is the caller's own reading
/// and nothing may stand in for it; then the namespace vantage where the
/// partition bears one, and the addressable vantage where it does not.
/// Both doors hand out the same node (D26), so this order settles which
/// question was asked rather than what comes back.
fn open_vantage<'a>(
    view: remanence::PartitionView<'a>,
    declared: Option<&str>,
) -> PyResult<remanence::StorageSpace<'a>> {
    if let Some(id) = declared {
        return view.filesystem_as(id).map_err(to_py_err);
    }
    if view.partition().bears_namespace() {
        return Ok(view
            .filesystem()
            .expect("a partition bearing a namespace opens that door"));
    }
    if view.partition().is_addressable() {
        return Ok(view
            .volume()
            .expect("a partition composing an extent opens that door"));
    }
    Err(categorized_py_err(
        remanence::ErrorCategory::Unsupported,
        "this partition composes no addressed extent and nothing declares \
         a namespace over it, so there is no vantage left to reach it by",
    ))
}

/// The volume/filesystem node: **one object carrying two vantages**.
///
/// *Volume* is the addressable vantage — reads and writes by position
/// within the extent this space names. *Filesystem* is the namespace
/// vantage — the file verbs, which live here and nowhere else. They are
/// two words for one node, and an object has what it has: a FAT volume
/// both, unformatted space the addressable one alone, an archive's
/// namespace the namespace one alone (D26). Which of a partition's doors
/// handed it over changes nothing about which it has.
///
/// It is a view over its provider's state, never an instance: every verb
/// reads or writes the state beneath it, mutations project into the
/// active layer, and nothing here holds a listing that could go stale.
#[pyclass(module = "remanence")]
pub struct StorageSpace {
    /// The session the medium bearing this space lives in, or `None`
    /// where no medium composed it.
    session: Option<Arc<Mutex<remanence::Session>>>,
    /// The medium whose pool holds the partition that composed it, or
    /// `None` where no medium composed it at all.
    media: Option<remanence::MediaId>,
    /// The recording's own record layer it is presented over, where that
    /// family composed it: that family is reached through its own types
    /// rather than through a device, and the node still carries the file
    /// verbs.
    sectors: Option<Arc<remanence::C1541Sectors>>,
    /// The scheme's own ordinal of the partition that composed it — half
    /// the key it re-resolves by, and `None` where no medium composed it.
    ordinal: Option<u32>,
    /// The namespace declaration it was opened under, where a caller made
    /// one — the other half. It is re-declared on every re-resolution, so
    /// a space opened by a reading keeps being read that way.
    declared: Option<String>,
    /// The identity the inspection report issued for the volume composed
    /// over this space's partition, where it composed one. Reported, never
    /// resolved through.
    volume_id: Option<u64>,
    start_bytes: Option<u64>,
    length_bytes: Option<u64>,
    /// The namespace kind, where it has the namespace vantage.
    kind: Option<String>,
}

#[pymethods]
impl StorageSpace {
    /// Whether this space has the addressable vantage — an extent to
    /// read and write by position.
    #[getter]
    fn is_addressable(&self) -> bool {
        self.start_bytes.is_some()
    }

    /// The identity the inspection report issued for the volume composed
    /// over this space's partition, or `None` where it composed none — a
    /// space with no addressed extent at all, and a partition the report
    /// states as declared without composing a volume from it.
    ///
    /// It is the same identity in the report and here, so an identity
    /// names the same volume wherever it is met (P21, U4).
    #[getter]
    fn volume_id(&self) -> Option<u64> {
        self.volume_id
    }

    /// Where this space starts in the presented disk, or `None` where it
    /// has no addressable vantage.
    #[getter]
    fn start_bytes(&self) -> Option<u64> {
        self.start_bytes
    }

    /// How far this space runs, or `None` where it has no addressable
    /// vantage.
    #[getter]
    fn length_bytes(&self) -> Option<u64> {
        self.length_bytes
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.evidence(),
        )
    }

    /// Lists a directory (`""` is the root, `"A/B"` descends).
    #[pyo3(signature = (path = ""))]
    fn entries(&self, path: &str) -> PyResult<Vec<Entry>> {
        let entries = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.entries(path),
        )?;
        Ok(entries.iter().map(Entry::new).collect())
    }

    /// Every file under `path` (`""` is the whole namespace), gathered
    /// as a load's sources — the collection shape of
    /// `Session.load_media`.
    ///
    /// The sources are **free-standing**: each rides the claim of the
    /// medium it came from, so the walk that gathered them ends before
    /// the load begins and nothing is opened twice. A solid archive's
    /// coded stream decodes once for the whole gathering, not once per
    /// member. This release gathers from an archive's namespace alone —
    /// a volume-backed filesystem's files are read through the
    /// filesystem that names them, and this raises by name there.
    #[pyo3(signature = (path = ""))]
    fn files(&self, path: &str) -> PyResult<Vec<FileSource>> {
        let sources = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.files(path),
        )?;
        Ok(sources.into_iter().map(FileSource::over).collect())
    }

    /// Answers one path with its entry, or `None` when nothing exists
    /// there — a missing leaf, a missing parent, or a parent that is a
    /// file alike. Absence is an answer, distinguished from failure,
    /// which raises.
    fn stat(&self, path: &str) -> PyResult<Option<Entry>> {
        let entry = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| Ok(filesystem.get_file(path)?.entry().clone()),
        )?;
        Ok(File {
            session: self.session.clone(),
            media: self.media,
            sectors: self.sectors.clone(),
            ordinal: self.ordinal,
            declared: self.declared.clone(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.read_file(path),
        )?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Writes a file. An existing file is overwritten — shorter or
    /// longer, its old clusters released and reclaimed — while an
    /// existing directory is refused. Buffered until
    /// `Medium.commit()`.
    fn write_file(&self, path: &str, contents: &[u8]) -> PyResult<()> {
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.make_directory(path),
        )
    }

    fn __repr__(&self) -> String {
        match self.volume_id {
            Some(volume) => format!("StorageSpace(kind={:?}, volume_id={volume})", self.kind),
            None => format!("StorageSpace(kind={:?}, volume_id=None)", self.kind),
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
    /// As on `StorageSpace`: the recording's own record layer a namespace
    /// is presented over, where no device composed it.
    sectors: Option<Arc<remanence::C1541Sectors>>,
    /// The partition the space holding this file was reached through, and
    /// the declaration it was opened under — the same key that space
    /// re-resolves by, so a file reaches its bytes the way its space does.
    ordinal: Option<u32>,
    declared: Option<String>,
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.get_file(&path)?.discover(),
        )?;
        Ok(Discovery::over(discovery))
    }

    /// This file taken as a load's source — the single-`FileSource`
    /// shape of `Session.load_media`.
    ///
    /// The source is **free-standing**: it rides the claim of the medium
    /// it came from, so the walk that named it ends before the load
    /// begins and nothing is opened twice. This release takes a load's
    /// source from an archive's namespace alone — a file on a
    /// volume-backed filesystem is read through the filesystem that
    /// names it, and raises here by name.
    fn source(&self) -> PyResult<FileSource> {
        let path = self.path.clone();
        let source = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.get_file(&path)?.source(),
        )?;
        Ok(FileSource::over(source))
    }

    /// The whole file, copied out.
    fn bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let path = self.path.clone();
        let bytes = with_filesystem(
            self.session.as_ref(),
            self.media,
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
            |filesystem| filesystem.get_file(&path)?.read_at(offset, &mut buffer),
        )?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Writes `data` at `offset` in place — the streamed form,
    /// `os.pwrite`-shaped. The span must lie within the file's current
    /// size; `StorageSpace.resize_file` is what changes it. Buffered until
    /// commit.
    fn write_at(&self, offset: u64, data: &[u8]) -> PyResult<()> {
        let path = self.path.clone();
        with_filesystem(
            self.session.as_ref(),
            self.media,
            self.sectors.as_ref(),
            self.ordinal,
            self.declared.as_deref(),
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

/// One file taken out of another medium's namespace as a load's source —
/// one of `Session.load_media`'s source shapes, minted by
/// `File.source()` and `StorageSpace.files()`.
///
/// It is **free-standing**: it rides the claim of the medium it came
/// from, so the namespace walk that named it ends before the load begins
/// and nothing is opened twice. `Session.load_media` **consumes** it —
/// the source moves into the load, exactly as `load_discovery` consumes
/// a `Discovery`, whether or not the load is refused — and every
/// attribute below raises by name once it has been.
#[pyclass(module = "remanence")]
pub struct FileSource {
    /// `None` once a load has consumed it.
    inner: Mutex<Option<remanence::FileSource>>,
}

/// The refusal a consumed source answers everything with.
fn consumed_file_source() -> PyErr {
    categorized_py_err(
        remanence::ErrorCategory::NotFound,
        "this file source was consumed by a load; ask again from the \
         namespace that named it",
    )
}

impl FileSource {
    /// A source over one the core minted — from a file view, or one
    /// member of a space's gathering.
    fn over(source: remanence::FileSource) -> Self {
        Self {
            inner: Mutex::new(Some(source)),
        }
    }

    /// Reads one fact off the source, or refuses by name where a load
    /// has already taken it.
    fn read<T>(&self, read: impl FnOnce(&remanence::FileSource) -> T) -> PyResult<T> {
        let source = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match source.as_ref() {
            Some(source) => Ok(read(source)),
            None => Err(consumed_file_source()),
        }
    }

    /// Takes the source for the load that consumes it.
    fn take(&self) -> PyResult<remanence::FileSource> {
        let mut source = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        source.take().ok_or_else(consumed_file_source)
    }
}

#[pymethods]
impl FileSource {
    /// The name the namespace holds this file under.
    #[getter]
    fn name(&self) -> PyResult<String> {
        self.read(|source| source.name().to_owned())
    }

    /// The file's size in bytes, as the namespace claims it.
    #[getter]
    fn size_bytes(&self) -> PyResult<u64> {
        self.read(remanence::FileSource::size)
    }

    fn __repr__(&self) -> String {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.as_ref() {
            Some(source) => format!(
                "FileSource(name={:?}, size_bytes={})",
                source.name(),
                source.size()
            ),
            None => "FileSource(consumed)".to_owned(),
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

/// Where a bitstream's state lives: its own session storage, for one a
/// caller materialized from an image, or the pooled medium that caches
/// it — re-resolved on every access, so a released medium refuses by
/// name rather than reaching state that is gone.
enum BitstreamProvider {
    Owned(remanence::Bitstream),
    Medium {
        session: Arc<Mutex<remanence::Session>>,
        id: remanence::MediaId,
    },
}

impl BitstreamProvider {
    fn with<T>(
        &self,
        action: impl FnOnce(&remanence::Bitstream) -> remanence::Result<T>,
    ) -> PyResult<T> {
        match self {
            Self::Owned(bitstream) => action(bitstream).map_err(to_py_err),
            Self::Medium { session, id } => {
                let mut guard = session
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(medium) = guard.medium_mut(*id) else {
                    return Err(categorized_py_err(
                        remanence::ErrorCategory::NotFound,
                        "the medium this bitstream reads was released",
                    ));
                };
                let bitstream = medium.bitstream().map_err(to_py_err)?;
                action(bitstream).map_err(to_py_err)
            }
        }
    }
}

/// A hardware bitstream, held in the session. The bits stay behind this
/// surface: what a hardware presentation makes of them is that seam's
/// business.
#[pyclass(module = "remanence")]
pub struct Bitstream {
    provider: BitstreamProvider,
    report: BitstreamReport,
}

#[pymethods]
impl Bitstream {
    /// The transition that produced this bitstream, and everything it
    /// does not carry of the medium beneath it.
    fn inspect(&self) -> BitstreamReport {
        self.report.clone()
    }

    /// How many locations the bitstream claims.
    #[getter]
    fn location_count(&self) -> PyResult<u64> {
        self.provider
            .with(|bitstream| Ok(bitstream.location_count()))
    }

    #[getter]
    fn backing_bytes(&self) -> PyResult<u64> {
        self.provider
            .with(|bitstream| Ok(bitstream.backing_bytes()))
    }

    #[getter]
    fn resident_bytes(&self) -> PyResult<u64> {
        self.provider
            .with(|bitstream| Ok(bitstream.resident_bytes()))
    }

    /// Materializes the family's encoded bytestream from this bitstream
    /// under the family's declared group code and codec reading.
    ///
    /// It takes no policy because the type carries one: being a
    /// bitstream of this family *means* resolving through the profile's
    /// declared codec policy, and what was used travels into the result
    /// as provenance. The bitstream is untouched and stays exactly what
    /// it was; the bytestream is separate session state with its own
    /// provenance, which is this bitstream's with the codec added to it.
    #[pyo3(signature = (*, cache_bytes = None))]
    fn materialize_bytestream(&self, cache_bytes: Option<u64>) -> PyResult<Bytestream> {
        let inner = self.provider.with(|bitstream| {
            bitstream.materialize_bytestream(cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES))
        })?;
        let report = BytestreamReport::new(inner.inspect());
        Ok(Bytestream {
            provider: BytestreamProvider::Owned(Arc::new(inner)),
            report,
        })
    }

    fn __repr__(&self) -> String {
        format!("Bitstream(locations={})", self.report.locations.len())
    }
}

/// Where a bytestream's state lives, under the same rule as the
/// bitstream's provider. It is shared rather than held, because the
/// location reads below re-resolve through it.
#[derive(Clone)]
enum BytestreamProvider {
    Owned(Arc<remanence::Bytestream>),
    Medium {
        session: Arc<Mutex<remanence::Session>>,
        id: remanence::MediaId,
    },
}

impl BytestreamProvider {
    fn with<T>(
        &self,
        action: impl FnOnce(&remanence::Bytestream) -> remanence::Result<T>,
    ) -> PyResult<T> {
        match self {
            Self::Owned(bytestream) => action(bytestream).map_err(to_py_err),
            Self::Medium { session, id } => {
                let mut guard = session
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(medium) = guard.medium_mut(*id) else {
                    return Err(categorized_py_err(
                        remanence::ErrorCategory::NotFound,
                        "the medium this bytestream reads was released",
                    ));
                };
                let bytestream = medium.bytestream().map_err(to_py_err)?;
                action(bytestream).map_err(to_py_err)
            }
        }
    }
}

/// An encoded bytestream, held in the session.
#[pyclass(module = "remanence")]
pub struct Bytestream {
    provider: BytestreamProvider,
    report: BytestreamReport,
}

#[pymethods]
impl Bytestream {
    fn inspect(&self) -> BytestreamReport {
        self.report.clone()
    }

    /// How many locations the bytestream resolves.
    #[getter]
    fn location_count(&self) -> PyResult<u64> {
        self.provider
            .with(|bytestream| Ok(bytestream.location_count()))
    }

    #[getter]
    fn backing_bytes(&self) -> PyResult<u64> {
        self.provider
            .with(|bytestream| Ok(bytestream.backing_bytes()))
    }

    #[getter]
    fn resident_bytes(&self) -> PyResult<u64> {
        self.provider
            .with(|bytestream| Ok(bytestream.resident_bytes()))
    }

    /// The framed bytes one whole track holds, addressed in the family's
    /// own terms — the Commodore 1541 numbers its tracks from 1.
    ///
    /// A location the stream does not hold is refused naming what it
    /// does hold: the stream's locations are what the medium carried,
    /// and a track it does not hold is absent rather than blank.
    fn location(&self, track: u32) -> PyResult<LocationBytes> {
        let bytes = self.provider.with(|bytestream| {
            Ok(bytestream
                .location(remanence::Location::track(track))?
                .bytes())
        })?;
        Ok(LocationBytes {
            provider: self.provider.clone(),
            track,
            bytes,
        })
    }

    /// Recognizes the recording's own sectors out of this bytestream,
    /// under the family's declared record grammar and sector reading.
    ///
    /// It takes no policy because the profile carries one, and what was
    /// used travels into the result as provenance. The bytestream is
    /// untouched and stays exactly what it was; the sector layer is
    /// separate session state, and there is no way back down — a sector
    /// is not lowered into bytes.
    /// The FM or MFM reading of this bytestream's records.
    ///
    /// A bytestream whose records are not FM or MFM sectors is refused
    /// by name rather than read as though they were — use
    /// `recognize_sectors` for a CBM DOS recording.
    #[pyo3(signature = (*, cache_bytes = None))]
    fn recognize_ibm_sectors(&self, cache_bytes: Option<u64>) -> PyResult<IbmSectors> {
        let inner = self.provider.with(|bytestream| {
            bytestream.recognize_sectors(cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES))
        })?;
        let family = inner.family().to_string();
        let Some(reading) = inner.into_ibm() else {
            return Err(to_py_err(remanence::Error::io(format!(
                "this recording's records were recognized by the '{family}' family,                  whose claims are not FM or MFM sectors"
            ))));
        };
        let report = IbmSectorReport::new(reading.inspect());
        Ok(IbmSectors {
            inner: Arc::new(reading),
            report,
        })
    }

    #[pyo3(signature = (*, cache_bytes = None))]
    fn recognize_sectors(&self, cache_bytes: Option<u64>) -> PyResult<C1541Sectors> {
        let inner = self.provider.with(|bytestream| {
            bytestream.recognize_sectors(cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES))
        })?;
        // The rung is one and the reading is the family's. A recording
        // whose records are not CBM DOS sectors is refused here by name
        // rather than read as though they were.
        let family = inner.family().to_string();
        let Some(reading) = inner.into_c1541() else {
            return Err(to_py_err(remanence::Error::io(format!(
                "this recording's records were recognized by the '{family}' family,                  whose claims are not CBM DOS sectors"
            ))));
        };
        let report = SectorReport::new(reading.inspect());
        Ok(C1541Sectors {
            inner: Arc::new(reading),
            report,
        })
    }

    fn __repr__(&self) -> String {
        format!("Bytestream(locations={})", self.report.locations.len())
    }
}

/// The framed bytes one location holds — the byte sequence the declared
/// group code resolved there, and nothing beneath it.
///
/// Bytes number from the first framed byte, because nothing before sync
/// is a byte at all; the unframed bits and the bit-level state stay
/// behind the surface, reported in the stream's own account. The reads
/// re-resolve through whatever provides the bytestream, so a released
/// medium refuses by name rather than reaching state that is gone.
#[pyclass(module = "remanence")]
pub struct LocationBytes {
    provider: BytestreamProvider,
    track: u32,
    bytes: u64,
}

#[pymethods]
impl LocationBytes {
    /// The whole track this location addresses — the family's own
    /// numbering, which starts at 1.
    #[getter]
    fn track(&self) -> u32 {
        self.track
    }

    /// How many framed bytes this location holds.
    #[getter]
    fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Exactly `length` framed bytes at `offset`, whole or not at all.
    ///
    /// A byte whose recorded pattern the family's table does not assign
    /// has no value to serve: it is stated as unresolved in the stream's
    /// account, and a read that touches one is refused naming it rather
    /// than answered with an invented value.
    fn read_at<'py>(
        &self,
        py: Python<'py>,
        offset: u64,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut buffer = vec![0u8; length];
        self.provider.with(|bytestream| {
            bytestream
                .location(remanence::Location::track(self.track))?
                .read_at(offset, &mut buffer)
        })?;
        Ok(PyBytes::new(py, &buffer))
    }

    fn __repr__(&self) -> String {
        format!("LocationBytes(track={}, bytes={})", self.track, self.bytes)
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
            self.half_track_numerator, self.half_track_denominator, self.readable, self.headers
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

    /// The **direct partition** over this recording — the library's own
    /// composition of the whole content, which is what a namespace above
    /// is reached through (P19).
    ///
    /// A recording records no partition scheme, so there is one member and
    /// it is synthetic: its account is `provenance` and never evidence,
    /// and it composes no addressed extent, because a recording's blocks
    /// are addressed by the recording rather than by position. The
    /// addressable vantage is therefore absent and the namespace vantage
    /// is *declared* — `filesystem_as("cbmdos")` — because nothing here
    /// determines a reading and this layer will not pick one.
    ///
    /// **The sector layer carries no file verbs of its own**: it may be
    /// asked what it composes — this — and may not be told to act as a
    /// namespace it is not. The declaration's refusal is the seam that ran
    /// out of answers stating it: a disk whose directory track does not
    /// read says so with the sector layer's own rule identity, and one
    /// that reads but claims no CBM DOS says *that*. Everything beneath
    /// stays readable either way.
    fn partition(&self) -> Partition {
        let view = self.inner.partition();
        partition_record(view.partition(), None, None, Some(Arc::clone(&self.inner)))
    }

    /// How many records the recognition read.
    #[getter]
    fn claim_count(&self) -> u64 {
        self.inner.claim_count()
    }

    /// How many locations it read them out of.
    #[getter]
    fn location_count(&self) -> u64 {
        self.inner.location_count()
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
        format!("C1541Sectors(claims={})", self.inner.claim_count())
    }
}

/// One record an FM or MFM recording states, exactly as it states it.
///
/// It is a separate type from `SectorClaim` rather than one carrying
/// both families' fields, because the two vocabularies have nothing in
/// common to share: a CBM DOS claim states a track and a sector under a
/// one-byte exclusive-or, and this states a cylinder, head and size code
/// under a sixteen-bit CRC. A type carrying both would be half-absent
/// whichever recording it described.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct IbmSectorClaim {
    /// The location of the family's own addressing this record sits on.
    pub location: u64,
    pub surface: Option<u64>,
    /// The byte of that location's bytestream the id field's mark sits
    /// at.
    pub at_byte: u64,
    /// The address the id field states for itself, as recorded — not
    /// where the record happens to sit.
    pub cylinder: u8,
    pub head: u8,
    pub sector: u8,
    /// The size code as recorded; the field it declares is `128 << code`
    /// bytes, and the code is kept because it is what was written.
    pub size_code: u8,
    /// The check the id field states, and the one its own bytes compute.
    pub header_checksum_stated: u16,
    pub header_checksum_computed: u16,
    /// Whether a data field followed the id field, and where it sat.
    pub has_data: bool,
    pub data_at_byte: u64,
    /// Whether the mark opening the data field was the deleted-data one.
    /// It is what the recording says, carried rather than judged:
    /// nothing here decides on a caller's behalf whether such a record
    /// counts.
    pub data_deleted: bool,
    pub data_checksum_stated: u16,
    pub data_checksum_computed: u16,
    /// Whether both checks agree. Only such a record is served.
    pub readable: bool,
}

#[pymethods]
impl IbmSectorClaim {
    fn __repr__(&self) -> String {
        format!(
            "IbmSectorClaim(cylinder={}, head={}, sector={}, readable={})",
            self.cylinder, self.head, self.sector, self.readable
        )
    }
}

/// The recognition an FM or MFM recording's records produced.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct IbmSectorReport {
    pub profile_id: String,
    /// The FM or MFM codec that framed these records.
    pub encoding_id: String,
    pub claims: Vec<IbmSectorClaim>,
    pub declared_loss: Vec<DeclaredLoss>,
    pub evidence: Vec<String>,
}

#[pymethods]
impl IbmSectorReport {
    fn __repr__(&self) -> String {
        format!(
            "IbmSectorReport(encoding_id={:?}, claims={})",
            self.encoding_id,
            self.claims.len()
        )
    }
}

impl IbmSectorReport {
    fn new(report: &remanence::IbmSectorReport) -> Self {
        Self {
            profile_id: report.profile_id.clone(),
            encoding_id: report.encoding_id.clone(),
            claims: report
                .claims
                .iter()
                .map(|claim| IbmSectorClaim {
                    location: claim.location,
                    surface: claim.surface,
                    at_byte: claim.at_byte,
                    cylinder: claim.cylinder,
                    head: claim.head,
                    sector: claim.sector,
                    size_code: claim.size_code,
                    header_checksum_stated: claim.header_checksum_stated,
                    header_checksum_computed: claim.header_checksum_computed,
                    has_data: claim.has_data,
                    data_at_byte: claim.data_at_byte,
                    data_deleted: claim.data_deleted,
                    data_checksum_stated: claim.data_checksum_stated,
                    data_checksum_computed: claim.data_checksum_computed,
                    readable: claim.readable(),
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

/// The uniform geometry an FM or MFM recording's records state for
/// themselves. Every number is read off the claims rather than off the
/// drive profile: a profile declares what the mechanism records, and
/// this says what this disk holds.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct IbmGeometry {
    pub cylinders: u32,
    pub heads: u32,
    pub sectors_per_track: u32,
    /// The lowest sector number the records state. IBM recordings
    /// conventionally number from one, but that is a convention and this
    /// reads what is there.
    pub first_sector: u8,
    pub sector_bytes: u32,
    /// What the whole extent spans, which is the geometry's rather than
    /// the sum of what reads: a hole still occupies its place.
    pub length_bytes: u64,
}

#[pymethods]
impl IbmGeometry {
    fn __repr__(&self) -> String {
        format!(
            "IbmGeometry(cylinders={}, heads={}, sectors_per_track={}, sector_bytes={})",
            self.cylinders, self.heads, self.sectors_per_track, self.sector_bytes
        )
    }
}

/// The recording's own sectors as an FM or MFM recording states them,
/// held in the session. The payloads stay behind this surface and are
/// read by the address the recording states for them.
#[pyclass(module = "remanence")]
pub struct IbmSectors {
    inner: Arc<remanence::IbmSectors>,
    report: IbmSectorReport,
}

#[pymethods]
impl IbmSectors {
    /// The recognition that produced these records, with every claim's
    /// evidence beside it.
    fn inspect(&self) -> IbmSectorReport {
        self.report.clone()
    }

    /// Reads one record by the address the recording states for it.
    ///
    /// Only a record whose checks both agree is served. One whose
    /// checksum disagrees holds what it holds and is reported by
    /// `inspect` with both numbers; serving it as though it read cleanly
    /// would answer a question the evidence does not. Nothing is
    /// repaired and no field is filled in.
    fn read_sector<'py>(
        &self,
        py: Python<'py>,
        cylinder: u8,
        head: u8,
        sector: u8,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let payload = self
            .inner
            .read_sector(cylinder, head, sector)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &payload))
    }

    /// How many records the recognition read.
    #[getter]
    fn claim_count(&self) -> u64 {
        self.inner.claim_count()
    }

    /// The uniform geometry these records state for themselves, or the
    /// refusal naming what makes them non-uniform.
    fn geometry(&self) -> PyResult<IbmGeometry> {
        let geometry = self.inner.geometry().map_err(to_py_err)?;
        Ok(IbmGeometry {
            cylinders: geometry.cylinders,
            heads: geometry.heads,
            sectors_per_track: geometry.sectors_per_track,
            first_sector: geometry.first_sector,
            sector_bytes: geometry.sector_bytes,
            length_bytes: geometry.length_bytes(),
        })
    }

    /// The **direct partition** over this recording — the library's own
    /// composition of the whole content, which is what a namespace above
    /// is reached through (P19).
    ///
    /// **Unlike a CBM DOS recording's, this partition is addressable.**
    /// Its records state a cylinder, a head and a sector number, and
    /// those compose exactly the geometry ordering FAT, HDOS and CP/M
    /// were all written against — so a volume here opens through the
    /// same seam a hard-disk image opens through, with no flux
    /// vocabulary reaching the filesystem adapter and none of the
    /// filesystem's reaching the recording.
    ///
    /// The namespace vantage is *declared*: nothing about an FM or MFM
    /// recording determines which of those it holds, and this layer will
    /// not pick one. `filesystem_as("fat")`, `"hdos"`, `"cpm"` or a
    /// `"cpm-*"` layout is the door; `"cbmdos"` is refused, because
    /// those blocks are addressed by the recording rather than by
    /// position.
    ///
    /// The extent's length is the geometry's rather than the sum of what
    /// reads: a record the recording never stated, or one whose CRC
    /// disagrees, is a hole that still occupies its place. Reads that
    /// touch it are refused naming the address and every other read
    /// answers — nothing is zeroed.
    fn partition(&self) -> PyResult<Partition> {
        // Composing it here is the check that the records make a uniform
        // image; the refusal travels now rather than at the first read.
        self.inner.partition().map_err(to_py_err)?;
        Ok(partition_over_ibm_sectors(Arc::clone(&self.inner)))
    }

    fn __repr__(&self) -> String {
        format!("IbmSectors(claims={})", self.inner.claim_count())
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

// ---------------------------------------------------------------------------
// The remanence image: the flux family's physical stratum, and the
// `.remanence` artifact it is read from and written to. The model
// beneath the root — orbits' points, magnetization, write geometry —
// does not cross this boundary; what crosses is the image's shape.

/// One index hole, as the image holds it: an exact fraction of a turn
/// for the centre and another for the extent. Nothing radial is stored.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone, PartialEq, Eq)]
pub struct FluxHole {
    pub center_numerator: u64,
    pub center_denominator: u64,
    pub extent_numerator: u64,
    pub extent_denominator: u64,
}

#[pymethods]
impl FluxHole {
    fn __repr__(&self) -> String {
        format!(
            "FluxHole(center={}/{}, extent={}/{})",
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
pub struct FluxOrbit {
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
impl FluxOrbit {
    fn __repr__(&self) -> String {
        format!(
            "FluxOrbit(surface={}, radius_microns={}, points={})",
            self.surface, self.radius_microns, self.points
        )
    }
}

/// A remanence image as it stands: the physical facts of one disk.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct FluxImageReport {
    /// The medium's shape in the model's own spelling: `"8-inch"`,
    /// `"5.25-inch"` or `"3.5-inch"`.
    pub form_factor: String,
    /// The angular unit every angle in the image is stated over — a
    /// unit rather than a measurement, so equality is exact.
    pub angular_divisions: u64,
    pub holes: Vec<FluxHole>,
    /// The surfaces carrying orbits, ascending.
    pub surfaces: Vec<u64>,
    /// Every orbit, ordered by surface then radius.
    pub orbits: Vec<FluxOrbit>,
    /// How the image came to be known, in human-readable terms.
    pub provenance: Vec<String>,
}

#[pymethods]
impl FluxImageReport {
    fn __repr__(&self) -> String {
        format!(
            "FluxImageReport(form_factor={:?}, orbits={})",
            self.form_factor,
            self.orbits.len()
        )
    }
}

impl FluxImageReport {
    fn new(report: &remanence::FluxImageReport) -> Self {
        Self {
            form_factor: report.form_factor.clone(),
            angular_divisions: report.angular_divisions,
            holes: report
                .holes
                .iter()
                .map(|hole| FluxHole {
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
                .map(|orbit| FluxOrbit {
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
pub struct FluxWriteReport {
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
impl FluxWriteReport {
    fn __repr__(&self) -> String {
        format!(
            "FluxWriteReport(path={:?}, orbits={}, points={})",
            self.path, self.orbits, self.points
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
pub struct FluxImage {
    inner: Option<remanence::FluxImage>,
    report: FluxImageReport,
}

#[pymethods]
impl FluxImage {
    /// Opens the `.remanence` artifact at `path`. The magic, the binary
    /// sentinel and the layout version are checked before anything else
    /// is believed, and a version past this release's claim is refused
    /// by name. `cache_bytes` declares the session working set; the
    /// bound narrows what stays resident and never refuses service.
    #[new]
    #[pyo3(signature = (path, *, cache_bytes = None))]
    fn new(path: PathBuf, cache_bytes: Option<u64>) -> PyResult<Self> {
        let image = match cache_bytes {
            Some(cache_bytes) => remanence::FluxImage::open_with_cache(path, cache_bytes),
            None => remanence::FluxImage::open(path),
        }
        .map_err(to_py_err)?;
        let report = FluxImageReport::new(&image.inspect());
        Ok(Self {
            inner: Some(image),
            report,
        })
    }

    /// The image as it stands: its shape, its holes, and every orbit's
    /// identity and counts.
    fn inspect(&self) -> FluxImageReport {
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
    fn write(&self, path: PathBuf) -> PyResult<FluxWriteReport> {
        let written = self.get()?.write(path).map_err(to_py_err)?;
        Ok(FluxWriteReport {
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
    /// holds, under the family's declared mechanics and read-channel
    /// rules.
    ///
    /// It takes no policy because the profile carries one; `cache_bytes`
    /// is the working-set bound, which is a bound rather than a reading.
    /// The image carries no clock — a cell length is a property of a
    /// recording, recoverable from the image, never a field of it — so
    /// the ladder stands on the served projection of it rather than on
    /// the image directly. The image is untouched and stays exactly what
    /// it was: the bitstream is separate session state, carrying the
    /// image's own provenance beneath the channel that produced it.
    /// There is no way back down.
    #[pyo3(signature = (*, cache_bytes = None))]
    fn materialize_bitstream(&self, cache_bytes: Option<u64>) -> PyResult<Bitstream> {
        let inner = self
            .get()?
            .materialize_bitstream(cache_bytes.unwrap_or(remanence::DEFAULT_CACHE_BYTES))
            .map_err(to_py_err)?;
        let report = BitstreamReport::new(inner.inspect());
        Ok(Bitstream {
            provider: BitstreamProvider::Owned(inner),
            report,
        })
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
            "FluxImage(form_factor={:?}, orbits={})",
            self.report.form_factor,
            self.report.orbits.len()
        )
    }
}

impl FluxImage {
    fn get(&self) -> PyResult<&remanence::FluxImage> {
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
    m.add("Error", m.py().get_type::<Error>())?;
    m.add_class::<Bitstream>()?;
    m.add_class::<Bytestream>()?;
    m.add_class::<LocationBytes>()?;
    m.add_class::<BitstreamReport>()?;
    m.add_class::<BitstreamLocation>()?;
    m.add_class::<BytestreamReport>()?;
    m.add_class::<BytestreamLocation>()?;
    m.add_class::<C1541Sectors>()?;
    m.add_class::<SectorReport>()?;
    m.add_class::<SectorLocation>()?;
    m.add_class::<SectorClaim>()?;
    m.add_class::<IbmSectors>()?;
    m.add_class::<IbmSectorClaim>()?;
    m.add_class::<IbmSectorReport>()?;
    m.add_class::<IbmGeometry>()?;
    m.add_class::<ContestedAddress>()?;
    m.add_class::<FluxImage>()?;
    m.add_class::<FluxImageReport>()?;
    m.add_class::<FluxHole>()?;
    m.add_class::<FluxOrbit>()?;
    m.add_class::<FluxWriteReport>()?;
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
    m.add_class::<StorageDevice>()?;
    m.add_class::<Medium>()?;
    m.add_class::<Partition>()?;
    m.add_class::<DeviceSlot>()?;
    m.add_class::<Assurance>()?;
    m.add_class::<Geometry>()?;
    m.add_class::<GeometryReading>()?;
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
    m.add_class::<FileSource>()?;
    m.add_function(wrap_pyfunction!(assurance_conditions, m)?)?;
    m.add_function(wrap_pyfunction!(device_slots, m)?)?;
    m.add_function(wrap_pyfunction!(discover_media, m)?)?;
    m.add_function(wrap_pyfunction!(formats, m)?)?;
    m.add_function(wrap_pyfunction!(geometry_sources, m)?)?;
    m.add_function(wrap_pyfunction!(new_media_kinds, m)?)?;
    m.add_function(wrap_pyfunction!(partition_schemes, m)?)?;
    m.add_function(wrap_pyfunction!(partition_types, m)?)?;
    Ok(())
}
