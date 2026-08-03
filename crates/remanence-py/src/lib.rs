// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Python bindings for the Remanence disk image analysis library.
//!
//! The module mirrors the Rust crate's public surface: `Archive` lists
//! what a supported archive holds (`.zip`, `.7z`), `Session` opens a disk
//! image (optionally an entry inside one of those archives),
//! `Session.identify()` reports the detected container layers, and
//! `list_hdos_files` parses HDOS directories. Failures raise
//! `RemanenceError`.

use std::path::PathBuf;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

create_exception!(
    remanence,
    RemanenceError,
    PyException,
    "Raised when the remanence library reports an error; `category` is stable."
);

fn categorized_py_err(category: remanence::ErrorCategory, message: impl Into<String>) -> PyErr {
    let error = RemanenceError::new_err(message.into());
    Python::attach(|py| {
        error
            .value(py)
            .setattr("category", category.as_str())
            .expect("RemanenceError instances accept attributes");
    });
    error
}

fn to_py_err(error: remanence::Error) -> PyErr {
    categorized_py_err(error.category(), error.to_string())
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
#[pyclass(module = "remanence")]
pub struct Session {
    inner: remanence::Session,
}

#[pymethods]
impl Session {
    /// Opens `path` — a raw disk image, or `archive[/entry]` naming an
    /// entry inside a supported archive (`.zip`, `.7z`) — with the
    /// built-in format catalogs. Omitting the entry works only when the
    /// archive holds exactly one file; `Archive` shows what one holds.
    /// `cache_bytes` declares the session cache bound (rounded up to
    /// whole 64 KiB extents, one extent at minimum); omitted, the
    /// stated default `DEFAULT_CACHE_BYTES` governs.
    #[new]
    #[pyo3(signature = (path, *, cache_bytes = None))]
    fn new(path: PathBuf, cache_bytes: Option<u64>) -> PyResult<Self> {
        match cache_bytes {
            Some(cache_bytes) => remanence::Session::open_with_cache(path, cache_bytes),
            None => remanence::Session::open(path),
        }
        .map(|inner| Self { inner })
        .map_err(to_py_err)
    }

    /// The path the session was opened from (the archive path for archive inputs).
    #[getter]
    fn path(&self) -> String {
        self.inner.path().display().to_string()
    }

    /// The resolved image path (the entry name for archive inputs).
    #[getter]
    fn image_path(&self) -> String {
        self.inner.image_path().display().to_string()
    }

    /// The resolved image's size in bytes.
    #[getter]
    fn size_bytes(&self) -> u64 {
        self.inner.size_bytes()
    }

    /// Reads `length` bytes of the resolved image at `offset` — the
    /// bounded access form: the image streams from its backing and is
    /// never resident whole.
    fn read_at<'py>(&self, py: Python<'py>, offset: u64, length: usize) -> PyResult<Bound<'py, PyBytes>> {
        let mut buffer = vec![0u8; length];
        self.inner.read_at(offset, &mut buffer).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Whether the session has unsaved modifications.
    #[getter]
    fn is_modified(&self) -> bool {
        self.inner.is_modified()
    }

    /// `"read-write"` or `"read-only"`: which mode the deny-write claim
    /// on the source file was obtained in.
    #[getter]
    fn access_mode(&self) -> &'static str {
        mode_str(self.inner.access_mode())
    }

    /// Identifies the image's container layers and probable filesystem.
    fn identify(&self, py: Python<'_>) -> PyResult<Identification> {
        let identification = self.inner.identify();
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

    /// Parses the HDOS directory from the session's image.
    fn list_hdos_files(&self) -> PyResult<Vec<HdosFile>> {
        self.inner
            .list_hdos_files()
            .map(|files| files.iter().map(HdosFile::new).collect())
            .map_err(to_py_err)
    }

    /// Reads a cataloged HDOS file's contents out of the session's image.
    fn read_hdos_file<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner.read_hdos_file(name).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    #[doc(hidden)]
    #[pyo3(name = "_mark_modified_for_test")]
    fn mark_modified_for_test(&mut self) {
        self.inner.mark_modified_for_test();
    }
}

/// One discovered partition row; every entry the table declares is
/// reported. `kind` is `"primary"` or `"logical"`; `type_name` is `None`
/// when the type byte is outside the claim. A row the library cannot
/// read stays here carrying `issue_category` (a stable category string)
/// and `issue` (the diagnostic) instead of vanishing — no volume is read
/// from it, and the rows behind it never renumber.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct PartitionInfo {
    pub number: u32,
    pub kind: String,
    pub type_byte: u8,
    pub type_name: Option<String>,
    pub start_bytes: u64,
    pub length_bytes: u64,
    pub issue_category: Option<String>,
    pub issue: Option<String>,
}

