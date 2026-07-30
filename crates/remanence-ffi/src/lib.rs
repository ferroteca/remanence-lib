// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! C ABI for the Remanence disk image analysis library.
//!
//! Conventions:
//! - Handles (`RmnSession`, `RmnIdentification`, `RmnHdosFileList`) are opaque
//!   and freed with their matching `*_free` function.
//! - `const char*` return values are UTF-8, owned by the handle they were read
//!   from, and valid until that handle is freed. Do not free them.
//! - Fallible constructors take an optional `char** error_out`; on failure they
//!   return null and store a message to free with `rmn_string_free`.
//! - Accessors taking an index return null / false / 0 when the index is out of
//!   range or the field does not apply to the container's layout.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use remanence::{
    Container, ContainerKind, ContainerLayout, DiskLayout, HdosFile, Identification,
    PhysicalMediaLayout, SectorLayout, Session, list_hdos_files,
};

/// What role a detected container plays in the image's layering.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RmnContainerKind {
    Archive,
    Image,
    PhysicalMedia,
    Filesystem,
    Unknown,
}

/// Which layout details a container carries.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RmnLayoutKind {
    Unknown,
    Archive,
    Image,
    PhysicalMedia,
    Filesystem,
}

/// Sector arrangement across a disk.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RmnSectorLayoutKind {
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

unsafe fn set_error(error_out: *mut *mut c_char, error: &remanence::Error) {
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
    sector_layout: RmnSectorLayoutKind,
    sectors_per_track: u32,
    tracks: Vec<TrackView>,
    total_sectors: Option<u64>,
}

impl DiskView {
    fn new(layout: &DiskLayout) -> Self {
        let (sector_layout, sectors_per_track, tracks) = match &layout.sectors {
            SectorLayout::Unknown => (RmnSectorLayoutKind::Unknown, 0, Vec::new()),
            SectorLayout::Fixed { sectors_per_track } => {
                (RmnSectorLayoutKind::Fixed, *sectors_per_track, Vec::new())
            }
            SectorLayout::Variable { tracks } => (
                RmnSectorLayoutKind::Variable,
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
    kind: RmnContainerKind,
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
            ContainerKind::Archive => RmnContainerKind::Archive,
            ContainerKind::Image => RmnContainerKind::Image,
            ContainerKind::PhysicalMedia => RmnContainerKind::PhysicalMedia,
            ContainerKind::Filesystem => RmnContainerKind::Filesystem,
            ContainerKind::Unknown => RmnContainerKind::Unknown,
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
pub struct RmnSession {
    session: Session,
    path: CString,
    image_path: CString,
}

/// The result of identifying a session's image.
pub struct RmnIdentification {
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
pub struct RmnHdosFileList {
    files: Vec<HdosFileView>,
}

unsafe fn container_view<'a>(
    identification: *const RmnIdentification,
    index: usize,
) -> Option<&'a ContainerView> {
    let identification = unsafe { identification.as_ref() }?;
    identification.containers.get(index)
}

unsafe fn hdos_file_view<'a>(
    list: *const RmnHdosFileList,
    index: usize,
) -> Option<&'a HdosFileView> {
    let list = unsafe { list.as_ref() }?;
    list.files.get(index)
}

fn hdos_list_from_bytes(
    bytes: &[u8],
    error_out: *mut *mut c_char,
) -> *mut RmnHdosFileList {
    match list_hdos_files(bytes) {
        Ok(files) => {
            let files = files.iter().map(HdosFileView::new).collect();
            Box::into_raw(Box::new(RmnHdosFileList { files }))
        }
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Returns the library version as a static string. Do not free.
#[unsafe(no_mangle)]
pub extern "C" fn rmn_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Frees a string returned through an `error_out` parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_string_free(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}

/// Opens `path` (UTF-8) — a raw disk image, or `archive.zip[/entry]` — with
/// the default format registry. Returns null on failure and stores a message
/// in `error_out` (free with `rmn_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_open(
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut RmnSession {
    unsafe { clear_error(error_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    }

    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match Session::open(path.as_ref()) {
        Ok(session) => {
            let path = to_cstring(&session.path().display().to_string());
            let image_path = to_cstring(&session.image_path().display().to_string());
            Box::into_raw(Box::new(RmnSession { session, path, image_path }))
        }
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a session handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_free(session: *mut RmnSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// The path the session was opened from (the archive path for ZIP inputs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_path(session: *const RmnSession) -> *const c_char {
    match unsafe { session.as_ref() } {
        Some(session) => session.path.as_ptr(),
        None => ptr::null(),
    }
}

/// The resolved image path (the entry name for ZIP inputs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_image_path(
    session: *const RmnSession,
) -> *const c_char {
    match unsafe { session.as_ref() } {
        Some(session) => session.image_path.as_ptr(),
        None => ptr::null(),
    }
}

/// The resolved image bytes; valid until the session is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_bytes(
    session: *const RmnSession,
    length_out: *mut usize,
) -> *const u8 {
    match unsafe { session.as_ref() } {
        Some(session) => {
            let bytes = session.session.bytes();
            if !length_out.is_null() {
                unsafe { *length_out = bytes.len() };
            }
            bytes.as_ptr()
        }
        None => {
            if !length_out.is_null() {
                unsafe { *length_out = 0 };
            }
            ptr::null()
        }
    }
}

/// Whether the session has unsaved modifications.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_is_modified(session: *const RmnSession) -> bool {
    unsafe { session.as_ref() }.is_some_and(|session| session.session.is_modified())
}

/// Identifies the image's container layers and probable filesystem. Free the
/// result with `rmn_identification_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_identify(
    session: *const RmnSession,
) -> *mut RmnIdentification {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return ptr::null_mut();
    };
    let Identification { containers, modified, evidence } = session.session.identify();

    Box::into_raw(Box::new(RmnIdentification {
        modified,
        containers: containers.iter().map(ContainerView::new).collect(),
        evidence: evidence.iter().map(|line| to_cstring(line)).collect(),
    }))
}

/// Frees an identification handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_identification_free(
    identification: *mut RmnIdentification,
) {
    if !identification.is_null() {
        drop(unsafe { Box::from_raw(identification) });
    }
}

