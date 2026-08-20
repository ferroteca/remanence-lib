// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The static catalogues a caller can enumerate before it holds anything:
//! image formats, new-media kinds, partition schemes and types, assurance
//! conditions and geometry sources.

use crate::abi::{to_cstring, utf8_arg};
use remanence::{DeviceType, Format, NewMedia, PartitionScheme, PartitionType};
use std::ffi::{CString, c_char};
use std::ptr;

/// How many concrete formats a load may declare.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_format_count() -> usize {
    format_views().len()
}

/// One declarable format's stable spelling (`qcow2`, `7z`), by index, or
/// null out of range. Owned by the library; do not free.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_format_id(index: usize) -> *const c_char {
    format_views()
        .get(index)
        .map_or(ptr::null(), |view| view.id.as_ptr())
}

/// That format's name, fit to show a user, or null out of range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_format_name(index: usize) -> *const c_char {
    format_views()
        .get(index)
        .map_or(ptr::null(), |view| view.name.as_ptr())
}

pub(crate) struct FormatView {
    id: CString,
    name: CString,
    /// The device types this format's adapter records — what a
    /// declaration may name, and what a refusal quotes back.
    devices: Vec<CString>,
    block_bytes: bool,
    collection: bool,
}

pub(crate) fn format_views() -> &'static [FormatView] {
    static FORMATS: std::sync::OnceLock<Vec<FormatView>> = std::sync::OnceLock::new();
    FORMATS.get_or_init(|| {
        Format::claimed()
            .iter()
            .map(|claim| FormatView {
                id: to_cstring(claim.id()),
                name: to_cstring(claim.name()),
                devices: claim
                    .devices()
                    .iter()
                    .map(|device| to_cstring(device.id()))
                    .collect(),
                block_bytes: claim.takes_block_bytes(),
                collection: claim.takes_collection(),
            })
            .collect()
    })
}

/// How many device types format `index` records: one where the format
/// carries it bare, several where the load declares which, and zero for
/// an archive grammar, which records no device at all.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_format_device_count(index: usize) -> usize {
    format_views()
        .get(index)
        .map_or(0, |view| view.devices.len())
}

/// The stable spelling of the `device`th device type format `index`
/// records — a value `remanence_session_load_media` accepts for it.
/// Null when either index is out of range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_format_device(index: usize, device: usize) -> *const c_char {
    format_views()
        .get(index)
        .and_then(|view| view.devices.get(device))
        .map_or(ptr::null(), |device| device.as_ptr())
}

/// Builds a load declaration out of the stable spellings a C caller
/// has: the format, the device type it records (null where the format
/// carries one bare), and the block size (zero where the format records
/// its own). Every half refuses by name on its own terms.
pub(crate) unsafe fn declared_format(
    format: &str,
    device_type: *const c_char,
    block_bytes: u64,
) -> Result<Format, remanence::Error> {
    let device = match unsafe { utf8_arg(device_type) } {
        Some(device) => Some(DeviceType::from_id(device.as_ref())?),
        None => None,
    };
    Format::declared(format, device, (block_bytes > 0).then_some(block_bytes))
}

/// Whether a declaration of format `index` carries the block size —
/// true for the raw reading alone, which records no addressable unit of
/// its own.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_format_takes_block_bytes(index: usize) -> bool {
    format_views()
        .get(index)
        .is_some_and(|view| view.block_bytes)
}

/// Whether format `index` reads a collection of sources rather than one
/// artifact — true for the KryoFlux capture set alone, which is one
/// disk spread over a stream per head per drive-step position. A
/// collection format loads through
/// `remanence_session_load_media_collection` or
/// `remanence_session_load_media_sources`; every other format loads one
/// source.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_format_takes_collection(index: usize) -> bool {
    format_views()
        .get(index)
        .is_some_and(|view| view.collection)
}

pub(crate) struct NewMediaView {
    id: CString,
    name: CString,
    article: CString,
    geometry: bool,
}

pub(crate) fn new_media_views() -> &'static [NewMediaView] {
    static KINDS: std::sync::OnceLock<Vec<NewMediaView>> = std::sync::OnceLock::new();
    KINDS.get_or_init(|| {
        NewMedia::claimed()
            .iter()
            .map(|claim| NewMediaView {
                id: to_cstring(claim.id()),
                name: to_cstring(claim.name()),
                article: to_cstring(claim.article()),
                geometry: claim.takes_geometry(),
            })
            .collect()
    })
}

/// How many kinds of blank medium this release authors.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_new_media_count() -> usize {
    new_media_views().len()
}

/// One authored kind's stable spelling (`chs-disk`, `flexible-5.25-soft`),
/// by index, or null out of range. Owned by the library; do not free.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_new_media_id(index: usize) -> *const c_char {
    new_media_views()
        .get(index)
        .map_or(ptr::null(), |view| view.id.as_ptr())
}

