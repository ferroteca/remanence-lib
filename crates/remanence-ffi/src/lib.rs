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

// ---------------------------------------------------------------------------
// The Disk surface (U3/U4): open a raw or qcow2 image under the
// P7 claim, report partitions and volumes, read/write FAT files with a
// commit point.

use remanence::{
    AccessMode, Disk, DiskFormat, DiskGeometry, FatEntry, FatEntryKind,
    read_hdos_file,
};

/// Which P7 mode an open obtained.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RmnAccessMode {
    ReadWrite,
    ReadOnly,
}

/// The container format a disk image turned out to be.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RmnDiskFormat {
    Raw,
    Qcow2,
}

/// What a FAT directory entry is.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RmnFatEntryKind {
    File,
    Directory,
}

fn access_mode(mode: AccessMode) -> RmnAccessMode {
    match mode {
        AccessMode::ReadWrite => RmnAccessMode::ReadWrite,
        AccessMode::ReadOnly => RmnAccessMode::ReadOnly,
    }
}

/// An open disk image.
pub struct RmnDisk {
    disk: Disk,
}

/// A snapshot of a disk's partitions and volumes.
pub struct RmnDiskGeometry {
    partitions: Vec<PartitionView>,
    volumes: Vec<VolumeView>,
}

struct PartitionView {
    number: u32,
    type_byte: u8,
    type_name: CString,
    start_bytes: u64,
    length_bytes: u64,
}

struct VolumeView {
    partition_number: Option<u32>,
    kind_name: CString,
    label: Option<CString>,
    offset_bytes: u64,
    length_bytes: u64,
    cluster_bytes: u64,
    cluster_count: u64,
    sectors_per_track: Option<u16>,
    heads: Option<u16>,
}

/// A directory listing.
pub struct RmnFatEntryList {
    entries: Vec<FatEntryView>,
}

struct FatEntryView {
    name: CString,
    kind: RmnFatEntryKind,
    size_bytes: u64,
}

/// Bytes read out of a volume or catalog.
pub struct RmnFileData {
    bytes: Vec<u8>,
}

unsafe fn utf8_arg<'a>(value: *const c_char) -> Option<std::borrow::Cow<'a, str>> {
    if value.is_null() {
        return None;
    }
    Some(String::from_utf8_lossy(unsafe { CStr::from_ptr(value) }.to_bytes()))
}

/// Opens `path` (UTF-8) as a disk image — raw or qcow2, detected
/// by magic — under the P7 claim: read/write with writes denied to others
/// (preferred), read-only fallback, fail fast when deny-write cannot be
/// obtained. Returns null on failure with a message in `error_out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_open(
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut RmnDisk {
    unsafe { clear_error(error_out) };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    };
    match Disk::open(path.as_ref()) {
        Ok(disk) => Box::into_raw(Box::new(RmnDisk { disk })),
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a disk handle, releasing the P7 claim. Uncommitted changes are
/// discarded (the commit point never reached the file).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_free(disk: *mut RmnDisk) {
    if !disk.is_null() {
        drop(unsafe { Box::from_raw(disk) });
    }
}

/// Which P7 mode the open obtained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_mode(disk: *const RmnDisk) -> RmnAccessMode {
    unsafe { disk.as_ref() }
        .map_or(RmnAccessMode::ReadOnly, |disk| access_mode(disk.disk.mode()))
}

/// The detected container format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_format(disk: *const RmnDisk) -> RmnDiskFormat {
    match unsafe { disk.as_ref() }.map(|disk| disk.disk.format()) {
        Some(DiskFormat::Qcow2 { .. }) => RmnDiskFormat::Qcow2,
        _ => RmnDiskFormat::Raw,
    }
}

/// The qcow2 version, or 0 for a raw image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_qcow2_version(disk: *const RmnDisk) -> u32 {
    match unsafe { disk.as_ref() }.map(|disk| disk.disk.format()) {
        Some(DiskFormat::Qcow2 { version }) => version,
        _ => 0,
    }
}

/// The virtual disk size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_size(disk: *const RmnDisk) -> u64 {
    unsafe { disk.as_ref() }.map_or(0, |disk| disk.disk.size())
}

/// Whether uncommitted changes exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_is_modified(disk: *const RmnDisk) -> bool {
    unsafe { disk.as_ref() }.is_some_and(|disk| disk.disk.is_modified())
}