/// Whether the session reported unsaved modifications at identify time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_identification_modified(
    identification: *const RmnIdentification,
) -> bool {
    unsafe { identification.as_ref() }.is_some_and(|identification| identification.modified)
}

/// Number of detected container layers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_identification_container_count(
    identification: *const RmnIdentification,
) -> usize {
    unsafe { identification.as_ref() }
        .map_or(0, |identification| identification.containers.len())
}

/// Number of evidence lines.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_identification_evidence_count(
    identification: *const RmnIdentification,
) -> usize {
    unsafe { identification.as_ref() }
        .map_or(0, |identification| identification.evidence.len())
}

/// One evidence line, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_identification_evidence(
    identification: *const RmnIdentification,
    index: usize,
) -> *const c_char {
    unsafe { identification.as_ref() }
        .and_then(|identification| identification.evidence.get(index))
        .map_or(ptr::null(), |line| line.as_ptr())
}

/// The container's kind, or `RmnContainerKind::Unknown` when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_kind(
    identification: *const RmnIdentification,
    index: usize,
) -> RmnContainerKind {
    unsafe { container_view(identification, index) }
        .map_or(RmnContainerKind::Unknown, |container| container.kind)
}

/// The container's id (e.g. "h8d", "zip", "hdos").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_id(
    identification: *const RmnIdentification,
    index: usize,
) -> *const c_char {
    unsafe { container_view(identification, index) }
        .map_or(ptr::null(), |container| container.id.as_ptr())
}

/// The container's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_name(
    identification: *const RmnIdentification,
    index: usize,
) -> *const c_char {
    unsafe { container_view(identification, index) }
        .map_or(ptr::null(), |container| container.name.as_ptr())
}

/// Detection confidence, 0-100.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_confidence(
    identification: *const RmnIdentification,
    index: usize,
) -> u8 {
    unsafe { container_view(identification, index) }
        .map_or(0, |container| container.confidence)
}

