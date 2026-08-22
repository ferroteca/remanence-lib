// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The raw rendition of a sector medium (`remanence_medium_*_raw`), and
//! the report it answers with.
//!
//! It is shaped exactly as the flux renditions beside it: a describe
//! verb that computes everything and writes nothing, a write verb that
//! produces the artifact, and one owned report carrying what the
//! destination could not hold (P29). The report is the caller's to free.

use std::ffi::{CStr, c_char};
use std::ptr;

use remanence::RawReport;

use crate::abi::{RemanenceErrorCategory, clear_error, set_error};
use crate::flux::rendition::RenditionView;
use crate::session::RemanenceMedium;

/// What a raw rendition carried, or will carry, of one medium.
pub struct RemanenceRawReport {
    report: RawReport,
    view: RenditionView,
}

fn boxed(report: RawReport) -> *mut RemanenceRawReport {
    let view = RenditionView::new(report.path.as_ref(), &report.declared_loss);
    Box::into_raw(Box::new(RemanenceRawReport { report, view }))
}

/// Computes the raw image this medium renders to, **writing nothing**.
///
/// Read it before writing: the write adds nothing to the account. Returns
/// null on failure; free the report with `remanence_raw_report_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_describe_raw(
    medium: *const RemanenceMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceRawReport {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        return ptr::null_mut();
    };
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released from its session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match medium.describe_raw() {
        Ok(report) => boxed(report),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Writes this medium into a new raw image at `path` and reports what
/// the artifact carried.
///
/// The sectors go in the recording's own order — cylinder-major,
/// head-minor, sectors from one — and nothing else does: raw is bytes
/// and no ecosystem, so what the medium says about itself is named in
/// the report's declared-loss account instead. The rendition is of
/// **committed** state, and the report says how many uncommitted extents
/// were left behind.
///
/// An existing destination is a named refusal rather than an overwrite,
/// and the artifact is moved into place whole, so an interruption leaves
/// the destination absent rather than half an image. Returns null on
/// failure; free the report with `remanence_raw_report_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_write_raw(
    medium: *const RemanenceMedium,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceRawReport {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(handle), false) = (unsafe { medium.as_ref() }, path.is_null()) else {
        let error = remanence::Error::io("null medium or path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes()).into_owned();
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released from its session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match medium.write_raw(&path) {
        Ok(report) => boxed(report),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a raw report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_free(report: *mut RemanenceRawReport) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

/// Where the artifact was written, or null for a rendition computed and
/// not written. Owned by the report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_path(
    report: *const RemanenceRawReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.view.path())
}

/// What the artifact occupies: every sector the coordinates address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_artifact_bytes(
    report: *const RemanenceRawReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.artifact_bytes)
}

/// How many sectors were written, in the coordinates below.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_sectors_written(
    report: *const RemanenceRawReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.sectors_written)
}

/// The coordinates the sectors were written in, into the four outs.
/// False for a null report, with nothing written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_geometry(
    report: *const RemanenceRawReport,
    cylinders_out: *mut u32,
    heads_out: *mut u32,
    sectors_per_track_out: *mut u32,
    sector_bytes_out: *mut u64,
) -> bool {
    let Some(report) = (unsafe { report.as_ref() }) else {
        return false;
    };
    let geometry = report.report.geometry;
    unsafe {
        if let Some(out) = cylinders_out.as_mut() {
            *out = geometry.cylinders;
        }
        if let Some(out) = heads_out.as_mut() {
            *out = geometry.heads;
        }
        if let Some(out) = sectors_per_track_out.as_mut() {
            *out = geometry.sectors_per_track;
        }
        if let Some(out) = sector_bytes_out.as_mut() {
            *out = geometry.sector_bytes;
        }
    }
    true
}

/// Cached extents holding writes the medium has not committed. They are
/// **not** in the artifact: a rendition is of committed state, and this
/// is what says how much was left behind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_uncommitted_extents(
    report: *const RemanenceRawReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.uncommitted_extents)
}

/// How many kinds of loss the destination declared.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_declared_loss_count(
    report: *const RemanenceRawReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_declared_loss_code(
    report: *const RemanenceRawReport,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.view.loss_code(index))
}

/// What was lost, in the medium's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_declared_loss_detail(
    report: *const RemanenceRawReport,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.view.loss_detail(index))
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_raw_report_declared_loss_amount(
    report: *const RemanenceRawReport,
    index: usize,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| {
        report
            .report
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}
