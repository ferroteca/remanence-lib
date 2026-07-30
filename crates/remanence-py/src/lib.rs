// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Python bindings for the Remanence disk image analysis library.
//!
//! The module mirrors the Rust crate's public surface: `Session` opens a disk
//! image (optionally inside a `.zip`), `Session.identify()` reports the
//! detected container layers, and `list_hdos_files` parses HDOS directories.
//! Failures raise `RemanenceError`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

create_exception!(
    remanence,
    RemanenceError,
    PyException,
    "Raised when the remanence library reports an error."
);

fn to_py_err(error: remanence::Error) -> PyErr {
    RemanenceError::new_err(error.to_string())
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

/// A container format definition from the registry.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct ContainerFormat {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    pub media_kind: Option<String>,
    pub sector_size: Option<usize>,
    pub cylinders: Option<usize>,
    pub sides: Option<usize>,
    pub tracks: Option<usize>,
    pub sectors_per_track: Option<usize>,
    pub filesystem_candidates: Vec<String>,
    pub attributes: BTreeMap<String, String>,
    pub expected_size: Option<usize>,
}

impl ContainerFormat {
    fn new(format: &remanence::ContainerFormat) -> Self {
        Self {
            id: format.id.clone(),
            name: format.name.clone(),
            extensions: format.extensions.clone(),
            media_kind: format.media_kind.clone(),
            sector_size: format.sector_size,
            cylinders: format.cylinders,
            sides: format.sides,
            tracks: format.tracks,
            sectors_per_track: format.sectors_per_track,
            filesystem_candidates: format.filesystem_candidates.clone(),
            attributes: format.attributes.clone(),
            expected_size: format.expected_size(),
        }
    }
}

/// A filesystem format definition from the registry.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct FilesystemFormat {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub container_candidates: Vec<String>,
    pub heuristics: Vec<String>,
    pub markers: Vec<String>,
    pub attributes: BTreeMap<String, String>,
}

impl FilesystemFormat {
    fn new(format: &remanence::FilesystemFormat) -> Self {
        Self {
            id: format.id.clone(),
            name: format.name.clone(),
            aliases: format.aliases.clone(),
            container_candidates: format.container_candidates.clone(),
            heuristics: format.heuristics.clone(),
            markers: format.markers.clone(),
            attributes: format.attributes.clone(),
        }
    }
}

/// Parsed container and filesystem format definitions.
#[pyclass(frozen, module = "remanence")]
pub struct FormatRegistry {
    inner: remanence::FormatRegistry,
}

#[pymethods]
impl FormatRegistry {
    /// Parses definition text into a registry.
    #[new]
    fn new(container_formats: &str, filesystem_formats: &str) -> PyResult<Self> {
        remanence::FormatRegistry::parse(container_formats, filesystem_formats)
            .map(|inner| Self { inner })
            .map_err(to_py_err)
    }

    /// The built-in starter registry.
    #[staticmethod]
    fn default() -> PyResult<Self> {
        remanence::default_format_registry()
            .map(|inner| Self { inner })
            .map_err(to_py_err)
    }

    /// Parses definition files into a registry.
    #[staticmethod]
    fn from_files(
        container_formats_path: PathBuf,
        filesystem_formats_path: PathBuf,
    ) -> PyResult<Self> {
        remanence::FormatRegistry::from_files(
            &container_formats_path,
            &filesystem_formats_path,
        )
        .map(|inner| Self { inner })
        .map_err(to_py_err)
    }

    /// Looks up one container format by id.
    fn container(&self, id: &str) -> Option<ContainerFormat> {
        self.inner.container(id).map(ContainerFormat::new)
    }

    /// Looks up one filesystem format by id.
    fn filesystem(&self, id: &str) -> Option<FilesystemFormat> {
        self.inner.filesystem(id).map(FilesystemFormat::new)
    }

    /// All container formats, keyed by id.
    #[getter]
    fn containers(&self) -> BTreeMap<String, ContainerFormat> {
        self.inner
            .containers()
            .iter()
            .map(|(id, format)| (id.clone(), ContainerFormat::new(format)))
            .collect()
    }

