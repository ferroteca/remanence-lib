// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C64 renditions — d64, g64 and p64: what each container carries, or
//! will carry, of one remanence image, under a claim stated before the
//! file exists. Each is answered by a `describe` that writes nothing and a
//! `write` that produces the artifact, both returning the same report.
//!
//! Reading any of them back is a session load like any other medium's —
//! the declared format "d64", "g64" or "p64" — so the reports here are the
//! rendition direction's account and no root of their own.

use crate::abi::{RemanenceErrorCategory, clear_error, set_error, to_cstring};
use crate::flux::image::RemanenceFluxImage;
use remanence::{D64Report, G64Report, P64Report};
use std::ffi::{CStr, CString, c_char};
use std::ptr;

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

pub(crate) struct P64View {
    format_id: CString,
    format_name: CString,
    profile_id: CString,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
    evidence: Vec<CString>,
}

impl P64View {
    pub(crate) fn new(report: &P64Report) -> Self {
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
            evidence: report
                .evidence
                .iter()
                .map(|line| to_cstring(line))
                .collect(),
        }
    }
}

/// What a container carried, or will carry, of one remanence image.
pub struct RemanenceP64Report {
    report: P64Report,
    view: P64View,
}

/// Frees a report handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_report_free(report: *mut RemanenceP64Report) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

/// A described rendition and a written artifact report the same thing,
/// so the accessors below serve both doors' reports alike.
pub(crate) unsafe fn p64_reported<'a>(
    report: *const RemanenceP64Report,
) -> Option<(&'a P64Report, &'a P64View)> {
    unsafe { report.as_ref() }.map(|report| (&report.report, &report.view))
}

/// The container format's stable identifier, "p64".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_format_id(
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(report) }.map_or(ptr::null(), |(_, view)| view.format_id.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_format_name(
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(report) }.map_or(ptr::null(), |(_, view)| view.format_name.as_ptr())
}