/// One FAT volume actually read from a disk. `cylinders` is present only
/// where an exact derivation exists — the boot record's track geometry
/// divides the total sector count with no remainder — never invented.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct VolumeInfo {
    /// Opaque stable identifier accepted by every file verb.
    pub id: String,
    pub partition_number: Option<u32>,
    pub kind: String,
    pub label: Option<String>,
    pub offset_bytes: u64,
    pub length_bytes: u64,
    pub cluster_bytes: u64,
    pub cluster_count: u64,
    pub sectors_per_track: Option<u16>,
    pub heads: Option<u16>,
    pub cylinders: Option<u64>,
}

/// A disk's complete report, as it actually is: `blank` marks an
/// all-zero sector 0 — a blank disk with zero volumes, an answer rather
/// than an error.
#[pyclass(frozen, get_all, module = "remanence")]
pub struct DiskGeometry {
    pub blank: bool,
    pub partitions: Vec<PartitionInfo>,
    pub volumes: Vec<VolumeInfo>,
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

/// An open disk image (raw or qcow2), held under the claim the
/// declared intent takes until the object is closed or dropped.
#[pyclass(module = "remanence")]
pub struct Disk {
    inner: Option<remanence::Disk>,
}

impl Disk {
    fn get(&mut self) -> PyResult<&mut remanence::Disk> {
        self.inner
            .as_mut()
            .ok_or_else(|| categorized_py_err(remanence::ErrorCategory::Io, "disk is closed"))
    }
}

#[pymethods]
impl Disk {
    /// Opens `path` with the caller's declared intent (P7).
    /// `writable=True` claims the image exclusively — no other reader
    /// or writer for the session's whole life — and fails at the open,
    /// naming the reason, when the claim cannot be secured, never by
    /// falling back to read-only. `writable=False` takes read access
    /// only, denies writes to others, and admits other readers. An
    /// interrupted commit left by an earlier session is reconciled
    /// before the disk is exposed (P9): the image comes back wholly
    /// the old state or wholly the committed new one, never a partial
    /// third state.
    #[new]
    #[pyo3(signature = (path, *, writable, cache_bytes = None))]
    fn new(path: PathBuf, writable: bool, cache_bytes: Option<u64>) -> PyResult<Self> {
        let intent = if writable {
            remanence::AccessIntent::Write
        } else {
            remanence::AccessIntent::Read
        };
        match cache_bytes {
            Some(cache_bytes) => remanence::Disk::open_with_cache(path, intent, cache_bytes),
            None => remanence::Disk::open(path, intent),
        }
        .map(|inner| Self { inner: Some(inner) })
        .map_err(to_py_err)
    }

    /// `"read-write"` or `"read-only"` — an echo of the declared intent.
    #[getter]
    fn mode(&mut self) -> PyResult<&'static str> {
        Ok(mode_str(self.get()?.mode()))
    }

