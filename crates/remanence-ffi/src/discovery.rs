// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Looking at an artifact before loading it: what it appears to be, which
//! devices would accept it, and what a load would cost.

use crate::abi::{
    RemanenceAccessIntent, RemanenceAccessMode, RemanenceDiskFormat, RemanenceErrorCategory,
    access_intent, access_mode, clear_error, set_error, to_cstring, utf8_arg,
};
use crate::assurance::{RemanenceAssurance, assurance_view};
use crate::identify::{NestedLayerView, RemanenceIdentification};
use remanence::{DiskFormat, Identification};
use std::ffi::{CString, c_char};
use std::ptr;

/// What one artifact turned out to be, and the claim under which that was
/// established.
///
/// Free it with `remanence_discovery_free`, or hand it to
/// `remanence_session_load_discovery`, which consumes it. Every string it
/// returns is owned by it and freed with it.
pub struct RemanenceDiscovery {
    pub(crate) discovery: remanence::Discovery,
    /// The artifact's names, absent where the handle beneath it has none.
    path: Option<CString>,
    image_path: Option<CString>,
    image_format: CString,
    image_format_name: CString,
    article: CString,
    article_name: CString,
    /// Every device served this article, derived from the device
    /// catalog's own declarations.
    accepting: Vec<CString>,
    /// Every device type the recognizing format records.
    recorded: Vec<CString>,
    /// What recorded this artifact — null where the format records
    /// several types and nothing says which, which is the honest answer
    /// rather than a deficiency.
    device_type: Option<CString>,
}

impl RemanenceDiscovery {
    pub(crate) fn new(discovery: remanence::Discovery) -> Self {
        Self {
            path: discovery.path().map(to_cstring),
            image_path: discovery
                .image_path()
                .map(|path| to_cstring(&path.display().to_string())),
            image_format: to_cstring(discovery.image_format()),
            image_format_name: to_cstring(discovery.image_format_name()),
            article: to_cstring(discovery.article()),
            article_name: to_cstring(discovery.article_name()),
            accepting: discovery
                .accepting_devices()
                .iter()
                .map(|device| to_cstring(device.id()))
                .collect(),
            recorded: discovery
                .device_types()
                .iter()
                .map(|device| to_cstring(device.id()))
                .collect(),
            device_type: discovery
                .device_type()
                .map(|device| to_cstring(device.id())),
            discovery,
        }
    }
}

/// Identifies the artifact at `path` (UTF-8) — a disk image, or
/// an archive — under the caller's declared intent, and answers
/// with what it is and where it could go.
///
/// It is on no handle at all: no session and no machine, because it
/// consults catalogs and evidence rather than configuration, and it
/// mutates nothing (P2). The claim it takes is held by the returned
/// discovery until that is consumed or freed, so a `Write` discovery
/// claims the artifact exclusively and fails here when it cannot, never
/// by falling back (P7).
///
/// **Nothing is created**: no medium, no session cache, no spilled
/// backing. A cache bound is the load's declaration (P27), so there is
/// no `_with_cache` sibling here — the bound is stated at
/// `remanence_session_load_discovery_with_cache`, where the medium
/// comes into existence. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discover_media(
    path: *const c_char,
    intent: RemanenceAccessIntent,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDiscovery {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match remanence::discover_media(path.as_ref(), access_intent(intent)) {
        Ok(discovery) => Box::into_raw(Box::new(RemanenceDiscovery::new(discovery))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a discovery, releasing its claim. A discovery already consumed
/// by `remanence_session_load_discovery` must not be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_free(discovery: *mut RemanenceDiscovery) {
    if !discovery.is_null() {
        drop(unsafe { Box::from_raw(discovery) });
    }
}

/// The artifact claimed — the archive itself for an image discovered
/// inside one. Owned by the discovery; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_path(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| {
        discovery
            .path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr())
    })
}

/// The resolved image — the entry name for an image inside an archive,
/// else the source path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_image_path(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| {
        discovery
            .image_path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr())
    })
}

/// The image format's stable spelling — `h8d`, `qcow2`, `vdi`, `raw`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_image_format(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| discovery.image_format.as_ptr())
}

/// The image format's name, fit to show a user.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_image_format_name(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| {
        discovery.image_format_name.as_ptr()
    })
}

