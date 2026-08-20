// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The remanence image: the flux family's physical stratum, and the
//! `.remanence` artifact it is read from and written to. The model
//! beneath the root — orbits' points, magnetization, write geometry —
//! does not cross this boundary; what crosses is the image's shape.
//!
//! The core's own name for the root is `FluxImage`; C prefixes it, as it
//! prefixes every exported type, giving `RemanenceFluxImage`. The alias
//! keeps both spellings honest inside this module.

use crate::abi::{
    RemanenceAccessMode, RemanenceErrorCategory, access_mode, clear_error, set_error, to_cstring,
};
use remanence::{FluxImage as PhysicalImage, FluxImageReport, FluxWriteReport};
use std::ffi::{CStr, CString, c_char};
use std::ptr;

/// One index hole, as the image holds it: an exact fraction of a turn
/// for the centre and another for the extent. Nothing radial is stored.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceFluxHole {
    pub center_numerator: u64,
    pub center_denominator: u64,
    pub extent_numerator: u64,
    pub extent_denominator: u64,
}

/// One orbit's identity and shape — never its points.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceFluxOrbit {
    pub surface: u64,
    /// The centre radius of the recorded band, in whole microns.
    pub radius_microns: u64,
    pub points: u64,
    /// How many carry a sense a reversal can be drawn from.
    pub coherent_points: u64,
    /// How many spans the image declines to read.
    pub unaligned_spans: u64,
}

pub(crate) struct ImageView {
    path: CString,
    format_id: CString,
    format_name: CString,
    form_factor: CString,
    provenance: Vec<CString>,
}

impl ImageView {
    pub(crate) fn new(image: &PhysicalImage, report: &FluxImageReport) -> Self {
        Self {
            path: to_cstring(
                &image
                    .path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            format_id: to_cstring(image.format_id()),
            format_name: to_cstring(image.format_name()),
            form_factor: to_cstring(&report.form_factor),
            provenance: report
                .provenance
                .iter()
                .map(|line| to_cstring(line))
                .collect(),
        }
    }
}

/// An opened remanence image, holding its claim on the artifact and the
/// points it decoded into private session storage.
pub struct RemanenceFluxImage {
    pub(crate) image: PhysicalImage,
    report: FluxImageReport,
    view: ImageView,
}

/// What writing an image into a `.remanence` artifact carried.
pub struct RemanenceFluxWriteReport {
    report: FluxWriteReport,
    path: CString,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
}

pub(crate) unsafe fn open_remanence_image(
    path: *const c_char,
    cache_bytes: Option<u64>,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFluxImage {
    unsafe { clear_error(error_out, error_rule_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    let opened = match cache_bytes {
        Some(cache_bytes) => PhysicalImage::open_with_cache(path.as_ref(), cache_bytes),
        None => PhysicalImage::open(path.as_ref()),
    };
    match opened {
        Ok(image) => {
            let report = image.inspect();
            let view = ImageView::new(&image, &report);
            Box::into_raw(Box::new(RemanenceFluxImage {
                image,
                report,
                view,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Opens the `.remanence` artifact at `path` (UTF-8), claiming the file
/// and decoding the whole image once into private session storage. The
/// magic, the binary sentinel and the layout version are checked before
/// anything else is believed, and a version past this release's claim is
/// refused by name. Returns null on failure and stores a message in
/// `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_open(
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFluxImage {
    unsafe { open_remanence_image(path, None, error_category_out, error_out, error_rule_out) }
}

/// Opens a remanence image as `remanence_flux_image_open` does, under a
/// declared cache bound: at most `cache_bytes` of the decoded image
/// stays resident. The bound narrows the working set; it never refuses
/// service.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_open_with_cache(
    path: *const c_char,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFluxImage {
    unsafe {
        open_remanence_image(
            path,
            Some(cache_bytes),
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Frees an image handle, releasing its claim on the artifact and
/// discarding the private session storage its points decoded into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_free(image: *mut RemanenceFluxImage) {
    if !image.is_null() {
        drop(unsafe { Box::from_raw(image) });
    }
}

/// The artifact the image was opened from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_path(
    image: *const RemanenceFluxImage,
) -> *const c_char {
    unsafe { image.as_ref() }.map_or(ptr::null(), |image| image.view.path.as_ptr())
}

/// The artifact format's stable identifier: `"remanence"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_format_id(
    image: *const RemanenceFluxImage,
) -> *const c_char {
    unsafe { image.as_ref() }.map_or(ptr::null(), |image| image.view.format_id.as_ptr())
}

/// That format's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_format_name(
    image: *const RemanenceFluxImage,
) -> *const c_char {
    unsafe { image.as_ref() }.map_or(ptr::null(), |image| image.view.format_name.as_ptr())
}

/// Which P7 mode the open obtained on the artifact.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_access_mode(
    image: *const RemanenceFluxImage,
) -> RemanenceAccessMode {
    unsafe { image.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |image| {
        image
            .image
            .access_mode()
            .map_or(RemanenceAccessMode::ReadOnly, access_mode)
    })
}

/// The medium's shape in the model's own spelling: `"8-inch"`,
/// `"5.25-inch"` or `"3.5-inch"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_form_factor(
    image: *const RemanenceFluxImage,
) -> *const c_char {
    unsafe { image.as_ref() }.map_or(ptr::null(), |image| image.view.form_factor.as_ptr())
}

/// The angular unit every angle in the image is stated over — a unit
/// rather than a measurement, so equality is exact.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_angular_divisions(
    image: *const RemanenceFluxImage,
) -> u64 {
    unsafe { image.as_ref() }.map_or(0, |image| image.report.angular_divisions)
}

/// How many bytes of private session storage the decoded points occupy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_backing_bytes(
    image: *const RemanenceFluxImage,
) -> u64 {
    unsafe { image.as_ref() }.map_or(0, |image| image.image.backing_bytes())
}

/// How much of that backing is currently resident. The points are never
/// held whole.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_resident_bytes(
    image: *const RemanenceFluxImage,
) -> u64 {
    unsafe { image.as_ref() }.map_or(0, |image| image.image.resident_bytes())
}

/// How many index holes the image holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_hole_count(
    image: *const RemanenceFluxImage,
) -> usize {
    unsafe { image.as_ref() }.map_or(0, |image| image.report.holes.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_hole(
    image: *const RemanenceFluxImage,
    index: usize,
    out: *mut RemanenceFluxHole,
) -> bool {
    let Some(image) = (unsafe { image.as_ref() }) else {
        return false;
    };
    let Some(hole) = image.report.holes.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceFluxHole {
                center_numerator: hole.center_numerator,
                center_denominator: hole.center_denominator,
                extent_numerator: hole.extent_numerator,
                extent_denominator: hole.extent_denominator,
            };
        }
    }
    true
}

/// How many surfaces carry orbits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_surface_count(
    image: *const RemanenceFluxImage,
) -> usize {
    unsafe { image.as_ref() }.map_or(0, |image| image.report.surfaces.len())
}

/// One surface's index, written into `out`, ascending. Returns false
/// when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_surface(
    image: *const RemanenceFluxImage,
    index: usize,
    out: *mut u64,
) -> bool {
    let Some(image) = (unsafe { image.as_ref() }) else {
        return false;
    };
    let Some(surface) = image.report.surfaces.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe { *out = *surface };
    }
    true
}

