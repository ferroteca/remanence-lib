// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! A medium's cylinder/head/sector geometry: what settled, what the sources
//! disagreed about, and the reading each source offered.

use crate::abi::{to_cstring, write_opt_u32, write_opt_u64};
use crate::session::RemanenceMedium;
use std::ffi::{CString, c_char};
use std::ptr;

// ---------------------------------------------------------------------------
// Discovered geometry, and the recording's own coordinates.
//
// A medium's geometry is *read* when it is loaded and is evidence from
// then on: there is no verb here that declares one, because nothing is
// ever declared onto a medium that exists. What the surface carries is
// what the sources said — each reading with where it was taken — what
// they settled between them, and what they contradict each other about.
//
// `remanence_medium_read_sector` and `remanence_medium_write_sector` address
// in what that established, on the device types whose
// `remanence_device_slot_addressing` says `sector`. Everything else
// refuses by name, carrying one of this seam's rule identities in
// `error_rule_out`: `not-sector-addressed`, `geometry-unstated`,
// `geometry-undetermined`, `outside-geometry`, `partial-sector`.

/// What the evidence established about a medium's geometry.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemanenceGeometryState {
    /// No source beneath the medium states a whole geometry — an
    /// archive's answer, and a block image whose sources stayed silent.
    Unstated = 0,
    /// Every part is established and the readings agree.
    Determined = 1,
    /// Two sources state different values for the same part. Both
    /// readings stand and neither settles it.
    Undetermined = 2,
}

/// One source's own statement about the recording's coordinates, in the
/// C view: the strings are owned by the geometry that carries it.
pub(crate) struct GeometryReadingView {
    source: CString,
    at: CString,
    detail: CString,
    cylinders: Option<u32>,
    heads: Option<u32>,
    sectors_per_track: Option<u32>,
    sector_bytes: Option<u64>,
}

/// One medium's geometry as the evidence left it. Free it with
/// `remanence_geometry_free`; every string it returns is owned by it.
pub struct RemanenceGeometry {
    state: RemanenceGeometryState,
    determined: Option<remanence::RecordingGeometry>,
    conflicts: Vec<CString>,
    unsettled: Vec<CString>,
    readings: Vec<GeometryReadingView>,
}

/// The geometry the sources beneath this medium stated: what was
/// settled, what they contradict each other about, and every reading
/// taken.
///
/// It was established when the medium was loaded and is evidence from
/// then on — nothing re-reads a boot record behind a caller. Null only
/// once the medium itself has been released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_geometry(
    medium: *const RemanenceMedium,
) -> *mut RemanenceGeometry {
    let Some(medium) = (unsafe { medium.as_ref() }).and_then(RemanenceMedium::medium) else {
        return ptr::null_mut();
    };
    let geometry = medium.geometry();
    Box::into_raw(Box::new(RemanenceGeometry {
        state: match geometry.state() {
            remanence::GeometryState::Unstated => RemanenceGeometryState::Unstated,
            remanence::GeometryState::Determined => RemanenceGeometryState::Determined,
            remanence::GeometryState::Undetermined => RemanenceGeometryState::Undetermined,
        },
        determined: geometry.determined(),
        conflicts: geometry
            .conflicts()
            .iter()
            .map(|line| to_cstring(line))
            .collect(),
        unsettled: geometry
            .unsettled()
            .iter()
            .map(|part| to_cstring(part))
            .collect(),
        readings: geometry
            .readings()
            .iter()
            .map(|reading| GeometryReadingView {
                source: to_cstring(reading.source.as_str()),
                at: to_cstring(&reading.at),
                detail: to_cstring(&reading.detail),
                cylinders: reading.cylinders,
                heads: reading.heads,
                sectors_per_track: reading.sectors_per_track,
                sector_bytes: reading.sector_bytes,
            })
            .collect(),
    }))
}

/// Frees a geometry record and everything borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_free(geometry: *mut RemanenceGeometry) {
    if !geometry.is_null() {
        drop(unsafe { Box::from_raw(geometry) });
    }
}

/// What the evidence established.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_state(
    geometry: *const RemanenceGeometry,
) -> RemanenceGeometryState {
    unsafe { geometry.as_ref() }.map_or(RemanenceGeometryState::Unstated, |geometry| geometry.state)
}