    /// All filesystem formats, keyed by id.
    #[getter]
    fn filesystems(&self) -> BTreeMap<String, FilesystemFormat> {
        self.inner
            .filesystems()
            .iter()
            .map(|(id, format)| (id.clone(), FilesystemFormat::new(format)))
            .collect()
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
    /// Opens `path` — a raw disk image, or `archive.zip[/entry]` — with the
    /// default format registry.
    #[new]
    fn new(path: PathBuf) -> PyResult<Self> {
        remanence::Session::open(path).map(|inner| Self { inner }).map_err(to_py_err)
    }

    /// Opens `path` with a caller-supplied format registry.
    #[staticmethod]
    fn with_registry(path: PathBuf, registry: &FormatRegistry) -> PyResult<Self> {
        remanence::Session::open_with_registry(path, registry.inner.clone())
            .map(|inner| Self { inner })
            .map_err(to_py_err)
    }

    /// The path the session was opened from (the archive path for ZIP inputs).
    #[getter]
    fn path(&self) -> String {
        self.inner.path().display().to_string()
    }

    /// The resolved image path (the entry name for ZIP inputs).
    #[getter]
    fn image_path(&self) -> String {
        self.inner.image_path().display().to_string()
    }

    /// The resolved image bytes.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.bytes())
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

    /// Parses the HDOS directory from the session's image bytes.
    fn list_hdos_files(&self) -> PyResult<Vec<HdosFile>> {
        remanence::list_hdos_files(self.inner.bytes())
            .map(|files| files.iter().map(HdosFile::new).collect())
            .map_err(to_py_err)
    }

    /// Reads a cataloged HDOS file's contents out of the session's image.
    fn read_hdos_file<'py>(
        &self,
        py: Python<'py>,
        name: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes =
            remanence::read_hdos_file(self.inner.bytes(), name).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    #[doc(hidden)]
    #[pyo3(name = "_mark_modified_for_test")]
    fn mark_modified_for_test(&mut self) {
        self.inner.mark_modified_for_test();
    }
}

/// One discovered MBR partition.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct PartitionInfo {
    pub number: u32,
    pub type_byte: u8,
    pub type_name: String,
    pub start_bytes: u64,
    pub length_bytes: u64,
}

/// One FAT volume actually read from a disk.
#[pyclass(frozen, get_all, skip_from_py_object, module = "remanence")]
#[derive(Clone)]
pub struct VolumeInfo {
    pub partition_number: Option<u32>,
    pub kind: String,
    pub label: Option<String>,
    pub offset_bytes: u64,
    pub length_bytes: u64,
    pub cluster_bytes: u64,
    pub cluster_count: u64,
    pub sectors_per_track: Option<u16>,
    pub heads: Option<u16>,
}

/// A disk's partitions and volumes, as they actually are.
#[pyclass(frozen, get_all, module = "remanence")]
pub struct DiskGeometry {
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

fn mode_str(mode: remanence::AccessMode) -> &'static str {
    match mode {
        remanence::AccessMode::ReadWrite => "read-write",
        remanence::AccessMode::ReadOnly => "read-only",
    }
}

/// An open at-rest disk image (raw or qcow2), held under the deny-write
/// claim until the object is closed or dropped.
#[pyclass(module = "remanence")]
pub struct Disk {
    inner: Option<remanence::Disk>,
}

impl Disk {
    fn get(&mut self) -> PyResult<&mut remanence::Disk> {
        self.inner
            .as_mut()
            .ok_or_else(|| RemanenceError::new_err("disk is closed"))
    }
}

#[pymethods]
impl Disk {
    /// Opens `path` under the deny-write claim: read/write preferred,
    /// read-only fallback, immediate failure when another process holds
    /// write access.
    #[new]
    fn new(path: PathBuf) -> PyResult<Self> {
        remanence::Disk::open(path)
            .map(|inner| Self { inner: Some(inner) })
            .map_err(to_py_err)
    }

    /// `"read-write"` or `"read-only"`.
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