/// How many orbits the image holds, across every surface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_orbit_count(
    image: *const RemanenceFluxImage,
) -> usize {
    unsafe { image.as_ref() }.map_or(0, |image| image.report.orbits.len())
}

/// One of them, written into `out`, ordered by surface then radius.
/// Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_orbit(
    image: *const RemanenceFluxImage,
    index: usize,
    out: *mut RemanenceFluxOrbit,
) -> bool {
    let Some(image) = (unsafe { image.as_ref() }) else {
        return false;
    };
    let Some(orbit) = image.report.orbits.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceFluxOrbit {
                surface: orbit.surface,
                radius_microns: orbit.radius_microns,
                points: orbit.points,
                coherent_points: orbit.coherent_points,
                unaligned_spans: orbit.unaligned_spans,
            };
        }
    }
    true
}

/// How the image came to be known, in human-readable terms.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_provenance_count(
    image: *const RemanenceFluxImage,
) -> usize {
    unsafe { image.as_ref() }.map_or(0, |image| image.view.provenance.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_provenance(
    image: *const RemanenceFluxImage,
    index: usize,
) -> *const c_char {
    unsafe { image.as_ref() }.map_or(ptr::null(), |image| {
        image
            .view
            .provenance
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// Writes the image into a new `.remanence` artifact at `path` (UTF-8)
/// and reports what the artifact carried. An existing destination is a
/// named refusal rather than an overwrite, and an interruption leaves
/// the destination absent rather than half an artifact. Returns null on
/// failure; free the report with `remanence_flux_write_report_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_write(
    image: *const RemanenceFluxImage,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFluxWriteReport {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(image) = (unsafe { image.as_ref() }) else {
        let error = remanence::Error::io("null image");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let destination = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match image.image.write(destination.as_ref()) {
        Ok(report) => {
            let path = to_cstring(&report.path);
            let loss_codes = report
                .declared_loss
                .iter()
                .map(|loss| to_cstring(&loss.code))
                .collect();
            let loss_details = report
                .declared_loss
                .iter()
                .map(|loss| to_cstring(&loss.detail))
                .collect();
            Box::into_raw(Box::new(RemanenceFluxWriteReport {
                report,
                path,
                loss_codes,
                loss_details,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a write report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_free(report: *mut RemanenceFluxWriteReport) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

/// Where the artifact was written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_path(
    report: *const RemanenceFluxWriteReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.path.as_ptr())
}

/// The artifact's size on storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_artifact_bytes(
    report: *const RemanenceFluxWriteReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.artifact_bytes)
}

/// How many orbits it carried.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_orbits(
    report: *const RemanenceFluxWriteReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.orbits)
}

/// Every point across every orbit it carried.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_points(
    report: *const RemanenceFluxWriteReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.points)
}

/// How many kinds of loss the crossing did not carry. Zero for this
/// format, always: the remanence artifact is the model's own, so it
/// carries every fact the image holds. An empty account is the claim,
/// not a missing one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_declared_loss_count(
    report: *const RemanenceFluxWriteReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.report.declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_declared_loss_code(
    report: *const RemanenceFluxWriteReport,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the source's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_declared_loss_detail(
    report: *const RemanenceFluxWriteReport,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_write_report_declared_loss_amount(
    report: *const RemanenceFluxWriteReport,
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