/// Whether the container matched a known format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_known(
    identification: *const RmnIdentification,
    index: usize,
) -> bool {
    unsafe { container_view(identification, index) }
        .is_some_and(|container| container.known)
}

/// Current size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_current_bytes(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value =
        unsafe { container_view(identification, index) }.and_then(|c| c.current_bytes);
    unsafe { write_opt_u64(value, out) }
}

/// Expected size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_expected_bytes(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value =
        unsafe { container_view(identification, index) }.and_then(|c| c.expected_bytes);
    unsafe { write_opt_u64(value, out) }
}

/// Which layout details this container carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_layout_kind(
    identification: *const RmnIdentification,
    index: usize,
) -> RmnLayoutKind {
    unsafe { container_view(identification, index) }.map_or(
        RmnLayoutKind::Unknown,
        |container| match &container.layout {
            LayoutView::Unknown => RmnLayoutKind::Unknown,
            LayoutView::Archive { .. } => RmnLayoutKind::Archive,
            LayoutView::Image { .. } => RmnLayoutKind::Image,
            LayoutView::PhysicalMedia(_) => RmnLayoutKind::PhysicalMedia,
            LayoutView::Filesystem { .. } => RmnLayoutKind::Filesystem,
        },
    )
}

/// Archive layout: the archive file path; null for other layouts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_archive_path(
    identification: *const RmnIdentification,
    index: usize,
) -> *const c_char {
    match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { path, .. }) => path.as_ptr(),
        _ => ptr::null(),
    }
}

/// Archive layout: the entry name inside the archive; null for other layouts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_archive_entry_name(
    identification: *const RmnIdentification,
    index: usize,
) -> *const c_char {
    match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { entry_name, .. }) => entry_name.as_ptr(),
        _ => ptr::null(),
    }
}

/// Archive layout: compressed entry size; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_archive_compressed_size(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { compressed_size, .. }) => *compressed_size,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Archive layout: uncompressed entry size; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_archive_uncompressed_size(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { uncompressed_size, .. }) => *uncompressed_size,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Image layout: payload offset in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_image_payload_offset(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Image { payload_offset_bytes, .. }) => *payload_offset_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Image layout: payload length in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_image_payload_length(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Image { payload_length_bytes, .. }) => *payload_length_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

unsafe fn disk_view<'a>(
    identification: *const RmnIdentification,
    index: usize,
) -> Option<&'a DiskView> {
    match unsafe { container_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::PhysicalMedia(disk)) => disk.as_ref(),
        _ => None,
    }
}

/// Physical media layout: whether disk geometry is known.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_has_disk_layout(
    identification: *const RmnIdentification,
    index: usize,
) -> bool {
    unsafe { disk_view(identification, index) }.is_some()
}

/// Disk layout: media kind (e.g. "floppy"); null when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_media_kind(
    identification: *const RmnIdentification,
    index: usize,
) -> *const c_char {
    unsafe { disk_view(identification, index) }
        .and_then(|disk| disk.media_kind.as_ref())
        .map_or(ptr::null(), |kind| kind.as_ptr())
}

/// Disk layout: sector size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_sector_size(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.sector_size);
    unsafe { write_opt_u64(value, out) }
}

/// Disk layout: cylinder count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_cylinders(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.cylinders);
    unsafe { write_opt_u32(value, out) }
}

/// Disk layout: side count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_sides(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.sides);
    unsafe { write_opt_u32(value, out) }
}

/// Disk layout: how sectors are arranged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_sector_layout_kind(
    identification: *const RmnIdentification,
    index: usize,
) -> RmnSectorLayoutKind {
    unsafe { disk_view(identification, index) }
        .map_or(RmnSectorLayoutKind::Unknown, |disk| disk.sector_layout)
}

/// Disk layout: sectors per track for fixed layouts; 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_sectors_per_track(
    identification: *const RmnIdentification,
    index: usize,
) -> u32 {
    unsafe { disk_view(identification, index) }.map_or(0, |disk| disk.sectors_per_track)
}

