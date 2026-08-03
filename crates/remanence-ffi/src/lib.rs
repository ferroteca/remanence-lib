// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! C ABI for the Remanence disk image analysis library.
//!
//! Conventions:
//! - Handles (`RemanenceSession`, `RemanenceIdentification`, `RemanenceHdosFileList`,
//!   `RemanenceArchive`) are opaque and freed with their matching `*_free` function.
//! - `const char*` return values are UTF-8, owned by the handle they were read
//!   from, and valid until that handle is freed. Do not free them.
//! - Fallible calls take optional category and message outputs; on failure they
//!   store a stable [`RemanenceErrorCategory`] and a message to free with
//!   `remanence_string_free`.
//! - Accessors taking an index return null / false / 0 when the index is out of
//!   range or the field does not apply to the container's layout.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use remanence::{
    Container, ContainerKind, ContainerLayout, DiskLayout, ErrorCategory, HdosFile, Identification,
    PhysicalMediaLayout, SectorLayout, Session, list_hdos_files,
};

/// Stable, machine-readable classification of a library refusal. A fallible
/// call writes one beside its error message; the output is untouched on success.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemanenceErrorCategory {
    Locked = 0,
    InvalidImage = 1,
    Unsupported = 2,
    ReadOnly = 3,
    NotFound = 4,
    NotDirectory = 5,
    IsDirectory = 6,
    NoSpace = 7,
    Io = 8,
}

impl From<ErrorCategory> for RemanenceErrorCategory {
    fn from(category: ErrorCategory) -> Self {
        match category {
            ErrorCategory::Locked => Self::Locked,
            ErrorCategory::InvalidImage => Self::InvalidImage,
            ErrorCategory::Unsupported => Self::Unsupported,
            ErrorCategory::ReadOnly => Self::ReadOnly,
            ErrorCategory::NotFound => Self::NotFound,
            ErrorCategory::NotDirectory => Self::NotDirectory,
            ErrorCategory::IsDirectory => Self::IsDirectory,
            ErrorCategory::NoSpace => Self::NoSpace,
            ErrorCategory::Io => Self::Io,
        }
    }
}

/// What role a detected container plays in the image's layering.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceContainerKind {
    Archive,
    Image,
    PhysicalMedia,
    Filesystem,
    Unknown,
}

/// Which layout details a container carries.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceLayoutKind {
    Unknown,
    Archive,
    Image,
    PhysicalMedia,
    Filesystem,
}

/// Sector arrangement across a disk.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceSectorLayoutKind {
    Unknown,
    Fixed,
    Variable,
}

fn to_cstring(value: &str) -> CString {
    CString::new(value.replace('\0', "\u{fffd}")).expect("interior NULs replaced")
}

fn to_owned_c_char(value: &str) -> *mut c_char {
    to_cstring(value).into_raw()
}

unsafe fn clear_error(error_out: *mut *mut c_char) {
    if !error_out.is_null() {
        unsafe { *error_out = ptr::null_mut() };
    }
}

unsafe fn set_error(
    category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error: &remanence::Error,
) {
    if !category_out.is_null() {
        unsafe { *category_out = error.category().into() };
    }
    if !error_out.is_null() {
        unsafe { *error_out = to_owned_c_char(&error.to_string()) };
    }
}

unsafe fn write_opt_u64(value: Option<u64>, out: *mut u64) -> bool {
    match value {
        Some(value) => {
            if !out.is_null() {
                unsafe { *out = value };
            }
            true
        }
        None => false,
    }
}

unsafe fn write_opt_u32(value: Option<u32>, out: *mut u32) -> bool {
    match value {
        Some(value) => {
            if !out.is_null() {
                unsafe { *out = value };
            }
            true
        }
        None => false,
    }
}

struct TrackView {
    cylinder: u32,
    side: u32,
    sectors: u32,
    sector_size: Option<u64>,
}

struct DiskView {
    media_kind: Option<CString>,
    sector_size: Option<u64>,
    cylinders: Option<u32>,
    sides: Option<u32>,
    sector_layout: RemanenceSectorLayoutKind,
    sectors_per_track: u32,
    tracks: Vec<TrackView>,
    total_sectors: Option<u64>,
}

impl DiskView {
    fn new(layout: &DiskLayout) -> Self {
        let (sector_layout, sectors_per_track, tracks) = match &layout.sectors {
            SectorLayout::Unknown => (RemanenceSectorLayoutKind::Unknown, 0, Vec::new()),
            SectorLayout::Fixed { sectors_per_track } => {
                (RemanenceSectorLayoutKind::Fixed, *sectors_per_track, Vec::new())
            }
            SectorLayout::Variable { tracks } => (
                RemanenceSectorLayoutKind::Variable,
                0,
                tracks
                    .iter()
                    .map(|track| TrackView {
                        cylinder: track.cylinder,
                        side: track.side,
                        sectors: track.sectors,
                        sector_size: track.sector_size,
                    })
                    .collect(),
            ),
        };

        Self {
            media_kind: layout.media_kind.as_deref().map(to_cstring),
            sector_size: layout.sector_size,
            cylinders: layout.cylinders,
            sides: layout.sides,
            sector_layout,
            sectors_per_track,
            tracks,
            total_sectors: layout.total_sectors,
        }
    }
}

enum LayoutView {
    Unknown,
    Archive {
        path: CString,
        entry_name: CString,
        compressed_size: Option<u64>,
        uncompressed_size: Option<u64>,
    },
    Image {
        payload_offset_bytes: Option<u64>,
        payload_length_bytes: Option<u64>,
    },
    PhysicalMedia(Option<DiskView>),
    Filesystem {
        offset_bytes: Option<u64>,
        length_bytes: Option<u64>,
    },
}

struct ContainerView {
    kind: RemanenceContainerKind,
    id: CString,
    name: CString,
    confidence: u8,
    known: bool,
    current_bytes: Option<u64>,
    expected_bytes: Option<u64>,
    layout: LayoutView,
}

impl ContainerView {
    fn new(container: &Container) -> Self {
        let kind = match container.kind {
            ContainerKind::Archive => RemanenceContainerKind::Archive,
            ContainerKind::Image => RemanenceContainerKind::Image,
            ContainerKind::PhysicalMedia => RemanenceContainerKind::PhysicalMedia,
            ContainerKind::Filesystem => RemanenceContainerKind::Filesystem,
            ContainerKind::Unknown => RemanenceContainerKind::Unknown,
        };

        let layout = match &container.layout {
            ContainerLayout::Unknown => LayoutView::Unknown,
            ContainerLayout::Archive(layout) => LayoutView::Archive {
                path: to_cstring(&layout.path.display().to_string()),
                entry_name: to_cstring(&layout.entry_name),
                compressed_size: layout.compressed_size,
                uncompressed_size: layout.uncompressed_size,
            },
            ContainerLayout::Image(layout) => LayoutView::Image {
                payload_offset_bytes: layout.payload_offset_bytes,
                payload_length_bytes: layout.payload_length_bytes,
            },
            ContainerLayout::PhysicalMedia(layout) => match layout {
                PhysicalMediaLayout::Unknown => LayoutView::PhysicalMedia(None),
                PhysicalMediaLayout::Disk(disk) => {
                    LayoutView::PhysicalMedia(Some(DiskView::new(disk)))
                }
            },
            ContainerLayout::Filesystem(layout) => LayoutView::Filesystem {
                offset_bytes: layout.offset_bytes,
                length_bytes: layout.length_bytes,
            },
        };

        Self {
            kind,
            id: to_cstring(&container.id),
            name: to_cstring(&container.name),
            confidence: container.confidence,
            known: container.known,
            current_bytes: container.size.current_bytes,
            expected_bytes: container.size.expected_bytes,
            layout,
        }
    }
}

/// An open analysis session over one disk image.
pub struct RemanenceSession {
    session: Session,
    path: CString,
    image_path: CString,
}

/// The result of identifying a session's image.
pub struct RemanenceIdentification {
    modified: bool,
    containers: Vec<ContainerView>,
    evidence: Vec<CString>,
}

struct HdosFileView {
    name: CString,
    extension: CString,
    display_name: CString,
    size_sectors: u32,
    size_bytes: u64,
    modified_date_raw: u16,
    flags_raw: u8,
    flags: CString,
    modified_date: CString,
}

impl HdosFileView {
    fn new(file: &HdosFile) -> Self {
        Self {
            name: to_cstring(&file.name),
            extension: to_cstring(&file.extension),
            display_name: to_cstring(&file.display_name()),
            size_sectors: file.size_sectors,
            size_bytes: file.size_bytes(),
            modified_date_raw: file.modified_date,
            flags_raw: file.flags,
            flags: to_cstring(&file.flags_string()),
            modified_date: to_cstring(&file.modified_date_string()),
        }
    }
}

/// A parsed HDOS directory listing.
pub struct RemanenceHdosFileList {
    files: Vec<HdosFileView>,
}

unsafe fn container_view<'a>(
    identification: *const RemanenceIdentification,
    index: usize,
) -> Option<&'a ContainerView> {
    let identification = unsafe { identification.as_ref() }?;
    identification.containers.get(index)
}

unsafe fn hdos_file_view<'a>(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> Option<&'a HdosFileView> {
    let list = unsafe { list.as_ref() }?;
    list.files.get(index)
}