    /// `"raw"` or `"qcow2"`.
    #[getter]
    fn format(&mut self) -> PyResult<&'static str> {
        Ok(match self.get()?.format() {
            remanence::DiskFormat::Raw => "raw",
            remanence::DiskFormat::Qcow2 { .. } => "qcow2",
        })
    }

    /// The qcow2 version, or `None` for a raw image.
    #[getter]
    fn qcow2_version(&mut self) -> PyResult<Option<u32>> {
        Ok(match self.get()?.format() {
            remanence::DiskFormat::Qcow2 { version } => Some(version),
            remanence::DiskFormat::Raw => None,
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

    /// The disk's complete report (U4): its partitions and
    /// volumes as they actually are. Blank is an answer — zero volumes,
    /// `blank` set — while non-zero data that is neither a supported
    /// filesystem nor a partition table raises by name, kept distinct
    /// from blank. A partition row the library cannot read stays in the
    /// report carrying its issue instead of failing the whole disk.
    fn geometry(&mut self) -> PyResult<DiskGeometry> {
        let geometry = self.get()?.geometry().map_err(to_py_err)?;
        Ok(DiskGeometry {
            blank: geometry.blank,
            partitions: geometry
                .partitions
                .iter()
                .map(|partition| PartitionInfo {
                    number: partition.number,
                    kind: partition.kind.name().to_owned(),
                    type_byte: partition.type_byte,
                    type_name: partition.type_name.clone(),
                    start_bytes: partition.start_bytes,
                    length_bytes: partition.length_bytes,
                    issue_category: partition
                        .issue
                        .as_ref()
                        .map(|issue| issue.category().as_str().to_owned()),
                    issue: partition.issue.as_ref().map(|issue| issue.to_string()),
                })
                .collect(),
            volumes: geometry
                .volumes
                .iter()
                .map(|volume| VolumeInfo {
                    id: volume.id.clone(),
                    partition_number: volume.partition_number,
                    kind: volume.kind.name().to_owned(),
                    label: volume.label.clone(),
                    offset_bytes: volume.offset_bytes,
                    length_bytes: volume.length_bytes,
                    cluster_bytes: volume.cluster_bytes,
                    cluster_count: volume.cluster_count,
                    sectors_per_track: volume.sectors_per_track,
                    heads: volume.heads,
                    cylinders: volume.cylinders,
                })
                .collect(),
        })
    }

    /// Lists a directory in `volume_id` ("" = root, "A/B" descends).
    #[pyo3(signature = (volume_id, path = ""))]
    fn entries(&mut self, volume_id: &str, path: &str) -> PyResult<Vec<FatEntry>> {
        Ok(self
            .get()?
            .entries(volume_id, path)
            .map_err(to_py_err)?
            .iter()
            .map(FatEntry::new)
            .collect())
    }

    /// Answers one path in `volume_id` with its entry, or `None` when
    /// nothing exists at that path — a missing leaf, a missing parent,
    /// or a parent that is a file alike. Absence is an answer,
    /// distinguished from failure, which raises.
    fn stat(&mut self, volume_id: &str, path: &str) -> PyResult<Option<FatEntry>> {
        Ok(self
            .get()?
            .stat(volume_id, path)
            .map_err(to_py_err)?
            .as_ref()
            .map(FatEntry::new))
    }

    /// Copies a file's bytes out of `volume_id`.
    fn read_file<'py>(
        &mut self,
        py: Python<'py>,
        volume_id: &str,
        path: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.get()?.read_file(volume_id, path).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Reads part of a file — the streamed form, `os.pread`-shaped:
    /// exactly `length` bytes at `offset`, which must lie within the
    /// file.
    fn read_file_at<'py>(
        &mut self,
        py: Python<'py>,
        volume_id: &str,
        path: &str,
        offset: u64,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut buffer = vec![0u8; length];
        self.get()?
            .read_file_at(volume_id, path, offset, &mut buffer)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &buffer))
    }

    /// Writes a file into `volume_id`. An existing file is overwritten —
    /// shorter or longer, its old clusters released and reclaimed —
    /// while an existing directory is refused. Buffered until
    /// `commit()`.
    fn write_file(&mut self, volume_id: &str, path: &str, contents: &[u8]) -> PyResult<()> {
        self.get()?
            .write_file(volume_id, path, contents)
            .map_err(to_py_err)
    }

    /// Sets a file's size, creating it when absent — `truncate`-shaped:
    /// kept bytes preserved in place, a grown region reads as zeros.
    /// With `write_file_at` this is the streamed replacement for
    /// `write_file`. Buffered until `commit()`.
    fn resize_file(&mut self, volume_id: &str, path: &str, size: u64) -> PyResult<()> {
        self.get()?
            .resize_file(volume_id, path, size)
            .map_err(to_py_err)
    }

    /// Writes part of a file in place — the streamed form,
    /// `os.pwrite`-shaped: the span must lie within the file's current
    /// size (resize first to change it). Buffered until `commit()`.
    fn write_file_at(
        &mut self,
        volume_id: &str,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> PyResult<()> {
        self.get()?
            .write_file_at(volume_id, path, offset, data)
            .map_err(to_py_err)
    }

    /// Ensures a directory exists in `volume_id`: missing parents are
    /// created, and a path that already leads to a directory succeeds
    /// unchanged. Buffered until `commit()`.
    fn make_directory(&mut self, volume_id: &str, path: &str) -> PyResult<()> {
        self.get()?
            .make_directory(volume_id, path)
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

    /// Releases the claim. Uncommitted changes are discarded.
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
    m.add_class::<Session>()?;
    m.add_class::<Identification>()?;
    m.add_class::<Container>()?;
    m.add_class::<SizeInformation>()?;
    m.add_class::<ArchiveLayout>()?;
    m.add_class::<ImageLayout>()?;
    m.add_class::<DiskLayout>()?;
    m.add_class::<TrackSectorLayout>()?;
    m.add_class::<FilesystemLayout>()?;
    m.add_class::<HdosFile>()?;
    m.add_class::<Disk>()?;
    m.add_class::<DiskGeometry>()?;
    m.add_class::<PartitionInfo>()?;
    m.add_class::<VolumeInfo>()?;
    m.add_class::<FatEntry>()?;
    m.add_function(wrap_pyfunction!(list_hdos_files, m)?)?;
    m.add_function(wrap_pyfunction!(read_hdos_file, m)?)?;
    Ok(())
}