/// The image container format, as the device reader reports it.
///
/// A medium that is no disk image — an archive — has none, and reports
/// false here; its grammar is `remanence_discovery_image_format`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_format(
    discovery: *const RemanenceDiscovery,
    format_out: *mut RemanenceDiskFormat,
) -> bool {
    let Some(handle) = (unsafe { discovery.as_ref() }) else {
        return false;
    };
    let Ok(format) = handle.discovery.format() else {
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

/// The **exact article**, by the catalog's stable spelling (P14). The
/// image-format adapter that loaded the state named it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_article(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| discovery.article.as_ptr())
}

/// The article's name, fit to show a user beside the drive it goes in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_article_name(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| discovery.article_name.as_ptr())
}

/// How many devices are served this article — the answer to "where
/// could this go?", derived from the device catalog's own declarations.
/// Zero means no device this release claims takes it, which is an
/// archive's honest answer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_accepting_device_count(
    discovery: *const RemanenceDiscovery,
) -> usize {
    unsafe { discovery.as_ref() }.map_or(0, |discovery| discovery.accepting.len())
}

/// The stable spelling of the `index`th device served this article. Null
/// when out of range; owned by the discovery.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_accepting_device(
    discovery: *const RemanenceDiscovery,
    index: usize,
) -> *const c_char {
    unsafe { discovery.as_ref() }
        .and_then(|discovery| discovery.accepting.get(index))
        .map_or(ptr::null(), |device| device.as_ptr())
}

/// How many device types the recognizing format records — one where it
/// carries the type bare, several where a load declares which, and zero
/// for an archive grammar, which records no device at all.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_recorded_device_count(
    discovery: *const RemanenceDiscovery,
) -> usize {
    unsafe { discovery.as_ref() }.map_or(0, |discovery| discovery.recorded.len())
}

/// The stable spelling of the `index`th device type the format records —
/// the set a declaration may name. Null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_recorded_device(
    discovery: *const RemanenceDiscovery,
    index: usize,
) -> *const c_char {
    unsafe { discovery.as_ref() }
        .and_then(|discovery| discovery.recorded.get(index))
        .map_or(ptr::null(), |device| device.as_ptr())
}

/// The device this artifact's content was recorded by — the answer to
/// "what wrote it?" — or null where the format records several types
/// and nothing in the artifact says which.
///
/// Null is honest rather than deficient, and it is also a refusal
/// waiting to happen: a load takes the discovery only where this
/// answers, so a caller who meets null declares the type at
/// `remanence_session_load_media` instead.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_device_type(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }
        .and_then(|discovery| discovery.device_type.as_ref())
        .map_or(ptr::null(), |device| device.as_ptr())
}

/// The resolved image's own size in bytes — the raw plane.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_image_size_bytes(
    discovery: *const RemanenceDiscovery,
) -> u64 {
    unsafe { discovery.as_ref() }.map_or(0, |discovery| discovery.discovery.image_size_bytes())
}

/// The presented disk's size in bytes (the guest-visible size for
/// qcow2), or zero for a medium that presents no disk — an archive,
/// whose artifact's own extent is
/// `remanence_discovery_image_size_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_size(discovery: *const RemanenceDiscovery) -> u64 {
    unsafe { discovery.as_ref() }
        .and_then(|discovery| discovery.discovery.size().ok())
        .unwrap_or(0)
}

/// The **effective** access mode this discovery established, which a
/// load consuming it inherits (P28).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_mode(
    discovery: *const RemanenceDiscovery,
) -> RemanenceAccessMode {
    match unsafe { discovery.as_ref() } {
        Some(discovery) => access_mode(discovery.discovery.mode()),
        None => RemanenceAccessMode::ReadOnly,
    }
}

/// What this discovery established about the evidence beneath the medium
/// (P28), before anything is read. Free with `remanence_assurance_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_assurance(
    discovery: *const RemanenceDiscovery,
) -> *mut RemanenceAssurance {
    let Some(discovery) = (unsafe { discovery.as_ref() }) else {
        return ptr::null_mut();
    };
    Box::into_raw(Box::new(assurance_view(discovery.discovery.assurance())))
}

/// Identifies the artifact's nesting layers and probable filesystem —
/// the same reading `remanence_medium_identify` gives once a medium is
/// loaded. Free with `remanence_identification_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_identify(
    discovery: *const RemanenceDiscovery,
) -> *mut RemanenceIdentification {
    let Some(discovery) = (unsafe { discovery.as_ref() }) else {
        return ptr::null_mut();
    };
    let Identification {
        layers,
        modified,
        evidence,
    } = discovery.discovery.identify();
    Box::into_raw(Box::new(RemanenceIdentification {
        modified,
        layers: layers.iter().map(NestedLayerView::new).collect(),
        evidence: evidence.iter().map(|line| to_cstring(line)).collect(),
    }))
}
