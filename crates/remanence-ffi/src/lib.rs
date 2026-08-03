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
    AccessIntent, AccessMode, Disk, DiskFormat, DiskGeometry, FatEntry, FatEntryKind,
    read_hdos_file,
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

/// A snapshot of a disk's complete report (U4): blank is an
/// answer, and every declared partition row stays, issues and all.
pub struct RemanenceDiskGeometry {
    blank: bool,
    partitions: Vec<PartitionView>,
    volumes: Vec<VolumeView>,
}

struct PartitionView {
    number: u32,
    kind_name: CString,
    type_byte: u8,
    type_name: Option<CString>,
    start_bytes: u64,
    length_bytes: u64,
    issue_category: Option<RemanenceErrorCategory>,
    issue: Option<CString>,
}

struct VolumeView {
    id: CString,
    partition_number: Option<u32>,
    kind_name: CString,
    label: Option<CString>,
    offset_bytes: u64,
    length_bytes: u64,
    cluster_bytes: u64,
    cluster_count: u64,
    sectors_per_track: Option<u16>,
    heads: Option<u16>,
    cylinders: Option<u64>,
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

/// Reads the disk's complete report (U4): its partitions and
/// volumes as they actually are. Blank is an answer (zero volumes, see
/// `remanence_geometry_is_blank`), a partition row the library cannot read
/// stays in the report carrying its issue, and non-zero data that is
/// neither a supported filesystem nor a partition table fails by name,
/// kept distinct from blank. Free the result with
/// `remanence_disk_geometry_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_geometry(
    disk: *mut RemanenceDisk,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceDiskGeometry {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    match disk.disk.geometry() {
        Ok(DiskGeometry {
            blank,
            partitions,
            volumes,
        }) => {
            let partitions = partitions
                .iter()
                .map(|partition| PartitionView {
                    number: partition.number,
                    kind_name: to_cstring(partition.kind.name()),
                    type_byte: partition.type_byte,
                    type_name: partition.type_name.as_deref().map(to_cstring),
                    start_bytes: partition.start_bytes,
                    length_bytes: partition.length_bytes,
                    issue_category: partition
                        .issue
                        .as_ref()
                        .map(|issue| issue.category().into()),
                    issue: partition
                        .issue
                        .as_ref()
                        .map(|issue| to_cstring(&issue.to_string())),
                })
                .collect();
            let volumes = volumes
                .iter()
                .map(|volume| VolumeView {
                    id: to_cstring(&volume.id),
                    partition_number: volume.partition_number,
                    kind_name: to_cstring(volume.kind.name()),
                    label: volume.label.as_deref().map(to_cstring),
                    offset_bytes: volume.offset_bytes,
                    length_bytes: volume.length_bytes,
                    cluster_bytes: volume.cluster_bytes,
                    cluster_count: volume.cluster_count,
                    sectors_per_track: volume.sectors_per_track,
                    heads: volume.heads,
                    cylinders: volume.cylinders,
                })
                .collect();
            Box::into_raw(Box::new(RemanenceDiskGeometry {
                blank,
                partitions,
                volumes,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a geometry snapshot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_geometry_free(geometry: *mut RemanenceDiskGeometry) {
    if !geometry.is_null() {
        drop(unsafe { Box::from_raw(geometry) });
    }
}

unsafe fn partition_view<'a>(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> Option<&'a PartitionView> {
    unsafe { geometry.as_ref() }?.partitions.get(index)
}

unsafe fn volume_view<'a>(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> Option<&'a VolumeView> {
    unsafe { geometry.as_ref() }?.volumes.get(index)
}

/// Whether sector 0 was all zero: a blank disk with zero volumes — an
/// answer, not an error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_is_blank(geometry: *const RemanenceDiskGeometry) -> bool {
    unsafe { geometry.as_ref() }.is_some_and(|geometry| geometry.blank)
}

/// Number of partitions (0 for a partitionless image).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_count(geometry: *const RemanenceDiskGeometry) -> usize {
    unsafe { geometry.as_ref() }.map_or(0, |geometry| geometry.partitions.len())
}

/// A partition's 1-based number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_number(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u32 {
    unsafe { partition_view(geometry, index) }.map_or(0, |partition| partition.number)
}

/// A partition's MBR type byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_type_byte(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u8 {
    unsafe { partition_view(geometry, index) }.map_or(0, |partition| partition.type_byte)
}

/// A partition row's kind: "primary" (an MBR slot, the extended
/// container included) or "logical" (a row of the extended chain).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_kind(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { partition_view(geometry, index) }
        .map_or(ptr::null(), |partition| partition.kind_name.as_ptr())
}

/// A partition's pinned type name, or null when the type byte is outside
/// the claim — the row's issue then names the refusal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_type_name(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { partition_view(geometry, index) }
        .and_then(|partition| partition.type_name.as_ref())
        .map_or(ptr::null(), |type_name| type_name.as_ptr())
}

/// A partition's start offset in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_start_bytes(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { partition_view(geometry, index) }.map_or(0, |partition| partition.start_bytes)
}

/// A partition's length in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_length_bytes(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { partition_view(geometry, index) }.map_or(0, |partition| partition.length_bytes)
}

/// Stores a partition row's issue category and returns true; returns
/// false when the row was read cleanly (or the index is out of range).
/// A row carrying an issue stays in the report with no volume read from
/// it, and the rows behind it never renumber (U4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_issue_category(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    match unsafe { partition_view(geometry, index) }.and_then(|partition| partition.issue_category)
    {
        Some(category) => {
            if !category_out.is_null() {
                unsafe { *category_out = category };
            }
            true
        }
        None => false,
    }
}

