// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The medium's own facts and its byte-level doors: what it is, how big,
//! whether it is modified, and the sector and commit verbs.

use crate::abi::{
    RemanenceAccessMode, RemanenceDiskFormat, RemanenceErrorCategory, access_mode, clear_error,
    set_error,
};
use crate::session::RemanenceMedium;
use remanence::{AccessMode, DiskFormat};
use std::ffi::c_char;
use std::ptr;

/// This medium's identity in its session's pool.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_id(medium: *const RemanenceMedium) -> u64 {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle.id.value(),
        None => 0,
    }
}

/// Whether a device currently links this medium. An unlinked medium is
/// ordinary rather than idle: it is loaded, claimed, and answering.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_is_linked(medium: *const RemanenceMedium) -> bool {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle.medium().is_some_and(|medium| medium.is_linked()),
        None => false,
    }
}

/// The article this medium is (P14), by the catalog's stable spelling —
/// the physical substrate. Owned by the library; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_article(medium: *const RemanenceMedium) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .article
            .as_ref()
            .map_or(ptr::null(), |article| article.as_ptr()),
        None => ptr::null(),
    }
}

/// The device this medium's content was recorded by, by the device
/// catalog's stable spelling — or null where no device recorded it,
/// which is an archive's honest answer rather than a gap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_device_type(
    medium: *const RemanenceMedium,
) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .device_type
            .as_ref()
            .map_or(ptr::null(), |device| device.as_ptr()),
        None => ptr::null(),
    }
}

/// The artifact the medium was loaded from (the archive itself for an
/// image loaded out of one).
///
/// **Null where the caller's handle has no recoverable name** — a name
/// serves location alone, and a nameless handle is served everywhere that
/// does not need a neighbourhood.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_path(medium: *const RemanenceMedium) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr()),
        None => ptr::null(),
    }
}

/// The resolved image path (the entry name for archive inputs), or null
/// as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_image_path(
    medium: *const RemanenceMedium,
) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .image_path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr()),
        None => ptr::null(),
    }
}

/// The resolved image's size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_image_size_bytes(medium: *const RemanenceMedium) -> u64 {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .medium()
            .map(|medium| medium.image_size_bytes())
            .unwrap_or(0),
        None => 0,
    }
}

/// Reads `length` bytes of the resolved image at `offset` into
/// `buffer_out` — the bounded access form: the image streams from its
/// backing and is never resident whole. Returns false on failure and
/// stores a message in `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_read_at(
    medium: *const RemanenceMedium,
    offset: u64,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match medium.read_at(offset, buffer) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// The medium's **effective** access mode: the declared intent's
/// echo where the evidence supports it, and read-only where it does not
/// (P28). `remanence_assurance_access_mode` reports the same value beside
/// the reason for it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_mode(
    medium: *const RemanenceMedium,
) -> RemanenceAccessMode {
    unsafe { medium.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |handle| {
        access_mode(
            handle
                .medium()
                .map(|medium| medium.mode())
                .unwrap_or(AccessMode::ReadOnly),
        )
    })
}

/// The image container format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_format(
    medium: *const RemanenceMedium,
    format_out: *mut RemanenceDiskFormat,
) -> bool {
    let Some(format) =
        (unsafe { medium.as_ref() }).and_then(|handle| handle.medium()?.format().ok())
    else {
        return false;
    };
    if !format_out.is_null() {
        unsafe {
            *format_out = match format {
                DiskFormat::Qcow2 { .. } => RemanenceDiskFormat::Qcow2,
                DiskFormat::Vdi { .. } => RemanenceDiskFormat::Vdi,
                DiskFormat::Raw => RemanenceDiskFormat::Raw,
                DiskFormat::Imd => RemanenceDiskFormat::Imd,
            }
        };
    }
    true
}

/// The qcow2 version, or 0 for an image of any other format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_qcow2_version(medium: *const RemanenceMedium) -> u32 {
    match unsafe { medium.as_ref() }.and_then(|handle| handle.medium()?.format().ok()) {
        Some(DiskFormat::Qcow2 { version }) => version,
        _ => 0,
    }
}

/// The VDI version's major part, or 0 for an image of any other format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_vdi_version_major(medium: *const RemanenceMedium) -> u32 {
    match unsafe { medium.as_ref() }.and_then(|handle| handle.medium()?.format().ok()) {
        Some(DiskFormat::Vdi { major, .. }) => major,
        _ => 0,
    }
}

/// The VDI version's minor part, or 0 for an image of any other format.
/// Read it beside the major part: on its own, 0 is both "minor zero" and
/// "not a VDI".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_vdi_version_minor(medium: *const RemanenceMedium) -> u32 {
    match unsafe { medium.as_ref() }.and_then(|handle| handle.medium()?.format().ok()) {
        Some(DiskFormat::Vdi { minor, .. }) => minor,
        _ => 0,
    }
}

/// The virtual disk size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_size(medium: *const RemanenceMedium) -> u64 {
    unsafe { medium.as_ref() }
        .and_then(|handle| handle.medium()?.size().ok())
        .unwrap_or(0)
}

/// Whether uncommitted changes exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_is_modified(medium: *const RemanenceMedium) -> bool {
    unsafe { medium.as_ref() }
        .and_then(|handle| handle.medium())
        .is_some_and(|medium| medium.is_modified())
}

/// Reads one whole sector in the recording's own coordinates into
/// `buffer_out`, which is exactly one sector of this recording.
///
/// Cylinders and heads number from zero and sectors from one. It answers
/// on a sector-addressed recording whose geometry the evidence
/// established and refuses by name otherwise, the rule identity in
/// `error_rule_out` naming which: `not-sector-addressed`,
/// `geometry-unstated`, `geometry-undetermined`, `outside-geometry` or
/// `partial-sector`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_read_sector(
    medium: *mut RemanenceMedium,
    cylinder: u32,
    head: u32,
    sector: u32,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    match medium.read_sector(cylinder, head, sector, buffer) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Writes one whole sector in the recording's own coordinates,
/// **buffered until `remanence_medium_commit`** like every other write
/// (P2), under the same rules `remanence_medium_read_sector` answers by.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_write_sector(
    medium: *mut RemanenceMedium,
    cylinder: u32,
    head: u32,
    sector: u32,
    data: *const u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if data.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let data = unsafe { std::slice::from_raw_parts(data, length) };
    match medium.write_sector(cylinder, head, sector, data) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
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
pub unsafe extern "C" fn remanence_medium_commit(
    medium: *mut RemanenceMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_mut() }) else {
        return false;
    };
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match medium.commit() {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Discards everything buffered; the image is untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_rollback(medium: *mut RemanenceMedium) {
    if let Some(handle) = unsafe { medium.as_mut() } {
        if let Some(medium) = handle.medium() {
            let _ = medium.rollback();
        }
    }
}