fn hdos_list_from_bytes(
    bytes: &[u8],
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceHdosFileList {
    match list_hdos_files(bytes) {
        Ok(files) => {
            let files = files.iter().map(HdosFileView::new).collect();
            Box::into_raw(Box::new(RemanenceHdosFileList { files }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Returns the library version as a static string. Do not free.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// The stated default session cache bound, in bytes: what an open
/// without a declared bound uses.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_default_cache_bytes() -> u64 {
    remanence::DEFAULT_CACHE_BYTES
}

/// Frees a string returned through an `error_out` parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_string_free(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}

/// Opens `path` (UTF-8) — a raw disk image, or `archive[/entry]` naming an
/// entry inside a supported archive (`.zip`, `.7z`) — with the built-in
/// format adapters. Returns null on failure and stores a message in
/// `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_open(
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceSession {
    unsafe { clear_error(error_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }

    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match Session::open(path.as_ref()) {
        Ok(session) => {
            let path = to_cstring(&session.path().display().to_string());
            let image_path = to_cstring(&session.image_path().display().to_string());
            Box::into_raw(Box::new(RemanenceSession {
                session,
                path,
                image_path,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Opens a session as `remanence_session_open` does, under a declared
/// session cache bound: at most `cache_bytes` stays resident, rounded
/// up to whole 64 KiB extents with one extent as the floor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_open_with_cache(
    path: *const c_char,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceSession {
    unsafe { clear_error(error_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }

    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match Session::open_with_cache(path.as_ref(), cache_bytes) {
        Ok(session) => {
            let path = to_cstring(&session.path().display().to_string());
            let image_path = to_cstring(&session.image_path().display().to_string());
            Box::into_raw(Box::new(RemanenceSession {
                session,
                path,
                image_path,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a session handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_free(session: *mut RemanenceSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// The path the session was opened from (the archive path for archive inputs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_path(session: *const RemanenceSession) -> *const c_char {
    match unsafe { session.as_ref() } {
        Some(session) => session.path.as_ptr(),
        None => ptr::null(),
    }
}

/// The resolved image path (the entry name for archive inputs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_image_path(session: *const RemanenceSession) -> *const c_char {
    match unsafe { session.as_ref() } {
        Some(session) => session.image_path.as_ptr(),
        None => ptr::null(),
    }
}

/// The resolved image's size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_size_bytes(session: *const RemanenceSession) -> u64 {
    match unsafe { session.as_ref() } {
        Some(session) => session.session.size_bytes(),
        None => 0,
    }
}

/// Reads `length` bytes of the resolved image at `offset` into
/// `buffer_out` — the bounded access form: the image streams from its
/// backing and is never resident whole. Returns false on failure and
/// stores a message in `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_read_at(
    session: *const RemanenceSession,
    offset: u64,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(session) = (unsafe { session.as_ref() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    match session.session.read_at(offset, buffer) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            false
        }
    }
}

/// Whether the session has unsaved modifications.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_is_modified(session: *const RemanenceSession) -> bool {
    unsafe { session.as_ref() }.is_some_and(|session| session.session.is_modified())
}

/// Identifies the image's container layers and probable filesystem. Free the
/// result with `remanence_identification_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_identify(
    session: *const RemanenceSession,
) -> *mut RemanenceIdentification {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return ptr::null_mut();
    };
    let Identification {
        containers,
        modified,
        evidence,
    } = session.session.identify();

    Box::into_raw(Box::new(RemanenceIdentification {
        modified,
        containers: containers.iter().map(ContainerView::new).collect(),
        evidence: evidence.iter().map(|line| to_cstring(line)).collect(),
    }))
}

/// Frees an identification handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_free(identification: *mut RemanenceIdentification) {
    if !identification.is_null() {
        drop(unsafe { Box::from_raw(identification) });
    }
}

/// Whether the session reported unsaved modifications at identify time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_modified(
    identification: *const RemanenceIdentification,
) -> bool {
    unsafe { identification.as_ref() }.is_some_and(|identification| identification.modified)
}

/// Number of detected container layers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_container_count(
    identification: *const RemanenceIdentification,
) -> usize {
    unsafe { identification.as_ref() }.map_or(0, |identification| identification.containers.len())
}

/// Number of evidence lines.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_evidence_count(
    identification: *const RemanenceIdentification,
) -> usize {
    unsafe { identification.as_ref() }.map_or(0, |identification| identification.evidence.len())
}

/// One evidence line, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_evidence(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { identification.as_ref() }
        .and_then(|identification| identification.evidence.get(index))
        .map_or(ptr::null(), |line| line.as_ptr())
}

/// The container's kind, or `RemanenceContainerKind::Unknown` when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_kind(
    identification: *const RemanenceIdentification,
    index: usize,
) -> RemanenceContainerKind {
    unsafe { container_view(identification, index) }
        .map_or(RemanenceContainerKind::Unknown, |container| container.kind)
}

/// The container's id (e.g. "h8d", "zip", "hdos").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_id(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { container_view(identification, index) }
        .map_or(ptr::null(), |container| container.id.as_ptr())
}

/// The container's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_name(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { container_view(identification, index) }
        .map_or(ptr::null(), |container| container.name.as_ptr())
}

/// Detection confidence, 0-100.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_confidence(
    identification: *const RemanenceIdentification,
    index: usize,
) -> u8 {
    unsafe { container_view(identification, index) }.map_or(0, |container| container.confidence)
}

/// Whether the container matched a known format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_known(
    identification: *const RemanenceIdentification,
    index: usize,
) -> bool {
    unsafe { container_view(identification, index) }.is_some_and(|container| container.known)
}

/// Current size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_current_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { container_view(identification, index) }.and_then(|c| c.current_bytes);
    unsafe { write_opt_u64(value, out) }
}

/// Expected size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_expected_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { container_view(identification, index) }.and_then(|c| c.expected_bytes);
    unsafe { write_opt_u64(value, out) }
}

/// Which layout details this container carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_layout_kind(
    identification: *const RemanenceIdentification,
    index: usize,
) -> RemanenceLayoutKind {
    unsafe { container_view(identification, index) }.map_or(RemanenceLayoutKind::Unknown, |container| {
        match &container.layout {
            LayoutView::Unknown => RemanenceLayoutKind::Unknown,
            LayoutView::Archive { .. } => RemanenceLayoutKind::Archive,
            LayoutView::Image { .. } => RemanenceLayoutKind::Image,
            LayoutView::PhysicalMedia(_) => RemanenceLayoutKind::PhysicalMedia,
            LayoutView::Filesystem { .. } => RemanenceLayoutKind::Filesystem,
        }
    })
}

/// Archive layout: the archive file path; null for other layouts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_archive_path(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { path, .. }) => path.as_ptr(),
        _ => ptr::null(),
    }
}

/// Archive layout: the entry name inside the archive; null for other layouts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_archive_entry_name(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { entry_name, .. }) => entry_name.as_ptr(),
        _ => ptr::null(),
    }
}

/// Archive layout: compressed entry size; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_archive_compressed_size(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive {
            compressed_size, ..
        }) => *compressed_size,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Archive layout: uncompressed entry size; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_archive_uncompressed_size(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive {
            uncompressed_size, ..
        }) => *uncompressed_size,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Image layout: payload offset in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_image_payload_offset(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Image {
            payload_offset_bytes,
            ..
        }) => *payload_offset_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Image layout: payload length in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_image_payload_length(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Image {
            payload_length_bytes,
            ..
        }) => *payload_length_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

unsafe fn disk_view<'a>(
    identification: *const RemanenceIdentification,
    index: usize,
) -> Option<&'a DiskView> {
    match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::PhysicalMedia(disk)) => disk.as_ref(),
        _ => None,
    }
}

/// Physical media layout: whether disk geometry is known.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_has_disk_layout(
    identification: *const RemanenceIdentification,
    index: usize,
) -> bool {
    unsafe { disk_view(identification, index) }.is_some()
}

/// Disk layout: media kind (e.g. "floppy"); null when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_media_kind(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { disk_view(identification, index) }
        .and_then(|disk| disk.media_kind.as_ref())
        .map_or(ptr::null(), |kind| kind.as_ptr())
}

/// Disk layout: sector size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_sector_size(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.sector_size);
    unsafe { write_opt_u64(value, out) }
}

/// Disk layout: cylinder count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_cylinders(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.cylinders);
    unsafe { write_opt_u32(value, out) }
}

/// Disk layout: side count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_sides(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.sides);
    unsafe { write_opt_u32(value, out) }
}

/// Disk layout: how sectors are arranged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_sector_layout_kind(
    identification: *const RemanenceIdentification,
    index: usize,
) -> RemanenceSectorLayoutKind {
    unsafe { disk_view(identification, index) }
        .map_or(RemanenceSectorLayoutKind::Unknown, |disk| disk.sector_layout)
}

/// Disk layout: sectors per track for fixed layouts; 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_sectors_per_track(
    identification: *const RemanenceIdentification,
    index: usize,
) -> u32 {
    unsafe { disk_view(identification, index) }.map_or(0, |disk| disk.sectors_per_track)
}

/// Disk layout: per-track entry count for variable layouts; 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_track_count(
    identification: *const RemanenceIdentification,
    index: usize,
) -> usize {
    unsafe { disk_view(identification, index) }.map_or(0, |disk| disk.tracks.len())
}

/// Disk layout: one per-track entry for variable layouts. Returns false when
/// out of range. `has_sector_size` and `sector_size` report the optional
/// per-track sector size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_track(
    identification: *const RemanenceIdentification,
    index: usize,
    track_index: usize,
    cylinder: *mut u32,
    side: *mut u32,
    sectors: *mut u32,
    has_sector_size: *mut bool,
    sector_size: *mut u64,
) -> bool {
    let Some(track) =
        unsafe { disk_view(identification, index) }.and_then(|disk| disk.tracks.get(track_index))
    else {
        return false;
    };

    unsafe {
        if !cylinder.is_null() {
            *cylinder = track.cylinder;
        }
        if !side.is_null() {
            *side = track.side;
        }
        if !sectors.is_null() {
            *sectors = track.sectors;
        }
        if !has_sector_size.is_null() {
            *has_sector_size = track.sector_size.is_some();
        }
        if !sector_size.is_null() {
            *sector_size = track.sector_size.unwrap_or(0);
        }
    }
    true
}

/// Disk layout: total sector count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_disk_total_sectors(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.total_sectors);
    unsafe { write_opt_u64(value, out) }
}

/// Filesystem layout: offset in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_fs_offset_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Filesystem { offset_bytes, .. }) => *offset_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Filesystem layout: length in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_container_fs_length_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Filesystem { length_bytes, .. }) => *length_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Parses the HDOS directory from raw image bytes. Returns null on failure and
/// stores a message in `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_list_hdos_files(
    bytes: *const u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceHdosFileList {
    unsafe { clear_error(error_out) };
    if bytes.is_null() {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, length) };
    hdos_list_from_bytes(bytes, error_category_out, error_out)
}

/// Parses the HDOS directory from a session's image. Returns null on
/// failure and stores a message in `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_list_hdos_files(
    session: *const RemanenceSession,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceHdosFileList {
    unsafe { clear_error(error_out) };
    let Some(session) = (unsafe { session.as_ref() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match session.session.list_hdos_files() {
        Ok(files) => {
            let files = files.iter().map(HdosFileView::new).collect();
            Box::into_raw(Box::new(RemanenceHdosFileList { files }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees an HDOS file list handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_list_free(list: *mut RemanenceHdosFileList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

/// Number of files in the listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_count(list: *const RemanenceHdosFileList) -> usize {
    unsafe { list.as_ref() }.map_or(0, |list| list.files.len())
}

/// File name without extension, e.g. "HDOS".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_name(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }.map_or(ptr::null(), |file| file.name.as_ptr())
}

/// File extension, possibly empty, e.g. "SYS".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_extension(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }.map_or(ptr::null(), |file| file.extension.as_ptr())
}

/// `"NAME.EXT"`, or `"NAME"` when the extension is empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_display_name(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }.map_or(ptr::null(), |file| file.display_name.as_ptr())
}

/// Size in 256-byte sectors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_size_sectors(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> u32 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.size_sectors)
}

/// Size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_size_bytes(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> u64 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.size_bytes)
}

/// Raw HDOS date word.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_modified_date_raw(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> u16 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.modified_date_raw)
}

/// Raw HDOS flag byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_flags_raw(list: *const RemanenceHdosFileList, index: usize) -> u8 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.flags_raw)
}

/// HDOS flag letters (subset of "SLWC"), possibly empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_flags(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }.map_or(ptr::null(), |file| file.flags.as_ptr())
}

/// HDOS catalog date, e.g. "09-May-78", or "No-Date".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_hdos_file_modified_date(
    list: *const RemanenceHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }.map_or(ptr::null(), |file| file.modified_date.as_ptr())
}

// ---------------------------------------------------------------------------
// The Disk surface (U3/U4): open a raw or qcow2 image under the
// P7 claim, report partitions and volumes, read/write FAT files with a
// commit point.

use remanence::{
    AccessIntent, AccessMode, Disk, DiskContent, DiskFormat, FatEntry, FatEntryKind, RegionRole,
    VolumeId, VolumeOrigin, read_hdos_file,
};

/// The caller's declared intent when opening a disk (P7).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceAccessIntent {
    Read,
    Write,
}

/// A session's access mode. For a disk this echoes the declared intent;
/// for an identification session it reports what the P7 ladder obtained.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceAccessMode {
    ReadWrite,
    ReadOnly,
}

/// The container format a disk image turned out to be.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDiskFormat {
    Raw,
    Qcow2,
}

/// What a FAT directory entry is.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceFatEntryKind {
    File,
    Directory,
}