/// The container's declared format version.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_version(report: *const RemanenceP64Report) -> u32 {
    unsafe { p64_reported(report) }.map_or(0, |(report, _)| report.version)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_write_protected(report: *const RemanenceP64Report) -> bool {
    unsafe { p64_reported(report) }.is_some_and(|(report, _)| report.write_protected)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_double_sided(report: *const RemanenceP64Report) -> bool {
    unsafe { p64_reported(report) }.is_some_and(|(report, _)| report.double_sided)
}

/// The drive profile the container's own signature names, and the frame
/// that profile declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_profile_id(
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(report) }.map_or(ptr::null(), |(_, view)| view.profile_id.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_reference_clock_hz(
    report: *const RemanenceP64Report,
) -> u64 {
    unsafe { p64_reported(report) }.map_or(0, |(report, _)| report.reference_clock_hz)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_cycles_per_rotation(
    report: *const RemanenceP64Report,
) -> u64 {
    unsafe { p64_reported(report) }.map_or(0, |(report, _)| report.cycles_per_rotation)
}

/// How many half-tracks the container holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_half_track_count(
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(report) }.map_or(0, |(report, _)| report.half_tracks.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_half_track(
    report: *const RemanenceP64Report,
    index: usize,
    out: *mut RemanenceP64HalfTrack,
) -> bool {
    let Some((report, _)) = (unsafe { p64_reported(report) }) else {
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
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(report) }.map_or(0, |(report, _)| report.declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_code(
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(report) }.map_or(ptr::null(), |(_, view)| {
        view.loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the source's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_detail(
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(report) }.map_or(ptr::null(), |(_, view)| {
        view.loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_amount(
    report: *const RemanenceP64Report,
    index: usize,
) -> u64 {
    unsafe { p64_reported(report) }.map_or(0, |(report, _)| {
        report.declared_loss.get(index).map_or(0, |loss| loss.count)
    })
}

/// How the container was recognized and what this adapter claims of it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_evidence_count(report: *const RemanenceP64Report) -> usize {
    unsafe { p64_reported(report) }.map_or(0, |(_, view)| view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_evidence(
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(report) }.map_or(ptr::null(), |(_, view)| {
        view.evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// The strings a rendition report owns: where it was written, if it was,
/// and its declared-loss account.
pub(crate) struct RenditionView {
    path: Option<CString>,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
}

impl RenditionView {
    /// The destination's name, or null for a rendition computed and not
    /// written.
    pub(crate) fn path(&self) -> *const c_char {
        self.path.as_ref().map_or(ptr::null(), |path| path.as_ptr())
    }

    /// One loss entry's stable code, or null out of range.
    pub(crate) fn loss_code(&self, index: usize) -> *const c_char {
        self.loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    }

    /// One loss entry's detail, or null out of range.
    pub(crate) fn loss_detail(&self, index: usize) -> *const c_char {
        self.loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    }

    pub(crate) fn new(path: Option<&String>, loss: &[remanence::DeclaredLoss]) -> Self {
        Self {
            path: path.map(|path| to_cstring(path)),
            loss_codes: loss.iter().map(|loss| to_cstring(&loss.code)).collect(),
            loss_details: loss.iter().map(|loss| to_cstring(&loss.detail)).collect(),
        }
    }
}

/// One CBM DOS block, by the address the recording states for it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceD64Block {
    pub track: u8,
    pub sector: u8,
}

/// One half-track slot a g64 carries.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceG64HalfTrack {
    /// The g64's own slot: 0 is track 1, and the odd indices are the
    /// half-tracks between the whole ones.
    pub index: u64,
    /// How many channel bits the slot carries.
    pub bits: u64,
    /// Which of the 1541's four rates it was packed at.
    pub speed_zone: u8,
    /// Whether the orbit was clocked at its zone's nominal cell because
    /// its own measured figure was not a recording's.
    pub clocked_at_nominal: bool,
}

/// What a d64 rendition carried, or will carry, of one image.
pub struct RemanenceD64Report {
    report: D64Report,
    view: RenditionView,
}

/// What a g64 rendition carried, or will carry, of one image.
pub struct RemanenceG64Report {
    report: G64Report,
    view: RenditionView,
}

pub(crate) fn boxed_d64(report: D64Report) -> *mut RemanenceD64Report {
    let view = RenditionView::new(report.path.as_ref(), &report.declared_loss);
    Box::into_raw(Box::new(RemanenceD64Report { report, view }))
}

pub(crate) fn boxed_g64(report: G64Report) -> *mut RemanenceG64Report {
    let view = RenditionView::new(report.path.as_ref(), &report.declared_loss);
    Box::into_raw(Box::new(RemanenceG64Report { report, view }))
}

/// Computes the d64 this image renders to, writing nothing. Read it
/// before writing: the write adds nothing to the account. Returns null
/// on failure; free the report with `remanence_d64_report_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_describe_d64(
    image: *const RemanenceFluxImage,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceD64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(image) = (unsafe { image.as_ref() }) else {
        let error = remanence::Error::io("null image");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match image.image.describe_d64() {
        Ok(report) => boxed_d64(report),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Writes the image into a new d64 at `path` (UTF-8) and reports what
/// the artifact carried. The recording's own sectors are read by the
/// family's group code and laid into the CBM DOS 683-block grid;
/// nothing is repaired and nothing is rejected, and an incomplete disk
/// carries the error map. An existing destination is a named refusal
/// rather than an overwrite. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_write_d64(
    image: *const RemanenceFluxImage,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceD64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(image), false) = (unsafe { image.as_ref() }, path.is_null()) else {
        let error = remanence::Error::io("null image or path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match image.image.write_d64(path.as_ref()) {
        Ok(report) => boxed_d64(report),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a d64 report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_free(report: *mut RemanenceD64Report) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

/// Where the artifact was written, or null for a rendition computed and
/// not written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_path(
    report: *const RemanenceD64Report,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .view
            .path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr())
    })
}

/// What the artifact occupies on storage: 683 blocks, and the error map
/// beside them wherever the disk is incomplete.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_artifact_bytes(
    report: *const RemanenceD64Report,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.artifact_bytes)
}

/// How many blocks the recording yielded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_blocks_read(
    report: *const RemanenceD64Report,
) -> u32 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.blocks_read)
}

/// What the CBM DOS grid defines, which is 683 whatever was read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_blocks_defined(
    report: *const RemanenceD64Report,
) -> u32 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.blocks_defined)
}

/// Sectors whose header or data failed its own checksum — recorded and
/// left out, never repaired.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_failed_checksums(
    report: *const RemanenceD64Report,
) -> u32 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.failed_checksums)
}

/// How many blocks the recording did not yield.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_missing_count(
    report: *const RemanenceD64Report,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.missing.len())
}