/// Reads the disk's partitions and volumes as they actually are. Free the
/// result with `rmn_disk_geometry_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_geometry(
    disk: *mut RmnDisk,
    error_out: *mut *mut c_char,
) -> *mut RmnDiskGeometry {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    match disk.disk.geometry() {
        Ok(DiskGeometry { partitions, volumes }) => {
            let partitions = partitions
                .iter()
                .map(|partition| PartitionView {
                    number: partition.number,
                    type_byte: partition.type_byte,
                    type_name: to_cstring(&partition.type_name),
                    start_bytes: partition.start_bytes,
                    length_bytes: partition.length_bytes,
                })
                .collect();
            let volumes = volumes
                .iter()
                .map(|volume| VolumeView {
                    partition_number: volume.partition_number,
                    kind_name: to_cstring(volume.kind.name()),
                    label: volume.label.as_deref().map(to_cstring),
                    offset_bytes: volume.offset_bytes,
                    length_bytes: volume.length_bytes,
                    cluster_bytes: volume.cluster_bytes,
                    cluster_count: volume.cluster_count,
                    sectors_per_track: volume.sectors_per_track,
                    heads: volume.heads,
                })
                .collect();
            Box::into_raw(Box::new(RmnDiskGeometry { partitions, volumes }))
        }
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a geometry snapshot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_geometry_free(geometry: *mut RmnDiskGeometry) {
    if !geometry.is_null() {
        drop(unsafe { Box::from_raw(geometry) });
    }
}

unsafe fn partition_view<'a>(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> Option<&'a PartitionView> {
    unsafe { geometry.as_ref() }?.partitions.get(index)
}

unsafe fn volume_view<'a>(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> Option<&'a VolumeView> {
    unsafe { geometry.as_ref() }?.volumes.get(index)
}

/// Number of partitions (0 for a partitionless image).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_partition_count(
    geometry: *const RmnDiskGeometry,
) -> usize {
    unsafe { geometry.as_ref() }.map_or(0, |geometry| geometry.partitions.len())
}

/// A partition's 1-based number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_partition_number(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u32 {
    unsafe { partition_view(geometry, index) }.map_or(0, |partition| partition.number)
}

/// A partition's MBR type byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_partition_type_byte(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u8 {
    unsafe { partition_view(geometry, index) }.map_or(0, |partition| partition.type_byte)
}

/// A partition's pinned type name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_partition_type_name(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { partition_view(geometry, index) }
        .map_or(ptr::null(), |partition| partition.type_name.as_ptr())
}

/// A partition's start offset in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_partition_start_bytes(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { partition_view(geometry, index) }
        .map_or(0, |partition| partition.start_bytes)
}

/// A partition's length in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_partition_length_bytes(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { partition_view(geometry, index) }
        .map_or(0, |partition| partition.length_bytes)
}

/// Number of volumes actually read (one guest drive letter each).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_count(
    geometry: *const RmnDiskGeometry,
) -> usize {
    unsafe { geometry.as_ref() }.map_or(0, |geometry| geometry.volumes.len())
}

/// The 1-based partition number a volume sits in; returns false for a
/// partitionless image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_partition_number(
    geometry: *const RmnDiskGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    let value =
        unsafe { volume_view(geometry, index) }.and_then(|volume| volume.partition_number);
    unsafe { write_opt_u32(value, out) }
}

/// The volume's FAT kind name ("FAT12" or "FAT16").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_kind(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { volume_view(geometry, index) }
        .map_or(ptr::null(), |volume| volume.kind_name.as_ptr())
}

/// The volume label, or null when it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_label(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> *const c_char {
    unsafe { volume_view(geometry, index) }
        .and_then(|volume| volume.label.as_ref())
        .map_or(ptr::null(), |label| label.as_ptr())
}

/// The volume's offset in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_offset_bytes(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.offset_bytes)
}

/// The volume's length in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_length_bytes(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.length_bytes)
}

/// The volume's cluster size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_cluster_bytes(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.cluster_bytes)
}

/// The volume's data-cluster count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_cluster_count(
    geometry: *const RmnDiskGeometry,
    index: usize,
) -> u64 {
    unsafe { volume_view(geometry, index) }.map_or(0, |volume| volume.cluster_count)
}

/// The BPB-stated sectors per track; returns false where the boot record
/// states none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_geometry_volume_sectors_per_track(
    geometry: *const RmnDiskGeometry,
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
pub unsafe extern "C" fn rmn_geometry_volume_heads(
    geometry: *const RmnDiskGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { volume_view(geometry, index) }
        .and_then(|volume| volume.heads.map(u32::from));
    unsafe { write_opt_u32(value, out) }
}

/// Lists a directory in volume `volume` ("" = root, "A/B" descends). Free
/// with `rmn_fat_entry_list_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_entries(
    disk: *mut RmnDisk,
    volume: usize,
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut RmnFatEntryList {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let path = unsafe { utf8_arg(path) }.unwrap_or_default();
    match disk.disk.entries(volume, path.as_ref()) {
        Ok(entries) => {
            let entries = entries
                .iter()
                .map(|entry: &FatEntry| FatEntryView {
                    name: to_cstring(&entry.name),
                    kind: match entry.kind {
                        FatEntryKind::File => RmnFatEntryKind::File,
                        FatEntryKind::Directory => RmnFatEntryKind::Directory,
                    },
                    size_bytes: entry.size_bytes,
                })
                .collect();
            Box::into_raw(Box::new(RmnFatEntryList { entries }))
        }
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a directory listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_fat_entry_list_free(list: *mut RmnFatEntryList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

unsafe fn fat_entry_view<'a>(
    list: *const RmnFatEntryList,
    index: usize,
) -> Option<&'a FatEntryView> {
    unsafe { list.as_ref() }?.entries.get(index)
}

/// Number of entries in the listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_fat_entry_count(list: *const RmnFatEntryList) -> usize {
    unsafe { list.as_ref() }.map_or(0, |list| list.entries.len())
}