fn access_mode(mode: AccessMode) -> RemanenceAccessMode {
    match mode {
        AccessMode::ReadWrite => RemanenceAccessMode::ReadWrite,
        AccessMode::ReadOnly => RemanenceAccessMode::ReadOnly,
    }
}

/// An open disk image.
pub struct RemanenceDisk {
    disk: Disk,
}

/// A directory listing.
pub struct RemanenceFatEntryList {
    entries: Vec<FatEntryView>,
}

struct FatEntryView {
    name: CString,
    kind: RemanenceFatEntryKind,
    size_bytes: u64,
}

impl FatEntryView {
    fn new(entry: &FatEntry) -> Self {
        Self {
            name: to_cstring(&entry.name),
            kind: match entry.kind {
                FatEntryKind::File => RemanenceFatEntryKind::File,
                FatEntryKind::Directory => RemanenceFatEntryKind::Directory,
            },
            size_bytes: entry.size_bytes,
        }
    }
}

/// Bytes read out of a volume or catalog.
pub struct RemanenceFileData {
    bytes: Vec<u8>,
}

unsafe fn utf8_arg<'a>(value: *const c_char) -> Option<std::borrow::Cow<'a, str>> {
    if value.is_null() {
        return None;
    }
    Some(String::from_utf8_lossy(
        unsafe { CStr::from_ptr(value) }.to_bytes(),
    ))
}

/// Opens `path` (UTF-8) as a disk image — raw or qcow2, detected by
/// magic — with the caller's declared intent (P7). A `Write` open
/// claims the image exclusively for the session's whole life and fails
/// at the open, naming the reason, when the claim cannot be secured —
/// never by falling back; a `Read` open takes read access only, denies
/// writes to others, and admits other readers. An interrupted commit
/// left by an earlier session is reconciled before the disk is exposed
/// (P9): the image comes back wholly the old state or wholly the
/// committed new one, never a partial third state. Returns null on
/// failure with a message in `error_out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_open(
    path: *const c_char,
    intent: RemanenceAccessIntent,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceDisk {
    unsafe { clear_error(error_out) };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let intent = match intent {
        RemanenceAccessIntent::Read => AccessIntent::Read,
        RemanenceAccessIntent::Write => AccessIntent::Write,
    };
    match Disk::open(path.as_ref(), intent) {
        Ok(disk) => Box::into_raw(Box::new(RemanenceDisk { disk })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Opens a disk as `remanence_disk_open` does, under a declared session
/// cache bound: at most `cache_bytes` of session state stays resident,
/// rounded up to whole 64 KiB extents with one extent as the floor;
/// altered state past the bound spills to private session storage,
/// never the image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_open_with_cache(
    path: *const c_char,
    intent: RemanenceAccessIntent,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceDisk {
    unsafe { clear_error(error_out) };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let intent = match intent {
        RemanenceAccessIntent::Read => AccessIntent::Read,
        RemanenceAccessIntent::Write => AccessIntent::Write,
    };
    match Disk::open_with_cache(path.as_ref(), intent, cache_bytes) {
        Ok(disk) => Box::into_raw(Box::new(RemanenceDisk { disk })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a disk handle, releasing the P7 claim. Uncommitted changes are
/// discarded (the commit point never reached the file).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_free(disk: *mut RemanenceDisk) {
    if !disk.is_null() {
        drop(unsafe { Box::from_raw(disk) });
    }
}

/// The disk session's access mode — an echo of the declared intent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_mode(disk: *const RemanenceDisk) -> RemanenceAccessMode {
    unsafe { disk.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |disk| {
        access_mode(disk.disk.mode())
    })
}

/// The detected container format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_format(disk: *const RemanenceDisk) -> RemanenceDiskFormat {
    match unsafe { disk.as_ref() }.map(|disk| disk.disk.format()) {
        Some(DiskFormat::Qcow2 { .. }) => RemanenceDiskFormat::Qcow2,
        _ => RemanenceDiskFormat::Raw,
    }
}

/// The qcow2 version, or 0 for a raw image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_qcow2_version(disk: *const RemanenceDisk) -> u32 {
    match unsafe { disk.as_ref() }.map(|disk| disk.disk.format()) {
        Some(DiskFormat::Qcow2 { version }) => version,
        _ => 0,
    }
}

/// The virtual disk size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_size(disk: *const RemanenceDisk) -> u64 {
    unsafe { disk.as_ref() }.map_or(0, |disk| disk.disk.size())
}

/// Whether uncommitted changes exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_is_modified(disk: *const RemanenceDisk) -> bool {
    unsafe { disk.as_ref() }.is_some_and(|disk| disk.disk.is_modified())
}

/// Lists a directory in `volume_id` ("" = root, "A/B" descends). Free
/// with `remanence_fat_entry_list_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_entries(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFatEntryList {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let path = unsafe { utf8_arg(path) }.unwrap_or_default();
    match disk.disk.entries(VolumeId::from_value(volume_id), path.as_ref()) {
        Ok(entries) => {
            let entries = entries.iter().map(FatEntryView::new).collect();
            Box::into_raw(Box::new(RemanenceFatEntryList { entries }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Answers one path in `volume_id` (U3): a one-entry listing when
/// something exists there, an empty listing when nothing does — a
/// missing leaf, a missing parent, or a parent that is a file alike.
/// Absence is an answer, distinguished from failure, which returns null
/// with the error set. Free with `remanence_fat_entry_list_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_stat(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFatEntryList {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match disk.disk.stat(VolumeId::from_value(volume_id), path.as_ref()) {
        Ok(entry) => {
            let entries = entry.iter().map(FatEntryView::new).collect();
            Box::into_raw(Box::new(RemanenceFatEntryList { entries }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a directory listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_fat_entry_list_free(list: *mut RemanenceFatEntryList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

unsafe fn fat_entry_view<'a>(
    list: *const RemanenceFatEntryList,
    index: usize,
) -> Option<&'a FatEntryView> {
    unsafe { list.as_ref() }?.entries.get(index)
}

/// Number of entries in the listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_fat_entry_count(list: *const RemanenceFatEntryList) -> usize {
    unsafe { list.as_ref() }.map_or(0, |list| list.entries.len())
}

/// An entry's 8.3 name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_fat_entry_name(
    list: *const RemanenceFatEntryList,
    index: usize,
) -> *const c_char {
    unsafe { fat_entry_view(list, index) }.map_or(ptr::null(), |entry| entry.name.as_ptr())
}

/// Whether an entry is a file or a directory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_fat_entry_kind(
    list: *const RemanenceFatEntryList,
    index: usize,
) -> RemanenceFatEntryKind {
    unsafe { fat_entry_view(list, index) }.map_or(RemanenceFatEntryKind::File, |entry| entry.kind)
}

/// An entry's size in bytes (0 for directories).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_fat_entry_size_bytes(
    list: *const RemanenceFatEntryList,
    index: usize,
) -> u64 {
    unsafe { fat_entry_view(list, index) }.map_or(0, |entry| entry.size_bytes)
}

/// Copies a file's bytes out of `volume_id`. Free with
/// `remanence_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_read_file(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFileData {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match disk.disk.read_file(VolumeId::from_value(volume_id), path.as_ref()) {
        Ok(bytes) => Box::into_raw(Box::new(RemanenceFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Reads part of a file into `buffer_out` — the streamed form beside
/// `remanence_disk_read_file`: exactly `length` bytes at `offset`,
/// which must lie within the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_read_file_at(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    offset: u64,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    match disk
        .disk
        .read_file_at(VolumeId::from_value(volume_id), path.as_ref(), offset, buffer)
    {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            false
        }
    }
}

/// Sets a file's size, creating it when absent — with
/// `remanence_disk_write_file_at`, the streamed replacement for
/// `remanence_disk_write_file`. Buffered until commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_resize_file(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    size: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    match disk.disk.resize_file(VolumeId::from_value(volume_id), path.as_ref(), size) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            false
        }
    }
}

/// Writes part of a file in place — the streamed form beside
/// `remanence_disk_write_file`: the span must lie within the file's
/// current size. Buffered until commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_write_file_at(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    offset: u64,
    bytes: *const u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    if bytes.is_null() && length != 0 {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    }
    let data = if length == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    match disk
        .disk
        .write_file_at(VolumeId::from_value(volume_id), path.as_ref(), offset, data)
    {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            false
        }
    }
}

/// The bytes of a read-out file; valid until the handle is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_data_bytes(
    data: *const RemanenceFileData,
    length_out: *mut usize,
) -> *const u8 {
    match unsafe { data.as_ref() } {
        Some(data) => {
            if !length_out.is_null() {
                unsafe { *length_out = data.bytes.len() };
            }
            data.bytes.as_ptr()
        }
        None => {
            if !length_out.is_null() {
                unsafe { *length_out = 0 };
            }
            ptr::null()
        }
    }
}

/// Frees read-out file bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_data_free(data: *mut RemanenceFileData) {
    if !data.is_null() {
        drop(unsafe { Box::from_raw(data) });
    }
}

/// Writes a file into `volume_id`. An existing file is overwritten —
/// shorter or longer, its old clusters released and reclaimed — while
/// an existing directory is refused. Buffered until `remanence_disk_commit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_write_file(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    bytes: *const u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    if bytes.is_null() && length > 0 {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    }
    let contents = if length == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    match disk
        .disk
        .write_file(VolumeId::from_value(volume_id), path.as_ref(), contents)
    {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            false
        }
    }
}

/// Ensures a directory exists in `volume_id`: missing parents are
/// created, and a path that already leads to a directory succeeds
/// unchanged. Buffered until commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_make_directory(
    disk: *mut RemanenceDisk,
    volume_id: u64,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    match disk.disk.make_directory(VolumeId::from_value(volume_id), path.as_ref()) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            false
        }
    }
}

/// The commit point (P2): everything buffered reaches the image, then a
/// flush. Until this call, nothing has touched the file. The commit is
/// durable (P9): a private recovery journal is armed before the first
/// byte of the file changes, so an interruption at any point leaves
/// state the next open reconciles to wholly the old image or wholly
/// the committed new one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_commit(
    disk: *mut RemanenceDisk,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    match disk.disk.commit() {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            false
        }
    }
}

/// Discards everything buffered; the image is untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_rollback(disk: *mut RemanenceDisk) {
    if let Some(disk) = unsafe { disk.as_mut() } {
        disk.disk.rollback();
    }
}

/// Which P7 mode the session's open obtained on its source file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_access_mode(session: *const RemanenceSession) -> RemanenceAccessMode {
    unsafe { session.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |session| {
        access_mode(session.session.access_mode())
    })
}