/// One missing block, in grid order. False when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_missing(
    report: *const RemanenceD64Report,
    index: usize,
    out: *mut RemanenceD64Block,
) -> bool {
    let (Some(report), false) = (unsafe { report.as_ref() }, out.is_null()) else {
        return false;
    };
    let Some(block) = report.report.missing.get(index) else {
        return false;
    };
    unsafe {
        *out = RemanenceD64Block {
            track: block.track,
            sector: block.sector,
        };
    }
    true
}

/// How many kinds of loss the crossing did not carry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_declared_loss_count(
    report: *const RemanenceD64Report,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_declared_loss_code(
    report: *const RemanenceD64Report,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .view
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the image's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_declared_loss_detail(
    report: *const RemanenceD64Report,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .view
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_d64_report_declared_loss_amount(
    report: *const RemanenceD64Report,
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

/// Computes the g64 this image renders to, writing nothing. Returns null
/// on failure; free the report with `remanence_g64_report_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_describe_g64(
    image: *const RemanenceFluxImage,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceG64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(image) = (unsafe { image.as_ref() }) else {
        let error = remanence::Error::io("null image");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match image.image.describe_g64() {
        Ok(report) => boxed_g64(report),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Writes the image into a new g64 at `path` (UTF-8) and reports what
/// the artifact carried. Every on-grid orbit is clocked at its measured
/// cell — or at its zone's nominal where the measured figure is not a
/// recording's — and packed under the `GCR-1541` grammar, one speed
/// zone per half-track. An existing destination is a named refusal
/// rather than an overwrite. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_write_g64(
    image: *const RemanenceFluxImage,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceG64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(image), false) = (unsafe { image.as_ref() }, path.is_null()) else {
        let error = remanence::Error::io("null image or path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match image.image.write_g64(path.as_ref()) {
        Ok(report) => boxed_g64(report),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a g64 report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_free(report: *mut RemanenceG64Report) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

/// Where the artifact was written, or null for a rendition computed and
/// not written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_path(
    report: *const RemanenceG64Report,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .view
            .path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr())
    })
}

/// What the artifact occupies on storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_artifact_bytes(
    report: *const RemanenceG64Report,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.artifact_bytes)
}

/// How many half-track slots the artifact carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_half_track_count(
    report: *const RemanenceG64Report,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.half_tracks.len())
}

/// One carried half-track, ascending. False when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_half_track(
    report: *const RemanenceG64Report,
    index: usize,
    out: *mut RemanenceG64HalfTrack,
) -> bool {
    let (Some(report), false) = (unsafe { report.as_ref() }, out.is_null()) else {
        return false;
    };
    let Some(half_track) = report.report.half_tracks.get(index) else {
        return false;
    };
    unsafe {
        *out = RemanenceG64HalfTrack {
            index: half_track.index,
            bits: half_track.bits,
            speed_zone: half_track.speed_zone,
            clocked_at_nominal: half_track.clocked_at_nominal,
        };
    }
    true
}

/// How many kinds of loss the crossing did not carry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_declared_loss_count(
    report: *const RemanenceG64Report,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_declared_loss_code(
    report: *const RemanenceG64Report,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .view
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the image's own terms.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_declared_loss_detail(
    report: *const RemanenceG64Report,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .view
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_g64_report_declared_loss_amount(
    report: *const RemanenceG64Report,
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

/// Computes what a p64 will and will not carry of this image, writing
/// nothing. The report is the delivered P64 one, and is freed with
/// `remanence_p64_report_free`. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_describe_p64(
    image: *const RemanenceFluxImage,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceP64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(image) = (unsafe { image.as_ref() }) else {
        let error = remanence::Error::io("null image");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match image.image.describe_p64() {
        Ok(report) => {
            let view = P64View::new(&report);
            Box::into_raw(Box::new(RemanenceP64Report { report, view }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Writes the image into a new p64 at `path` (UTF-8) and reports what
/// the container carried: one multiply from angle to cycle over the
/// coherent points, an orbit with no pulse left absent rather than
/// written empty. An existing destination is a named refusal rather
/// than an overwrite. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_write_p64(
    image: *const RemanenceFluxImage,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceP64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(image), false) = (unsafe { image.as_ref() }, path.is_null()) else {
        let error = remanence::Error::io("null image or path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match image.image.write_p64(path.as_ref()) {
        Ok(report) => {
            let view = P64View::new(&report);
            Box::into_raw(Box::new(RemanenceP64Report { report, view }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}