/// The coordinates, where the evidence settled them: cylinders, heads,
/// sectors per track and bytes per sector, written to whichever outputs
/// are non-null. False where nothing settled them, leaving every output
/// untouched — the state says which of the two absences it is.
///
/// Cylinders and heads number from zero and sectors from one, which is
/// the recording's own convention.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_coordinates(
    geometry: *const RemanenceGeometry,
    cylinders_out: *mut u32,
    heads_out: *mut u32,
    sectors_per_track_out: *mut u32,
    sector_bytes_out: *mut u64,
) -> bool {
    let Some(coordinates) = (unsafe { geometry.as_ref() }).and_then(|geometry| geometry.determined)
    else {
        return false;
    };
    if !cylinders_out.is_null() {
        unsafe { *cylinders_out = coordinates.cylinders };
    }
    if !heads_out.is_null() {
        unsafe { *heads_out = coordinates.heads };
    }
    if !sectors_per_track_out.is_null() {
        unsafe { *sectors_per_track_out = coordinates.sectors_per_track };
    }
    if !sector_bytes_out.is_null() {
        unsafe { *sector_bytes_out = coordinates.sector_bytes };
    }
    true
}

/// How many parts of the coordinates the sources contradict each other
/// about.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_conflict_count(
    geometry: *const RemanenceGeometry,
) -> usize {
    unsafe { geometry.as_ref() }.map_or(0, |geometry| geometry.conflicts.len())
}

/// One conflict, naming both readings, or null when the index is out of
/// range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_conflict(
    geometry: *const RemanenceGeometry,
    index: usize,
) -> *const c_char {
    unsafe { geometry.as_ref() }
        .and_then(|geometry| geometry.conflicts.get(index))
        .map_or(ptr::null(), |line| line.as_ptr())
}

/// How many parts of the coordinates no source settled. Zero for a
/// determined geometry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_unsettled_count(
    geometry: *const RemanenceGeometry,
) -> usize {
    unsafe { geometry.as_ref() }.map_or(0, |geometry| geometry.unsettled.len())
}

/// One unsettled part, named the way the refusals name it, or null when
/// the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_unsettled(
    geometry: *const RemanenceGeometry,
    index: usize,
) -> *const c_char {
    unsafe { geometry.as_ref() }
        .and_then(|geometry| geometry.unsettled.get(index))
        .map_or(ptr::null(), |part| part.as_ptr())
}

/// How many readings were taken, in the order the sources were read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_count(
    geometry: *const RemanenceGeometry,
) -> usize {
    unsafe { geometry.as_ref() }.map_or(0, |geometry| geometry.readings.len())
}

pub(crate) fn reading_string(
    geometry: *const RemanenceGeometry,
    index: usize,
    read: fn(&GeometryReadingView) -> &CString,
) -> *const c_char {
    unsafe { geometry.as_ref() }
        .and_then(|geometry| geometry.readings.get(index))
        .map_or(ptr::null(), |reading| read(reading).as_ptr())
}

/// Reading `index`'s source, by its stable spelling —
/// `format-declaration`, `boot-record`, `partition-table` or
/// `extent-arithmetic`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_source(
    geometry: *const RemanenceGeometry,
    index: usize,
) -> *const c_char {
    reading_string(geometry, index, |reading| &reading.source)
}

/// Where in the artifact reading `index` was taken.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_at(
    geometry: *const RemanenceGeometry,
    index: usize,
) -> *const c_char {
    reading_string(geometry, index, |reading| &reading.at)
}

/// What reading `index`'s source states, in its own terms.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_detail(
    geometry: *const RemanenceGeometry,
    index: usize,
) -> *const c_char {
    reading_string(geometry, index, |reading| &reading.detail)
}

pub(crate) fn reading_part(
    geometry: *const RemanenceGeometry,
    index: usize,
    read: fn(&GeometryReadingView) -> Option<u32>,
) -> Option<u32> {
    unsafe { geometry.as_ref() }
        .and_then(|geometry| geometry.readings.get(index))
        .and_then(read)
}

/// The cylinder count reading `index` states. False where that source
/// states none, which is ordinary: a boot record states no cylinder
/// count at all.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_cylinders(
    geometry: *const RemanenceGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    unsafe {
        write_opt_u32(
            reading_part(geometry, index, |reading| reading.cylinders),
            out,
        )
    }
}

/// The head count reading `index` states. False where it states none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_heads(
    geometry: *const RemanenceGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    unsafe { write_opt_u32(reading_part(geometry, index, |reading| reading.heads), out) }
}

/// The sectors-per-track reading `index` states. False where it states
/// none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_sectors_per_track(
    geometry: *const RemanenceGeometry,
    index: usize,
    out: *mut u32,
) -> bool {
    unsafe {
        write_opt_u32(
            reading_part(geometry, index, |reading| reading.sectors_per_track),
            out,
        )
    }
}

/// The sector size reading `index` states. False where it states none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_geometry_reading_sector_bytes(
    geometry: *const RemanenceGeometry,
    index: usize,
    out: *mut u64,
) -> bool {
    unsafe {
        write_opt_u64(
            geometry
                .as_ref()
                .and_then(|geometry| geometry.readings.get(index))
                .and_then(|reading| reading.sector_bytes),
            out,
        )
    }
}