/// Reads a cataloged HDOS file's contents out of raw image bytes. Free
/// with `remanence_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_read_hdos_file(
    bytes: *const u8,
    length: usize,
    name: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFileData {
    unsafe { clear_error(error_out) };
    if bytes.is_null() {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }
    let Some(name) = (unsafe { utf8_arg(name) }) else {
        let error = remanence::Error::io("null name");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let image = unsafe { std::slice::from_raw_parts(bytes, length) };
    match read_hdos_file(image, name.as_ref()) {
        Ok(bytes) => Box::into_raw(Box::new(RemanenceFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Reads a cataloged HDOS file out of a session's image. Free with
/// `remanence_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_read_hdos_file(
    session: *const RemanenceSession,
    name: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFileData {
    unsafe { clear_error(error_out) };
    let Some(session) = (unsafe { session.as_ref() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let Some(name) = (unsafe { utf8_arg(name) }) else {
        let error = remanence::Error::io("null name");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match session.session.read_hdos_file(name.as_ref()) {
        Ok(bytes) => Box::into_raw(Box::new(RemanenceFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// The archive catalog: list what a supported archive holds — ZIP and 7z —
// under the P7 claim the listing holds, reading the archive's index and
// never its entry data.

use remanence::{Archive, ArchiveEntry};

struct ArchiveEntryView {
    name: CString,
    is_dir: bool,
    compressed_size: Option<u64>,
    uncompressed_size: u64,
}

impl ArchiveEntryView {
    fn new(entry: &ArchiveEntry) -> Self {
        Self {
            name: to_cstring(&entry.name),
            is_dir: entry.is_dir,
            compressed_size: entry.compressed_size,
            uncompressed_size: entry.uncompressed_size,
        }
    }
}

/// An open archive listing, holding the claim on its file.
pub struct RemanenceArchive {
    archive: Archive,
    path: CString,
    format_id: CString,
    format_name: CString,
    entries: Vec<ArchiveEntryView>,
}

unsafe fn archive_entry_view<'a>(
    archive: *const RemanenceArchive,
    index: usize,
) -> Option<&'a ArchiveEntryView> {
    let archive = unsafe { archive.as_ref() }?;
    archive.entries.get(index)
}

/// Opens the archive at `path` (UTF-8) and reads its entry list. A path
/// naming no archive format this library reads is refused by name.
/// Returns null on failure and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_open(
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceArchive {
    unsafe { clear_error(error_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }

    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match Archive::open(path.as_ref()) {
        Ok(archive) => {
            let entries = archive.entries().iter().map(ArchiveEntryView::new).collect();
            let path = to_cstring(&archive.path().display().to_string());
            let format_id = to_cstring(archive.format_id());
            let format_name = to_cstring(archive.format_name());
            Box::into_raw(Box::new(RemanenceArchive {
                archive,
                path,
                format_id,
                format_name,
                entries,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees an archive handle, releasing its claim on the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_free(archive: *mut RemanenceArchive) {
    if !archive.is_null() {
        drop(unsafe { Box::from_raw(archive) });
    }
}

/// The path the archive was opened from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_path(
    archive: *const RemanenceArchive,
) -> *const c_char {
    unsafe { archive.as_ref() }.map_or(ptr::null(), |archive| archive.path.as_ptr())
}

/// The archive format's stable identifier, e.g. "zip" or "7z".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_format_id(
    archive: *const RemanenceArchive,
) -> *const c_char {
    unsafe { archive.as_ref() }.map_or(ptr::null(), |archive| archive.format_id.as_ptr())
}

/// The archive format's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_format_name(
    archive: *const RemanenceArchive,
) -> *const c_char {
    unsafe { archive.as_ref() }.map_or(ptr::null(), |archive| archive.format_name.as_ptr())
}

/// Which P7 mode the open obtained on the archive file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_access_mode(
    archive: *const RemanenceArchive,
) -> RemanenceAccessMode {
    unsafe { archive.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |archive| {
        access_mode(archive.archive.access_mode())
    })
}

/// The archive file's own size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_size_bytes(archive: *const RemanenceArchive) -> u64 {
    unsafe { archive.as_ref() }.map_or(0, |archive| archive.archive.size_bytes())
}

/// Number of entries the archive holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_entry_count(archive: *const RemanenceArchive) -> usize {
    unsafe { archive.as_ref() }.map_or(0, |archive| archive.entries.len())
}

/// One entry's `/`-separated path inside the archive, or null when out
/// of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_entry_name(
    archive: *const RemanenceArchive,
    index: usize,
) -> *const c_char {
    unsafe { archive_entry_view(archive, index) }.map_or(ptr::null(), |entry| entry.name.as_ptr())
}

/// Whether the entry is a directory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_entry_is_dir(
    archive: *const RemanenceArchive,
    index: usize,
) -> bool {
    unsafe { archive_entry_view(archive, index) }.is_some_and(|entry| entry.is_dir)
}

/// The entry's size once decoded, as the archive declares it; 0 when out
/// of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_entry_uncompressed_size(
    archive: *const RemanenceArchive,
    index: usize,
) -> u64 {
    unsafe { archive_entry_view(archive, index) }.map_or(0, |entry| entry.uncompressed_size)
}

/// The entry's packed size; returns false when the grammar attributes
/// none to a single entry — a member of a solid 7z folder — or when the
/// index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_archive_entry_compressed_size(
    archive: *const RemanenceArchive,
    index: usize,
    out: *mut u64,
) -> bool {
    let Some(entry) = (unsafe { archive_entry_view(archive, index) }) else {
        return false;
    };
    unsafe { write_opt_u64(entry.compressed_size, out) }
}

// ---------------------------------------------------------------------------
// The KryoFlux capture set: one disk spread over a stream per head per
// drive-step position, recognized from a catalog subtree and reported as
// the adapter recognized it. Counts, identities and shapes cross this
// surface; the pulses stay behind it.

use remanence::CaptureSet;

struct ObservationView {
    ordinal: u64,
    span_ticks: u64,
    transitions: u64,
    markers: u64,
}

struct CaptureRunView {
    ordinal: u64,
    transitions: u64,
    extent_ticks: u64,
    markers: u64,
    index_markers: u64,
    transfer_result: Option<u32>,
    transitions_before_first_index: u64,
    transitions_after_last_index: u64,
    observations: Vec<ObservationView>,
}

struct CaptureIssueView {
    code: CString,
    detail: CString,
}

struct CaptureMemberView {
    entry_name: CString,
    entry_bytes: u64,
    position_numerator: u64,
    position_denominator: u64,
    head: Option<u64>,
    runs: Vec<CaptureRunView>,
    issues: Vec<CaptureIssueView>,
}

/// An open capture set, holding the claim on its archive.
pub struct RemanenceCaptureSet {
    set: CaptureSet,
    path: CString,
    subtree: Option<CString>,
    format_id: CString,
    format_name: CString,
    archive_format_id: CString,
    evidence: Vec<CString>,
    members: Vec<CaptureMemberView>,
}

impl RemanenceCaptureSet {
    fn new(set: CaptureSet) -> Self {
        let report = set.inspect();
        let members = report
            .members
            .iter()
            .map(|member| CaptureMemberView {
                entry_name: to_cstring(&member.entry_name),
                entry_bytes: member.entry_bytes,
                position_numerator: member.position.numerator,
                position_denominator: member.position.denominator,
                head: member.head,
                runs: member
                    .runs
                    .iter()
                    .map(|run| CaptureRunView {
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
                            .map(|observation| ObservationView {
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
                    .map(|issue| CaptureIssueView {
                        code: to_cstring(&issue.code),
                        detail: to_cstring(&issue.detail),
                    })
                    .collect(),
            })
            .collect();
        let evidence = report.evidence.iter().map(|line| to_cstring(line)).collect();
        let path = to_cstring(&set.path().display().to_string());
        let subtree = set.subtree().map(to_cstring);
        let format_id = to_cstring(set.format_id());
        let format_name = to_cstring(set.format_name());
        let archive_format_id = to_cstring(set.archive_format_id());
        Self {
            set,
            path,
            subtree,
            format_id,
            format_name,
            archive_format_id,
            evidence,
            members,
        }
    }
}

unsafe fn capture_member<'a>(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> Option<&'a CaptureMemberView> {
    unsafe { set.as_ref() }?.members.get(member)
}

unsafe fn capture_run<'a>(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> Option<&'a CaptureRunView> {
    unsafe { capture_member(set, member) }?.runs.get(run)
}

unsafe fn capture_observation<'a>(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> Option<&'a ObservationView> {
    unsafe { capture_run(set, member, run) }?
        .observations
        .get(observation)
}

unsafe fn open_capture_set(
    path: *const c_char,
    cache_bytes: Option<u64>,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceCaptureSet {
    unsafe { clear_error(error_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    let opened = match cache_bytes {
        Some(cache_bytes) => CaptureSet::open_with_cache(path.as_ref(), cache_bytes),
        None => CaptureSet::open(path.as_ref()),
    };
    match opened {
        Ok(set) => Box::into_raw(Box::new(RemanenceCaptureSet::new(set))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Opens the KryoFlux capture set held by `path` (UTF-8) — an archive
/// this library reads, optionally followed by the subtree inside it that
/// holds the members — with the stated default session cache bound.
/// An incomplete, duplicate, contradictory, or unrelated member refuses
/// the whole set. Returns null on failure and stores a message in
/// `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_open(
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceCaptureSet {
    unsafe { open_capture_set(path, None, error_category_out, error_out) }
}

/// Opens a capture set as `remanence_capture_set_open` does, under a
/// declared cache bound: at most `cache_bytes` of the decoded capture
/// stays resident. The bound narrows the working set; it never refuses
/// service.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_open_with_cache(
    path: *const c_char,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceCaptureSet {
    unsafe { open_capture_set(path, Some(cache_bytes), error_category_out, error_out) }
}

/// Frees a capture-set handle, releasing its claim on the archive and
/// discarding the private session storage the capture decoded into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_free(set: *mut RemanenceCaptureSet) {
    if !set.is_null() {
        drop(unsafe { Box::from_raw(set) });
    }
}

/// The path the set was opened from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_path(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.path.as_ptr())
}

/// The subtree inside the archive the members were read from, or null
/// when the whole archive is the set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_subtree(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| {
        set.subtree
            .as_ref()
            .map_or(ptr::null(), |subtree| subtree.as_ptr())
    })
}

/// The capture format's stable identifier, "kryoflux".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_format_id(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.format_id.as_ptr())
}

/// The capture format's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_format_name(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.format_name.as_ptr())
}

/// The archive grammar the members were read through, e.g. "7z".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_archive_format_id(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.archive_format_id.as_ptr())
}

/// Which P7 mode the open obtained on the archive file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_access_mode(
    set: *const RemanenceCaptureSet,
) -> RemanenceAccessMode {
    unsafe { set.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |set| {
        access_mode(set.set.access_mode())
    })
}

/// The capture's declared timing basis, as an exact ratio of ticks per
/// second. Returns false when the handle is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_ticks_per_second(
    set: *const RemanenceCaptureSet,
    numerator_out: *mut u64,
    denominator_out: *mut u64,
) -> bool {
    let Some(set) = (unsafe { set.as_ref() }) else {
        return false;
    };
    let base = set.set.inspect().time_base;
    if !numerator_out.is_null() {
        unsafe { *numerator_out = base.ticks_per_second_numerator };
    }
    if !denominator_out.is_null() {
        unsafe { *denominator_out = base.ticks_per_second_denominator };
    }
    true
}

/// How many bytes of private session storage the decoded capture
/// occupies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_backing_bytes(
    set: *const RemanenceCaptureSet,
) -> u64 {
    unsafe { set.as_ref() }.map_or(0, |set| set.set.backing_bytes())
}

/// How much of that backing is currently resident. The capture is never
/// held whole.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_resident_bytes(
    set: *const RemanenceCaptureSet,
) -> u64 {
    unsafe { set.as_ref() }.map_or(0, |set| set.set.resident_bytes())
}

/// Number of evidence lines behind the recognition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_evidence_count(
    set: *const RemanenceCaptureSet,
) -> usize {
    unsafe { set.as_ref() }.map_or(0, |set| set.evidence.len())
}

/// One evidence line, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_evidence(
    set: *const RemanenceCaptureSet,
    index: usize,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| {
        set.evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// Number of members the set holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_count(
    set: *const RemanenceCaptureSet,
) -> usize {
    unsafe { set.as_ref() }.map_or(0, |set| set.members.len())
}

/// One member's catalog identity, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_entry_name(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> *const c_char {
    unsafe { capture_member(set, member) }.map_or(ptr::null(), |member| member.entry_name.as_ptr())
}

/// One member's size in bytes as the catalog declares it; 0 when out of
/// range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_entry_bytes(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> u64 {
    unsafe { capture_member(set, member) }.map_or(0, |member| member.entry_bytes)
}

/// One member's drive-step position, as an exact ratio. Returns false
/// when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_position(
    set: *const RemanenceCaptureSet,
    member: usize,
    numerator_out: *mut u64,
    denominator_out: *mut u64,
) -> bool {
    let Some(member) = (unsafe { capture_member(set, member) }) else {
        return false;
    };
    if !numerator_out.is_null() {
        unsafe { *numerator_out = member.position_numerator };
    }
    if !denominator_out.is_null() {
        unsafe { *denominator_out = member.position_denominator };
    }
    true
}

/// The head that captured this position; returns false when the source
/// numbers no head, which is a different fact from head zero, or when
/// the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_head(
    set: *const RemanenceCaptureSet,
    member: usize,
    out: *mut u64,
) -> bool {
    let Some(member) = (unsafe { capture_member(set, member) }) else {
        return false;
    };
    unsafe { write_opt_u64(member.head, out) }
}

