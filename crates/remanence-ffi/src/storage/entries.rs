// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Directory listings: the entries a filesystem answers, and the facts
//! each entry declares.

use crate::abi::{RemanenceErrorCategory, clear_error, set_error, to_cstring, utf8_arg};
use crate::storage::space::{RemanenceSpace, with_space};
use remanence::{Entry, EntryKind};
use std::ffi::{CString, c_char};
use std::ptr;

/// What a FAT directory entry is.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceEntryKind {
    File,
    Directory,
}

/// A directory listing.
pub struct RemanenceEntryList {
    pub(crate) entries: Vec<EntryView>,
}

pub(crate) struct EntryView {
    name: CString,
    kind: RemanenceEntryKind,
    size_bytes: u64,
    /// What the recognizing filesystem declares beyond the fields above,
    /// in its own spelling and order — HDOS's catalog date and flag
    /// letters are the delivered case.
    declared: Vec<(CString, CString)>,
}

impl EntryView {
    pub(crate) fn new(entry: &Entry) -> Self {
        Self {
            name: to_cstring(&entry.name),
            kind: entry_kind(entry.kind),
            size_bytes: entry.size_bytes,
            declared: entry
                .declared
                .iter()
                .map(|fact| (to_cstring(&fact.key), to_cstring(&fact.value)))
                .collect(),
        }
    }
}

pub(crate) fn entry_kind(kind: EntryKind) -> RemanenceEntryKind {
    match kind {
        EntryKind::File => RemanenceEntryKind::File,
        EntryKind::Directory => RemanenceEntryKind::Directory,
    }
}

/// Lists a directory ("" = root, "A/B" descends). Free with
/// `remanence_entry_list_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_entries(
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
    let path = unsafe { utf8_arg(path) }.unwrap_or_default();
    match unsafe { with_space(handle.origin(), |target| target.entries(path.as_ref())) } {
        Ok(entries) => {
            let entries = entries.iter().map(EntryView::new).collect();
            Box::into_raw(Box::new(RemanenceEntryList { entries }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a directory listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_list_free(list: *mut RemanenceEntryList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

pub(crate) unsafe fn entry_view<'a>(
    list: *const RemanenceEntryList,
    index: usize,
) -> Option<&'a EntryView> {
    unsafe { list.as_ref() }?.entries.get(index)
}

/// Number of entries in the listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_count(list: *const RemanenceEntryList) -> usize {
    unsafe { list.as_ref() }.map_or(0, |list| list.entries.len())
}

/// An entry's name, as the filesystem stores it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_name(
    list: *const RemanenceEntryList,
    index: usize,
) -> *const c_char {
    unsafe { entry_view(list, index) }.map_or(ptr::null(), |entry| entry.name.as_ptr())
}

/// Whether an entry is a file or a directory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_kind(
    list: *const RemanenceEntryList,
    index: usize,
) -> RemanenceEntryKind {
    unsafe { entry_view(list, index) }.map_or(RemanenceEntryKind::File, |entry| entry.kind)
}

/// An entry's size in bytes (0 for directories).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_size_bytes(
    list: *const RemanenceEntryList,
    index: usize,
) -> u64 {
    unsafe { entry_view(list, index) }.map_or(0, |entry| entry.size_bytes)
}

/// How many facts the recognizing filesystem declares about this entry
/// beyond name, kind and size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_declared_count(
    list: *const RemanenceEntryList,
    index: usize,
) -> usize {
    unsafe { entry_view(list, index) }.map_or(0, |entry| entry.declared.len())
}

/// One declared fact's key, as the recognizing filesystem spells it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_declared_key(
    list: *const RemanenceEntryList,
    index: usize,
    fact: usize,
) -> *const c_char {
    unsafe { entry_view(list, index) }
        .and_then(|entry| entry.declared.get(fact))
        .map_or(ptr::null(), |(key, _)| key.as_ptr())
}

/// One declared fact's value, as that filesystem reads it. Nothing is
/// normalized on the way through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_declared_value(
    list: *const RemanenceEntryList,
    index: usize,
    fact: usize,
) -> *const c_char {
    unsafe { entry_view(list, index) }
        .and_then(|entry| entry.declared.get(fact))
        .map_or(ptr::null(), |(_, value)| value.as_ptr())
}
