// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Files inside a filesystem: opening, reading, writing, resizing, and the
//! load sources a space offers.

use crate::abi::{RemanenceErrorCategory, clear_error, set_error, to_cstring, utf8_arg};
use crate::discovery::RemanenceDiscovery;
use crate::storage::entries::{EntryView, RemanenceEntryKind, RemanenceEntryList, entry_kind};
use crate::storage::space::{RemanenceFile, RemanenceSpace, with_space};
use remanence::FileSource;
use std::ffi::{CString, c_char};
use std::ptr;

/// Bytes read out of a volume or catalog.
pub struct RemanenceFileData {
    bytes: Vec<u8>,
}

/// Answers one path (U3): a one-entry listing when something is there, an
/// empty listing when nothing is — a missing leaf, a missing parent, or a
/// parent that is a file alike. Absence is an answer, distinguished from
/// failure, which returns null with the error set. Free with
/// `remanence_entry_list_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_stat(
    filesystem: *const RemanenceSpace,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceEntryList {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match unsafe { with_space(handle.origin(), |target| target.stat(path.as_ref())) } {
        Ok(entry) => {
            let entries = entry.iter().map(EntryView::new).collect();
            Box::into_raw(Box::new(RemanenceEntryList { entries }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The file at `path`, or null with the refusal set.
///
/// This is where absence stops being an answer: `remanence_filesystem_stat`
/// asks whether something is there, and this asks for the file, so nothing
/// and a directory are both refused by name. Free with
/// `remanence_file_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_get_file(
    filesystem: *const RemanenceSpace,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFile {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match unsafe {
        with_space(handle.origin(), |target| {
            let file = target.get_file(path.as_ref())?;
            Ok((
                file.name().to_owned(),
                entry_kind(file.entry().kind),
                file.size_bytes(),
            ))
        })
    } {
        Ok((name, kind, size_bytes)) => Box::into_raw(Box::new(RemanenceFile {
            session: handle.session,
            media: handle.media,
            sectors: handle.sectors,
            ibm_sectors: handle.ibm_sectors,
            ordinal: handle.ordinal,
            declared: handle.declared.clone(),
            path: to_cstring(path.as_ref()),
            name: to_cstring(&name),
            kind,
            size_bytes,
        })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Opens the file at `path` as an artifact of its own, answering with
/// the discovery a device loads it from.
///
/// **Recursion is the same journey again.** An entry recognized as an
/// image is not read through the namespace that names it: it is loaded
/// into a device of its own — in a machine of its own where one is being
/// reconstructed, the host's archive never having been part of the
/// machine whose disk it holds. The claim is the one the archive already
/// holds, so nothing is re-opened.
///
/// This release mints a discovery from an **archive entry**; a file on a
/// volume-backed filesystem is refused by name. Free the result with
/// `remanence_discovery_free`, or consume it with
/// `remanence_session_load_discovery`. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_discover(
    filesystem: *const RemanenceSpace,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDiscovery {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let discovered = unsafe {
        with_space(handle.origin(), |target| {
            target.get_file(path.as_ref())?.discover()
        })
    };
    match discovered {
        Ok(discovery) => Box::into_raw(Box::new(RemanenceDiscovery::new(discovery))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Copies a file's bytes out — the whole-value convenience beside
/// `remanence_file_read_at`. Free with `remanence_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_read_file(
    filesystem: *const RemanenceSpace,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFileData {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match unsafe { with_space(handle.origin(), |target| target.read_file(path.as_ref())) } {
        Ok(bytes) => Box::into_raw(Box::new(RemanenceFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Sets a file's size, creating it when absent: kept bytes preserved in
/// place, a grown region reads as zeros. Buffered until commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_resize_file(
    filesystem: *const RemanenceSpace,
    path: *const c_char,
    size: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match unsafe {
        with_space(handle.origin(), |target| {
            target.resize_file(path.as_ref(), size)
        })
    } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Writes a file. An existing file is overwritten — shorter or longer,
/// its old clusters released and reclaimed — while an existing directory
/// is refused. Buffered until `remanence_device_commit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_write_file(
    filesystem: *const RemanenceSpace,
    path: *const c_char,
    bytes: *const u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if bytes.is_null() && length > 0 {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let contents = if length == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    match unsafe {
        with_space(handle.origin(), |target| {
            target.write_file(path.as_ref(), contents)
        })
    } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Ensures a directory exists: missing parents are created, and a path
/// that already leads to one succeeds unchanged. Buffered until commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_make_directory(
    filesystem: *const RemanenceSpace,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match unsafe {
        with_space(handle.origin(), |target| {
            target.make_directory(path.as_ref())
        })
    } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Frees a file handle. Nothing it was a view of is disturbed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_free(file: *mut RemanenceFile) {
    if !file.is_null() {
        drop(unsafe { Box::from_raw(file) });
    }
}

/// The path this file was reached by.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_path(file: *const RemanenceFile) -> *const c_char {
    unsafe { file.as_ref() }.map_or(ptr::null(), |file| file.path.as_ptr())
}

/// The name as the filesystem stores it, which is not always the
/// spelling the caller asked by.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_name(file: *const RemanenceFile) -> *const c_char {
    unsafe { file.as_ref() }.map_or(ptr::null(), |file| file.name.as_ptr())
}

/// What the filesystem claims this file's size is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_size_bytes(file: *const RemanenceFile) -> u64 {
    unsafe { file.as_ref() }.map_or(0, |file| file.size_bytes)
}

/// What this entry is. Always a file — `remanence_filesystem_get_file`
/// refuses a directory by name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_kind(file: *const RemanenceFile) -> RemanenceEntryKind {
    unsafe { file.as_ref() }.map_or(RemanenceEntryKind::File, |file| file.kind)
}

/// The whole file, copied out. Free with `remanence_file_data_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_bytes(
    file: *const RemanenceFile,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFileData {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { file.as_ref() }) else {
        return ptr::null_mut();
    };
    let path = handle.path.to_string_lossy().into_owned();
    match unsafe { with_space(handle.origin(), |target| target.get_file(&path)?.bytes()) } {
        Ok(bytes) => Box::into_raw(Box::new(RemanenceFileData { bytes })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Reads exactly `length` bytes at `offset` into `buffer_out` — the
/// bounded streamed form beside `remanence_file_bytes`. The span must lie
/// within the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_read_at(
    file: *const RemanenceFile,
    offset: u64,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { file.as_ref() }) else {
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    let path = handle.path.to_string_lossy().into_owned();
    match unsafe {
        with_space(handle.origin(), |target| {
            target.get_file(&path)?.read_at(offset, buffer)
        })
    } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Writes `length` bytes at `offset` in place — the streamed form beside
/// `remanence_filesystem_write_file`. The span must lie within the file's
/// current size; `remanence_filesystem_resize_file` is what changes it.
/// Buffered until commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_write_at(
    file: *const RemanenceFile,
    offset: u64,
    bytes: *const u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { file.as_ref() }) else {
        return false;
    };
    if bytes.is_null() && length > 0 {
        let error = remanence::Error::io("null bytes");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let data = if length == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    let path = handle.path.to_string_lossy().into_owned();
    match unsafe {
        with_space(handle.origin(), |target| {
            target.get_file(&path)?.write_at(offset, data)
        })
    } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
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

/// One file taken from an archive medium's namespace as a load's source
/// — free-standing, riding the claim of the medium it came from. Free
/// with `remanence_file_source_free`, unless
/// `remanence_session_load_media_source` consumed it.
pub struct RemanenceFileSource {
    pub(crate) source: FileSource,
    name: CString,
}

/// Every file gathered under one namespace path as a load's sources.
/// Free with `remanence_file_source_list_free`, unless
/// `remanence_session_load_media_sources` consumed it.
pub struct RemanenceFileSourceList {
    pub(crate) sources: Vec<FileSource>,
    pub(crate) names: Vec<CString>,
}

/// This file taken as a load's source — what
/// `remanence_session_load_media_source` consumes.
///
/// The source is **free-standing**: it rides the claim of the medium it
/// came from, so the namespace walk that named the file ends here and
/// the load opens nothing twice. This release takes a load's source
/// from an archive's namespace alone — a file on a volume-backed
/// filesystem is read through the filesystem that names it, and refuses
/// here by name. Returns null on failure and stores a message in
/// `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_source(
    file: *mut RemanenceFile,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFileSource {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { file.as_ref() }) else {
        let error = remanence::Error::io("null file");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let path = handle.path.to_string_lossy().into_owned();
    match unsafe { with_space(handle.origin(), |target| target.get_file(&path)?.source()) } {
        Ok(source) => {
            let name = to_cstring(source.name());
            Box::into_raw(Box::new(RemanenceFileSource { source, name }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The name the namespace holds this source's file under. Owned by the
/// handle; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_source_name(
    source: *const RemanenceFileSource,
) -> *const c_char {
    unsafe { source.as_ref() }.map_or(ptr::null(), |source| source.name.as_ptr())
}

/// The file's size in bytes, as the namespace claims it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_source_size_bytes(
    source: *const RemanenceFileSource,
) -> u64 {
    unsafe { source.as_ref() }.map_or(0, |source| source.source.size())
}

/// Frees a source no load consumed, ending its ride on its medium's
/// claim.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_source_free(source: *mut RemanenceFileSource) {
    if !source.is_null() {
        drop(unsafe { Box::from_raw(source) });
    }
}

/// How many sources the gathering holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_source_list_count(
    list: *const RemanenceFileSourceList,
) -> usize {
    unsafe { list.as_ref() }.map_or(0, |list| list.sources.len())
}

/// The name the namespace holds the `index`th source's file under, or
/// null out of range. Owned by the handle; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_source_list_name(
    list: *const RemanenceFileSourceList,
    index: usize,
) -> *const c_char {
    unsafe { list.as_ref() }.map_or(ptr::null(), |list| {
        list.names
            .get(index)
            .map_or(ptr::null(), |name| name.as_ptr())
    })
}

/// Frees a gathering no load consumed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_file_source_list_free(list: *mut RemanenceFileSourceList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}