/// Number of things recorded as qualified about this member.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_issue_count(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> usize {
    unsafe { capture_member(set, member) }.map_or(0, |member| member.issues.len())
}

/// One issue's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_issue_code(
    set: *const RemanenceCaptureSet,
    member: usize,
    issue: usize,
) -> *const c_char {
    unsafe { capture_member(set, member) }.map_or(ptr::null(), |member| {
        member
            .issues
            .get(issue)
            .map_or(ptr::null(), |issue| issue.code.as_ptr())
    })
}

/// One issue's human-readable detail, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_issue_detail(
    set: *const RemanenceCaptureSet,
    member: usize,
    issue: usize,
) -> *const c_char {
    unsafe { capture_member(set, member) }.map_or(ptr::null(), |member| {
        member
            .issues
            .get(issue)
            .map_or(ptr::null(), |issue| issue.detail.as_ptr())
    })
}

/// Number of source transfers recorded at this member's location.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_run_count(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> usize {
    unsafe { capture_member(set, member) }.map_or(0, |member| member.runs.len())
}

/// One run's place in the member's recorded order; 0 when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_ordinal(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.ordinal)
}

/// How many flux transitions the run recorded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transitions(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.transitions)
}

/// The last transition's tick: the extent of what was recorded, not a
/// circumference. A run states no period.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_extent_ticks(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.extent_ticks)
}

/// How many timed markers sit on channels parallel to the run's flux.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_markers(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.markers)
}

/// How many of those markers are index events.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_index_markers(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.index_markers)
}

/// The result the capture tool declared for this transfer, where it
/// declared one; zero is a clean read. Returns false when it declared
/// none or the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transfer_result(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    out: *mut u32,
) -> bool {
    let Some(run) = (unsafe { capture_run(set, member, run) }) else {
        return false;
    };
    match run.transfer_result {
        Some(result) => {
            if !out.is_null() {
                unsafe { *out = result };
            }
            true
        }
        None => false,
    }
}

/// Transitions recorded before the run's first index: evidence that
/// bounding into circular observations does not consume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transitions_before_first_index(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.transitions_before_first_index)
}

/// Transitions recorded after the run's last index, on the same terms.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transitions_after_last_index(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.transitions_after_last_index)
}

/// How many circular observations the run's indices bounded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_observation_count(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> usize {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.observations.len())
}

/// One observation's place in the location's source-record order. Not a
/// rank: nothing here says it is a good or complete revolution.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_ordinal(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.ordinal)
}

/// The observation's declared circumference, in the capture's own ticks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_span_ticks(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.span_ticks)
}

/// How many transitions the observation holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_transitions(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.transitions)
}

/// How many markers the observation holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_markers(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.markers)
}

// ---------------------------------------------------------------------------
// Drive-profile recognition: which family's conventions a capture was
// recorded under, ranked, with the observations that produced the
// verdict. A count, a density, an angle and an absence cross this
// surface; nothing that was decoded, because nothing was.

use remanence::Recognition;

/// One zone as a profile declares it, and what the capture recovered of
/// it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceZoneClaim {
    pub first_location: u64,
    pub last_location: u64,
    /// What the family claims one location in this zone holds.
    pub records_declared: u32,
    pub locations_declared: u64,
    pub locations_claimed: u64,
    /// The cell this zone claims, in thousandths of a reference cycle.
    pub nominal_cell_millicycles: u64,
}

/// What the probe found at one source position. Every `has_*` flag says
/// whether the value beside it was established at all: an absence is a
/// finding, not a zero.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceLocationVerdict {
    pub position_numerator: u64,
    pub position_denominator: u64,
    pub has_head: bool,
    pub head: u64,
    /// The family location this position addresses, where the family's
    /// addressing covers it at all.
    pub has_family_location: bool,
    pub family_location: u64,
    pub has_zone: bool,
    pub zone: u32,
    pub records: u32,
    /// The bit distance between record starts, where it repeats.
    pub has_record_bits: bool,
    pub record_bits: u64,
    /// How far that spacing departs from its own median. Zero is a
    /// spacing that repeats to the bit.
    pub record_bits_deviation: u64,
    /// The one departure from it, as an angle in reference-clock cycles.
    pub has_seam: bool,
    pub seam_cycles: u64,
    /// The derived cell projected onto the family's nominal rotation,
    /// in thousandths of a reference cycle, beside what the zone claims.
    pub has_cell: bool,
    pub cell_millicycles: u64,
    pub has_nominal_cell: bool,
    pub nominal_cell_millicycles: u64,
    /// How much of the interval population classified, per thousand.
    pub resolved_permille: u32,
    pub observations: u32,
    pub observations_agreeing: u32,
    /// The adjacent position holding the same content, where one does.
    pub has_duplicate: bool,
    pub duplicate_numerator: u64,
    pub duplicate_denominator: u64,
    pub claimed: bool,
}

struct VerdictView {
    profile_id: CString,
    profile_name: CString,
    evidence: Vec<CString>,
    artifacts: Vec<CString>,
    refusals: Vec<Option<CString>>,
}

/// A recognition result, ranked highest confidence first.
pub struct RemanenceRecognition {
    recognition: Recognition,
    pinned: Option<CString>,
    evidence: Vec<CString>,
    verdicts: Vec<VerdictView>,
}

impl RemanenceRecognition {
    fn new(recognition: Recognition) -> Self {
        let pinned = recognition.pinned.as_deref().map(to_cstring);
        let evidence = recognition.evidence.iter().map(|line| to_cstring(line)).collect();
        let verdicts = recognition
            .verdicts
            .iter()
            .map(|verdict| VerdictView {
                profile_id: to_cstring(&verdict.profile_id),
                profile_name: to_cstring(&verdict.profile_name),
                evidence: verdict.evidence.iter().map(|line| to_cstring(line)).collect(),
                artifacts: verdict
                    .locations
                    .iter()
                    .map(|location| to_cstring(&location.artifact))
                    .collect(),
                refusals: verdict
                    .locations
                    .iter()
                    .map(|location| location.refusal.as_deref().map(to_cstring))
                    .collect(),
                            })
            .collect();
        Self {
            recognition,
            pinned,
            evidence,
            verdicts,
        }
    }
}

unsafe fn recognition_verdict<'a>(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> Option<(&'a remanence::ProfileVerdict, &'a VerdictView)> {
    let recognition = unsafe { recognition.as_ref() }?;
    Some((
        recognition.recognition.verdicts.get(verdict)?,
        recognition.verdicts.get(verdict)?,
    ))
}

/// Recognizes the drive family a capture set was recorded under. Every
/// enrolled profile is consulted and what claims the capture is ranked;
/// a capture no profile claims is a named refusal. Returns null on
/// failure and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_recognize(
    set: *const RemanenceCaptureSet,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceRecognition {
    unsafe { clear_error(error_out) };
    let Some(set) = (unsafe { set.as_ref() }) else {
        let error = remanence::Error::io("null capture set");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match set.set.recognize() {
        Ok(recognition) => Box::into_raw(Box::new(RemanenceRecognition::new(recognition))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Recognizes the capture against one named profile, whether or not it
/// would have won the ranking. A profile this build does not enroll is
/// refused by name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_recognize_as(
    set: *const RemanenceCaptureSet,
    profile_id: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceRecognition {
    unsafe { clear_error(error_out) };
    let (Some(set), false) = (unsafe { set.as_ref() }, profile_id.is_null()) else {
        let error = remanence::Error::io("null capture set or profile id");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let id = String::from_utf8_lossy(unsafe { CStr::from_ptr(profile_id) }.to_bytes());
    match set.set.recognize_as(id.as_ref()) {
        Ok(recognition) => Box::into_raw(Box::new(RemanenceRecognition::new(recognition))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a recognition handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_free(recognition: *mut RemanenceRecognition) {
    if !recognition.is_null() {
        drop(unsafe { Box::from_raw(recognition) });
    }
}

/// The profile the caller pinned, or null when the ranking was open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_pinned(
    recognition: *const RemanenceRecognition,
) -> *const c_char {
    unsafe { recognition.as_ref() }.map_or(ptr::null(), |recognition| {
        recognition
            .pinned
            .as_ref()
            .map_or(ptr::null(), |pinned| pinned.as_ptr())
    })
}

/// Number of evidence lines about the recognition itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_evidence_count(
    recognition: *const RemanenceRecognition,
) -> usize {
    unsafe { recognition.as_ref() }.map_or(0, |recognition| recognition.evidence.len())
}

/// One of those lines, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_evidence(
    recognition: *const RemanenceRecognition,
    index: usize,
) -> *const c_char {
    unsafe { recognition.as_ref() }.map_or(ptr::null(), |recognition| {
        recognition
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many profiles claimed the capture, highest confidence first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_verdict_count(
    recognition: *const RemanenceRecognition,
) -> usize {
    unsafe { recognition.as_ref() }.map_or(0, |recognition| recognition.verdicts.len())
}

/// One verdict's profile identifier, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_profile_id(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(ptr::null(), |(_, view)| view.profile_id.as_ptr())
}

/// One verdict's human-readable family name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_profile_name(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(ptr::null(), |(_, view)| view.profile_name.as_ptr())
}

/// Detection confidence, 0-100. Never an answer on its own: read the
/// evidence beside it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_confidence(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> u8 {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.confidence)
}

/// How many of the profile's declared locations the capture claimed,
/// and how many it declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_locations_claimed(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> u32 {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.locations_claimed)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_locations_declared(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> u64 {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.locations_declared)
}

/// Number of evidence lines behind this verdict.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_verdict_evidence_count(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> usize {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(_, view)| view.evidence.len())
}

/// One of those lines, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_verdict_evidence(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    index: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }.map_or(ptr::null(), |(_, view)| {
        view.evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many density zones the profile declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_zone_count(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> usize {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.zones.len())
}

/// One zone, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_zone(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    zone: usize,
    out: *mut RemanenceZoneClaim,
) -> bool {
    let Some((verdict, _)) = (unsafe { recognition_verdict(recognition, verdict) }) else {
        return false;
    };
    let Some(claim) = verdict.zones.get(zone) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceZoneClaim {
                first_location: claim.first_location,
                last_location: claim.last_location,
                records_declared: claim.records_declared,
                locations_declared: claim.locations_declared,
                locations_claimed: claim.locations_claimed,
                nominal_cell_millicycles: claim.nominal_cell_millicycles,
            };
        }
    }
    true
}

/// How many source positions the probe accounted for.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location_count(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> usize {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.locations.len())
}