    /// The disk's partitions and volumes, as they actually are.
    fn geometry(&mut self) -> PyResult<DiskGeometry> {
        let geometry = self.get()?.geometry().map_err(to_py_err)?;
        Ok(DiskGeometry {
            partitions: geometry
                .partitions
                .iter()
                .map(|partition| PartitionInfo {
                    number: partition.number,
                    type_byte: partition.type_byte,
                    type_name: partition.type_name.clone(),
                    start_bytes: partition.start_bytes,
                    length_bytes: partition.length_bytes,
                })
                .collect(),
            volumes: geometry
                .volumes
                .iter()
                .map(|volume| VolumeInfo {
                    partition_number: volume.partition_number,
                    kind: volume.kind.name().to_owned(),
                    label: volume.label.clone(),
                    offset_bytes: volume.offset_bytes,
                    length_bytes: volume.length_bytes,
                    cluster_bytes: volume.cluster_bytes,
                    cluster_count: volume.cluster_count,
                    sectors_per_track: volume.sectors_per_track,
                    heads: volume.heads,
                })
                .collect(),
        })
    }

    /// Lists a directory in volume `volume` ("" = root, "A/B" descends).
    #[pyo3(signature = (volume, path = ""))]
    fn entries(&mut self, volume: usize, path: &str) -> PyResult<Vec<FatEntry>> {
        Ok(self
            .get()?
            .entries(volume, path)
            .map_err(to_py_err)?
            .iter()
            .map(|entry| FatEntry {
                name: entry.name.clone(),
                kind: match entry.kind {
                    remanence::FatEntryKind::File => "file".to_owned(),
                    remanence::FatEntryKind::Directory => "directory".to_owned(),
                },
                size_bytes: entry.size_bytes,
            })
            .collect())
    }

    /// Copies a file's bytes out of volume `volume`.
    fn read_file<'py>(
        &mut self,
        py: Python<'py>,
        volume: usize,
        path: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.get()?.read_file(volume, path).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Writes a file into volume `volume`. Buffered until `commit()`.
    fn write_file(&mut self, volume: usize, path: &str, contents: &[u8]) -> PyResult<()> {
        self.get()?.write_file(volume, path, contents).map_err(to_py_err)
    }

    /// Creates a directory in volume `volume`. Buffered until `commit()`.
    fn make_directory(&mut self, volume: usize, path: &str) -> PyResult<()> {
        self.get()?.make_directory(volume, path).map_err(to_py_err)
    }

    /// The commit point: everything buffered reaches the image, flushed.
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

/// Parses the built-in starter format definitions.
#[pyfunction]
fn default_format_registry() -> PyResult<FormatRegistry> {
    FormatRegistry::default()
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
    m.add("DEFAULT_CONTAINER_FORMATS", remanence::DEFAULT_CONTAINER_FORMATS)?;
    m.add("DEFAULT_FILESYSTEM_FORMATS", remanence::DEFAULT_FILESYSTEM_FORMATS)?;
    m.add("RemanenceError", m.py().get_type::<RemanenceError>())?;
    m.add_class::<Session>()?;
    m.add_class::<Identification>()?;
    m.add_class::<Container>()?;
    m.add_class::<SizeInformation>()?;
    m.add_class::<ArchiveLayout>()?;
    m.add_class::<ImageLayout>()?;
    m.add_class::<DiskLayout>()?;
    m.add_class::<TrackSectorLayout>()?;
    m.add_class::<FilesystemLayout>()?;
    m.add_class::<FormatRegistry>()?;
    m.add_class::<ContainerFormat>()?;
    m.add_class::<FilesystemFormat>()?;
    m.add_class::<HdosFile>()?;
    m.add_class::<Disk>()?;
    m.add_class::<DiskGeometry>()?;
    m.add_class::<PartitionInfo>()?;
    m.add_class::<VolumeInfo>()?;
    m.add_class::<FatEntry>()?;
    m.add_function(wrap_pyfunction!(list_hdos_files, m)?)?;
    m.add_function(wrap_pyfunction!(read_hdos_file, m)?)?;
    m.add_function(wrap_pyfunction!(default_format_registry, m)?)?;
    Ok(())
}