/// An entry's 8.3 name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_fat_entry_name(
    list: *const RmnFatEntryList,
    index: usize,
) -> *const c_char {
    unsafe { fat_entry_view(list, index) }.map_or(ptr::null(), |entry| entry.name.as_ptr())
}

/// Whether an entry is a file or a directory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_fat_entry_kind(
    list: *const RmnFatEntryList,
    index: usize,
) -> RmnFatEntryKind {
    unsafe { fat_entry_view(list, index) }
        .map_or(RmnFatEntryKind::File, |entry| entry.kind)
}

/// An entry's size in bytes (0 for directories).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_fat_entry_size_bytes(
    list: *const RmnFatEntryList,
    index: usize,
) -> u64 {
    unsafe { fat_entry_view(list, index) }.map_or(0, |entry| entry.size_bytes)
}

/// Copies a file's bytes out of volume `volume`. Free with
/// `rmn_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_read_file(
    disk: *mut RmnDisk,
    volume: usize,
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut RmnFileData {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    };
    match disk.disk.read_file(volume, path.as_ref()) {
        Ok(bytes) => Box::into_raw(Box::new(RmnFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The bytes of a read-out file; valid until the handle is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_file_data_bytes(
    data: *const RmnFileData,
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
pub unsafe extern "C" fn rmn_file_data_free(data: *mut RmnFileData) {
    if !data.is_null() {
        drop(unsafe { Box::from_raw(data) });
    }
}

/// Writes a file into volume `volume`. Buffered until `rmn_disk_commit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_write_file(
    disk: *mut RmnDisk,
    volume: usize,
    path: *const c_char,
    bytes: *const u8,
    length: usize,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_out, &error) };
        return false;
    };
    if bytes.is_null() && length > 0 {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_out, &error) };
        return false;
    }
    let contents = if length == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    match disk.disk.write_file(volume, path.as_ref(), contents) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            false
        }
    }
}

/// Creates a directory in volume `volume`. Buffered until commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_make_directory(
    disk: *mut RmnDisk,
    volume: usize,
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_out, &error) };
        return false;
    };
    match disk.disk.make_directory(volume, path.as_ref()) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            false
        }
    }
}

/// The commit point (P2): everything buffered reaches the image, then a
/// flush. Until this call, nothing has touched the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_commit(
    disk: *mut RmnDisk,
    error_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out) };
    let Some(disk) = (unsafe { disk.as_mut() }) else {
        return false;
    };
    match disk.disk.commit() {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            false
        }
    }
}

/// Discards everything buffered; the image is untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_disk_rollback(disk: *mut RmnDisk) {
    if let Some(disk) = unsafe { disk.as_mut() } {
        disk.disk.rollback();
    }
}

/// Which P7 mode the session's open obtained on its source file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_access_mode(
    session: *const RmnSession,
) -> RmnAccessMode {
    unsafe { session.as_ref() }.map_or(RmnAccessMode::ReadOnly, |session| {
        access_mode(session.session.access_mode())
    })
}

/// Reads a cataloged HDOS file's contents out of raw image bytes. Free
/// with `rmn_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_read_hdos_file(
    bytes: *const u8,
    length: usize,
    name: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut RmnFileData {
    unsafe { clear_error(error_out) };
    if bytes.is_null() {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    }
    let Some(name) = (unsafe { utf8_arg(name) }) else {
        let error = remanence::Error::io("null name");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    };
    let image = unsafe { std::slice::from_raw_parts(bytes, length) };
    match read_hdos_file(image, name.as_ref()) {
        Ok(bytes) => Box::into_raw(Box::new(RmnFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Reads a cataloged HDOS file out of a session's image bytes. Free with
/// `rmn_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmn_session_read_hdos_file(
    session: *const RmnSession,
    name: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut RmnFileData {
    unsafe { clear_error(error_out) };
    let Some(session) = (unsafe { session.as_ref() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    };
    let Some(name) = (unsafe { utf8_arg(name) }) else {
        let error = remanence::Error::io("null name");
        unsafe { set_error(error_out, &error) };
        return ptr::null_mut();
    };
    match read_hdos_file(session.session.bytes(), name.as_ref()) {
        Ok(bytes) => Box::into_raw(Box::new(RmnFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_out, &error) };
            ptr::null_mut()
        }
    }
}