/// One position's findings, written into `out`. Returns false when out
/// of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    location: usize,
    out: *mut RemanenceLocationVerdict,
) -> bool {
    let Some((verdict, _)) = (unsafe { recognition_verdict(recognition, verdict) }) else {
        return false;
    };
    let Some(found) = verdict.locations.get(location) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceLocationVerdict {
                position_numerator: found.position.numerator,
                position_denominator: found.position.denominator,
                has_head: found.head.is_some(),
                head: found.head.unwrap_or(0),
                has_family_location: found.family_location.is_some(),
                family_location: found.family_location.unwrap_or(0),
                has_zone: found.zone.is_some(),
                zone: found.zone.unwrap_or(0),
                records: found.records,
                has_record_bits: found.record_bits.is_some(),
                record_bits: found.record_bits.unwrap_or(0),
                record_bits_deviation: found.record_bits_deviation,
                has_seam: found.seam_cycles.is_some(),
                seam_cycles: found.seam_cycles.unwrap_or(0),
                has_cell: found.cell_millicycles.is_some(),
                cell_millicycles: found.cell_millicycles.unwrap_or(0),
                has_nominal_cell: found.nominal_cell_millicycles.is_some(),
                nominal_cell_millicycles: found.nominal_cell_millicycles.unwrap_or(0),
                resolved_permille: found.resolved_permille,
                observations: found.observations,
                observations_agreeing: found.observations_agreeing,
                has_duplicate: found.duplicate_of.is_some(),
                duplicate_numerator: found.duplicate_of.map_or(0, |of| of.numerator),
                duplicate_denominator: found.duplicate_of.map_or(0, |of| of.denominator),
                claimed: found.claimed,
            };
        }
    }
    true
}

/// The member one position was read from, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location_artifact(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    location: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }.map_or(ptr::null(), |(_, view)| {
        view.artifacts
            .get(location)
            .map_or(ptr::null(), |artifact| artifact.as_ptr())
    })
}

/// Why a position was not claimed, in the profile's own terms; null when
/// it was claimed or the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location_refusal(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    location: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }.map_or(ptr::null(), |(_, view)| {
        view.refusals
            .get(location)
            .and_then(Option::as_ref)
            .map_or(ptr::null(), |refusal| refusal.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// C1541 mastering: reducing an opened capture to one half-track-addressed
// flux medium under a declared policy. Every reduction is a named input,
// the plan writes nothing, and the loss is declared before the medium
// exists.

use remanence::{MasteredMedium, MasteringPlan};

/// What to do with a location whose content its neighbour also holds.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDuplicatePolicy {
    /// Take the profile's declaration, which for a 1541 refuses.
    Declared = 0,
    AdmitAsObserved = 1,
    Omit = 2,
}

/// What a projection does with two transitions landing on one cycle.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceProjectionPolicy {
    Refuse = 0,
    DeclareLoss = 1,
}

/// How the selected evidence becomes pulse strength.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanencePulseStrengthPolicy {
    /// Every pulse carries `strength_state`; disagreement across the
    /// unselected observations is declared loss rather than expressed.
    Declared = 0,
    /// A pulse every observation places within `strength_window_cycles`
    /// is strong; one only some corroborate is weak.
    FromAgreement = 1,
}

/// Where the medium's circle begins.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceOriginPolicy {
    /// The track's own seam, which is what the profile declares.
    Declared = 0,
    /// The angle in `origin_cycles`, stated outright by the caller.
    Angle = 1,
}

/// The complete declared policy for one reduction. There is no default:
/// every field is a decision about evidence, and a reduction no input
/// names is a refusal.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceMasteringPolicy {
    /// The captured head supplying the family's one recorded surface.
    pub side: u64,
    /// Which observation of each location is used.
    pub observation_ordinal: u64,
    pub duplicate: RemanenceDuplicatePolicy,
    pub projection: RemanenceProjectionPolicy,
    pub pulse_strength: RemanencePulseStrengthPolicy,
    pub strength_state: u32,
    pub strength_window_cycles: u64,
    pub origin: RemanenceOriginPolicy,
    pub origin_cycles: u64,
    /// What makes any stochastic element reproducible.
    pub seed: u64,
}

/// One half-track the medium will hold.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceMasteredLocation {
    pub source_position_numerator: u64,
    pub source_position_denominator: u64,
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub observation_ordinal: u64,
    pub pulses: u64,
    pub strong_pulses: u64,
    pub weak_pulses: u64,
    pub origin_cycles: u64,
    pub has_seam: bool,
    pub seam_cycles: u64,
}

struct PlanView {
    profile_id: CString,
    origin_rule: CString,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
    evidence: Vec<CString>,
}

impl PlanView {
    fn new(report: &remanence::MasteringPlanReport) -> Self {
        Self {
            profile_id: to_cstring(&report.profile_id),
            origin_rule: to_cstring(&report.origin_rule),
            loss_codes: report.declared_loss.iter().map(|l| to_cstring(&l.code)).collect(),
            loss_details: report
                .declared_loss
                .iter()
                .map(|l| to_cstring(&l.detail))
                .collect(),
            evidence: report.evidence.iter().map(|line| to_cstring(line)).collect(),
        }
    }
}

/// A planned reduction: everything computed, nothing written.
pub struct RemanenceMasteringPlan {
    plan: Option<MasteringPlan>,
    report: remanence::MasteringPlanReport,
    view: PlanView,
}

/// A mastered medium, held in the session.
pub struct RemanenceMasteredMedium {
    medium: MasteredMedium,
    report: remanence::MasteringPlanReport,
    view: PlanView,
}

fn to_policy(policy: &RemanenceMasteringPolicy) -> remanence::MasteringPolicy {
    remanence::MasteringPolicy {
        side: policy.side,
        observation: remanence::ObservationPolicy::Selected {
            ordinal: policy.observation_ordinal,
        },
        duplicate: match policy.duplicate {
            RemanenceDuplicatePolicy::Declared => remanence::DuplicatePolicy::Declared,
            RemanenceDuplicatePolicy::AdmitAsObserved => {
                remanence::DuplicatePolicy::AdmitAsObserved
            }
            RemanenceDuplicatePolicy::Omit => remanence::DuplicatePolicy::Omit,
        },
        projection: match policy.projection {
            RemanenceProjectionPolicy::Refuse => remanence::ProjectionPolicy::Refuse,
            RemanenceProjectionPolicy::DeclareLoss => remanence::ProjectionPolicy::DeclareLoss,
        },
        pulse_strength: match policy.pulse_strength {
            RemanencePulseStrengthPolicy::Declared => remanence::PulseStrengthPolicy::Declared {
                state: policy.strength_state,
            },
            RemanencePulseStrengthPolicy::FromAgreement => {
                remanence::PulseStrengthPolicy::FromAgreement {
                    window_cycles: policy.strength_window_cycles,
                }
            }
        },
        origin: match policy.origin {
            RemanenceOriginPolicy::Declared => remanence::OriginPolicy::Declared,
            RemanenceOriginPolicy::Angle => remanence::OriginPolicy::Angle {
                cycles: policy.origin_cycles,
            },
        },
        seed: policy.seed,
    }
}