/// Disk layout: per-track entry count for variable layouts; 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_track_count(
    identification: *const RmnIdentification,
    index: usize,
) -> usize {
    unsafe { disk_view(identification, index) }.map_or(0, |disk| disk.tracks.len())
}

/// Disk layout: one per-track entry for variable layouts. Returns false when
/// out of range. `has_sector_size` and `sector_size` report the optional
/// per-track sector size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_disk_track(
    identification: *const RmnIdentification,
    index: usize,
    track_index: usize,
    cylinder: *mut u32,
    side: *mut u32,
    sectors: *mut u32,
    has_sector_size: *mut bool,
    sector_size: *mut u64,
) -> bool {
    let Some(track) = unsafe { disk_view(identification, index) }
        .and_then(|disk| disk.tracks.get(track_index))
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
pub unsafe extern "C" fn rmn_container_disk_total_sectors(
    identification: *const RmnIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value =
        unsafe { disk_view(identification, index) }.and_then(|disk| disk.total_sectors);
    unsafe { write_opt_u64(value, out) }
}

/// Filesystem layout: offset in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_container_fs_offset_bytes(
    identification: *const RmnIdentification,
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
pub unsafe extern "C" fn rmn_container_fs_length_bytes(
    identification: *const RmnIdentification,
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
/// stores a message in `error_out` (free with `rmn_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_list_hdos_files(
    bytes: *const u8,
    length: usize,
    error_out: *mut *mut c_char,
) -> *mut RmnHdosFileList {
    unsafe { clear_error(error_out) };
    if bytes.is_null() {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, length) };
    hdos_list_from_bytes(bytes, error_out)
}

/// Parses the HDOS directory from a session's image bytes. Returns null on
/// failure and stores a message in `error_out` (free with `rmn_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_list_hdos_files(
    session: *const RmnSession,
    error_out: *mut *mut c_char,
) -> *mut RmnHdosFileList {
    unsafe { clear_error(error_out) };
    let Some(session) = (unsafe { session.as_ref() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    };
    hdos_list_from_bytes(session.session.bytes(), error_out)
}

/// Frees an HDOS file list handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_list_free(list: *mut RmnHdosFileList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

/// Number of files in the listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_count(list: *const RmnHdosFileList) -> usize {
    unsafe { list.as_ref() }.map_or(0, |list| list.files.len())
}

/// File name without extension, e.g. "HDOS".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_name(
    list: *const RmnHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }.map_or(ptr::null(), |file| file.name.as_ptr())
}

/// File extension, possibly empty, e.g. "SYS".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_extension(
    list: *const RmnHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }
        .map_or(ptr::null(), |file| file.extension.as_ptr())
}

/// `"NAME.EXT"`, or `"NAME"` when the extension is empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_display_name(
    list: *const RmnHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }
        .map_or(ptr::null(), |file| file.display_name.as_ptr())
}

/// Size in 256-byte sectors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_size_sectors(
    list: *const RmnHdosFileList,
    index: usize,
) -> u32 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.size_sectors)
}

/// Size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_size_bytes(
    list: *const RmnHdosFileList,
    index: usize,
) -> u64 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.size_bytes)
}

/// Raw HDOS date word.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_modified_date_raw(
    list: *const RmnHdosFileList,
    index: usize,
) -> u16 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.modified_date_raw)
}

/// Raw HDOS flag byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_flags_raw(
    list: *const RmnHdosFileList,
    index: usize,
) -> u8 {
    unsafe { hdos_file_view(list, index) }.map_or(0, |file| file.flags_raw)
}

/// HDOS flag letters (subset of "SLWC"), possibly empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_flags(
    list: *const RmnHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }.map_or(ptr::null(), |file| file.flags.as_ptr())
}

/// HDOS catalog date, e.g. "09-May-78", or "No-Date".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_hdos_file_modified_date(
    list: *const RmnHdosFileList,
    index: usize,
) -> *const c_char {
    unsafe { hdos_file_view(list, index) }
        .map_or(ptr::null(), |file| file.modified_date.as_ptr())
}