/// A partition row's issue diagnostic — why no volume was read from the
/// row — or null when the row was read cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_partition_issue(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { partition_view(geometry, index) }
        .and_then(|partition| partition.issue.as_ref())
        .map_or(ptr::null(), |issue| issue.as_ptr())
}

/// Number of volumes actually read (one guest drive letter each).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_count(geometry: *const RemanenceDiskGeometry) -> usize {
    unsafe { geometry.as_ref() }.map_or(0, |geometry| geometry.volumes.len())
}

/// A volume's opaque stable identifier. The borrowed string is owned by
/// `geometry`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_id(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { volume_view(geometry, index) }.map_or(ptr::null(), |volume| volume.id.as_ptr())
}

/// The 1-based partition number a volume sits in; returns false for a
/// partitionless image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_partition_number(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { volume_view(geometry, index) }.and_then(|volume| volume.partition_number);
    unsafe { write_opt_u32(value, out) }
}

/// The volume's FAT kind name ("FAT12" or "FAT16").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_kind(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { volume_view(geometry, index) }.map_or(ptr::null(), |volume| volume.kind_name.as_ptr())
}

/// The volume label, or null when it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_label(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { volume_view(geometry, index) }
        .and_then(|volume| volume.label.as_ref())
        .map_or(ptr::null(), |label| label.as_ptr())
}

/// The volume's offset in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_offset_bytes(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.offset_bytes)
}

/// The volume's length in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_length_bytes(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.length_bytes)
}

/// The volume's cluster size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_cluster_bytes(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.cluster_bytes)
}

/// The volume's data-cluster count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_cluster_count(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.cluster_count)
}

/// The BPB-stated sectors per track; returns false where the boot record
/// states none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_sectors_per_track(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { volume_view(geometry, index) }
        .and_then(|volume| volume.sectors_per_track.map(u32::from));
    unsafe { write_opt_u32(value, out) }
}

/// The BPB-stated head count; returns false where the boot record states
/// none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_heads(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    let value =
        unsafe { volume_view(geometry, index) }.and_then(|volume| volume.heads.map(u32::from));
    unsafe { write_opt_u32(value, out) }
}

/// The volume's cylinder count, only where an exact derivation exists —
/// the boot record's track geometry divides the total sector count with
/// no remainder; returns false otherwise, never an invented value
/// (U4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_volume_cylinders(
    geometry: *const RemanenceDiskGeometry,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { volume_view(geometry, index) }.and_then(|volume| volume.cylinders);
    unsafe { write_opt_u64(value, out) }
}

/// Lists a directory in `volume_id` ("" = root, "A/B" descends). Free
/// with `remanence_fat_entry_list_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_disk_entries(
    disk: *mut RemanenceDisk,
    volume_id: *const c_char,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFatEntryList {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(volume_id) = (unsafe { utf8_arg(volume_id) }) else {
        let error = remanence::Error::io("null volume identifier");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let path = unsafe { utf8_arg(path) }.unwrap_or_default();
    match disk.disk.entries(volume_id.as_ref(), path.as_ref()) {
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
    volume_id: *const c_char,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFatEntryList {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(volume_id) = (unsafe { utf8_arg(volume_id) }) else {
        let error = remanence::Error::io("null volume identifier");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match disk.disk.stat(volume_id.as_ref(), path.as_ref()) {
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
    volume_id: *const c_char,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> *mut RemanenceFileData {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(volume_id) = (unsafe { utf8_arg(volume_id) }) else {
        let error = remanence::Error::io("null volume identifier");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return ptr::null_mut();
    };
    match disk.disk.read_file(volume_id.as_ref(), path.as_ref()) {
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
    volume_id: *const c_char,
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
    let (Some(volume_id), Some(path)) =
        (unsafe { utf8_arg(volume_id) }, unsafe { utf8_arg(path) })
    else {
        let error = remanence::Error::io("null volume identifier or path");
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
        .read_file_at(volume_id.as_ref(), path.as_ref(), offset, buffer)
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
    volume_id: *const c_char,
    path: *const c_char,
    size: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let (Some(volume_id), Some(path)) =
        (unsafe { utf8_arg(volume_id) }, unsafe { utf8_arg(path) })
    else {
        let error = remanence::Error::io("null volume identifier or path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    match disk.disk.resize_file(volume_id.as_ref(), path.as_ref(), size) {
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
    volume_id: *const c_char,
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
    let (Some(volume_id), Some(path)) =
        (unsafe { utf8_arg(volume_id) }, unsafe { utf8_arg(path) })
    else {
        let error = remanence::Error::io("null volume identifier or path");
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
        .write_file_at(volume_id.as_ref(), path.as_ref(), offset, data)
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
    volume_id: *const c_char,
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
    let Some(volume_id) = (unsafe { utf8_arg(volume_id) }) else {
        let error = remanence::Error::io("null volume identifier");
        unsafe { set_error(error_category_out, error_out, &error) };
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
        .write_file(volume_id.as_ref(), path.as_ref(), contents)
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
    volume_id: *const c_char,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(volume_id) = (unsafe { utf8_arg(volume_id) }) else {
        let error = remanence::Error::io("null volume identifier");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, &error) };
        return false;
    };
    match disk.disk.make_directory(volume_id.as_ref(), path.as_ref()) {
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