/// Plans the reduction of a capture set to one 1541 flux medium.
/// Nothing is written and nothing is mutated. Returns null on failure
/// and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_plan_c1541_mastering(
    set: *const RemanenceCaptureSet,
    policy: *const RemanenceMasteringPolicy,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceMasteringPlan {
    unsafe { clear_error(error_out) };
    let (Some(set), Some(policy)) = (unsafe { set.as_ref() }, unsafe { policy.as_ref() }) else {
        let error = remanence::Error::io("null capture set or policy");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match set.set.plan_c1541_mastering(to_policy(policy)) {
        Ok(plan) => {
            let report = plan.report().clone();
            let view = PlanView::new(&report);
            Box::into_raw(Box::new(RemanenceMasteringPlan {
                plan: Some(plan),
                report,
                view,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a plan handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_plan_free(plan: *mut RemanenceMasteringPlan) {
    if !plan.is_null() {
        drop(unsafe { Box::from_raw(plan) });
    }
}

/// Produces the medium the plan described, consuming the plan: the
/// handle is freed whether this succeeds or fails, and must not be used
/// again. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_plan_execute(
    plan: *mut RemanenceMasteringPlan,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceMasteredMedium {
    unsafe { clear_error(error_out) };
    if plan.is_null() {
        let error = remanence::Error::io("null plan");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }
    let owned = unsafe { Box::from_raw(plan) };
    let RemanenceMasteringPlan {
        plan: Some(plan),
        report,
        view,
    } = *owned
    else {
        let error = remanence::Error::io("plan has already been executed");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match plan.execute(cache_bytes) {
        Ok(medium) => Box::into_raw(Box::new(RemanenceMasteredMedium {
            medium,
            report,
            view,
        })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a mastered-medium handle, discarding its private session
/// storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_free(medium: *mut RemanenceMasteredMedium) {
    if !medium.is_null() {
        drop(unsafe { Box::from_raw(medium) });
    }
}

/// How many locations the medium claims.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_locations(
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { medium.as_ref() }.map_or(0, |medium| medium.medium.locations())
}

/// How many bytes of private session storage the medium occupies, and
/// how much of that is currently resident.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_backing_bytes(
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { medium.as_ref() }.map_or(0, |medium| medium.medium.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_resident_bytes(
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { medium.as_ref() }.map_or(0, |medium| medium.medium.resident_bytes())
}

/// A plan and the medium produced from it report the same thing:
/// executing adds nothing to the account. So the accessors below take
/// either handle through one small indirection rather than being
/// written out twice.
enum ReportedPlan<'a> {
    Planned(&'a RemanenceMasteringPlan),
    Mastered(&'a RemanenceMasteredMedium),
}

impl ReportedPlan<'_> {
    fn report(&self) -> &remanence::MasteringPlanReport {
        match self {
            Self::Planned(plan) => &plan.report,
            Self::Mastered(medium) => &medium.report,
        }
    }

    fn view(&self) -> &PlanView {
        match self {
            Self::Planned(plan) => &plan.view,
            Self::Mastered(medium) => &medium.view,
        }
    }
}

unsafe fn reported<'a>(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> Option<ReportedPlan<'a>> {
    if let Some(plan) = unsafe { plan.as_ref() } {
        return Some(ReportedPlan::Planned(plan));
    }
    unsafe { medium.as_ref() }.map(ReportedPlan::Mastered)
}

/// The profile the reduction was declared by. Pass whichever handle you
/// hold and null for the other.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_profile_id(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> *const c_char {
    unsafe { reported(plan, medium) }
        .map_or(ptr::null(), |reported| reported.view().profile_id.as_ptr())
}

/// The frame the medium is expressed in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_reference_clock_hz(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().reference_clock_hz)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_cycles_per_rotation(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().cycles_per_rotation)
}

/// Which rule placed the circle's origin.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_origin_rule(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> *const c_char {
    unsafe { reported(plan, medium) }
        .map_or(ptr::null(), |reported| reported.view().origin_rule.as_ptr())
}

/// How many half-tracks the reduction produces.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_location_count(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> usize {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().locations.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_location(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
    out: *mut RemanenceMasteredLocation,
) -> bool {
    let Some(reported) = (unsafe { reported(plan, medium) }) else {
        return false;
    };
    let Some(location) = reported.report().locations.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceMasteredLocation {
                source_position_numerator: location.source_position.numerator,
                source_position_denominator: location.source_position.denominator,
                half_track_numerator: location.half_track_numerator,
                half_track_denominator: location.half_track_denominator,
                observation_ordinal: location.observation_ordinal,
                pulses: location.pulses,
                strong_pulses: location.strong_pulses,
                weak_pulses: location.weak_pulses,
                origin_cycles: location.origin_cycles,
                has_seam: location.seam_cycles.is_some(),
                seam_cycles: location.seam_cycles.unwrap_or(0),
            };
        }
    }
    true
}

/// How many kinds of loss the destination will not carry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_count(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> usize {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_code(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> *const c_char {
    unsafe { reported(plan, medium) }.map_or(ptr::null(), |reported| {
        reported
            .view()
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the source's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_detail(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> *const c_char {
    unsafe { reported(plan, medium) }.map_or(ptr::null(), |reported| {
        reported
            .view()
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_amount(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> u64 {
    unsafe { reported(plan, medium) }.map_or(0, |reported| {
        reported
            .report()
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// The policy that produced the plan, stated in full.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_evidence_count(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> usize {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.view().evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_evidence(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> *const c_char {
    unsafe { reported(plan, medium) }.map_or(ptr::null(), |reported| {
        reported
            .view()
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// The P64 image-format adapter: one container claimed in both
// directions. Reading it decodes a medium at rest; writing it produces a
// new artifact from a mastered one, under a claim stated before the file
// exists.

use remanence::{P64Image, P64Report};

/// One half-track a P64 holds, in the container's addressing and the
/// family's both.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceP64HalfTrack {
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

struct P64View {
    format_id: CString,
    format_name: CString,
    profile_id: CString,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
    evidence: Vec<CString>,
}

impl P64View {
    fn new(report: &P64Report) -> Self {
        Self {
            format_id: to_cstring(&report.format_id),
            format_name: to_cstring(&report.format_name),
            profile_id: to_cstring(&report.profile_id),
            loss_codes: report
                .declared_loss
                .iter()
                .map(|loss| to_cstring(&loss.code))
                .collect(),
            loss_details: report
                .declared_loss
                .iter()
                .map(|loss| to_cstring(&loss.detail))
                .collect(),
            evidence: report.evidence.iter().map(|line| to_cstring(line)).collect(),
        }
    }
}

/// An opened P64 image, holding its claim on the file and the medium it
/// decoded into private session storage.
pub struct RemanenceP64Image {
    image: P64Image,
    path: CString,
    report: P64Report,
    view: P64View,
}

/// What a container carried, or will carry, of one mastered medium.
pub struct RemanenceP64Report {
    report: P64Report,
    view: P64View,
}

unsafe fn open_p64(
    path: *const c_char,
    cache_bytes: Option<u64>,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceP64Image {
    unsafe { clear_error(error_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    }
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    let opened = match cache_bytes {
        Some(cache_bytes) => P64Image::open_with_cache(path.as_ref(), cache_bytes),
        None => P64Image::open(path.as_ref()),
    };
    match opened {
        Ok(image) => {
            let report = image.inspect().clone();
            let view = P64View::new(&report);
            Box::into_raw(Box::new(RemanenceP64Image {
                path: to_cstring(&image.path().to_string_lossy()),
                image,
                report,
                view,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Opens the P64 image at `path` (UTF-8), claiming the file and decoding
/// every half-track once into private session storage. The version is
/// checked before anything else is touched, and a version, flag bit, or
/// chunk signature past this release's claim is refused by name. Returns
/// null on failure and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_open(
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceP64Image {
    unsafe { open_p64(path, None, error_category_out, error_out) }
}

/// Opens a P64 image as `remanence_p64_image_open` does, under a
/// declared cache bound: at most `cache_bytes` of the decoded medium
/// stays resident. The bound narrows the working set; it never refuses
/// service.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_open_with_cache(
    path: *const c_char,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceP64Image {
    unsafe { open_p64(path, Some(cache_bytes), error_category_out, error_out) }
}

/// Frees an image handle, releasing its claim on the file and discarding
/// the private session storage the medium decoded into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_free(image: *mut RemanenceP64Image) {
    if !image.is_null() {
        drop(unsafe { Box::from_raw(image) });
    }
}

/// The path the image was opened from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_path(
    image: *const RemanenceP64Image,
) -> *const c_char {
    unsafe { image.as_ref() }.map_or(ptr::null(), |image| image.path.as_ptr())
}

/// Which P7 mode the open obtained on the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_access_mode(
    image: *const RemanenceP64Image,
) -> RemanenceAccessMode {
    unsafe { image.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |image| {
        access_mode(image.image.access_mode())
    })
}

/// How many bytes of private session storage the decoded medium
/// occupies, and how much of that is currently resident.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_backing_bytes(
    image: *const RemanenceP64Image,
) -> u64 {
    unsafe { image.as_ref() }.map_or(0, |image| image.image.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_resident_bytes(
    image: *const RemanenceP64Image,
) -> u64 {
    unsafe { image.as_ref() }.map_or(0, |image| image.image.resident_bytes())
}

/// Computes what a P64 will and will not carry of a mastered medium,
/// writing nothing. Read it before writing: the write adds nothing to
/// the account. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_describe_p64(
    medium: *const RemanenceMasteredMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceP64Report {
    unsafe { clear_error(error_out) };
    let Some(medium) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null mastered medium");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match medium.medium.describe_p64() {
        Ok(report) => {
            let view = P64View::new(&report);
            Box::into_raw(Box::new(RemanenceP64Report { report, view }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Writes a mastered medium into a new P64 image at `path` (UTF-8) and
/// reports what the container carried. The medium is untouched, an
/// existing destination is a named refusal rather than an overwrite, and
/// an interruption leaves the destination absent rather than half an
/// artifact. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_write_p64(
    medium: *const RemanenceMasteredMedium,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceP64Report {
    unsafe { clear_error(error_out) };
    let (Some(medium), false) = (unsafe { medium.as_ref() }, path.is_null()) else {
        let error = remanence::Error::io("null mastered medium or path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match medium.medium.write_p64(path.as_ref()) {
        Ok(report) => {
            let view = P64View::new(&report);
            Box::into_raw(Box::new(RemanenceP64Report { report, view }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a report handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_report_free(report: *mut RemanenceP64Report) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

/// An opened image and a written artifact report the same thing, so the
/// accessors below take either handle: pass whichever you hold and null
/// for the other.
unsafe fn p64_reported<'a>(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> Option<(&'a P64Report, &'a P64View)> {
    if let Some(image) = unsafe { image.as_ref() } {
        return Some((&image.report, &image.view));
    }
    unsafe { report.as_ref() }.map(|report| (&report.report, &report.view))
}

/// The container format's stable identifier, "p64".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_format_id(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(image, report) }
        .map_or(ptr::null(), |(_, view)| view.format_id.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_format_name(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(image, report) }
        .map_or(ptr::null(), |(_, view)| view.format_name.as_ptr())
}

/// The container's declared format version.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_version(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> u32 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.version)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_write_protected(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> bool {
    unsafe { p64_reported(image, report) }.is_some_and(|(report, _)| report.write_protected)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_double_sided(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> bool {
    unsafe { p64_reported(image, report) }.is_some_and(|(report, _)| report.double_sided)
}

/// The drive profile the container's own signature names, and the frame
/// that profile declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_profile_id(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(image, report) }
        .map_or(ptr::null(), |(_, view)| view.profile_id.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_reference_clock_hz(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> u64 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.reference_clock_hz)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_cycles_per_rotation(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> u64 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.cycles_per_rotation)
}

/// How many half-tracks the container holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_half_track_count(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.half_tracks.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_half_track(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
    out: *mut RemanenceP64HalfTrack,
) -> bool {
    let Some((report, _)) = (unsafe { p64_reported(image, report) }) else {
        return false;
    };
    let Some(track) = report.half_tracks.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceP64HalfTrack {
                index: track.index,
                side: track.side,
                half_track_numerator: track.half_track_numerator,
                half_track_denominator: track.half_track_denominator,
                pulses: track.pulses,
                strong_pulses: track.strong_pulses,
                weak_pulses: track.weak_pulses,
                absent_pulses: track.absent_pulses,
            };
        }
    }
    true
}

/// How many kinds of loss the crossing does not carry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_count(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_code(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(image, report) }.map_or(ptr::null(), |(_, view)| {
        view.loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the source's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_detail(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(image, report) }.map_or(ptr::null(), |(_, view)| {
        view.loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_amount(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> u64 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| {
        report
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// How the container was recognized and what this adapter claims of it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_evidence_count(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(image, report) }.map_or(0, |(_, view)| view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_evidence(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(image, report) }.map_or(ptr::null(), |(_, view)| {
        view.evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// The layered disk inspection report: one owned handle over the whole
// report graph, with indexed bounds-checked access to its records and
// relationships. Strings are borrowed from the handle that owns them, and
// identities cross the ABI as opaque values a caller round-trips without
// parsing.

/// A snapshot of one disk's layered inspection. Owned by the caller and
/// released with `remanence_disk_report_free`; every string and record
/// reached through it is borrowed from it and dies with it.
pub struct RemanenceDiskReport {
    device_id: u64,
    device_image_format: CString,
    device_length_bytes: u64,
    device_authoritative_layer: CString,
    device_active_layer: CString,
    content: RemanenceDiskContent,
    content_evidence: Option<CString>,
    schema: Option<SchemaView>,
    regions: Vec<RegionView>,
    volumes: Vec<VolumeRecordView>,
    filesystems: Vec<FilesystemView>,
}

/// What the device's leading structure turned out to be. The report states
/// this rather than leaving a caller to reconstruct it from empty lists.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDiskContent {
    /// All zero: a blank disk, which is an answer.
    Blank,
    /// A partition schema was recognized, whether or not a volume composed.
    Schema,
    /// No schema, and the whole device is one volume.
    DirectVolume,
    /// Not blank, and no adapter claims it. An outcome, not a refusal.
    UnknownNonblank,
}

/// How a schema declares a region: data, which composition may consume, or
/// structure, which it may not.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceRegionRole {
    Data,
    Container,
}

/// Where a volume's storage came from.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceVolumeOrigin {
    WholeDevice,
    Regions,
}

struct SchemaView {
    kind: CString,
    evidence: Vec<CString>,
}

struct IssueView {
    category: RemanenceErrorCategory,
    message: CString,
}

struct RegionView {
    id: u64,
    declared_number: u32,
    declared_placement: CString,
    role: RemanenceRegionRole,
    declared_type: u8,
    declared_type_reading: CString,
    claimed: bool,
    start_bytes: u64,
    length_bytes: u64,
    issue: Option<IssueView>,
}

struct VolumeRecordView {
    id: u64,
    origin: RemanenceVolumeOrigin,
    origin_regions: Vec<u64>,
    start_bytes: u64,
    length_bytes: u64,
    evidence: Vec<CString>,
}

struct FilesystemView {
    id: u64,
    volume: u64,
    kind: Option<CString>,
    label: Option<CString>,
    cluster_bytes: Option<u64>,
    cluster_count: Option<u64>,
    sectors_per_track: Option<u16>,
    heads: Option<u16>,
    cylinders: Option<u64>,
    issues: Vec<IssueView>,
}

fn issue_view(issue: &remanence::Error) -> IssueView {
    IssueView {
        category: issue.category().into(),
        message: to_cstring(&issue.to_string()),
    }
}

fn evidence_views(evidence: &[String]) -> Vec<CString> {
    evidence.iter().map(|line| to_cstring(line)).collect()
}

/// Inspects an open disk and returns its layered report. Null on failure,
/// with the category and message written to the out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_inspect(
    disk: *mut RemanenceDisk,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceDiskReport {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    match disk.disk.inspect() {
        Ok(report) => {
            let (content, content_evidence) = match &report.content {
                DiskContent::Blank => (RemanenceDiskContent::Blank, None),
                DiskContent::Schema => (RemanenceDiskContent::Schema, None),
                DiskContent::DirectVolume => (RemanenceDiskContent::DirectVolume, None),
                DiskContent::UnknownNonblank { evidence } => (
                    RemanenceDiskContent::UnknownNonblank,
                    Some(to_cstring(evidence)),
                ),
            };
            let schema = report.partition_schema.as_ref().map(|schema| SchemaView {
                kind: to_cstring(&schema.kind),
                evidence: evidence_views(&schema.evidence),
            });
            let regions = report
                .regions
                .iter()
                .map(|region| RegionView {
                    id: region.id.value(),
                    declared_number: region.declared_number,
                    declared_placement: to_cstring(&region.declared_placement),
                    role: match region.role {
                        RegionRole::Data => RemanenceRegionRole::Data,
                        RegionRole::Container => RemanenceRegionRole::Container,
                    },
                    declared_type: region.declared_type,
                    declared_type_reading: to_cstring(&region.declared_type_reading),
                    claimed: region.claimed,
                    start_bytes: region.start_bytes,
                    length_bytes: region.length_bytes,
                    issue: region.issue.as_ref().map(issue_view),
                })
                .collect();
            let volumes = report
                .volumes
                .iter()
                .map(|volume| VolumeRecordView {
                    id: volume.id.value(),
                    origin: match &volume.origin {
                        VolumeOrigin::WholeDevice => RemanenceVolumeOrigin::WholeDevice,
                        VolumeOrigin::Regions(_) => RemanenceVolumeOrigin::Regions,
                    },
                    origin_regions: match &volume.origin {
                        VolumeOrigin::WholeDevice => Vec::new(),
                        VolumeOrigin::Regions(regions) => {
                            regions.iter().map(|region| region.value()).collect()
                        }
                    },
                    start_bytes: volume.start_bytes,
                    length_bytes: volume.length_bytes,
                    evidence: evidence_views(&volume.evidence),
                })
                .collect();
            let filesystems = report
                .filesystems
                .iter()
                .map(|filesystem| FilesystemView {
                    id: filesystem.id.value(),
                    volume: filesystem.volume.value(),
                    kind: filesystem.kind.as_deref().map(to_cstring),
                    label: filesystem.label.as_deref().map(to_cstring),
                    cluster_bytes: filesystem.cluster_bytes,
                    cluster_count: filesystem.cluster_count,
                    sectors_per_track: filesystem.declared_geometry.sectors_per_track,
                    heads: filesystem.declared_geometry.heads,
                    cylinders: filesystem.declared_geometry.cylinders,
                    issues: filesystem.issues.iter().map(issue_view).collect(),
                })
                .collect();
            Box::into_raw(Box::new(RemanenceDiskReport {
                device_id: report.device.id,
                device_image_format: to_cstring(&report.device.image_format),
                device_length_bytes: report.device.length_bytes,
                device_authoritative_layer: to_cstring(&report.device.authoritative_layer),
                device_active_layer: to_cstring(&report.device.active_layer),
                content,
                content_evidence,
                schema,
                regions,
                volumes,
                filesystems,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees an inspection report and everything borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_report_free(report: *mut RemanenceDiskReport) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

unsafe fn region_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a RegionView> {
    unsafe { report.as_ref() }?.regions.get(index)
}

unsafe fn volume_record_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a VolumeRecordView> {
    unsafe { report.as_ref() }?.volumes.get(index)
}

unsafe fn filesystem_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a FilesystemView> {
    unsafe { report.as_ref() }?.filesystems.get(index)
}

/// The device identity assigned by this loaded composition (P21), scoped
/// to the open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_id(report: *const RemanenceDiskReport) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.device_id)
}

/// The image format the container turned out to be.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_image_format(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_image_format.as_ptr())
}

/// The device's addressable length in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_length_bytes(
    report: *const RemanenceDiskReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.device_length_bytes)
}

/// The layer the image is authoritative at (P13).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_authoritative_layer(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report.device_authoritative_layer.as_ptr()
    })
}

/// The layer active for this composition (P23).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_active_layer(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_active_layer.as_ptr())
}

/// What the device's leading structure turned out to be.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_content(
    report: *const RemanenceDiskReport,
) -> RemanenceDiskContent {
    unsafe { report.as_ref() }.map_or(RemanenceDiskContent::Blank, |report| report.content)
}

/// Why no adapter claimed the content, for the unknown-nonblank outcome;
/// null for every other outcome.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_content_evidence(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .content_evidence
            .as_ref()
            .map_or(ptr::null(), |evidence| evidence.as_ptr())
    })
}

/// Whether a partition schema was recognized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_has_partition_schema(
    report: *const RemanenceDiskReport,
) -> bool {
    unsafe { report.as_ref() }.is_some_and(|report| report.schema.is_some())
}

/// The recognized schema's kind, or null where none was recognized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_kind(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .schema
            .as_ref()
            .map_or(ptr::null(), |schema| schema.kind.as_ptr())
    })
}

/// How many evidence lines the schema recognition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_evidence_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| {
        report
            .schema
            .as_ref()
            .map_or(0, |schema| schema.evidence.len())
    })
}

/// One evidence line from the schema recognition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_evidence(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report.schema.as_ref().map_or(ptr::null(), |schema| {
            schema
                .evidence
                .get(index)
                .map_or(ptr::null(), |line| line.as_ptr())
        })
    })
}

/// How many regions the schema declares. Every declared region is
/// reported, refused ones included.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_count(report: *const RemanenceDiskReport) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.regions.len())
}

/// A region's opaque identity. Pass it back to the library; never parse
/// it, and never build one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.id)
}

/// The number the schema itself declared this region at.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_number(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u32 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.declared_number)
}

/// How the schema places this region in its own vocabulary: for MBR,
/// "primary" for one of the four slots and "logical" for an entry on the
/// extended chain. A different axis from the role: the extended container
/// is a primary slot whose role is structural.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_placement(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }
        .map_or(ptr::null(), |region| region.declared_placement.as_ptr())
}

/// Whether the schema declares this region as data or as structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_role(
    report: *const RemanenceDiskReport,
    index: usize,
) -> RemanenceRegionRole {
    unsafe { region_view(report, index) }.map_or(RemanenceRegionRole::Data, |region| region.role)
}

/// The type value exactly as the schema records it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_type(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u8 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.declared_type)
}

/// What that value declares, in a sentence fit to quote in a refusal.
/// Present whether or not this release reads the type, and it describes
/// the declaration rather than the content.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_type_reading(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }
        .map_or(ptr::null(), |region| region.declared_type_reading.as_ptr())
}

/// Whether this release reads the declared type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_is_claimed(
    report: *const RemanenceDiskReport,
    index: usize,
) -> bool {
    unsafe { region_view(report, index) }.is_some_and(|region| region.claimed)
}