/// That kind's name, fit to show a user, or null out of range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_new_media_name(index: usize) -> *const c_char {
    new_media_views()
        .get(index)
        .map_or(ptr::null(), |view| view.name.as_ptr())
}

/// The article a medium of kind `index` is, by the article catalog's own
/// stable spelling — the manufactured substrate for a blank article kind,
/// and `authored` where no manufactured one stands behind it. Null out of
/// range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_new_media_article(index: usize) -> *const c_char {
    new_media_views()
        .get(index)
        .map_or(ptr::null(), |view| view.article.as_ptr())
}

/// Whether a declaration of kind `index` carries the recording's
/// coordinates — true for the CHS disk alone, which is the kind whose
/// facts *are* coordinates. Every other kind is a blank article and takes
/// zeros.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_new_media_takes_geometry(index: usize) -> bool {
    new_media_views()
        .get(index)
        .is_some_and(|view| view.geometry)
}

/// How many assurance conditions this release claims.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_assurance_condition_count() -> usize {
    remanence::AssuranceCondition::ALL.len()
}

/// One claimed condition's stable identity, or null when the index is out
/// of range. The set is enumerated (P3), so a caller can hold every
/// identity it may meet without waiting to meet one.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_assurance_condition_name(index: usize) -> *const c_char {
    static NAMES: std::sync::OnceLock<Vec<CString>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            remanence::AssuranceCondition::ALL
                .iter()
                .map(|condition| to_cstring(condition.as_str()))
                .collect()
        })
        .get(index)
        .map_or(ptr::null(), |name| name.as_ptr())
}

/// How many geometry sources this release reads.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_geometry_source_count() -> usize {
    remanence::GeometrySource::ALL.len()
}

/// One claimed source's stable identity, or null when the index is out
/// of range. The set is enumerated (P3), so a caller can hold every
/// identity it may meet without waiting to meet one.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_geometry_source_name(index: usize) -> *const c_char {
    static NAMES: std::sync::OnceLock<Vec<CString>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            remanence::GeometrySource::ALL
                .iter()
                .map(|source| to_cstring(source.as_str()))
                .collect()
        })
        .get(index)
        .map_or(ptr::null(), |name| name.as_ptr())
}

// ------------------------------------------------ the claimed vocabulary

/// A claimed enumerand's two spellings: the stable one that crosses the
/// boundary and the one fit to show a user.
pub(crate) struct SpellingView {
    pub(crate) id: CString,
    name: CString,
}

pub(crate) fn scheme_spellings() -> &'static [SpellingView] {
    static SCHEMES: std::sync::OnceLock<Vec<SpellingView>> = std::sync::OnceLock::new();
    SCHEMES.get_or_init(|| {
        PartitionScheme::ALL
            .iter()
            .map(|scheme| SpellingView {
                id: to_cstring(scheme.id()),
                name: to_cstring(scheme.name()),
            })
            .collect()
    })
}

pub(crate) fn partition_type_spellings() -> &'static [SpellingView] {
    static TYPES: std::sync::OnceLock<Vec<SpellingView>> = std::sync::OnceLock::new();
    TYPES.get_or_init(|| {
        PartitionType::ALL
            .iter()
            .map(|declared| SpellingView {
                id: to_cstring(declared.id()),
                name: to_cstring(declared.name()),
            })
            .collect()
    })
}

/// How many partition schemes this release reads (P16).
#[unsafe(no_mangle)]
pub extern "C" fn remanence_partition_scheme_count() -> usize {
    PartitionScheme::ALL.len()
}

/// One read scheme's stable spelling (`mbr`), by index, or null out of
/// range. The set is enumerated, so a caller can hold every spelling it
/// may meet without waiting to meet one. Owned by the library; do not
/// free.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_partition_scheme_id(index: usize) -> *const c_char {
    scheme_spellings()
        .get(index)
        .map_or(ptr::null(), |spelling| spelling.id.as_ptr())
}

/// That scheme's name, fit to show a user, or null out of range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_partition_scheme_name(index: usize) -> *const c_char {
    scheme_spellings()
        .get(index)
        .map_or(ptr::null(), |spelling| spelling.name.as_ptr())
}

/// How many readings of a partition's type value a declaration may name
/// (P3).
#[unsafe(no_mangle)]
pub extern "C" fn remanence_partition_type_count() -> usize {
    PartitionType::ALL.len()
}

/// One declarable reading's stable spelling (`dos-primary`), by index —
/// the value passed to `remanence_partition_check_type` — or null out of
/// range. Owned by the library; do not free.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_partition_type_id(index: usize) -> *const c_char {
    partition_type_spellings()
        .get(index)
        .map_or(ptr::null(), |spelling| spelling.id.as_ptr())
}

/// What that reading names, in a sentence fit to show a user beside the
/// value a partition records, or null out of range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_partition_type_name(index: usize) -> *const c_char {
    partition_type_spellings()
        .get(index)
        .map_or(ptr::null(), |spelling| spelling.name.as_ptr())
}
