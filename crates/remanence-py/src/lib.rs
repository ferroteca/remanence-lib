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
#[pyclass(frozen, get_all, module = "remanence")]
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
#[pyclass(frozen, get_all, module = "remanence")]
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
#[pyclass(frozen, get_all, module = "remanence")]
#[derive(Clone)]
pub struct SizeInformation {
    pub current_bytes: Option<u64>,
    pub expected_bytes: Option<u64>,
}

/// Where the image bytes came from inside an archive.
#[pyclass(frozen, get_all, module = "remanence")]
#[derive(Clone)]
pub struct ArchiveLayout {
    pub path: String,
    pub entry_name: String,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: Option<u64>,
}

/// Where the payload sits inside a raw image container.
#[pyclass(frozen, get_all, module = "remanence")]
#[derive(Clone)]
pub struct ImageLayout {
    pub payload_offset_bytes: Option<u64>,
    pub payload_length_bytes: Option<u64>,
}

/// Per-track sector geometry for variable layouts.
#[pyclass(frozen, get_all, module = "remanence")]
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
#[pyclass(frozen, get_all, module = "remanence")]
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
#[pyclass(frozen, get_all, module = "remanence")]
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
#[pyclass(frozen, get_all, module = "remanence")]
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
#[pyclass(frozen, get_all, module = "remanence")]
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

    #[doc(hidden)]
    #[pyo3(name = "_mark_modified_for_test")]
    fn mark_modified_for_test(&mut self) {
        self.inner.mark_modified_for_test();
    }
}

/// Parses the HDOS directory from raw image bytes.
#[pyfunction]
fn list_hdos_files(image: Vec<u8>) -> PyResult<Vec<HdosFile>> {
    remanence::list_hdos_files(&image)
        .map(|files| files.iter().map(HdosFile::new).collect())
        .map_err(to_py_err)
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
    m.add_function(wrap_pyfunction!(list_hdos_files, m)?)?;
    m.add_function(wrap_pyfunction!(default_format_registry, m)?)?;
    Ok(())
}