/// Where the region starts, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_start_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.start_bytes)
}

/// How long the region is, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_length_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.length_bytes)
}

/// The region's refusal category; false where the region reads cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_issue_category(
    report: *const RemanenceDiskReport,
    index: usize,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    let Some(issue) = (unsafe { region_view(report, index) }).and_then(|r| r.issue.as_ref()) else {
        return false;
    };
    if let Some(out) = unsafe { category_out.as_mut() } {
        *out = issue.category;
    }
    true
}

/// The region's refusal, or null where the region reads cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_issue(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }.map_or(ptr::null(), |region| {
        region
            .issue
            .as_ref()
            .map_or(ptr::null(), |issue| issue.message.as_ptr())
    })
}

/// How many volumes were composed, whatever was recognized on them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_count(report: *const RemanenceDiskReport) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.volumes.len())
}

/// How many volumes carry a filesystem the host actually read. Distinct
/// from the composed count on purpose: an unrecognized volume stays in the
/// report rather than vanishing to keep one number correct.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_readable_filesystem_volume_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| {
        report
            .filesystems
            .iter()
            .filter(|filesystem| filesystem.kind.is_some() && filesystem.issues.is_empty())
            .count()
    })
}

/// A volume's opaque identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.id)
}

/// What this volume was composed from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin(
    report: *const RemanenceDiskReport,
    index: usize,
) -> RemanenceVolumeOrigin {
    unsafe { volume_record_view(report, index) }
        .map_or(RemanenceVolumeOrigin::WholeDevice, |volume| volume.origin)
}

/// How many regions this volume was composed from; 0 for a whole-device
/// volume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin_region_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.origin_regions.len())
}

/// The identity of one region this volume was composed from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin_region_id(
    report: *const RemanenceDiskReport,
    index: usize,
    region_index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| {
        volume.origin_regions.get(region_index).copied().unwrap_or(0)
    })
}

/// Where the volume starts, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_start_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.start_bytes)
}

/// How long the volume is, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_length_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.length_bytes)
}

/// How many evidence lines this volume's composition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_evidence_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.evidence.len())
}

/// One evidence line from this volume's composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_evidence(
    report: *const RemanenceDiskReport,
    index: usize,
    evidence_index: usize,
) -> *const c_char {
    unsafe { volume_record_view(report, index) }.map_or(ptr::null(), |volume| {
        volume
            .evidence
            .get(evidence_index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many volumes filesystem recognition was attempted on. A refused
/// attempt is recorded here, at the seam that owns the refusal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.filesystems.len())
}

/// A filesystem's opaque identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.id)
}

/// The identity of the volume this recognition was attempted on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_volume_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.volume)
}

/// The recognized filesystem kind, or null where recognition was refused —
/// the issue then says why, and the volume still stands.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_kind(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .kind
            .as_ref()
            .map_or(ptr::null(), |kind| kind.as_ptr())
    })
}

/// The volume label, or null where the filesystem records none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .label
            .as_ref()
            .map_or(ptr::null(), |label| label.as_ptr())
    })
}

/// The allocation unit size, where the filesystem states one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cluster_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_bytes)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// The allocation unit count, where the filesystem states one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cluster_count(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_count)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Sectors per track as the filesystem's own structures declare it. A
/// filesystem declaration, which manufactures no physical drive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_sectors_per_track(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u16,
) -> bool {
    let Some(value) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.sectors_per_track)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Heads as the filesystem's own structures declare it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_heads(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u16,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.heads) else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Cylinders, only where the derivation is exact. Never invented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cylinders(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cylinders)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// How many issues this recognition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.issues.len())
}

/// One issue's stable category.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue_category(
    report: *const RemanenceDiskReport,
    index: usize,
    issue_index: usize,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    let Some(issue) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.issues.get(issue_index))
    else {
        return false;
    };
    if let Some(out) = unsafe { category_out.as_mut() } {
        *out = issue.category;
    }
    true
}

/// One issue's diagnostic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue(
    report: *const RemanenceDiskReport,
    index: usize,
    issue_index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .issues
            .get(issue_index)
            .map_or(ptr::null(), |issue| issue.message.as_ptr())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_output_carries_category_beside_unchanged_message() {
        let error = remanence::Error::invalid_image("qcow2", "malformed");
        let mut category = RemanenceErrorCategory::Io;
        let mut message = ptr::null_mut();

        unsafe { set_error(&mut category, &mut message, &error) };

        assert_eq!(category, RemanenceErrorCategory::InvalidImage);
        assert_eq!(
            unsafe { CStr::from_ptr(message) }.to_str().expect("UTF-8"),
            "invalid qcow2 disk image: malformed"
        );
        unsafe { remanence_string_free(message) };
    }
}
