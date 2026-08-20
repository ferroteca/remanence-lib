// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The plumbing every other module's verbs are built from: the stable
//! refusal category, the error and string outs, and the small
//! conversions between the core's types and this ABI's.

use remanence::{AccessIntent, AccessMode, ErrorCategory};
use std::ffi::{CStr, CString, c_char};
use std::ptr;

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
    /// The artifact does not hold what was asked for, and no retry or
    /// permission change will produce it — a degraded session's withheld
    /// read (P28), never a host failure.
    Unavailable = 8,
    Io = 9,
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
            ErrorCategory::Unavailable => Self::Unavailable,
            ErrorCategory::Io => Self::Io,
        }
    }
}

pub(crate) fn to_cstring(value: &str) -> CString {
    CString::new(value.replace('\0', "\u{fffd}")).expect("interior NULs replaced")
}

pub(crate) fn to_owned_c_char(value: &str) -> *mut c_char {
    to_cstring(value).into_raw()
}

pub(crate) unsafe fn clear_error(error_out: *mut *mut c_char, rule_out: *mut *mut c_char) {
    if !error_out.is_null() {
        unsafe { *error_out = ptr::null_mut() };
    }
    if !rule_out.is_null() {
        unsafe { *rule_out = ptr::null_mut() };
    }
}

pub(crate) unsafe fn set_error(
    category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    rule_out: *mut *mut c_char,
    error: &remanence::Error,
) {
    if !category_out.is_null() {
        unsafe { *category_out = error.category().into() };
    }
    if !error_out.is_null() {
        unsafe { *error_out = to_owned_c_char(&error.to_string()) };
    }
    if !rule_out.is_null() {
        unsafe {
            *rule_out = match error.rule() {
                Some(rule) => to_owned_c_char(rule),
                None => ptr::null_mut(),
            }
        };
    }
}

pub(crate) unsafe fn write_opt_u64(value: Option<u64>, out: *mut u64) -> bool {
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

pub(crate) unsafe fn write_opt_u32(value: Option<u32>, out: *mut u32) -> bool {
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

/// The caller's declared intent when opening a disk (P7).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceAccessIntent {
    Read,
    Write,
}

/// A medium's effective access mode: the declared intent's echo
/// (P7) where the evidence supports it, read-only where it does not (P28).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemanenceAccessMode {
    ReadWrite,
    ReadOnly,
}

/// The image container format a disk image turned out to be.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDiskFormat {
    Raw,
    Qcow2,
    Vdi,
    Imd,
}

pub(crate) fn access_intent(intent: RemanenceAccessIntent) -> AccessIntent {
    match intent {
        RemanenceAccessIntent::Read => AccessIntent::Read,
        RemanenceAccessIntent::Write => AccessIntent::Write,
    }
}

pub(crate) fn access_mode(mode: AccessMode) -> RemanenceAccessMode {
    match mode {
        AccessMode::ReadWrite => RemanenceAccessMode::ReadWrite,
        AccessMode::ReadOnly => RemanenceAccessMode::ReadOnly,
    }
}

pub(crate) unsafe fn utf8_arg<'a>(value: *const c_char) -> Option<std::borrow::Cow<'a, str>> {
    if value.is_null() {
        return None;
    }
    Some(String::from_utf8_lossy(
        unsafe { CStr::from_ptr(value) }.to_bytes(),
    ))
}

/// Takes ownership of a caller-opened OS file handle.
///
/// On Windows this is a `HANDLE` from `CreateFile`; elsewhere it is a
/// file descriptor. The library owns it from here: it is closed when the
/// medium is released or the session is freed, and the caller must not
/// close it themselves.
#[cfg(windows)]
pub(crate) unsafe fn file_from_raw(source: isize) -> std::fs::File {
    use std::os::windows::io::FromRawHandle;
    unsafe { std::fs::File::from_raw_handle(source as *mut std::ffi::c_void) }
}

#[cfg(not(windows))]
pub(crate) unsafe fn file_from_raw(source: isize) -> std::fs::File {
    use std::os::fd::FromRawFd;
    unsafe { std::fs::File::from_raw_fd(source as i32) }
}

/// The error outs are this module's whole contract with a caller, so they
/// are checked here rather than through a verb that happens to fail.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::remanence_string_free;

    #[test]
    fn error_output_carries_category_beside_unchanged_message() {
        let error = remanence::Error::invalid_image("qcow2", "malformed");
        let mut category = RemanenceErrorCategory::Io;
        let mut message = ptr::null_mut();
        let mut rule = ptr::null_mut();

        unsafe { set_error(&mut category, &mut message, &mut rule, &error) };

        assert_eq!(category, RemanenceErrorCategory::InvalidImage);
        assert_eq!(
            unsafe { CStr::from_ptr(message) }.to_str().expect("UTF-8"),
            "invalid qcow2 disk image: malformed"
        );
        // A refusal belonging to no rule set reports none, and null is
        // that answer rather than an omission.
        assert!(rule.is_null());
        unsafe { remanence_string_free(message) };
    }

    #[test]
    fn error_output_carries_the_rule_identity_where_one_applies() {
        let mut category = RemanenceErrorCategory::Io;
        let mut message = ptr::null_mut();
        let mut rule = ptr::null_mut();
        let error = remanence::Error::io("'CON.TXT' names the reserved device 'CON'")
            .broke_rule(remanence::DosNameRule::ReservedDeviceName.as_str());

        unsafe { set_error(&mut category, &mut message, &mut rule, &error) };

        assert_eq!(
            unsafe { CStr::from_ptr(rule) }.to_str().expect("UTF-8"),
            "reserved-device-name"
        );
        unsafe { remanence_string_free(message) };
        unsafe { remanence_string_free(rule) };
    }
}
