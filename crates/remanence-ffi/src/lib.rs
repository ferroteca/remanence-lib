// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! C ABI for the Remanence disk image analysis library.
//!
//! Conventions:
//! - Handles (`RemanenceIdentification`, `RemanenceVolume`,
//!   `RemanenceSpace`, `RemanenceFile`, `RemanenceArchive`) are
//!   opaque and freed with their matching `*_free` function.
//! - `const char*` return values are UTF-8, owned by the handle they were read
//!   from, and valid until that handle is freed. Do not free them.
//! - Fallible calls take optional category, message and rule outputs; on
//!   failure they store a stable [`RemanenceErrorCategory`], a message to free
//!   with `remanence_string_free`, and — where the refusal came from an
//!   enumerated rule set — the stable identity of the rule that was broken,
//!   also freed with `remanence_string_free`. The rule output is null where
//!   no rule set applies, which is ordinary rather than an omission: the
//!   category says how to behave, and the rule says which rule the input
//!   broke. Rule sets belong to the seam that defines them and are documented
//!   there — the DOS 8.3 namespace's is the set the file verbs draw on — so
//!   the identity is a string rather than a second library-wide enum.
//! - Accessors taking an index return null / false / 0 when the index is out of
//!   range or the field does not apply to the layer's layout.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use remanence::{
    AttachmentId, Layer, LayerKind, LayerLayout, DiskLayout, ErrorCategory,
    Identification, PhysicalMediaLayout, SectorLayout, Session,
};

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

/// What a recognized layer of an artifact's nesting is.
///
/// This is a different axis from the P13 authoritative layer and the P23
/// active layer a device reports.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceLayerKind {
    Archive,
    Image,
    PhysicalMedia,
    Filesystem,
    Unknown,
}

/// Which layout details a layer carries.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceLayoutKind {
    Unknown,
    Archive,
    Image,
    PhysicalMedia,
    Filesystem,
}

/// Sector arrangement across a disk.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceSectorLayoutKind {
    Unknown,
    Fixed,
    Variable,
}

fn to_cstring(value: &str) -> CString {
    CString::new(value.replace('\0', "\u{fffd}")).expect("interior NULs replaced")
}

fn to_owned_c_char(value: &str) -> *mut c_char {
    to_cstring(value).into_raw()
}

unsafe fn clear_error(error_out: *mut *mut c_char, rule_out: *mut *mut c_char) {
    if !error_out.is_null() {
        unsafe { *error_out = ptr::null_mut() };
    }
    if !rule_out.is_null() {
        unsafe { *rule_out = ptr::null_mut() };
    }
}

unsafe fn set_error(
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

unsafe fn write_opt_u64(value: Option<u64>, out: *mut u64) -> bool {
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

unsafe fn write_opt_u32(value: Option<u32>, out: *mut u32) -> bool {
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

struct TrackView {
    cylinder: u32,
    side: u32,
    sectors: u32,
    sector_size: Option<u64>,
}

struct DiskView {
    media_type: CString,
    sector_size: Option<u64>,
    cylinders: Option<u32>,
    sides: Option<u32>,
    sector_layout: RemanenceSectorLayoutKind,
    sectors_per_track: u32,
    tracks: Vec<TrackView>,
    total_sectors: Option<u64>,
}

impl DiskView {
    fn new(layout: &DiskLayout) -> Self {
        let (sector_layout, sectors_per_track, tracks) = match &layout.sectors {
            SectorLayout::Unknown => (RemanenceSectorLayoutKind::Unknown, 0, Vec::new()),
            SectorLayout::Fixed { sectors_per_track } => {
                (RemanenceSectorLayoutKind::Fixed, *sectors_per_track, Vec::new())
            }
            SectorLayout::Variable { tracks } => (
                RemanenceSectorLayoutKind::Variable,
                0,
                tracks
                    .iter()
                    .map(|track| TrackView {
                        cylinder: track.cylinder,
                        side: track.side,
                        sectors: track.sectors,
                        sector_size: track.sector_size,
                    })
                    .collect(),
            ),
        };

        Self {
            media_type: to_cstring(&layout.media_type),
            sector_size: layout.sector_size,
            cylinders: layout.cylinders,
            sides: layout.sides,
            sector_layout,
            sectors_per_track,
            tracks,
            total_sectors: layout.total_sectors,
        }
    }
}

enum LayoutView {
    Unknown,
    Archive {
        path: CString,
        entry_name: CString,
        compressed_size: Option<u64>,
        uncompressed_size: Option<u64>,
    },
    Image {
        payload_offset_bytes: Option<u64>,
        payload_length_bytes: Option<u64>,
    },
    PhysicalMedia(Option<DiskView>),
    Filesystem {
        offset_bytes: Option<u64>,
        length_bytes: Option<u64>,
    },
}

struct NestedLayerView {
    kind: RemanenceLayerKind,
    id: CString,
    name: CString,
    confidence: u8,
    known: bool,
    current_bytes: Option<u64>,
    expected_bytes: Option<u64>,
    layout: LayoutView,
}

impl NestedLayerView {
    fn new(layer: &Layer) -> Self {
        let kind = match layer.kind {
            LayerKind::Archive => RemanenceLayerKind::Archive,
            LayerKind::Image => RemanenceLayerKind::Image,
            LayerKind::PhysicalMedia => RemanenceLayerKind::PhysicalMedia,
            LayerKind::Filesystem => RemanenceLayerKind::Filesystem,
            LayerKind::Unknown => RemanenceLayerKind::Unknown,
        };

        let layout = match &layer.layout {
            LayerLayout::Unknown => LayoutView::Unknown,
            LayerLayout::Archive(layout) => LayoutView::Archive {
                path: to_cstring(&layout.path.display().to_string()),
                entry_name: to_cstring(&layout.entry_name),
                compressed_size: layout.compressed_size,
                uncompressed_size: layout.uncompressed_size,
            },
            LayerLayout::Image(layout) => LayoutView::Image {
                payload_offset_bytes: layout.payload_offset_bytes,
                payload_length_bytes: layout.payload_length_bytes,
            },
            LayerLayout::PhysicalMedia(layout) => match layout {
                PhysicalMediaLayout::Unknown => LayoutView::PhysicalMedia(None),
                PhysicalMediaLayout::Disk(disk) => {
                    LayoutView::PhysicalMedia(Some(DiskView::new(disk)))
                }
            },
            LayerLayout::Filesystem(layout) => LayoutView::Filesystem {
                offset_bytes: layout.offset_bytes,
                length_bytes: layout.length_bytes,
            },
        };

        Self {
            kind,
            id: to_cstring(&layer.id),
            name: to_cstring(&layer.name),
            confidence: layer.confidence,
            known: layer.known,
            current_bytes: layer.size.current_bytes,
            expected_bytes: layer.size.expected_bytes,
            layout,
        }
    }
}

/// The result of identifying a medium's image.
pub struct RemanenceIdentification {
    modified: bool,
    layers: Vec<NestedLayerView>,
    evidence: Vec<CString>,
}

unsafe fn layer_view<'a>(
    identification: *const RemanenceIdentification,
    index: usize,
) -> Option<&'a NestedLayerView> {
    let identification = unsafe { identification.as_ref() }?;
    identification.layers.get(index)
}

/// Returns the library version as a static string. Do not free.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// The stated default session cache bound, in bytes: what an open
/// without a declared bound uses.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_default_cache_bytes() -> u64 {
    remanence::DEFAULT_CACHE_BYTES
}

/// Frees a string returned through an `error_out` or `error_rule_out`
/// parameter.
///
/// A fallible call writes three things on failure: the stable
/// category, which says how to behave; the human diagnostic; and,
/// where the refusal is one of an enumerated set of rules a format,
/// namespace, or grammar defines, the stable identity of the rule the input
/// broke. `error_rule_out` is null where no such rule set applies, which is
/// the ordinary case rather than an omission — the rule identity never
/// substitutes for the category. Each output is optional; passing null for
/// any of them declines it. The DOS 8.3 namespace owns the set the file
/// verbs draw on: `empty-base`, `base-too-long`, `extension-too-long`,
/// `separator`, `excluded-character`, `reserved-device-name`,
/// `surrounding-space`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_string_free(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}




/// This device's attachment identity — `hdd0` and the like. Owned by the
/// view; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_attachment(
    device: *const RemanenceDevice,
) -> *const c_char {
    match unsafe { device.as_ref() } {
        Some(handle) => handle.attachment_c.as_ptr(),
        None => ptr::null(),
    }
}

/// This device's family, by its stable spelling (`hard-disk`). Owned by
/// the view; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_family(device: *const RemanenceDevice) -> *const c_char {
    match unsafe { device.as_ref() } {
        Some(handle) => handle.family_c.as_ptr(),
        None => ptr::null(),
    }
}

/// Whether a medium currently occupies this device's slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_is_occupied(device: *const RemanenceDevice) -> bool {
    match unsafe { device.as_ref() } {
        Some(handle) => handle.device().is_some_and(|device| device.is_occupied()),
        None => false,
    }
}

/// Loads the medium at `path` (UTF-8) — a disk image, or
/// an archive — into this device, and hands back nothing to hold: the
/// device is the one storage handle.
///
/// A path names a file. An artifact *inside* an archive is loaded from
/// the file view that names it: `remanence_filesystem_discover`, then
/// `remanence_device_load_discovery`.
///
/// A device accepts only the media its family is served (P14), and a
/// mismatch is refused naming both sides. A `Write` intent claims the
/// medium exclusively and fails here when the claim cannot be secured,
/// never by falling back. An occupied slot is refused rather than
/// displaced. Returns false on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_load_media(
    device: *mut RemanenceDevice,
    path: *const c_char,
    intent: RemanenceAccessIntent,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { device.as_mut() }) else {
        let error = remanence::Error::io("null device");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let Some(target) = handle.device() else {
        let error = remanence::Error::io("the device holding this medium was removed");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match target.load_media(path.as_ref(), access_intent(intent)) {
        Ok(()) => {
            handle.refresh();
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Loads the medium a discovery already opened into this device,
/// **consuming and freeing the discovery**.
///
/// This is the load that runs nothing twice: the discovery holds the
/// claim taken when the artifact was identified and the work that
/// identification did, and both move into the device, so no window
/// exists between the question and the load in which the artifact could
/// change (P7). The intent, the cache bound and the assurance are the
/// ones the discovery established.
///
/// **The discovery is freed either way** — a refused load releases its
/// claim with it — so the pointer must never be used or freed again
/// after this call, whatever it returns. Asking again is
/// `remanence_discover_media`. Returns false on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_load_discovery(
    device: *mut RemanenceDevice,
    discovery: *mut RemanenceDiscovery,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    if discovery.is_null() {
        let error = remanence::Error::io("null discovery");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    // Taken before anything can fail: the C contract is that this call
    // consumes the discovery whatever the outcome, exactly as the Rust
    // one does, so the two surfaces cannot disagree about who holds the
    // claim afterwards.
    let discovery = unsafe { Box::from_raw(discovery) };
    let Some(handle) = (unsafe { device.as_mut() }) else {
        let error = remanence::Error::io("null device");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let Some(target) = handle.device() else {
        let error = remanence::Error::io("the device holding this medium was removed");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match target.load_discovery(discovery.discovery) {
        Ok(()) => {
            handle.refresh();
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Ejects the medium, releasing its P7 claim, and leaves the device in
/// place. Every view taken through it stops answering, and the content
/// verbs refuse by name until another medium is loaded. Returns false
/// when the slot was already empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_eject(
    device: *mut RemanenceDevice,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { device.as_mut() }) else {
        let error = remanence::Error::io("null device");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let Some(target) = handle.device() else {
        let error = remanence::Error::io("the device holding this medium was removed");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match target.eject() {
        Ok(()) => {
            handle.refresh();
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// One family's strings, built once so every reader below answers with a
/// pointer the library owns and the caller never frees.
struct FamilyView {
    id: CString,
    name: CString,
    provenance: CString,
    kind_of: Option<CString>,
    slot_prefix: Option<CString>,
    media: Vec<CString>,
    flux_path: Option<CString>,
}

fn families() -> &'static [FamilyView] {
    static FAMILIES: std::sync::OnceLock<Vec<FamilyView>> = std::sync::OnceLock::new();
    FAMILIES.get_or_init(|| {
        DeviceFamily::enrolled()
            .into_iter()
            .map(|family| FamilyView {
                id: to_cstring(family.id()),
                name: to_cstring(family.name()),
                provenance: to_cstring(family.provenance()),
                kind_of: family.kind_of().map(|parent| to_cstring(parent.id())),
                slot_prefix: family.slot_prefix().map(to_cstring),
                media: family.accepted_media().into_iter().map(to_cstring).collect(),
                flux_path: family.flux_path().map(to_cstring),
            })
            .collect()
    })
}

fn family_string(index: usize, read: fn(&FamilyView) -> Option<&CString>) -> *const c_char {
    families()
        .get(index)
        .and_then(read)
        .map_or(ptr::null(), |value| value.as_ptr())
}

/// How many device families this release enrols, interior names of the
/// lineage among them (P32).
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_count() -> usize {
    families().len()
}

/// The stable spelling of family `index` — the value
/// `remanence_session_add_device` takes. Null when out of range; owned by
/// the library and never freed.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_id(index: usize) -> *const c_char {
    family_string(index, |family| Some(&family.id))
}

/// Family `index`'s name, fit to show a user beside the slot it fills.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_name(index: usize) -> *const c_char {
    family_string(index, |family| Some(&family.name))
}

/// Where family `index`'s declaration came from.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_provenance(index: usize) -> *const c_char {
    family_string(index, |family| Some(&family.provenance))
}

/// What family `index` is a kind of, by stable spelling — null for the
/// root of the lineage.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_kind_of(index: usize) -> *const c_char {
    family_string(index, |family| family.kind_of.as_ref())
}

/// Whether family `index` can be added to a machine. An interior name of
/// the lineage classifies and instantiates nothing.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_is_concrete(index: usize) -> bool {
    families()
        .get(index)
        .is_some_and(|family| family.slot_prefix.is_some())
}

/// The family half of every attachment identity in family `index` —
/// `hdd` for `hdd0`. Null for an interior name, which names no slot.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_slot_prefix(index: usize) -> *const c_char {
    family_string(index, |family| family.slot_prefix.as_ref())
}

/// How many media types family `index` accepts (P14). Zero for an
/// interior name.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_media_count(index: usize) -> usize {
    families().get(index).map_or(0, |family| family.media.len())
}

/// The stable spelling of the `media`th media type family `index`
/// accepts. Null when either index is out of range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_media(index: usize, media: usize) -> *const c_char {
    families()
        .get(index)
        .and_then(|family| family.media.get(media))
        .map_or(ptr::null(), |id| id.as_ptr())
}

/// The drive profile family `index` claims as its recording path (P22),
/// by stable spelling. Null where the family claims none, which is
/// ordinary rather than deficient.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_family_flux_path(index: usize) -> *const c_char {
    family_string(index, |family| family.flux_path.as_ref())
}

/// What one artifact turned out to be, and the claim under which that was
/// established.
///
/// Free it with `remanence_discovery_free`, or hand it to
/// `remanence_device_load_discovery`, which consumes it. Every string it
/// returns is owned by it and freed with it.
pub struct RemanenceDiscovery {
    discovery: remanence::Discovery,
    path: CString,
    image_path: CString,
    image_format: CString,
    image_format_name: CString,
    media_type: CString,
    media_type_name: CString,
    /// Every concrete family served this medium, derived from the
    /// families' own declarations.
    families: Vec<CString>,
    /// The family the image format declares — null where it declares
    /// none, which is ordinary rather than deficient.
    default_device: Option<CString>,
}

impl RemanenceDiscovery {
    fn new(discovery: remanence::Discovery) -> Self {
        Self {
            path: to_cstring(discovery.path()),
            image_path: to_cstring(&discovery.image_path().display().to_string()),
            image_format: to_cstring(discovery.image_format()),
            image_format_name: to_cstring(discovery.image_format_name()),
            media_type: to_cstring(discovery.media_type()),
            media_type_name: to_cstring(discovery.media_type_name()),
            families: discovery
                .accepting_families()
                .iter()
                .map(|family| to_cstring(family.id()))
                .collect(),
            default_device: discovery
                .default_device()
                .map(|family| to_cstring(family.id())),
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
/// by falling back (P7). Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discover_media(
    path: *const c_char,
    intent: RemanenceAccessIntent,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDiscovery {
    unsafe {
        discover(
            path,
            intent,
            remanence::DEFAULT_CACHE_BYTES,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// `remanence_discover_media` under a caller-declared session cache
/// bound (P27), which a load consuming the discovery keeps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discover_media_with_cache(
    path: *const c_char,
    intent: RemanenceAccessIntent,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDiscovery {
    unsafe {
        discover(
            path,
            intent,
            cache_bytes,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

unsafe fn discover(
    path: *const c_char,
    intent: RemanenceAccessIntent,
    cache_bytes: u64,
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
    match remanence::discover_media_with_cache(path.as_ref(), access_intent(intent), cache_bytes) {
        Ok(discovery) => Box::into_raw(Box::new(RemanenceDiscovery::new(discovery))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a discovery, releasing its claim. A discovery already consumed
/// by `remanence_device_load_discovery` must not be freed.
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
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| discovery.path.as_ptr())
}

/// The resolved image — the entry name for an image inside an archive,
/// else the source path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_image_path(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| discovery.image_path.as_ptr())
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
    unsafe { discovery.as_ref() }
        .map_or(ptr::null(), |discovery| discovery.image_format_name.as_ptr())
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
            }
        };
    }
    true
}

/// The **exact medium**, by the media-type catalog's stable spelling
/// (P14). The image-format adapter that loaded the state named it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_media_type(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }.map_or(ptr::null(), |discovery| discovery.media_type.as_ptr())
}

/// The medium's name, fit to show a user beside the drive it goes in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_media_type_name(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }
        .map_or(ptr::null(), |discovery| discovery.media_type_name.as_ptr())
}

/// How many concrete device families are served this medium — the
/// answer to "where could this go?", derived from the families' own
/// declarations. Zero means no drive this release claims takes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_device_family_count(
    discovery: *const RemanenceDiscovery,
) -> usize {
    unsafe { discovery.as_ref() }.map_or(0, |discovery| discovery.families.len())
}

/// The stable spelling of the `index`th family served this medium. Null
/// when out of range; owned by the discovery.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_device_family(
    discovery: *const RemanenceDiscovery,
    index: usize,
) -> *const c_char {
    unsafe { discovery.as_ref() }
        .and_then(|discovery| discovery.families.get(index))
        .map_or(ptr::null(), |family| family.as_ptr())
}

/// The device family the **image format** declares for the disks it
/// records — the answer to "where did this come from?" — or null where
/// the format declares none.
///
/// Null is ordinary: a raw image says nothing about its machine, and the
/// caller then states the drive itself in the two acts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_discovery_default_device(
    discovery: *const RemanenceDiscovery,
) -> *const c_char {
    unsafe { discovery.as_ref() }
        .and_then(|discovery| discovery.default_device.as_ref())
        .map_or(ptr::null(), |family| family.as_ptr())
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
/// the same reading `remanence_device_identify` gives once a medium is
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

/// The artifact the medium was opened from (the archive path for archive
/// inputs). Null while the device's slot is empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_path(device: *const RemanenceDevice) -> *const c_char {
    match unsafe { device.as_ref() } {
        Some(handle) => handle
            .path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr()),
        None => ptr::null(),
    }
}

/// The resolved image path (the entry name for archive inputs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_image_path(device: *const RemanenceDevice) -> *const c_char {
    match unsafe { device.as_ref() } {
        Some(handle) => handle
            .image_path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr()),
        None => ptr::null(),
    }
}

/// The resolved image's size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_image_size_bytes(device: *const RemanenceDevice) -> u64 {
    match unsafe { device.as_ref() } {
        Some(handle) => handle
            .device()
            .and_then(|device| device.image_size_bytes().ok())
            .unwrap_or(0),
        None => 0,
    }
}

/// Reads `length` bytes of the resolved image at `offset` into
/// `buffer_out` — the bounded access form: the image streams from its
/// backing and is never resident whole. Returns false on failure and
/// stores a message in `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_read_at(
    device: *const RemanenceDevice,
    offset: u64,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { device.as_ref() }) else {
        let error = remanence::Error::io("null device");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
        let Some(medium) = handle.device() else {
        let error = remanence::Error::io("the device holding this medium was removed");
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

/// Identifies the artifact's nesting layers and probable filesystem. Free the
/// result with `remanence_identification_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_identify(
    device: *const RemanenceDevice,
) -> *mut RemanenceIdentification {
    let Some(handle) = (unsafe { device.as_ref() }) else {
        return ptr::null_mut();
    };
    let Identification {
        layers,
        modified,
        evidence,
    } = match handle.device().map(|device| device.identify()) {
        Some(Ok(identification)) => identification,
        _ => return ptr::null_mut(),
    };

    Box::into_raw(Box::new(RemanenceIdentification {
        modified,
        layers: layers.iter().map(NestedLayerView::new).collect(),
        evidence: evidence.iter().map(|line| to_cstring(line)).collect(),
    }))
}

/// Frees an identification handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_free(identification: *mut RemanenceIdentification) {
    if !identification.is_null() {
        drop(unsafe { Box::from_raw(identification) });
    }
}

/// Whether the medium reported unsaved modifications at identify time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_modified(
    identification: *const RemanenceIdentification,
) -> bool {
    unsafe { identification.as_ref() }.is_some_and(|identification| identification.modified)
}

/// Number of recognized nesting layers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_layer_count(
    identification: *const RemanenceIdentification,
) -> usize {
    unsafe { identification.as_ref() }.map_or(0, |identification| identification.layers.len())
}

/// Number of evidence lines.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_evidence_count(
    identification: *const RemanenceIdentification,
) -> usize {
    unsafe { identification.as_ref() }.map_or(0, |identification| identification.evidence.len())
}

/// One evidence line, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_evidence(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { identification.as_ref() }
        .and_then(|identification| identification.evidence.get(index))
        .map_or(ptr::null(), |line| line.as_ptr())
}

/// The layer's kind, or `RemanenceLayerKind::Unknown` when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_kind(
    identification: *const RemanenceIdentification,
    index: usize,
) -> RemanenceLayerKind {
    unsafe { layer_view(identification, index) }
        .map_or(RemanenceLayerKind::Unknown, |layer| layer.kind)
}

/// The layer's id (e.g. "h8d", "zip", "hdos").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_id(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { layer_view(identification, index) }
        .map_or(ptr::null(), |layer| layer.id.as_ptr())
}

/// The layer's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_name(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { layer_view(identification, index) }
        .map_or(ptr::null(), |layer| layer.name.as_ptr())
}

/// Detection confidence, 0-100.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_confidence(
    identification: *const RemanenceIdentification,
    index: usize,
) -> u8 {
    unsafe { layer_view(identification, index) }.map_or(0, |layer| layer.confidence)
}

/// Whether the layer matched a known format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_known(
    identification: *const RemanenceIdentification,
    index: usize,
) -> bool {
    unsafe { layer_view(identification, index) }.is_some_and(|layer| layer.known)
}

/// Current size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_current_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { layer_view(identification, index) }.and_then(|c| c.current_bytes);
    unsafe { write_opt_u64(value, out) }
}

/// Expected size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_expected_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { layer_view(identification, index) }.and_then(|c| c.expected_bytes);
    unsafe { write_opt_u64(value, out) }
}

/// Which layout details this layer carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_layout_kind(
    identification: *const RemanenceIdentification,
    index: usize,
) -> RemanenceLayoutKind {
    unsafe { layer_view(identification, index) }.map_or(RemanenceLayoutKind::Unknown, |layer| {
        match &layer.layout {
            LayoutView::Unknown => RemanenceLayoutKind::Unknown,
            LayoutView::Archive { .. } => RemanenceLayoutKind::Archive,
            LayoutView::Image { .. } => RemanenceLayoutKind::Image,
            LayoutView::PhysicalMedia(_) => RemanenceLayoutKind::PhysicalMedia,
            LayoutView::Filesystem { .. } => RemanenceLayoutKind::Filesystem,
        }
    })
}

/// Archive layout: the archive file path; null for other layouts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_archive_path(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { path, .. }) => path.as_ptr(),
        _ => ptr::null(),
    }
}

/// Archive layout: the entry name inside the archive; null for other layouts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_archive_entry_name(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive { entry_name, .. }) => entry_name.as_ptr(),
        _ => ptr::null(),
    }
}

/// Archive layout: compressed entry size; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_archive_compressed_size(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive {
            compressed_size, ..
        }) => *compressed_size,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Archive layout: uncompressed entry size; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_archive_uncompressed_size(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Archive {
            uncompressed_size, ..
        }) => *uncompressed_size,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Image layout: payload offset in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_image_payload_offset(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Image {
            payload_offset_bytes,
            ..
        }) => *payload_offset_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Image layout: payload length in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_image_payload_length(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Image {
            payload_length_bytes,
            ..
        }) => *payload_length_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

unsafe fn disk_view<'a>(
    identification: *const RemanenceIdentification,
    index: usize,
) -> Option<&'a DiskView> {
    match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::PhysicalMedia(disk)) => disk.as_ref(),
        _ => None,
    }
}

/// Physical media layout: whether disk geometry is known.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_has_disk_layout(
    identification: *const RemanenceIdentification,
    index: usize,
) -> bool {
    unsafe { disk_view(identification, index) }.is_some()
}

/// Disk layout: the media type the image format names for its medium
/// (e.g. "logical-block-512"); null when the layer has no disk
/// layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_media_type(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { disk_view(identification, index) }
        .map_or(ptr::null(), |disk| disk.media_type.as_ptr())
}

/// Disk layout: sector size in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_sector_size(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.sector_size);
    unsafe { write_opt_u64(value, out) }
}

/// Disk layout: cylinder count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_cylinders(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.cylinders);
    unsafe { write_opt_u32(value, out) }
}

/// Disk layout: side count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_sides(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u32,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.sides);
    unsafe { write_opt_u32(value, out) }
}

/// Disk layout: how sectors are arranged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_sector_layout_kind(
    identification: *const RemanenceIdentification,
    index: usize,
) -> RemanenceSectorLayoutKind {
    unsafe { disk_view(identification, index) }
        .map_or(RemanenceSectorLayoutKind::Unknown, |disk| disk.sector_layout)
}

/// Disk layout: sectors per track for fixed layouts; 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_sectors_per_track(
    identification: *const RemanenceIdentification,
    index: usize,
) -> u32 {
    unsafe { disk_view(identification, index) }.map_or(0, |disk| disk.sectors_per_track)
}

/// Disk layout: per-track entry count for variable layouts; 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_track_count(
    identification: *const RemanenceIdentification,
    index: usize,
) -> usize {
    unsafe { disk_view(identification, index) }.map_or(0, |disk| disk.tracks.len())
}

/// Disk layout: one per-track entry for variable layouts. Returns false when
/// out of range. `has_sector_size` and `sector_size` report the optional
/// per-track sector size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_track(
    identification: *const RemanenceIdentification,
    index: usize,
    track_index: usize,
    cylinder: *mut u32,
    side: *mut u32,
    sectors: *mut u32,
    has_sector_size: *mut bool,
    sector_size: *mut u64,
) -> bool {
    let Some(track) =
        unsafe { disk_view(identification, index) }.and_then(|disk| disk.tracks.get(track_index))
    else {
        return false;
    };

    unsafe {
        if !cylinder.is_null() {
            *cylinder = track.cylinder;
        }
        if !side.is_null() {
            *side = track.side;
        }
        if !sectors.is_null() {
            *sectors = track.sectors;
        }
        if !has_sector_size.is_null() {
            *has_sector_size = track.sector_size.is_some();
        }
        if !sector_size.is_null() {
            *sector_size = track.sector_size.unwrap_or(0);
        }
    }
    true
}

/// Disk layout: total sector count; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_total_sectors(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = unsafe { disk_view(identification, index) }.and_then(|disk| disk.total_sectors);
    unsafe { write_opt_u64(value, out) }
}

/// Filesystem layout: offset in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_fs_offset_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Filesystem { offset_bytes, .. }) => *offset_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

/// Filesystem layout: length in bytes; returns false when unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_fs_length_bytes(
    identification: *const RemanenceIdentification,
    index: usize,
    out: *mut u64,
) -> bool {
    let value = match unsafe { layer_view(identification, index) }.map(|c| &c.layout) {
        Some(LayoutView::Filesystem { length_bytes, .. }) => *length_bytes,
        _ => None,
    };
    unsafe { write_opt_u64(value, out) }
}

// ---------------------------------------------------------------------------
// The storage-device surface (U3/U4): attach a raw, qcow2 or VDI image
// under the P7 claim, report partitions and volumes, read/write FAT
// files with a commit point.

use remanence::{
    AccessIntent, AccessMode, DeviceFamily, DiskContent, DiskFormat, Entry, EntryKind,
    RegionRole, StorageDevice, VolumeId, VolumeOrigin,
};

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
}

/// What a FAT directory entry is.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceEntryKind {
    File,
    Directory,
}

fn access_intent(intent: RemanenceAccessIntent) -> AccessIntent {
    match intent {
        RemanenceAccessIntent::Read => AccessIntent::Read,
        RemanenceAccessIntent::Write => AccessIntent::Write,
    }
}

fn access_mode(mode: AccessMode) -> RemanenceAccessMode {
    match mode {
        AccessMode::ReadWrite => RemanenceAccessMode::ReadWrite,
        AccessMode::ReadOnly => RemanenceAccessMode::ReadOnly,
    }
}

/// An open session: the claim and cache scope, holding the machines
/// within it (P32).
pub struct RemanenceSession {
    session: Session,
    /// Borrowed machine views handed to callers, owned here and freed
    /// with the session.
    machines: Vec<Box<RemanenceMachine>>,
    /// Borrowed device views handed to callers. Owned here so their
    /// strings outlive the call that produced them, and freed with the
    /// session.
    views: Vec<Box<RemanenceDevice>>,
}

/// A borrowed view of one machine in a session.
///
/// **The session owns this; never free it.** It stays valid until the
/// session is freed.
pub struct RemanenceMachine {
    session: *mut RemanenceSession,
    /// Null for the session's anonymous machine.
    identity: Option<String>,
    identity_c: Option<CString>,
}

impl RemanenceMachine {
    /// The machine this view names. The anonymous one always resolves;
    /// a named one resolves for as long as the session holds it.
    #[allow(clippy::mut_from_ref)]
    fn machine(&self) -> Option<&mut remanence::Machine> {
        let session = unsafe { &mut (*self.session).session };
        match &self.identity {
            Some(identity) => session.machine_mut(identity),
            None => Some(session.anonymous_mut()),
        }
    }
}

/// A borrowed view of one storage device — the slot, its family, and the
/// state of the medium in it.
///
/// **The session owns this; never free it.** It stays valid until the
/// device is removed or the session is freed.
///
/// It names the device by session, machine and attachment identity
/// rather than by pointer, and re-resolves on every call. That is
/// deliberate: a later attach may reallocate the machine's device
/// storage, so a cached pointer to the device itself would dangle
/// silently.
pub struct RemanenceDevice {
    session: *mut RemanenceSession,
    /// Null for the session's anonymous machine.
    machine: Option<String>,
    attachment: AttachmentId,
    /// The slot-side facts, which do not change while the device exists.
    attachment_c: CString,
    family_c: CString,
    /// The content-side strings, which change every time a medium is
    /// loaded or ejected.
    path: Option<CString>,
    image_path: Option<CString>,
}

impl RemanenceDevice {
    /// The device this view names, or `None` once it is removed.
    #[allow(clippy::mut_from_ref)]
    fn device(&self) -> Option<&mut StorageDevice> {
        let session = unsafe { &mut (*self.session).session };
        let machine = match &self.machine {
            Some(identity) => session.machine_mut(identity)?,
            None => session.anonymous_mut(),
        };
        machine.device_mut(self.attachment)
    }

    /// Restates the content-side strings from whatever occupies the slot
    /// now. Loading and ejecting both change what they are strings *of*,
    /// so a view that did not restate them would answer for a medium
    /// that has left.
    fn refresh(&mut self) {
        let (path, image_path) = match self.device() {
            Some(device) => (
                device.path().ok().map(to_cstring),
                device
                    .image_path()
                    .ok()
                    .map(|image_path| to_cstring(&image_path.display().to_string())),
            ),
            None => (None, None),
        };
        self.path = path;
        self.image_path = image_path;
    }
}

/// A directory listing.
pub struct RemanenceEntryList {
    entries: Vec<EntryView>,
}

struct EntryView {
    name: CString,
    kind: RemanenceEntryKind,
    size_bytes: u64,
    /// What the recognizing filesystem declares beyond the fields above,
    /// in its own spelling and order — HDOS's catalog date and flag
    /// letters are the delivered case.
    declared: Vec<(CString, CString)>,
}

impl EntryView {
    fn new(entry: &Entry) -> Self {
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

fn entry_kind(kind: EntryKind) -> RemanenceEntryKind {
    match kind {
        EntryKind::File => RemanenceEntryKind::File,
        EntryKind::Directory => RemanenceEntryKind::Directory,
    }
}

/// Bytes read out of a volume or catalog.
pub struct RemanenceFileData {
    bytes: Vec<u8>,
}

unsafe fn utf8_arg<'a>(value: *const c_char) -> Option<std::borrow::Cow<'a, str>> {
    if value.is_null() {
        return None;
    }
    Some(String::from_utf8_lossy(
        unsafe { CStr::from_ptr(value) }.to_bytes(),
    ))
}

/// Opens an empty session — the claim and cache scope, holding nothing
/// but its anonymous machine. Machines and devices are added over its
/// life; neither set is fixed at open. Free with
/// `remanence_session_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_new() -> *mut RemanenceSession {
    Box::into_raw(Box::new(RemanenceSession {
        session: Session::new(),
        machines: Vec::new(),
        views: Vec::new(),
    }))
}

/// Frees a session, dropping every device and releasing every P7 claim.
/// Every borrowed machine and device view obtained from it becomes
/// invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_free(session: *mut RemanenceSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Adds a device of `family` (UTF-8, a family's stable spelling such as
/// `hard-disk`) to the session's **anonymous machine**, taking the lowest
/// free slot of that family, and returns a **borrowed** view of it —
/// empty, until `remanence_device_load_media` puts a medium in it.
///
/// The session owns the view; never free it.
/// `remanence_machine_add_device` does the same in a named machine. A
/// family this release does not claim, and an interior name of the
/// lineage which classifies rather than instantiates, are both refused by
/// name (P3). Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_add_device(
    session: *mut RemanenceSession,
    family: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    unsafe {
        add_device(
            session,
            None,
            family,
            None,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Adds a device of `family` at slot `index` of the session's anonymous
/// machine — `hdd1` being family `hard-disk` at index 1. The caller
/// chooses the slot, never the name; a slot already taken is refused
/// rather than displaced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_add_device_at(
    session: *mut RemanenceSession,
    family: *const c_char,
    index: u32,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    unsafe {
        add_device(
            session,
            None,
            family,
            Some(index),
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Adds a device for the artifact at `path` (UTF-8) to the session's
/// **anonymous machine** and returns a **borrowed** view of it, as
/// `remanence_machine_add_device_for` does in a named machine.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_add_device_for(
    session: *mut RemanenceSession,
    path: *const c_char,
    intent: RemanenceAccessIntent,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    unsafe {
        add_device_for(
            session,
            None,
            path,
            intent,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Removes the device at `attachment` from the session's anonymous
/// machine, releasing any medium's P7 claim with it and freeing the slot.
/// Borrowed device views for that device become invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_remove_device(
    session: *mut RemanenceSession,
    attachment: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe {
        remove_device(
            session,
            None,
            attachment,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// How many devices the session's anonymous machine holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_device_count(
    session: *const RemanenceSession,
) -> usize {
    unsafe { session.as_ref() }.map_or(0, |handle| handle.session.devices().len())
}

/// Writes the attachment identity of device `index` to
/// `attachment_out`, freed with `remanence_string_free`. Returns false
/// when `index` is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_device_attachment(
    session: *const RemanenceSession,
    index: usize,
    attachment_out: *mut *mut c_char,
) -> bool {
    let Some(handle) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Some(device) = handle.session.devices().get(index) else {
        return false;
    };
    if !attachment_out.is_null() {
        unsafe { *attachment_out = to_owned_c_char(&device.attachment().to_string()) };
    }
    true
}

/// Adds a machine carrying `identity` (UTF-8) to the session and returns
/// a **borrowed** view of it.
///
/// The session owns it; never free it. An identity already in use is
/// refused by name rather than resolving to the machine holding it, and
/// the empty identity is refused too — the machine with no identity is
/// the session's anonymous one, and there is exactly one of it. Returns
/// null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_add_machine(
    session: *mut RemanenceSession,
    identity: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMachine {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { session.as_mut() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(identity) = (unsafe { utf8_arg(identity) }) else {
        let error = remanence::Error::io("null machine identity");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let identity = identity.into_owned();
    if let Err(error) = handle.session.add_machine(identity.clone()) {
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    unsafe { machine_view(session, Some(identity)) }
}

/// How many machines the session holds, the anonymous one among them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_machine_count(
    session: *const RemanenceSession,
) -> usize {
    unsafe { session.as_ref() }.map_or(0, |handle| handle.session.machines().len())
}

/// Writes the identity of machine `index` to `identity_out`, freed with
/// `remanence_string_free`. Returns false when `index` is out of range;
/// writes null for the anonymous machine, whose identity is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_machine_identity(
    session: *const RemanenceSession,
    index: usize,
    identity_out: *mut *mut c_char,
) -> bool {
    let Some(handle) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Some(machine) = handle.session.machines().get(index) else {
        return false;
    };
    if !identity_out.is_null() {
        unsafe {
            *identity_out = match machine.identity() {
                Some(identity) => to_owned_c_char(identity),
                None => ptr::null_mut(),
            }
        };
    }
    true
}

/// A **borrowed** view of the machine carrying `identity` (UTF-8), or of
/// the session's **anonymous machine** when `identity` is null — the
/// anonymous machine being exactly the one whose identity is null.
///
/// The session owns it; never free it. Returns null when the session
/// holds no machine of that identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_machine(
    session: *mut RemanenceSession,
    identity: *const c_char,
) -> *mut RemanenceMachine {
    let Some(handle) = (unsafe { session.as_ref() }) else {
        return ptr::null_mut();
    };
    let identity = unsafe { utf8_arg(identity) }.map(|identity| identity.into_owned());
    if let Some(identity) = &identity {
        if handle.session.machine(identity).is_none() {
            return ptr::null_mut();
        }
    }
    unsafe { machine_view(session, identity) }
}

/// The machine's identity, or null where it is the session's anonymous
/// machine. Owned by the view; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_identity(
    machine: *const RemanenceMachine,
) -> *const c_char {
    match unsafe { machine.as_ref() } {
        Some(handle) => handle
            .identity_c
            .as_ref()
            .map_or(ptr::null(), |identity| identity.as_ptr()),
        None => ptr::null(),
    }
}

/// Adds a device of `family` (UTF-8) to this machine, taking the lowest
/// free slot of that family, and returns a **borrowed** view of it. The
/// session owns the view; never free it. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_add_device(
    machine: *mut RemanenceMachine,
    family: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    let Some(handle) = (unsafe { machine.as_ref() }) else {
        let error = remanence::Error::io("null machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let (session, identity) = (handle.session, handle.identity.clone());
    unsafe {
        add_device(
            session,
            identity,
            family,
            None,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Adds a device of `family` at slot `index` in this machine. The caller
/// chooses the slot, never the name; a slot already taken is refused
/// rather than displaced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_add_device_at(
    machine: *mut RemanenceMachine,
    family: *const c_char,
    index: u32,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    let Some(handle) = (unsafe { machine.as_ref() }) else {
        let error = remanence::Error::io("null machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let (session, identity) = (handle.session, handle.identity.clone());
    unsafe {
        add_device(
            session,
            identity,
            family,
            Some(index),
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Adds a device of the artifact's **format-declared default family** to
/// this machine, loads the medium at `path` (UTF-8) into it, and returns
/// a **borrowed** view of that device. The session owns the view; never
/// free it.
///
/// It is the one convenience over discovery, and it composes the two
/// acts without changing the access path: one claim is held from the
/// question to the load (P7), and the device it answers with is an
/// ordinary device in this machine's own set — a fresh one, never a slot
/// already there.
///
/// **A format that declares no default is refused by name**, toward the
/// two explicit acts (`remanence_machine_add_device` then
/// `remanence_device_load_media`), with the refusal naming the families
/// the medium could go in. A refused call leaves no device behind.
/// Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_add_device_for(
    machine: *mut RemanenceMachine,
    path: *const c_char,
    intent: RemanenceAccessIntent,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    let Some(handle) = (unsafe { machine.as_ref() }) else {
        let error = remanence::Error::io("null machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let (session, identity) = (handle.session, handle.identity.clone());
    unsafe {
        add_device_for(
            session,
            identity,
            path,
            intent,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Removes the device at `attachment` from this machine, releasing any
/// medium's P7 claim with it and freeing the slot. Borrowed device views
/// for that device become invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_remove_device(
    machine: *mut RemanenceMachine,
    attachment: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    let Some(handle) = (unsafe { machine.as_ref() }) else {
        let error = remanence::Error::io("null machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let (session, identity) = (handle.session, handle.identity.clone());
    unsafe {
        remove_device(
            session,
            identity,
            attachment,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Composes this machine's DOS drive-letter mapping from its **own device
/// set** (P32, P35), reading attachment order from the order its devices
/// were added rather than from an assertion.
///
/// `rule` is a claimed rule's name, or null to apply every claimed rule
/// and leave a letter they disagree on undetermined. Families no claimed
/// rule letters are passed over by family, and the mapping's provenance
/// says which. Free the result with `remanence_drive_map_free`; returns
/// null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_compose_dos_letters(
    machine: *mut RemanenceMachine,
    rule: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDriveMap {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(target) = (unsafe { machine.as_ref() }).and_then(RemanenceMachine::machine) else {
        let error = remanence::Error::io("null or retired machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let rule = if rule.is_null() {
        None
    } else {
        let Some(name) = (unsafe { utf8_arg(rule) }) else {
            let error = remanence::Error::io("null rule");
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        };
        match DosAssignmentRule::from_name(name.as_ref()) {
            Ok(rule) => Some(rule),
            Err(error) => {
                unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
                return ptr::null_mut();
            }
        }
    };
    match target.compose_dos_letters(rule, &[]) {
        Ok(map) => Box::into_raw(Box::new(drive_map_view(&map))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// How many devices this machine holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_device_count(
    machine: *const RemanenceMachine,
) -> usize {
    unsafe { machine.as_ref() }
        .and_then(RemanenceMachine::machine)
        .map_or(0, |machine| machine.devices().len())
}

/// Writes the attachment identity of device `index` in this machine to
/// `attachment_out`, freed with `remanence_string_free`. Returns false
/// when `index` is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_device_attachment(
    machine: *const RemanenceMachine,
    index: usize,
    attachment_out: *mut *mut c_char,
) -> bool {
    let Some(target) = (unsafe { machine.as_ref() }).and_then(RemanenceMachine::machine) else {
        return false;
    };
    let Some(device) = target.devices().get(index) else {
        return false;
    };
    if !attachment_out.is_null() {
        unsafe { *attachment_out = to_owned_c_char(&device.attachment().to_string()) };
    }
    true
}

/// A **borrowed** view of the device at `attachment` in this machine.
///
/// The session owns it; never free it. It stays valid until that device
/// is removed or the session is freed. Returns null when this machine
/// has no device there.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_machine_device(
    machine: *mut RemanenceMachine,
    attachment: *const c_char,
) -> *mut RemanenceDevice {
    let Some(handle) = (unsafe { machine.as_ref() }) else {
        return ptr::null_mut();
    };
    let session = handle.session;
    let identity = handle.identity.clone();
    unsafe { device_view(session, identity, attachment) }
}

/// A **borrowed** view of the device at `attachment` in the session's
/// anonymous machine — `remanence_machine_device` reaches a named
/// machine's.
///
/// The session owns it; never free it. It stays valid until that device
/// is removed or the session is freed. Returns null when nothing is
/// attached there.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_device(
    session: *mut RemanenceSession,
    attachment: *const c_char,
) -> *mut RemanenceDevice {
    unsafe { device_view(session, None, attachment) }
}

/// Adds a device in one machine of one session, and answers with the
/// borrowed view of it. Both spellings — the session's anonymous machine
/// and a named one — land here, because the act is the machine's either
/// way.
#[allow(clippy::too_many_arguments)]
unsafe fn add_device(
    session: *mut RemanenceSession,
    machine: Option<String>,
    family: *const c_char,
    index: Option<u32>,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { session.as_mut() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(family) = (unsafe { utf8_arg(family) }) else {
        let error = remanence::Error::io("null device family");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let family = match DeviceFamily::from_id(family.as_ref()) {
        Ok(family) => family,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let target = match &machine {
        Some(identity) => handle.session.machine_mut(identity),
        None => Some(handle.session.anonymous_mut()),
    };
    let Some(target) = target else {
        let error = remanence::Error::io("null or retired machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let added = match index {
        Some(index) => target.add_device_at(family, index),
        None => target.add_device(family),
    };
    let attachment = match added {
        Ok(device) => device.attachment(),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let attachment = to_cstring(&attachment.to_string());
    unsafe { device_view(session, machine, attachment.as_ptr()) }
}

/// Adds a device for one artifact in one machine of one session, and
/// answers with the borrowed view of it. Both spellings land here, as
/// they do for the two-act form.
#[allow(clippy::too_many_arguments)]
unsafe fn add_device_for(
    session: *mut RemanenceSession,
    machine: Option<String>,
    path: *const c_char,
    intent: RemanenceAccessIntent,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { session.as_mut() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(path) = (unsafe { utf8_arg(path) }) else {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let target = match &machine {
        Some(identity) => handle.session.machine_mut(identity),
        None => Some(handle.session.anonymous_mut()),
    };
    let Some(target) = target else {
        let error = remanence::Error::io("null or retired machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let attachment = match target.add_device_for(path.as_ref(), access_intent(intent)) {
        Ok(device) => device.attachment(),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let attachment = to_cstring(&attachment.to_string());
    unsafe { device_view(session, machine, attachment.as_ptr()) }
}

/// Removes a device from one machine of one session and invalidates every
/// borrowed view of it.
unsafe fn remove_device(
    session: *mut RemanenceSession,
    machine: Option<String>,
    attachment: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { session.as_mut() }) else {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let Some(attachment) = (unsafe { utf8_arg(attachment) }) else {
        let error = remanence::Error::io("null attachment");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let attachment = match AttachmentId::parse(attachment.as_ref()) {
        Ok(attachment) => attachment,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return false;
        }
    };
    let target = match &machine {
        Some(identity) => handle.session.machine_mut(identity),
        None => Some(handle.session.anonymous_mut()),
    };
    let Some(target) = target else {
        let error = remanence::Error::io("null or retired machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match target.remove_device(attachment) {
        Ok(()) => {
            handle
                .views
                .retain(|view| view.machine != machine || view.attachment != attachment);
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// The borrowed machine view for `identity`, reusing the one already
/// handed out where there is one so a caller's pointers stay stable.
unsafe fn machine_view(
    session: *mut RemanenceSession,
    identity: Option<String>,
) -> *mut RemanenceMachine {
    let Some(handle) = (unsafe { session.as_mut() }) else {
        return ptr::null_mut();
    };
    if let Some(at) = handle
        .machines
        .iter()
        .position(|view| view.identity == identity)
    {
        return handle.machines[at].as_mut() as *mut RemanenceMachine;
    }
    let identity_c = identity.as_deref().map(to_cstring);
    handle.machines.push(Box::new(RemanenceMachine {
        session,
        identity,
        identity_c,
    }));
    handle.machines.last_mut().expect("just pushed").as_mut() as *mut RemanenceMachine
}

/// The borrowed device view for one attachment in one machine, reusing
/// the one already handed out where there is one.
unsafe fn device_view(
    session: *mut RemanenceSession,
    machine: Option<String>,
    attachment: *const c_char,
) -> *mut RemanenceDevice {
    let Some(handle) = (unsafe { session.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(text) = (unsafe { utf8_arg(attachment) }) else {
        return ptr::null_mut();
    };
    let Ok(attachment) = AttachmentId::parse(text.as_ref()) else {
        return ptr::null_mut();
    };
    if let Some(at) = handle
        .views
        .iter()
        .position(|view| view.machine == machine && view.attachment == attachment)
    {
        return handle.views[at].as_mut() as *mut RemanenceDevice;
    }
    let target = match &machine {
        Some(identity) => handle.session.machine_mut(identity),
        None => Some(handle.session.anonymous_mut()),
    };
    let Some(device) = target.and_then(|target| target.device_mut(attachment)) else {
        return ptr::null_mut();
    };
    let path = device.path().ok().map(to_cstring);
    let image_path = device
        .image_path()
        .ok()
        .map(|image_path| to_cstring(&image_path.display().to_string()));
    handle.views.push(Box::new(RemanenceDevice {
        session,
        machine,
        attachment,
        attachment_c: to_cstring(&attachment.to_string()),
        family_c: to_cstring(attachment.family().id()),
        path,
        image_path,
    }));
    handle.views.last_mut().expect("just pushed").as_mut() as *mut RemanenceDevice
}

/// The medium's **effective** access mode: the declared intent's
/// echo where the evidence supports it, and read-only where it does not
/// (P28). `remanence_assurance_access_mode` reports the same value beside
/// the reason for it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_mode(device: *const RemanenceDevice) -> RemanenceAccessMode {
    unsafe { device.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |device| {
        access_mode(
            device.device()
                .and_then(|device| device.mode().ok())
                .unwrap_or(AccessMode::ReadOnly),
        )
    })
}

/// What an open established about the evidence beneath it (P28).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemanenceAssuranceOutcome {
    /// Every fact and bound the interpretation needs is evidenced.
    Verified = 0,
    /// A material shortfall is known and a bounded read-only reading
    /// remains.
    Degraded = 1,
    /// No bounded interpretation exists. This outcome arrives as a
    /// refusal, carrying the same condition as its rule identity, so no
    /// open handle ever reports it.
    Refused = 2,
}

/// One open's assurance state (P28). Free with
/// `remanence_assurance_free`; the strings it returns are owned by it.
pub struct RemanenceAssurance {
    outcome: RemanenceAssuranceOutcome,
    condition: Option<CString>,
    evidence: Vec<CString>,
    readable: Vec<remanence::ByteRange>,
    access: RemanenceAccessMode,
    declared_bytes: Option<u64>,
    observed_bytes: Option<u64>,
    first_unavailable_byte: Option<u64>,
}

/// The assurance of one open medium: what the open established, why, the
/// exact extents that read, and the access the evidence permits.
///
/// It is available before anything is read, so a caller meets a deficiency
/// by being told rather than by an operation failing halfway. Null only
/// when the device holding this medium was removed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_assurance(
    device: *const RemanenceDevice,
) -> *mut RemanenceAssurance {
    let Some(medium) = (unsafe { device.as_ref() }).and_then(RemanenceDevice::device) else {
        return ptr::null_mut();
    };
    let Ok(assurance) = medium.assurance() else {
        return ptr::null_mut();
    };
    Box::into_raw(Box::new(assurance_view(assurance)))
}

/// One assurance's C view. A discovery and the device that consumed it
/// report the same open, so they build the same record.
fn assurance_view(assurance: &remanence::Assurance) -> RemanenceAssurance {
    RemanenceAssurance {
        outcome: match assurance.outcome {
            remanence::AssuranceOutcome::Verified => RemanenceAssuranceOutcome::Verified,
            remanence::AssuranceOutcome::Degraded => RemanenceAssuranceOutcome::Degraded,
            remanence::AssuranceOutcome::Refused => RemanenceAssuranceOutcome::Refused,
        },
        condition: assurance
            .condition
            .map(|condition| to_cstring(condition.as_str())),
        evidence: evidence_views(&assurance.evidence),
        readable: assurance.readable.clone(),
        access: access_mode(assurance.access),
        declared_bytes: assurance.declared_bytes,
        observed_bytes: assurance.observed_bytes,
        first_unavailable_byte: assurance.first_unavailable_byte,
    }
}

/// Frees an assurance record and everything borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_free(assurance: *mut RemanenceAssurance) {
    if !assurance.is_null() {
        drop(unsafe { Box::from_raw(assurance) });
    }
}

/// What the open established.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_outcome(
    assurance: *const RemanenceAssurance,
) -> RemanenceAssuranceOutcome {
    unsafe { assurance.as_ref() }
        .map_or(RemanenceAssuranceOutcome::Verified, |assurance| assurance.outcome)
}

/// The stable condition that narrowed this session — `source-truncated`
/// or `evidence-conflict` — or null where nothing did. It is the same
/// identity a withheld operation's refusal carries as its rule.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_condition(
    assurance: *const RemanenceAssurance,
) -> *const c_char {
    unsafe { assurance.as_ref() }
        .and_then(|assurance| assurance.condition.as_ref())
        .map_or(ptr::null(), |condition| condition.as_ptr())
}

/// How many evidence lines the assurance carries, in the order they were
/// observed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_evidence_count(
    assurance: *const RemanenceAssurance,
) -> usize {
    unsafe { assurance.as_ref() }.map_or(0, |assurance| assurance.evidence.len())
}

/// One evidence line, or null when the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_evidence(
    assurance: *const RemanenceAssurance,
    index: usize,
) -> *const c_char {
    unsafe { assurance.as_ref() }
        .and_then(|assurance| assurance.evidence.get(index))
        .map_or(ptr::null(), |line| line.as_ptr())
}

/// How many readable extents the medium has.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_readable_count(
    assurance: *const RemanenceAssurance,
) -> usize {
    unsafe { assurance.as_ref() }.map_or(0, |assurance| assurance.readable.len())
}

/// One readable extent as a half-open byte range. False when the index is
/// out of range, leaving the outputs untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_readable(
    assurance: *const RemanenceAssurance,
    index: usize,
    start_out: *mut u64,
    end_out: *mut u64,
) -> bool {
    let Some(range) = (unsafe { assurance.as_ref() })
        .and_then(|assurance| assurance.readable.get(index))
    else {
        return false;
    };
    if !start_out.is_null() {
        unsafe { *start_out = range.start };
    }
    if !end_out.is_null() {
        unsafe { *end_out = range.end };
    }
    true
}

/// The access this session actually has.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_access_mode(
    assurance: *const RemanenceAssurance,
) -> RemanenceAccessMode {
    unsafe { assurance.as_ref() }
        .map_or(RemanenceAccessMode::ReadOnly, |assurance| assurance.access)
}

/// The size the interpretation declares. False where it declares none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_declared_bytes(
    assurance: *const RemanenceAssurance,
    out: *mut u64,
) -> bool {
    unsafe {
        write_opt_u64(
            assurance.as_ref().and_then(|assurance| assurance.declared_bytes),
            out,
        )
    }
}

/// The size the source actually holds. False where it is unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_observed_bytes(
    assurance: *const RemanenceAssurance,
    out: *mut u64,
) -> bool {
    unsafe {
        write_opt_u64(
            assurance.as_ref().and_then(|assurance| assurance.observed_bytes),
            out,
        )
    }
}

/// The first byte the source does not hold. False where the session is not
/// bounded short of its declaration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_first_unavailable_byte(
    assurance: *const RemanenceAssurance,
    out: *mut u64,
) -> bool {
    unsafe {
        write_opt_u64(
            assurance
                .as_ref()
                .and_then(|assurance| assurance.first_unavailable_byte),
            out,
        )
    }
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

/// The image container format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_format(
    device: *const RemanenceDevice,
    format_out: *mut RemanenceDiskFormat,
) -> bool {
    let Some(format) = (unsafe { device.as_ref() })
        .and_then(|device| device.device()?.format().ok())
    else {
        return false;
    };
    if !format_out.is_null() {
        unsafe {
            *format_out = match format {
                DiskFormat::Qcow2 { .. } => RemanenceDiskFormat::Qcow2,
                DiskFormat::Vdi { .. } => RemanenceDiskFormat::Vdi,
                DiskFormat::Raw => RemanenceDiskFormat::Raw,
            }
        };
    }
    true
}

/// The qcow2 version, or 0 for an image of any other format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_qcow2_version(device: *const RemanenceDevice) -> u32 {
    match unsafe { device.as_ref() }.and_then(|device| device.device()?.format().ok()) {
        Some(DiskFormat::Qcow2 { version }) => version,
        _ => 0,
    }
}

/// The VDI version's major part, or 0 for an image of any other format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_vdi_version_major(device: *const RemanenceDevice) -> u32 {
    match unsafe { device.as_ref() }.and_then(|device| device.device()?.format().ok()) {
        Some(DiskFormat::Vdi { major, .. }) => major,
        _ => 0,
    }
}

/// The VDI version's minor part, or 0 for an image of any other format.
/// Read it beside the major part: on its own, 0 is both "minor zero" and
/// "not a VDI".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_vdi_version_minor(device: *const RemanenceDevice) -> u32 {
    match unsafe { device.as_ref() }.and_then(|device| device.device()?.format().ok()) {
        Some(DiskFormat::Vdi { minor, .. }) => minor,
        _ => 0,
    }
}

/// The virtual disk size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_size(device: *const RemanenceDevice) -> u64 {
    unsafe { device.as_ref() }
        .and_then(|device| device.device()?.size().ok())
        .unwrap_or(0)
}

/// Whether uncommitted changes exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_is_modified(device: *const RemanenceDevice) -> bool {
    unsafe { device.as_ref() }
        .and_then(|device| device.device()?.is_modified().ok())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// The namespace surface (P19): file access lives on one node.
//
// A device carries no file verbs at all. It answers what it *resolves*
// to — `remanence_device_filesystem`, and `remanence_device_volume` where
// several answers exist — and the verbs live on the filesystem that
// resolver hands back.
//
// The three handles below name their provider by session, machine,
// attachment and volume identity rather than by pointer, and re-resolve
// on every call: a medium that has been ejected answers by name instead
// of reaching state that has left.

/// One volume of a device's medium, selected by the identity the
/// inspection report issued. Free with `remanence_space_free`.
pub struct RemanenceSpace {
    session: *mut RemanenceSession,
    machine: Option<String>,
    attachment: AttachmentId,
    /// The volume that composed it, where it has the addressable
    /// vantage. `None` where the medium bears its namespace directly.
    volume: Option<u64>,
    start_bytes: u64,
    length_bytes: u64,
    /// The namespace kind, where it has the namespace vantage.
    kind: Option<CString>,
}



/// One file, named by the filesystem that holds it. Free with
/// `remanence_file_free`.
pub struct RemanenceFile {
    session: *mut RemanenceSession,
    machine: Option<String>,
    attachment: AttachmentId,
    volume: Option<u64>,
    path: CString,
    name: CString,
    kind: RemanenceEntryKind,
    size_bytes: u64,
}

/// Resolves the named filesystem and runs `action` over it.
///
/// Every verb below passes through here, so the refusals a caller meets
/// are the library's own — the resolver's where the walk had no single
/// answer, the namespace's where it did.
unsafe fn with_space<T>(
    session: *mut RemanenceSession,
    machine: &Option<String>,
    attachment: AttachmentId,
    volume: Option<u64>,
    action: impl FnOnce(&mut remanence::StorageSpace<'_>) -> remanence::Result<T>,
) -> remanence::Result<T> {
    let handle =
        unsafe { session.as_mut() }.ok_or_else(|| remanence::Error::io("null session"))?;
    let target = match machine {
        Some(identity) => handle.session.machine_mut(identity),
        None => Some(handle.session.anonymous_mut()),
    };
    let device = target
        .and_then(|target| target.device_mut(attachment))
        .ok_or_else(|| remanence::Error::io("the device holding this medium was removed"))?;
    let mut space = match volume {
        Some(id) => device.volume(VolumeId::from_value(id))?,
        None => device.filesystem()?,
    };
    action(&mut space)
}

/// The space this device resolves to, or null with the refusal set.
///
/// The walk device to volume to namespace is transparent where every
/// seam has exactly one supported answer, and refuses naming the
/// candidates where one does not. A volume bearing no filesystem is a
/// named absence, not an empty listing. Free with
/// `remanence_space_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_filesystem(
    device: *mut RemanenceDevice,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceSpace {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { device.as_mut() }) else {
        return ptr::null_mut();
    };
    let (session, machine, attachment) =
        (handle.session, handle.machine.clone(), handle.attachment);
    match unsafe {
        with_space(session, &machine, attachment, None, |space| {
            Ok((
                space.kind()?.to_owned(),
                space.volume_id(),
                space.start_bytes().unwrap_or(0),
                space.length_bytes().unwrap_or(0),
            ))
        })
    } {
        Ok((kind, volume, start_bytes, length_bytes)) => Box::into_raw(Box::new(RemanenceSpace {
            session,
            machine,
            attachment,
            volume: volume.map(VolumeId::value),
            start_bytes,
            length_bytes,
            kind: Some(to_cstring(&kind)),
        })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// One space of this device's medium, by the identity the inspection
/// report issued for its volume — the selector where several namespaces
/// exist, and the way to reach a volume bearing none. Free with
/// `remanence_space_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_volume(
    device: *mut RemanenceDevice,
    volume_id: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceSpace {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { device.as_mut() }) else {
        return ptr::null_mut();
    };
    let (session, machine, attachment) =
        (handle.session, handle.machine.clone(), handle.attachment);
    let Some(medium) = handle.device() else {
        let error = remanence::Error::io("the device holding this medium was removed");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match medium.volume(VolumeId::from_value(volume_id)) {
        Ok(space) => {
            let start_bytes = space.start_bytes().unwrap_or(0);
            let length_bytes = space.length_bytes().unwrap_or(0);
            // A volume bearing no namespace is an ordinary volume, so the
            // absence travels on the handle rather than failing here.
            let kind = space.kind().ok().map(to_cstring);
            Box::into_raw(Box::new(RemanenceSpace {
                session,
                machine,
                attachment,
                volume: Some(volume_id),
                start_bytes,
                length_bytes,
                kind,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a space handle. The device and its medium are untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_space_free(space: *mut RemanenceSpace) {
    if !space.is_null() {
        drop(unsafe { Box::from_raw(space) });
    }
}

/// Whether this space has the addressable vantage — an extent to read and
/// write by position. False where the medium bears its namespace directly
/// and composed no volume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_volume_is_addressable(space: *const RemanenceSpace) -> bool {
    unsafe { space.as_ref() }.is_some_and(|space| space.volume.is_some())
}

/// This space's opaque volume identity, as the inspection report issued
/// it, or 0 where it has no addressable vantage —
/// `remanence_volume_is_addressable` distinguishes the two.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_volume_id(space: *const RemanenceSpace) -> u64 {
    unsafe { space.as_ref() }.map_or(0, |space| space.volume.unwrap_or(0))
}

/// Where this space starts in the presented disk, or 0 where it has no
/// addressable vantage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_volume_start_bytes(space: *const RemanenceSpace) -> u64 {
    unsafe { space.as_ref() }.map_or(0, |space| space.start_bytes)
}

/// How far this space runs, or 0 where it has no addressable vantage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_volume_length_bytes(space: *const RemanenceSpace) -> u64 {
    unsafe { space.as_ref() }.map_or(0, |space| space.length_bytes)
}

/// Reads `len` bytes at `offset` **within this space**, not within the
/// medium: the vantage that reaches a boot record, allocation metadata,
/// the extents a filesystem calls free, or the bytes behind a listed
/// file. A read past the space's own end is refused by name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_volume_read_at(
    space: *mut RemanenceSpace,
    offset: u64,
    buffer: *mut u8,
    len: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { space.as_mut() }) else {
        return false;
    };
    if buffer.is_null() && len != 0 {
        return false;
    }
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, len) };
    match unsafe {
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |space| space.read_at(offset, buf),
        )
    } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Writes `len` bytes at `offset` within this space, buffered until
/// `remanence_device_commit` like every other write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_volume_write_at(
    space: *mut RemanenceSpace,
    offset: u64,
    data: *const u8,
    len: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { space.as_mut() }) else {
        return false;
    };
    if data.is_null() && len != 0 {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match unsafe {
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |space| space.write_at(offset, bytes),
        )
    } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Whether this space has the namespace vantage — files to name. False
/// for a volume bearing no filesystem, which is an ordinary volume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_has_namespace(space: *const RemanenceSpace) -> bool {
    unsafe { space.as_ref() }.is_some_and(|space| space.kind.is_some())
}

/// The filesystem kind in its stable spelling, `"FAT12"` or `"hdos"`, or
/// null where this space bears no namespace.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_kind(space: *const RemanenceSpace) -> *const c_char {
    unsafe { space.as_ref() }
        .and_then(|space| space.kind.as_ref())
        .map_or(ptr::null(), |kind| kind.as_ptr())
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
    match unsafe {
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.entries(path.as_ref()),
        )
    } {
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
    match unsafe {
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.stat(path.as_ref()),
        )
    } {
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

/// Frees a directory listing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_entry_list_free(list: *mut RemanenceEntryList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

unsafe fn entry_view<'a>(list: *const RemanenceEntryList, index: usize) -> Option<&'a EntryView> {
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
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| {
                let file = target.get_file(path.as_ref())?;
                Ok((
                    file.name().to_owned(),
                    entry_kind(file.entry().kind),
                    file.size_bytes(),
                ))
            },
        )
    } {
        Ok((name, kind, size_bytes)) => Box::into_raw(Box::new(RemanenceFile {
            session: handle.session,
            machine: handle.machine.clone(),
            attachment: handle.attachment,
            volume: handle.volume,
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
/// `remanence_device_load_discovery`. Returns null on failure.
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
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.get_file(path.as_ref())?.discover(),
        )
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
    match unsafe {
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.read_file(path.as_ref()),
        )
    } {
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
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.resize_file(path.as_ref(), size),
        )
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
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.write_file(path.as_ref(), contents),
        )
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
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.make_directory(path.as_ref()),
        )
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
    match unsafe {
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.get_file(&path)?.bytes(),
        )
    } {
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
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.get_file(&path)?.read_at(offset, buffer),
        )
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
        with_space(
            handle.session,
            &handle.machine,
            handle.attachment,
            handle.volume,
            |target| target.get_file(&path)?.write_at(offset, data),
        )
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

/// The commit point (P2): everything buffered reaches the image, then a
/// flush. Until this call, nothing has touched the file. The commit is
/// durable (P9): a private recovery journal is armed before the first
/// byte of the file changes, so an interruption at any point leaves
/// state the next open reconciles to wholly the old image or wholly
/// the committed new one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_commit(
    device: *mut RemanenceDevice,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(device) = (unsafe { device.as_mut() }) else {
        return false;
    };
        let Some(medium) = device.device() else {
        let error = remanence::Error::io("the device holding this medium was removed");
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
pub unsafe extern "C" fn remanence_device_rollback(device: *mut RemanenceDevice) {
    if let Some(device) = unsafe { device.as_mut() } {
        if let Some(medium) = device.device() {
            let _ = medium.rollback();
        }
    }
}

// ---------------------------------------------------------------------------
// The KryoFlux capture set: one disk spread over a stream per head per
// drive-step position, recognized from a catalog subtree and reported as
// the adapter recognized it. Counts, identities and shapes cross this
// surface; the pulses stay behind it.

use remanence::CaptureSet;

struct ObservationView {
    ordinal: u64,
    span_ticks: u64,
    transitions: u64,
    markers: u64,
}

struct CaptureRunView {
    ordinal: u64,
    transitions: u64,
    extent_ticks: u64,
    markers: u64,
    index_markers: u64,
    transfer_result: Option<u32>,
    transitions_before_first_index: u64,
    transitions_after_last_index: u64,
    observations: Vec<ObservationView>,
}

struct CaptureIssueView {
    code: CString,
    detail: CString,
}

struct CaptureMemberView {
    entry_name: CString,
    entry_bytes: u64,
    position_numerator: u64,
    position_denominator: u64,
    head: Option<u64>,
    runs: Vec<CaptureRunView>,
    issues: Vec<CaptureIssueView>,
}

/// An open capture set, holding the claim on its archive.
pub struct RemanenceCaptureSet {
    set: CaptureSet,
    path: CString,
    subtree: Option<CString>,
    format_id: CString,
    format_name: CString,
    archive_format_id: CString,
    evidence: Vec<CString>,
    members: Vec<CaptureMemberView>,
}

impl RemanenceCaptureSet {
    fn new(set: CaptureSet) -> Self {
        let report = set.inspect();
        let members = report
            .members
            .iter()
            .map(|member| CaptureMemberView {
                entry_name: to_cstring(&member.entry_name),
                entry_bytes: member.entry_bytes,
                position_numerator: member.position.numerator,
                position_denominator: member.position.denominator,
                head: member.head,
                runs: member
                    .runs
                    .iter()
                    .map(|run| CaptureRunView {
                        ordinal: run.ordinal,
                        transitions: run.transitions,
                        extent_ticks: run.extent_ticks,
                        markers: run.markers,
                        index_markers: run.index_markers,
                        transfer_result: run.transfer_result,
                        transitions_before_first_index: run.transitions_before_first_index,
                        transitions_after_last_index: run.transitions_after_last_index,
                        observations: run
                            .observations
                            .iter()
                            .map(|observation| ObservationView {
                                ordinal: observation.ordinal,
                                span_ticks: observation.span_ticks,
                                transitions: observation.transitions,
                                markers: observation.markers,
                            })
                            .collect(),
                    })
                    .collect(),
                issues: member
                    .issues
                    .iter()
                    .map(|issue| CaptureIssueView {
                        code: to_cstring(&issue.code),
                        detail: to_cstring(&issue.detail),
                    })
                    .collect(),
            })
            .collect();
        let evidence = report.evidence.iter().map(|line| to_cstring(line)).collect();
        let path = to_cstring(&set.path().display().to_string());
        let subtree = set.subtree().map(to_cstring);
        let format_id = to_cstring(set.format_id());
        let format_name = to_cstring(set.format_name());
        let archive_format_id = to_cstring(set.archive_format_id());
        Self {
            set,
            path,
            subtree,
            format_id,
            format_name,
            archive_format_id,
            evidence,
            members,
        }
    }
}

unsafe fn capture_member<'a>(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> Option<&'a CaptureMemberView> {
    unsafe { set.as_ref() }?.members.get(member)
}

unsafe fn capture_run<'a>(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> Option<&'a CaptureRunView> {
    unsafe { capture_member(set, member) }?.runs.get(run)
}

unsafe fn capture_observation<'a>(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> Option<&'a ObservationView> {
    unsafe { capture_run(set, member, run) }?
        .observations
        .get(observation)
}

unsafe fn open_capture_set(
    path: *const c_char,
    cache_bytes: Option<u64>,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceCaptureSet {
    unsafe { clear_error(error_out, error_rule_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    let opened = match cache_bytes {
        Some(cache_bytes) => CaptureSet::open_with_cache(path.as_ref(), cache_bytes),
        None => CaptureSet::open(path.as_ref()),
    };
    match opened {
        Ok(set) => Box::into_raw(Box::new(RemanenceCaptureSet::new(set))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Opens the KryoFlux capture set held by `path` (UTF-8) — an archive
/// this library reads, optionally followed by the subtree inside it that
/// holds the members — with the stated default session cache bound.
/// An incomplete, duplicate, contradictory, or unrelated member refuses
/// the whole set. Returns null on failure and stores a message in
/// `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_open(
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceCaptureSet {
    unsafe { open_capture_set(path, None, error_category_out, error_out, error_rule_out) }
}

/// Opens a capture set as `remanence_capture_set_open` does, under a
/// declared cache bound: at most `cache_bytes` of the decoded capture
/// stays resident. The bound narrows the working set; it never refuses
/// service.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_open_with_cache(
    path: *const c_char,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceCaptureSet {
    unsafe { open_capture_set(path, Some(cache_bytes), error_category_out, error_out, error_rule_out) }
}

/// Frees a capture-set handle, releasing its claim on the archive and
/// discarding the private session storage the capture decoded into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_free(set: *mut RemanenceCaptureSet) {
    if !set.is_null() {
        drop(unsafe { Box::from_raw(set) });
    }
}

/// The path the set was opened from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_path(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.path.as_ptr())
}

/// The subtree inside the archive the members were read from, or null
/// when the whole archive is the set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_subtree(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| {
        set.subtree
            .as_ref()
            .map_or(ptr::null(), |subtree| subtree.as_ptr())
    })
}

/// The capture format's stable identifier, "kryoflux".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_format_id(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.format_id.as_ptr())
}

/// The capture format's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_format_name(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.format_name.as_ptr())
}

/// The archive grammar the members were read through, e.g. "7z".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_archive_format_id(
    set: *const RemanenceCaptureSet,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| set.archive_format_id.as_ptr())
}

/// Which P7 mode the open obtained on the archive file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_access_mode(
    set: *const RemanenceCaptureSet,
) -> RemanenceAccessMode {
    unsafe { set.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |set| {
        access_mode(set.set.access_mode())
    })
}

/// The capture's declared timing basis, as an exact ratio of ticks per
/// second. Returns false when the handle is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_ticks_per_second(
    set: *const RemanenceCaptureSet,
    numerator_out: *mut u64,
    denominator_out: *mut u64,
) -> bool {
    let Some(set) = (unsafe { set.as_ref() }) else {
        return false;
    };
    let base = set.set.inspect().time_base;
    if !numerator_out.is_null() {
        unsafe { *numerator_out = base.ticks_per_second_numerator };
    }
    if !denominator_out.is_null() {
        unsafe { *denominator_out = base.ticks_per_second_denominator };
    }
    true
}

/// How many bytes of private session storage the decoded capture
/// occupies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_backing_bytes(
    set: *const RemanenceCaptureSet,
) -> u64 {
    unsafe { set.as_ref() }.map_or(0, |set| set.set.backing_bytes())
}

/// How much of that backing is currently resident. The capture is never
/// held whole.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_resident_bytes(
    set: *const RemanenceCaptureSet,
) -> u64 {
    unsafe { set.as_ref() }.map_or(0, |set| set.set.resident_bytes())
}

/// Number of evidence lines behind the recognition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_evidence_count(
    set: *const RemanenceCaptureSet,
) -> usize {
    unsafe { set.as_ref() }.map_or(0, |set| set.evidence.len())
}

/// One evidence line, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_evidence(
    set: *const RemanenceCaptureSet,
    index: usize,
) -> *const c_char {
    unsafe { set.as_ref() }.map_or(ptr::null(), |set| {
        set.evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// Number of members the set holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_count(
    set: *const RemanenceCaptureSet,
) -> usize {
    unsafe { set.as_ref() }.map_or(0, |set| set.members.len())
}

/// One member's catalog identity, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_entry_name(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> *const c_char {
    unsafe { capture_member(set, member) }.map_or(ptr::null(), |member| member.entry_name.as_ptr())
}

/// One member's size in bytes as the catalog declares it; 0 when out of
/// range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_entry_bytes(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> u64 {
    unsafe { capture_member(set, member) }.map_or(0, |member| member.entry_bytes)
}

/// One member's drive-step position, as an exact ratio. Returns false
/// when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_position(
    set: *const RemanenceCaptureSet,
    member: usize,
    numerator_out: *mut u64,
    denominator_out: *mut u64,
) -> bool {
    let Some(member) = (unsafe { capture_member(set, member) }) else {
        return false;
    };
    if !numerator_out.is_null() {
        unsafe { *numerator_out = member.position_numerator };
    }
    if !denominator_out.is_null() {
        unsafe { *denominator_out = member.position_denominator };
    }
    true
}

/// The head that captured this position; returns false when the source
/// numbers no head, which is a different fact from head zero, or when
/// the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_head(
    set: *const RemanenceCaptureSet,
    member: usize,
    out: *mut u64,
) -> bool {
    let Some(member) = (unsafe { capture_member(set, member) }) else {
        return false;
    };
    unsafe { write_opt_u64(member.head, out) }
}

/// Number of things recorded as qualified about this member.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_issue_count(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> usize {
    unsafe { capture_member(set, member) }.map_or(0, |member| member.issues.len())
}

/// One issue's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_issue_code(
    set: *const RemanenceCaptureSet,
    member: usize,
    issue: usize,
) -> *const c_char {
    unsafe { capture_member(set, member) }.map_or(ptr::null(), |member| {
        member
            .issues
            .get(issue)
            .map_or(ptr::null(), |issue| issue.code.as_ptr())
    })
}

/// One issue's human-readable detail, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_issue_detail(
    set: *const RemanenceCaptureSet,
    member: usize,
    issue: usize,
) -> *const c_char {
    unsafe { capture_member(set, member) }.map_or(ptr::null(), |member| {
        member
            .issues
            .get(issue)
            .map_or(ptr::null(), |issue| issue.detail.as_ptr())
    })
}

/// Number of source transfers recorded at this member's location.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_member_run_count(
    set: *const RemanenceCaptureSet,
    member: usize,
) -> usize {
    unsafe { capture_member(set, member) }.map_or(0, |member| member.runs.len())
}

/// One run's place in the member's recorded order; 0 when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_ordinal(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.ordinal)
}

/// How many flux transitions the run recorded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transitions(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.transitions)
}

/// The last transition's tick: the extent of what was recorded, not a
/// circumference. A run states no period.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_extent_ticks(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.extent_ticks)
}

/// How many timed markers sit on channels parallel to the run's flux.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_markers(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.markers)
}

/// How many of those markers are index events.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_index_markers(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.index_markers)
}

/// The result the capture tool declared for this transfer, where it
/// declared one; zero is a clean read. Returns false when it declared
/// none or the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transfer_result(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    out: *mut u32,
) -> bool {
    let Some(run) = (unsafe { capture_run(set, member, run) }) else {
        return false;
    };
    match run.transfer_result {
        Some(result) => {
            if !out.is_null() {
                unsafe { *out = result };
            }
            true
        }
        None => false,
    }
}

/// Transitions recorded before the run's first index: evidence that
/// bounding into circular observations does not consume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transitions_before_first_index(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.transitions_before_first_index)
}

/// Transitions recorded after the run's last index, on the same terms.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_transitions_after_last_index(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> u64 {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.transitions_after_last_index)
}

/// How many circular observations the run's indices bounded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_run_observation_count(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
) -> usize {
    unsafe { capture_run(set, member, run) }.map_or(0, |run| run.observations.len())
}

/// One observation's place in the location's source-record order. Not a
/// rank: nothing here says it is a good or complete revolution.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_ordinal(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.ordinal)
}

/// The observation's declared circumference, in the capture's own ticks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_span_ticks(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.span_ticks)
}

/// How many transitions the observation holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_transitions(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.transitions)
}

/// How many markers the observation holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_observation_markers(
    set: *const RemanenceCaptureSet,
    member: usize,
    run: usize,
    observation: usize,
) -> u64 {
    unsafe { capture_observation(set, member, run, observation) }
        .map_or(0, |observation| observation.markers)
}

// ---------------------------------------------------------------------------
// Drive-profile recognition: which family's conventions a capture was
// recorded under, ranked, with the observations that produced the
// verdict. A count, a density, an angle and an absence cross this
// surface; nothing that was decoded, because nothing was.

use remanence::Recognition;

/// One zone as a profile declares it, and what the capture recovered of
/// it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceZoneClaim {
    pub first_location: u64,
    pub last_location: u64,
    /// What the family claims one location in this zone holds.
    pub records_declared: u32,
    pub locations_declared: u64,
    pub locations_claimed: u64,
    /// The cell this zone claims, in thousandths of a reference cycle.
    pub nominal_cell_millicycles: u64,
}

/// What the probe found at one source position. Every `has_*` flag says
/// whether the value beside it was established at all: an absence is a
/// finding, not a zero.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceLocationVerdict {
    pub position_numerator: u64,
    pub position_denominator: u64,
    pub has_head: bool,
    pub head: u64,
    /// The family location this position addresses, where the family's
    /// addressing covers it at all.
    pub has_family_location: bool,
    pub family_location: u64,
    pub has_zone: bool,
    pub zone: u32,
    pub records: u32,
    /// The bit distance between record starts, where it repeats.
    pub has_record_bits: bool,
    pub record_bits: u64,
    /// How far that spacing departs from its own median. Zero is a
    /// spacing that repeats to the bit.
    pub record_bits_deviation: u64,
    /// The one departure from it, as an angle in reference-clock cycles.
    pub has_seam: bool,
    pub seam_cycles: u64,
    /// The derived cell projected onto the family's nominal rotation,
    /// in thousandths of a reference cycle, beside what the zone claims.
    pub has_cell: bool,
    pub cell_millicycles: u64,
    pub has_nominal_cell: bool,
    pub nominal_cell_millicycles: u64,
    /// How much of the interval population classified, per thousand.
    pub resolved_permille: u32,
    pub observations: u32,
    pub observations_agreeing: u32,
    /// The adjacent position holding the same content, where one does.
    pub has_duplicate: bool,
    pub duplicate_numerator: u64,
    pub duplicate_denominator: u64,
    pub claimed: bool,
}

struct VerdictView {
    profile_id: CString,
    profile_name: CString,
    evidence: Vec<CString>,
    artifacts: Vec<CString>,
    refusals: Vec<Option<CString>>,
}

/// A recognition result, ranked highest confidence first.
pub struct RemanenceRecognition {
    recognition: Recognition,
    pinned: Option<CString>,
    evidence: Vec<CString>,
    verdicts: Vec<VerdictView>,
}

impl RemanenceRecognition {
    fn new(recognition: Recognition) -> Self {
        let pinned = recognition.pinned.as_deref().map(to_cstring);
        let evidence = recognition.evidence.iter().map(|line| to_cstring(line)).collect();
        let verdicts = recognition
            .verdicts
            .iter()
            .map(|verdict| VerdictView {
                profile_id: to_cstring(&verdict.profile_id),
                profile_name: to_cstring(&verdict.profile_name),
                evidence: verdict.evidence.iter().map(|line| to_cstring(line)).collect(),
                artifacts: verdict
                    .locations
                    .iter()
                    .map(|location| to_cstring(&location.artifact))
                    .collect(),
                refusals: verdict
                    .locations
                    .iter()
                    .map(|location| location.refusal.as_deref().map(to_cstring))
                    .collect(),
                            })
            .collect();
        Self {
            recognition,
            pinned,
            evidence,
            verdicts,
        }
    }
}

unsafe fn recognition_verdict<'a>(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> Option<(&'a remanence::ProfileVerdict, &'a VerdictView)> {
    let recognition = unsafe { recognition.as_ref() }?;
    Some((
        recognition.recognition.verdicts.get(verdict)?,
        recognition.verdicts.get(verdict)?,
    ))
}

/// Recognizes the drive family a capture set was recorded under. Every
/// enrolled profile is consulted and what claims the capture is ranked;
/// a capture no profile claims is a named refusal. Returns null on
/// failure and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_recognize(
    set: *const RemanenceCaptureSet,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceRecognition {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(set) = (unsafe { set.as_ref() }) else {
        let error = remanence::Error::io("null capture set");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match set.set.recognize() {
        Ok(recognition) => Box::into_raw(Box::new(RemanenceRecognition::new(recognition))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Recognizes the capture against one named profile, whether or not it
/// would have won the ranking. A profile this build does not enroll is
/// refused by name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_recognize_as(
    set: *const RemanenceCaptureSet,
    profile_id: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceRecognition {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(set), false) = (unsafe { set.as_ref() }, profile_id.is_null()) else {
        let error = remanence::Error::io("null capture set or profile id");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let id = String::from_utf8_lossy(unsafe { CStr::from_ptr(profile_id) }.to_bytes());
    match set.set.recognize_as(id.as_ref()) {
        Ok(recognition) => Box::into_raw(Box::new(RemanenceRecognition::new(recognition))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a recognition handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_free(recognition: *mut RemanenceRecognition) {
    if !recognition.is_null() {
        drop(unsafe { Box::from_raw(recognition) });
    }
}

/// The profile the caller pinned, or null when the ranking was open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_pinned(
    recognition: *const RemanenceRecognition,
) -> *const c_char {
    unsafe { recognition.as_ref() }.map_or(ptr::null(), |recognition| {
        recognition
            .pinned
            .as_ref()
            .map_or(ptr::null(), |pinned| pinned.as_ptr())
    })
}

/// Number of evidence lines about the recognition itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_evidence_count(
    recognition: *const RemanenceRecognition,
) -> usize {
    unsafe { recognition.as_ref() }.map_or(0, |recognition| recognition.evidence.len())
}

/// One of those lines, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_evidence(
    recognition: *const RemanenceRecognition,
    index: usize,
) -> *const c_char {
    unsafe { recognition.as_ref() }.map_or(ptr::null(), |recognition| {
        recognition
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many profiles claimed the capture, highest confidence first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_verdict_count(
    recognition: *const RemanenceRecognition,
) -> usize {
    unsafe { recognition.as_ref() }.map_or(0, |recognition| recognition.verdicts.len())
}

/// One verdict's profile identifier, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_profile_id(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(ptr::null(), |(_, view)| view.profile_id.as_ptr())
}

/// One verdict's human-readable family name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_profile_name(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(ptr::null(), |(_, view)| view.profile_name.as_ptr())
}

/// Detection confidence, 0-100. Never an answer on its own: read the
/// evidence beside it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_confidence(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> u8 {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.confidence)
}

/// How many of the profile's declared locations the capture claimed,
/// and how many it declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_locations_claimed(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> u32 {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.locations_claimed)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_locations_declared(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> u64 {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.locations_declared)
}

/// Number of evidence lines behind this verdict.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_verdict_evidence_count(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> usize {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(_, view)| view.evidence.len())
}

/// One of those lines, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_verdict_evidence(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    index: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }.map_or(ptr::null(), |(_, view)| {
        view.evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many density zones the profile declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_zone_count(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> usize {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.zones.len())
}

/// One zone, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_zone(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    zone: usize,
    out: *mut RemanenceZoneClaim,
) -> bool {
    let Some((verdict, _)) = (unsafe { recognition_verdict(recognition, verdict) }) else {
        return false;
    };
    let Some(claim) = verdict.zones.get(zone) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceZoneClaim {
                first_location: claim.first_location,
                last_location: claim.last_location,
                records_declared: claim.records_declared,
                locations_declared: claim.locations_declared,
                locations_claimed: claim.locations_claimed,
                nominal_cell_millicycles: claim.nominal_cell_millicycles,
            };
        }
    }
    true
}

/// How many source positions the probe accounted for.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location_count(
    recognition: *const RemanenceRecognition,
    verdict: usize,
) -> usize {
    unsafe { recognition_verdict(recognition, verdict) }
        .map_or(0, |(verdict, _)| verdict.locations.len())
}

/// One position's findings, written into `out`. Returns false when out
/// of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    location: usize,
    out: *mut RemanenceLocationVerdict,
) -> bool {
    let Some((verdict, _)) = (unsafe { recognition_verdict(recognition, verdict) }) else {
        return false;
    };
    let Some(found) = verdict.locations.get(location) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceLocationVerdict {
                position_numerator: found.position.numerator,
                position_denominator: found.position.denominator,
                has_head: found.head.is_some(),
                head: found.head.unwrap_or(0),
                has_family_location: found.family_location.is_some(),
                family_location: found.family_location.unwrap_or(0),
                has_zone: found.zone.is_some(),
                zone: found.zone.unwrap_or(0),
                records: found.records,
                has_record_bits: found.record_bits.is_some(),
                record_bits: found.record_bits.unwrap_or(0),
                record_bits_deviation: found.record_bits_deviation,
                has_seam: found.seam_cycles.is_some(),
                seam_cycles: found.seam_cycles.unwrap_or(0),
                has_cell: found.cell_millicycles.is_some(),
                cell_millicycles: found.cell_millicycles.unwrap_or(0),
                has_nominal_cell: found.nominal_cell_millicycles.is_some(),
                nominal_cell_millicycles: found.nominal_cell_millicycles.unwrap_or(0),
                resolved_permille: found.resolved_permille,
                observations: found.observations,
                observations_agreeing: found.observations_agreeing,
                has_duplicate: found.duplicate_of.is_some(),
                duplicate_numerator: found.duplicate_of.map_or(0, |of| of.numerator),
                duplicate_denominator: found.duplicate_of.map_or(0, |of| of.denominator),
                claimed: found.claimed,
            };
        }
    }
    true
}

/// The member one position was read from, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location_artifact(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    location: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }.map_or(ptr::null(), |(_, view)| {
        view.artifacts
            .get(location)
            .map_or(ptr::null(), |artifact| artifact.as_ptr())
    })
}

/// Why a position was not claimed, in the profile's own terms; null when
/// it was claimed or the index is out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_recognition_location_refusal(
    recognition: *const RemanenceRecognition,
    verdict: usize,
    location: usize,
) -> *const c_char {
    unsafe { recognition_verdict(recognition, verdict) }.map_or(ptr::null(), |(_, view)| {
        view.refusals
            .get(location)
            .and_then(Option::as_ref)
            .map_or(ptr::null(), |refusal| refusal.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// C1541 mastering: reducing an opened capture to one half-track-addressed
// flux medium under a declared policy. Every reduction is a named input,
// the plan writes nothing, and the loss is declared before the medium
// exists.

use remanence::{MasteredMedium, MasteringPlan};

/// What to do with a location whose content its neighbour also holds.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDuplicatePolicy {
    /// Take the profile's declaration, which for a 1541 refuses.
    Declared = 0,
    AdmitAsObserved = 1,
    Omit = 2,
}

/// What a projection does with two transitions landing on one cycle.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceProjectionPolicy {
    Refuse = 0,
    DeclareLoss = 1,
}

/// How the selected evidence becomes pulse strength.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanencePulseStrengthPolicy {
    /// Every pulse carries `strength_state`; disagreement across the
    /// unselected observations is declared loss rather than expressed.
    Declared = 0,
    /// A pulse every observation places within `strength_window_cycles`
    /// is strong; one only some corroborate is weak.
    FromAgreement = 1,
}

/// Where the medium's circle begins.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceOriginPolicy {
    /// The track's own seam, which is what the profile declares.
    Declared = 0,
    /// The angle in `origin_cycles`, stated outright by the caller.
    Angle = 1,
}

/// The complete declared policy for one reduction. There is no default:
/// every field is a decision about evidence, and a reduction no input
/// names is a refusal.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceMasteringPolicy {
    /// The captured head supplying the family's one recorded surface.
    pub side: u64,
    /// Which observation of each location is used.
    pub observation_ordinal: u64,
    pub duplicate: RemanenceDuplicatePolicy,
    pub projection: RemanenceProjectionPolicy,
    pub pulse_strength: RemanencePulseStrengthPolicy,
    pub strength_state: u32,
    pub strength_window_cycles: u64,
    pub origin: RemanenceOriginPolicy,
    pub origin_cycles: u64,
    /// What makes any stochastic element reproducible.
    pub seed: u64,
}

/// One half-track the medium will hold.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceMasteredLocation {
    pub source_position_numerator: u64,
    pub source_position_denominator: u64,
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub observation_ordinal: u64,
    pub pulses: u64,
    pub strong_pulses: u64,
    pub weak_pulses: u64,
    pub origin_cycles: u64,
    pub has_seam: bool,
    pub seam_cycles: u64,
}

struct PlanView {
    profile_id: CString,
    origin_rule: CString,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
    evidence: Vec<CString>,
}

impl PlanView {
    fn new(report: &remanence::MasteringPlanReport) -> Self {
        Self {
            profile_id: to_cstring(&report.profile_id),
            origin_rule: to_cstring(&report.origin_rule),
            loss_codes: report.declared_loss.iter().map(|l| to_cstring(&l.code)).collect(),
            loss_details: report
                .declared_loss
                .iter()
                .map(|l| to_cstring(&l.detail))
                .collect(),
            evidence: report.evidence.iter().map(|line| to_cstring(line)).collect(),
        }
    }
}

/// A planned reduction: everything computed, nothing written.
pub struct RemanenceMasteringPlan {
    plan: Option<MasteringPlan>,
    report: remanence::MasteringPlanReport,
    view: PlanView,
}

/// A mastered medium, held in the session.
pub struct RemanenceMasteredMedium {
    medium: MasteredMedium,
    report: remanence::MasteringPlanReport,
    view: PlanView,
}

fn to_policy(policy: &RemanenceMasteringPolicy) -> remanence::MasteringPolicy {
    remanence::MasteringPolicy {
        side: policy.side,
        observation: remanence::ObservationPolicy::Selected {
            ordinal: policy.observation_ordinal,
        },
        duplicate: match policy.duplicate {
            RemanenceDuplicatePolicy::Declared => remanence::DuplicatePolicy::Declared,
            RemanenceDuplicatePolicy::AdmitAsObserved => {
                remanence::DuplicatePolicy::AdmitAsObserved
            }
            RemanenceDuplicatePolicy::Omit => remanence::DuplicatePolicy::Omit,
        },
        projection: match policy.projection {
            RemanenceProjectionPolicy::Refuse => remanence::ProjectionPolicy::Refuse,
            RemanenceProjectionPolicy::DeclareLoss => remanence::ProjectionPolicy::DeclareLoss,
        },
        pulse_strength: match policy.pulse_strength {
            RemanencePulseStrengthPolicy::Declared => remanence::PulseStrengthPolicy::Declared {
                state: policy.strength_state,
            },
            RemanencePulseStrengthPolicy::FromAgreement => {
                remanence::PulseStrengthPolicy::FromAgreement {
                    window_cycles: policy.strength_window_cycles,
                }
            }
        },
        origin: match policy.origin {
            RemanenceOriginPolicy::Declared => remanence::OriginPolicy::Declared,
            RemanenceOriginPolicy::Angle => remanence::OriginPolicy::Angle {
                cycles: policy.origin_cycles,
            },
        },
        seed: policy.seed,
    }
}

/// Plans the reduction of a capture set to one 1541 flux medium.
/// Nothing is written and nothing is mutated. Returns null on failure
/// and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_capture_set_plan_c1541_mastering(
    set: *const RemanenceCaptureSet,
    policy: *const RemanenceMasteringPolicy,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMasteringPlan {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(set), Some(policy)) = (unsafe { set.as_ref() }, unsafe { policy.as_ref() }) else {
        let error = remanence::Error::io("null capture set or policy");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match set.set.plan_c1541_mastering(to_policy(policy)) {
        Ok(plan) => {
            let report = plan.report().clone();
            let view = PlanView::new(&report);
            Box::into_raw(Box::new(RemanenceMasteringPlan {
                plan: Some(plan),
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

/// Frees a plan handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_plan_free(plan: *mut RemanenceMasteringPlan) {
    if !plan.is_null() {
        drop(unsafe { Box::from_raw(plan) });
    }
}

/// Produces the medium the plan described, consuming the plan: the
/// handle is freed whether this succeeds or fails, and must not be used
/// again. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_plan_execute(
    plan: *mut RemanenceMasteringPlan,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMasteredMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if plan.is_null() {
        let error = remanence::Error::io("null plan");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let owned = unsafe { Box::from_raw(plan) };
    let RemanenceMasteringPlan {
        plan: Some(plan),
        report,
        view,
    } = *owned
    else {
        let error = remanence::Error::io("plan has already been executed");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match plan.execute(cache_bytes) {
        Ok(medium) => Box::into_raw(Box::new(RemanenceMasteredMedium {
            medium,
            report,
            view,
        })),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a mastered-medium handle, discarding its private session
/// storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_free(medium: *mut RemanenceMasteredMedium) {
    if !medium.is_null() {
        drop(unsafe { Box::from_raw(medium) });
    }
}

/// How many locations the medium claims.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_locations(
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { medium.as_ref() }.map_or(0, |medium| medium.medium.locations())
}

/// How many bytes of private session storage the medium occupies, and
/// how much of that is currently resident.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_backing_bytes(
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { medium.as_ref() }.map_or(0, |medium| medium.medium.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_resident_bytes(
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { medium.as_ref() }.map_or(0, |medium| medium.medium.resident_bytes())
}

/// A plan and the medium produced from it report the same thing:
/// executing adds nothing to the account. So the accessors below take
/// either handle through one small indirection rather than being
/// written out twice.
enum ReportedPlan<'a> {
    Planned(&'a RemanenceMasteringPlan),
    Mastered(&'a RemanenceMasteredMedium),
}

impl ReportedPlan<'_> {
    fn report(&self) -> &remanence::MasteringPlanReport {
        match self {
            Self::Planned(plan) => &plan.report,
            Self::Mastered(medium) => &medium.report,
        }
    }

    fn view(&self) -> &PlanView {
        match self {
            Self::Planned(plan) => &plan.view,
            Self::Mastered(medium) => &medium.view,
        }
    }
}

unsafe fn reported<'a>(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> Option<ReportedPlan<'a>> {
    if let Some(plan) = unsafe { plan.as_ref() } {
        return Some(ReportedPlan::Planned(plan));
    }
    unsafe { medium.as_ref() }.map(ReportedPlan::Mastered)
}

/// The profile the reduction was declared by. Pass whichever handle you
/// hold and null for the other.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_profile_id(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> *const c_char {
    unsafe { reported(plan, medium) }
        .map_or(ptr::null(), |reported| reported.view().profile_id.as_ptr())
}

/// The frame the medium is expressed in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_reference_clock_hz(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().reference_clock_hz)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_cycles_per_rotation(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> u64 {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().cycles_per_rotation)
}

/// Which rule placed the circle's origin.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_origin_rule(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> *const c_char {
    unsafe { reported(plan, medium) }
        .map_or(ptr::null(), |reported| reported.view().origin_rule.as_ptr())
}

/// How many half-tracks the reduction produces.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_location_count(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> usize {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().locations.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_location(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
    out: *mut RemanenceMasteredLocation,
) -> bool {
    let Some(reported) = (unsafe { reported(plan, medium) }) else {
        return false;
    };
    let Some(location) = reported.report().locations.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceMasteredLocation {
                source_position_numerator: location.source_position.numerator,
                source_position_denominator: location.source_position.denominator,
                half_track_numerator: location.half_track_numerator,
                half_track_denominator: location.half_track_denominator,
                observation_ordinal: location.observation_ordinal,
                pulses: location.pulses,
                strong_pulses: location.strong_pulses,
                weak_pulses: location.weak_pulses,
                origin_cycles: location.origin_cycles,
                has_seam: location.seam_cycles.is_some(),
                seam_cycles: location.seam_cycles.unwrap_or(0),
            };
        }
    }
    true
}

/// How many kinds of loss the destination will not carry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_count(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> usize {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.report().declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_code(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> *const c_char {
    unsafe { reported(plan, medium) }.map_or(ptr::null(), |reported| {
        reported
            .view()
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the source's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_detail(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> *const c_char {
    unsafe { reported(plan, medium) }.map_or(ptr::null(), |reported| {
        reported
            .view()
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_declared_loss_amount(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> u64 {
    unsafe { reported(plan, medium) }.map_or(0, |reported| {
        reported
            .report()
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// The policy that produced the plan, stated in full.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_evidence_count(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
) -> usize {
    unsafe { reported(plan, medium) }.map_or(0, |reported| reported.view().evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastering_evidence(
    plan: *const RemanenceMasteringPlan,
    medium: *const RemanenceMasteredMedium,
    index: usize,
) -> *const c_char {
    unsafe { reported(plan, medium) }.map_or(ptr::null(), |reported| {
        reported
            .view()
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// The P64 image-format adapter: one container claimed in both
// directions. Reading it decodes a medium at rest; writing it produces a
// new artifact from a mastered one, under a claim stated before the file
// exists.

use remanence::{P64Image, P64Report};

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

struct P64View {
    format_id: CString,
    format_name: CString,
    profile_id: CString,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
    evidence: Vec<CString>,
}

impl P64View {
    fn new(report: &P64Report) -> Self {
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
            evidence: report.evidence.iter().map(|line| to_cstring(line)).collect(),
        }
    }
}

/// An opened P64 image, holding its claim on the file and the medium it
/// decoded into private session storage.
pub struct RemanenceP64Image {
    image: P64Image,
    path: CString,
    report: P64Report,
    view: P64View,
}

/// What a container carried, or will carry, of one mastered medium.
pub struct RemanenceP64Report {
    report: P64Report,
    view: P64View,
}

unsafe fn open_p64(
    path: *const c_char,
    cache_bytes: Option<u64>,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceP64Image {
    unsafe { clear_error(error_out, error_rule_out) };
    if path.is_null() {
        let error = remanence::Error::io("null path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    let opened = match cache_bytes {
        Some(cache_bytes) => P64Image::open_with_cache(path.as_ref(), cache_bytes),
        None => P64Image::open(path.as_ref()),
    };
    match opened {
        Ok(image) => {
            let report = image.inspect().clone();
            let view = P64View::new(&report);
            Box::into_raw(Box::new(RemanenceP64Image {
                path: to_cstring(&image.path().to_string_lossy()),
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

/// Opens the P64 image at `path` (UTF-8), claiming the file and decoding
/// every half-track once into private session storage. The version is
/// checked before anything else is touched, and a version, flag bit, or
/// chunk signature past this release's claim is refused by name. Returns
/// null on failure and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_open(
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceP64Image {
    unsafe { open_p64(path, None, error_category_out, error_out, error_rule_out) }
}

/// Opens a P64 image as `remanence_p64_image_open` does, under a
/// declared cache bound: at most `cache_bytes` of the decoded medium
/// stays resident. The bound narrows the working set; it never refuses
/// service.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_open_with_cache(
    path: *const c_char,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceP64Image {
    unsafe { open_p64(path, Some(cache_bytes), error_category_out, error_out, error_rule_out) }
}

/// Frees an image handle, releasing its claim on the file and discarding
/// the private session storage the medium decoded into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_free(image: *mut RemanenceP64Image) {
    if !image.is_null() {
        drop(unsafe { Box::from_raw(image) });
    }
}

/// The path the image was opened from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_path(
    image: *const RemanenceP64Image,
) -> *const c_char {
    unsafe { image.as_ref() }.map_or(ptr::null(), |image| image.path.as_ptr())
}

/// Which P7 mode the open obtained on the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_access_mode(
    image: *const RemanenceP64Image,
) -> RemanenceAccessMode {
    unsafe { image.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |image| {
        access_mode(image.image.access_mode())
    })
}

/// How many bytes of private session storage the decoded medium
/// occupies, and how much of that is currently resident.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_backing_bytes(
    image: *const RemanenceP64Image,
) -> u64 {
    unsafe { image.as_ref() }.map_or(0, |image| image.image.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_resident_bytes(
    image: *const RemanenceP64Image,
) -> u64 {
    unsafe { image.as_ref() }.map_or(0, |image| image.image.resident_bytes())
}

/// Computes what a P64 will and will not carry of a mastered medium,
/// writing nothing. Read it before writing: the write adds nothing to
/// the account. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_describe_p64(
    medium: *const RemanenceMasteredMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceP64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(medium) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null mastered medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match medium.medium.describe_p64() {
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

/// Writes a mastered medium into a new P64 image at `path` (UTF-8) and
/// reports what the container carried. The medium is untouched, an
/// existing destination is a named refusal rather than an overwrite, and
/// an interruption leaves the destination absent rather than half an
/// artifact. Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_write_p64(
    medium: *const RemanenceMasteredMedium,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceP64Report {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(medium), false) = (unsafe { medium.as_ref() }, path.is_null()) else {
        let error = remanence::Error::io("null mastered medium or path");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let path = String::from_utf8_lossy(unsafe { CStr::from_ptr(path) }.to_bytes());
    match medium.medium.write_p64(path.as_ref()) {
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

/// Frees a report handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_report_free(report: *mut RemanenceP64Report) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

/// An opened image and a written artifact report the same thing, so the
/// accessors below take either handle: pass whichever you hold and null
/// for the other.
unsafe fn p64_reported<'a>(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> Option<(&'a P64Report, &'a P64View)> {
    if let Some(image) = unsafe { image.as_ref() } {
        return Some((&image.report, &image.view));
    }
    unsafe { report.as_ref() }.map(|report| (&report.report, &report.view))
}

/// The container format's stable identifier, "p64".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_format_id(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(image, report) }
        .map_or(ptr::null(), |(_, view)| view.format_id.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_format_name(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(image, report) }
        .map_or(ptr::null(), |(_, view)| view.format_name.as_ptr())
}

/// The container's declared format version.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_version(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> u32 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.version)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_write_protected(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> bool {
    unsafe { p64_reported(image, report) }.is_some_and(|(report, _)| report.write_protected)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_double_sided(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> bool {
    unsafe { p64_reported(image, report) }.is_some_and(|(report, _)| report.double_sided)
}

/// The drive profile the container's own signature names, and the frame
/// that profile declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_profile_id(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> *const c_char {
    unsafe { p64_reported(image, report) }
        .map_or(ptr::null(), |(_, view)| view.profile_id.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_reference_clock_hz(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> u64 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.reference_clock_hz)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_cycles_per_rotation(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> u64 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.cycles_per_rotation)
}

/// How many half-tracks the container holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_half_track_count(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.half_tracks.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_half_track(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
    out: *mut RemanenceP64HalfTrack,
) -> bool {
    let Some((report, _)) = (unsafe { p64_reported(image, report) }) else {
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
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| report.declared_loss.len())
}

/// One loss entry's stable code, or null when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_code(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(image, report) }.map_or(ptr::null(), |(_, view)| {
        view.loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was lost, in the source's own terms. A count is not an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_detail(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(image, report) }.map_or(ptr::null(), |(_, view)| {
        view.loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

/// How much of it there was, in whatever the detail counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_declared_loss_amount(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> u64 {
    unsafe { p64_reported(image, report) }.map_or(0, |(report, _)| {
        report
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// How the container was recognized and what this adapter claims of it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_evidence_count(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
) -> usize {
    unsafe { p64_reported(image, report) }.map_or(0, |(_, view)| view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_evidence(
    image: *const RemanenceP64Image,
    report: *const RemanenceP64Report,
    index: usize,
) -> *const c_char {
    unsafe { p64_reported(image, report) }.map_or(ptr::null(), |(_, view)| {
        view.evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// The C1541 presentation: the hardware bitstream a declared read channel
// clocks out of a flux medium, and the encoded bytestream a declared
// group code resolves out of that. Neither layer assigns
// synchronization, headers, sectors or files to what it holds, and there
// is no way back down.

use remanence::{C1541Bitstream, C1541Bytestream};

/// Which declared density a location is clocked at.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDensityPolicy {
    /// The zone the family's density map declares for the location.
    Declared = 0,
    /// The zone in `density_zone`, for every location.
    Fixed = 1,
}

/// What a location no declared zone covers becomes.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceUnzonedPolicy {
    Refuse = 0,
    Omit = 1,
}

/// How a pulse the medium states does not read the same every time
/// becomes a definite bit.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceWeakPulsePolicy {
    /// Every such pulse is taken as `weak_pulse_detected`, uniformly.
    Declared = 0,
    /// Each is resolved reproducibly from `seed` and its own angle.
    Seeded = 1,
}

/// The complete declared policy for one medium-to-bitstream transition.
/// There is no default: every field is a decision about evidence.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceReadChannelPolicy {
    pub density: RemanenceDensityPolicy,
    pub density_zone: u32,
    pub unzoned: RemanenceUnzonedPolicy,
    pub weak_pulse: RemanenceWeakPulsePolicy,
    pub weak_pulse_detected: bool,
    pub seed: u64,
}

/// Where byte framing begins.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceAlignmentPolicy {
    /// At the family's declared landmark and nowhere else.
    Landmark = 0,
    /// At the circle's origin as well, the caller declaring it a byte
    /// boundary.
    Origin = 1,
}

/// What a group holding a pattern the family's table does not assign
/// becomes.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceUnassignedSymbolPolicy {
    Refuse = 0,
    DeclareLoss = 1,
}

/// The complete declared policy for one bitstream-to-bytestream
/// transition.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceGcrCodecPolicy {
    pub alignment: RemanenceAlignmentPolicy,
    pub unassigned_symbol: RemanenceUnassignedSymbolPolicy,
}

/// One location the bitstream holds, and what the channel resolved
/// there.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceBitstreamLocation {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub has_surface: bool,
    pub surface: u64,
    pub zone: u32,
    /// The cell, in reference-clock cycles, exactly.
    pub cell_cycles_numerator: u64,
    pub cell_cycles_denominator: u64,
    pub cells: u64,
    pub one_bits: u64,
    /// Bits the medium recorded, and bits a declared rule resolved.
    pub recorded_bits: u64,
    pub resolved_bits: u64,
    pub short_cells: u64,
    pub longest_zero_run: u64,
    /// What is left of the circle after the last whole cell, over
    /// `cell_cycles_denominator`.
    pub wrap_slack_numerator: u64,
}

/// One location the bytestream holds.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceBytestreamLocation {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub has_surface: bool,
    pub surface: u64,
    pub bytes: u64,
    pub resolved_bytes: u64,
    pub unassigned_groups: u64,
    pub alignments: u64,
    pub longest_landmark_bits: u64,
    pub unframed_bits: u64,
}

struct LayerView {
    first: CString,
    second: CString,
    third: CString,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
    evidence: Vec<CString>,
}

impl LayerView {
    fn new(
        first: &str,
        second: &str,
        third: &str,
        loss: &[remanence::DeclaredLoss],
        evidence: &[String],
    ) -> Self {
        Self {
            first: to_cstring(first),
            second: to_cstring(second),
            third: to_cstring(third),
            loss_codes: loss.iter().map(|entry| to_cstring(&entry.code)).collect(),
            loss_details: loss.iter().map(|entry| to_cstring(&entry.detail)).collect(),
            evidence: evidence.iter().map(|line| to_cstring(line)).collect(),
        }
    }
}

/// A hardware bitstream, held in the session. The bits stay behind this
/// handle; what it reports is the transition that produced them.
pub struct RemanenceC1541Bitstream {
    bitstream: C1541Bitstream,
    view: LayerView,
}

/// An encoded bytestream, held in the session.
pub struct RemanenceC1541Bytestream {
    bytestream: C1541Bytestream,
    view: LayerView,
}

fn to_channel_policy(policy: &RemanenceReadChannelPolicy) -> remanence::ReadChannelPolicy {
    remanence::ReadChannelPolicy {
        density: match policy.density {
            RemanenceDensityPolicy::Declared => remanence::DensityPolicy::Declared,
            RemanenceDensityPolicy::Fixed => remanence::DensityPolicy::Fixed {
                zone: policy.density_zone,
            },
        },
        unzoned: match policy.unzoned {
            RemanenceUnzonedPolicy::Refuse => remanence::UnzonedPolicy::Refuse,
            RemanenceUnzonedPolicy::Omit => remanence::UnzonedPolicy::Omit,
        },
        weak_pulse: match policy.weak_pulse {
            RemanenceWeakPulsePolicy::Declared => remanence::WeakPulsePolicy::Declared {
                detected: policy.weak_pulse_detected,
            },
            RemanenceWeakPulsePolicy::Seeded => remanence::WeakPulsePolicy::Seeded,
        },
        seed: policy.seed,
    }
}

fn to_codec_policy(policy: &RemanenceGcrCodecPolicy) -> remanence::GcrCodecPolicy {
    remanence::GcrCodecPolicy {
        alignment: match policy.alignment {
            RemanenceAlignmentPolicy::Landmark => remanence::AlignmentPolicy::Landmark,
            RemanenceAlignmentPolicy::Origin => remanence::AlignmentPolicy::Origin,
        },
        unassigned_symbol: match policy.unassigned_symbol {
            RemanenceUnassignedSymbolPolicy::Refuse => remanence::UnassignedSymbolPolicy::Refuse,
            RemanenceUnassignedSymbolPolicy::DeclareLoss => {
                remanence::UnassignedSymbolPolicy::DeclareLoss
            }
        },
    }
}

fn own_bitstream(bitstream: C1541Bitstream) -> *mut RemanenceC1541Bitstream {
    let report = bitstream.inspect();
    let view = LayerView::new(
        &report.profile_id,
        &report.profile_name,
        "",
        &report.declared_loss,
        &report.evidence,
    );
    Box::into_raw(Box::new(RemanenceC1541Bitstream { bitstream, view }))
}

/// Materializes the family's hardware bitstream from a mastered medium
/// under declared mechanics and read-channel rules. The medium is
/// untouched. Returns null on failure and stores a message in
/// `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_mastered_medium_materialize_c1541_bitstream(
    medium: *const RemanenceMasteredMedium,
    policy: *const RemanenceReadChannelPolicy,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceC1541Bitstream {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(medium), Some(policy)) = (unsafe { medium.as_ref() }, unsafe { policy.as_ref() })
    else {
        let error = remanence::Error::io("null mastered medium or policy");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match medium
        .medium
        .materialize_c1541_bitstream(to_channel_policy(policy), cache_bytes)
    {
        Ok(bitstream) => own_bitstream(bitstream),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The same, from the medium a P64 container holds at rest.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_p64_image_materialize_c1541_bitstream(
    image: *const RemanenceP64Image,
    policy: *const RemanenceReadChannelPolicy,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceC1541Bitstream {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(image), Some(policy)) = (unsafe { image.as_ref() }, unsafe { policy.as_ref() })
    else {
        let error = remanence::Error::io("null P64 image or policy");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match image
        .image
        .materialize_c1541_bitstream(to_channel_policy(policy), cache_bytes)
    {
        Ok(bitstream) => own_bitstream(bitstream),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a bitstream handle, discarding its private session storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_free(bitstream: *mut RemanenceC1541Bitstream) {
    if !bitstream.is_null() {
        drop(unsafe { Box::from_raw(bitstream) });
    }
}

/// Materializes the family's encoded bytestream from a bitstream under
/// its declared group code. The bitstream is untouched. Returns null on
/// failure and stores a message in `error_out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_materialize_bytestream(
    bitstream: *const RemanenceC1541Bitstream,
    policy: *const RemanenceGcrCodecPolicy,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceC1541Bytestream {
    unsafe { clear_error(error_out, error_rule_out) };
    let (Some(bitstream), Some(policy)) =
        (unsafe { bitstream.as_ref() }, unsafe { policy.as_ref() })
    else {
        let error = remanence::Error::io("null bitstream or policy");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match bitstream
        .bitstream
        .materialize_c1541_bytestream(to_codec_policy(policy), cache_bytes)
    {
        Ok(bytestream) => {
            let report = bytestream.inspect();
            let view = LayerView::new(
                &report.profile_id,
                &report.codec_id,
                &report.codec_name,
                &report.declared_loss,
                &report.evidence,
            );
            Box::into_raw(Box::new(RemanenceC1541Bytestream { bytestream, view }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a bytestream handle, discarding its private session storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_free(
    bytestream: *mut RemanenceC1541Bytestream,
) {
    if !bytestream.is_null() {
        drop(unsafe { Box::from_raw(bytestream) });
    }
}

/// The profile the channel was declared by.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_profile_id(
    bitstream: *const RemanenceC1541Bitstream,
) -> *const c_char {
    unsafe { bitstream.as_ref() }.map_or(ptr::null(), |held| held.view.first.as_ptr())
}

/// Its human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_profile_name(
    bitstream: *const RemanenceC1541Bitstream,
) -> *const c_char {
    unsafe { bitstream.as_ref() }.map_or(ptr::null(), |held| held.view.second.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_profile_version(
    bitstream: *const RemanenceC1541Bitstream,
) -> u32 {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.bitstream.inspect().profile_version)
}

/// The frame the cells are angles in, carried from the medium unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_reference_clock_hz(
    bitstream: *const RemanenceC1541Bitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.bitstream.inspect().reference_clock_hz)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_cycles_per_rotation(
    bitstream: *const RemanenceC1541Bitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.bitstream.inspect().cycles_per_rotation)
}

/// How many bytes of private session storage the bitstream occupies, and
/// how much of that is currently resident. It is never held whole (P27).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_backing_bytes(
    bitstream: *const RemanenceC1541Bitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.bitstream.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_resident_bytes(
    bitstream: *const RemanenceC1541Bitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.bitstream.resident_bytes())
}

/// How many locations the bitstream claims.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_location_count(
    bitstream: *const RemanenceC1541Bitstream,
) -> usize {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.bitstream.inspect().locations.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_location(
    bitstream: *const RemanenceC1541Bitstream,
    index: usize,
    out: *mut RemanenceBitstreamLocation,
) -> bool {
    let Some(held) = (unsafe { bitstream.as_ref() }) else {
        return false;
    };
    let Some(location) = held.bitstream.inspect().locations.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceBitstreamLocation {
                half_track_numerator: location.half_track_numerator,
                half_track_denominator: location.half_track_denominator,
                has_surface: location.surface.is_some(),
                surface: location.surface.unwrap_or(0),
                zone: location.zone,
                cell_cycles_numerator: location.cell_cycles_numerator,
                cell_cycles_denominator: location.cell_cycles_denominator,
                cells: location.cells,
                one_bits: location.one_bits,
                recorded_bits: location.recorded_bits,
                resolved_bits: location.resolved_bits,
                short_cells: location.short_cells,
                longest_zero_run: location.longest_zero_run,
                wrap_slack_numerator: location.wrap_slack_numerator,
            };
        }
    }
    true
}

/// How many kinds of thing the bitstream does not carry of the medium.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_declared_loss_count(
    bitstream: *const RemanenceC1541Bitstream,
) -> usize {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.view.loss_codes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_declared_loss_code(
    bitstream: *const RemanenceC1541Bitstream,
    index: usize,
) -> *const c_char {
    unsafe { bitstream.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

/// What was not carried, in the medium's own terms. A count is not an
/// account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_declared_loss_detail(
    bitstream: *const RemanenceC1541Bitstream,
    index: usize,
) -> *const c_char {
    unsafe { bitstream.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_declared_loss_amount(
    bitstream: *const RemanenceC1541Bitstream,
    index: usize,
) -> u64 {
    unsafe { bitstream.as_ref() }.map_or(0, |held| {
        held.bitstream
            .inspect()
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// The channel that produced the bitstream and the policy that produced
/// the medium, in that order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_evidence_count(
    bitstream: *const RemanenceC1541Bitstream,
) -> usize {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bitstream_evidence(
    bitstream: *const RemanenceC1541Bitstream,
    index: usize,
) -> *const c_char {
    unsafe { bitstream.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// The profile and the group code the bytes were resolved by.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_profile_id(
    bytestream: *const RemanenceC1541Bytestream,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| held.view.first.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_codec_id(
    bytestream: *const RemanenceC1541Bytestream,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| held.view.second.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_codec_name(
    bytestream: *const RemanenceC1541Bytestream,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| held.view.third.as_ptr())
}

/// How many bits of the recording carry how many bits of a byte, and how
/// many symbols make one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_symbol_bits(
    bytestream: *const RemanenceC1541Bytestream,
) -> u32 {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.bytestream.inspect().symbol_bits)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_data_bits(
    bytestream: *const RemanenceC1541Bytestream,
) -> u32 {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.bytestream.inspect().data_bits)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_symbols_per_byte(
    bytestream: *const RemanenceC1541Bytestream,
) -> u32 {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.bytestream.inspect().symbols_per_byte)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_backing_bytes(
    bytestream: *const RemanenceC1541Bytestream,
) -> u64 {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.bytestream.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_resident_bytes(
    bytestream: *const RemanenceC1541Bytestream,
) -> u64 {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.bytestream.resident_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_location_count(
    bytestream: *const RemanenceC1541Bytestream,
) -> usize {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.bytestream.inspect().locations.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_location(
    bytestream: *const RemanenceC1541Bytestream,
    index: usize,
    out: *mut RemanenceBytestreamLocation,
) -> bool {
    let Some(held) = (unsafe { bytestream.as_ref() }) else {
        return false;
    };
    let Some(location) = held.bytestream.inspect().locations.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceBytestreamLocation {
                half_track_numerator: location.half_track_numerator,
                half_track_denominator: location.half_track_denominator,
                has_surface: location.surface.is_some(),
                surface: location.surface.unwrap_or(0),
                bytes: location.bytes,
                resolved_bytes: location.resolved_bytes,
                unassigned_groups: location.unassigned_groups,
                alignments: location.alignments,
                longest_landmark_bits: location.longest_landmark_bits,
                unframed_bits: location.unframed_bits,
            };
        }
    }
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_declared_loss_count(
    bytestream: *const RemanenceC1541Bytestream,
) -> usize {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.view.loss_codes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_declared_loss_code(
    bytestream: *const RemanenceC1541Bytestream,
    index: usize,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_declared_loss_detail(
    bytestream: *const RemanenceC1541Bytestream,
    index: usize,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_declared_loss_amount(
    bytestream: *const RemanenceC1541Bytestream,
    index: usize,
) -> u64 {
    unsafe { bytestream.as_ref() }.map_or(0, |held| {
        held.bytestream
            .inspect()
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// The codec, the channel beneath it and the medium policy beneath that,
/// in that order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_evidence_count(
    bytestream: *const RemanenceC1541Bytestream,
) -> usize {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_bytestream_evidence(
    bytestream: *const RemanenceC1541Bytestream,
    index: usize,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// The layered disk inspection report: one owned handle over the whole
// report graph, with indexed bounds-checked access to its records and
// relationships. Strings are borrowed from the handle that owns them, and
// identities cross the ABI as opaque values a caller round-trips without
// parsing.

/// A snapshot of one disk's layered inspection. Owned by the caller and
/// released with `remanence_report_free`; every string and record
/// reached through it is borrowed from it and dies with it.
pub struct RemanenceDiskReport {
    /// The report as the core issued it, kept so a drive-letter machine
    /// can be asserted over the report a caller already holds rather than
    /// over a flattened copy of it.
    source: remanence::DiskReport,
    device_id: u64,
    device_image_format: CString,
    device_length_bytes: u64,
    device_media_type: CString,
    device_authoritative_layer: CString,
    device_active_layer: CString,
    content: RemanenceDiskContent,
    content_evidence: Option<CString>,
    schema: Option<SchemaView>,
    regions: Vec<RegionView>,
    volumes: Vec<VolumeRecordView>,
    filesystems: Vec<FilesystemView>,
}

/// What the device's leading structure turned out to be. The report states
/// this rather than leaving a caller to reconstruct it from empty lists.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDiskContent {
    /// All zero: a blank disk, which is an answer.
    Blank,
    /// A partition schema was recognized, whether or not a volume composed.
    Schema,
    /// No schema, and the whole device is one volume.
    DirectVolume,
    /// Not blank, and no adapter claims it. An outcome, not a refusal.
    UnknownNonblank,
}

/// How a schema declares a region: data, which composition may consume, or
/// structure, which it may not.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceRegionRole {
    Data,
    Structure,
}

/// Where a volume's storage came from.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceVolumeOrigin {
    WholeDevice,
    Regions,
}

struct SchemaView {
    kind: CString,
    evidence: Vec<CString>,
}

struct IssueView {
    category: RemanenceErrorCategory,
    message: CString,
}

struct RegionView {
    id: u64,
    declared_number: u32,
    declared_placement: CString,
    role: RemanenceRegionRole,
    declared_type: u8,
    declared_type_reading: CString,
    claimed: bool,
    start_bytes: u64,
    length_bytes: u64,
    issue: Option<IssueView>,
}

struct VolumeRecordView {
    id: u64,
    origin: RemanenceVolumeOrigin,
    origin_regions: Vec<u64>,
    start_bytes: u64,
    length_bytes: u64,
    evidence: Vec<CString>,
}

/// One source's reading of a volume label. `stored` is null where the
/// format gives this volume no such field at all, which is distinct from
/// a field that is present and blank.
struct LabelReadingView {
    source: CString,
    stored: Option<CString>,
}

/// A recognized volume's label answer, with every reading beside it.
struct VolumeLabelView {
    name: Option<CString>,
    answered_by: Option<CString>,
    readings: Vec<LabelReadingView>,
}

struct FilesystemView {
    id: u64,
    volume: u64,
    kind: Option<CString>,
    label: Option<VolumeLabelView>,
    cluster_bytes: Option<u64>,
    cluster_count: Option<u64>,
    sectors_per_track: Option<u16>,
    heads: Option<u16>,
    cylinders: Option<u64>,
    issues: Vec<IssueView>,
}

fn issue_view(issue: &remanence::Error) -> IssueView {
    IssueView {
        category: issue.category().into(),
        message: to_cstring(&issue.to_string()),
    }
}

fn evidence_views(evidence: &[String]) -> Vec<CString> {
    evidence.iter().map(|line| to_cstring(line)).collect()
}

fn label_view(label: &remanence::VolumeLabel) -> VolumeLabelView {
    VolumeLabelView {
        name: label.name.as_deref().map(to_cstring),
        answered_by: label.answered_by.as_deref().map(to_cstring),
        readings: label
            .readings
            .iter()
            .map(|reading| LabelReadingView {
                source: to_cstring(&reading.source),
                stored: reading.stored.as_deref().map(to_cstring),
            })
            .collect(),
    }
}

/// Inspects the medium in an occupied device and returns its layered
/// report. Null on failure,
/// with the category and message written to the out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_inspect(
    device: *mut RemanenceDevice,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDiskReport {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(device) = (unsafe { device.as_mut() }) else {
        return ptr::null_mut();
    };
        let Some(medium) = device.device() else {
        let error = remanence::Error::io("the device holding this medium was removed");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match medium.inspect() {
        Ok(report) => {
            let (content, content_evidence) = match &report.content {
                DiskContent::Blank => (RemanenceDiskContent::Blank, None),
                DiskContent::Schema => (RemanenceDiskContent::Schema, None),
                DiskContent::DirectVolume => (RemanenceDiskContent::DirectVolume, None),
                DiskContent::UnknownNonblank { evidence } => (
                    RemanenceDiskContent::UnknownNonblank,
                    Some(to_cstring(evidence)),
                ),
            };
            let schema = report.partition_schema.as_ref().map(|schema| SchemaView {
                kind: to_cstring(&schema.kind),
                evidence: evidence_views(&schema.evidence),
            });
            let regions = report
                .regions
                .iter()
                .map(|region| RegionView {
                    id: region.id.value(),
                    declared_number: region.declared_number,
                    declared_placement: to_cstring(&region.declared_placement),
                    role: match region.role {
                        RegionRole::Data => RemanenceRegionRole::Data,
                        RegionRole::Structure => RemanenceRegionRole::Structure,
                    },
                    declared_type: region.declared_type,
                    declared_type_reading: to_cstring(&region.declared_type_reading),
                    claimed: region.claimed,
                    start_bytes: region.start_bytes,
                    length_bytes: region.length_bytes,
                    issue: region.issue.as_ref().map(issue_view),
                })
                .collect();
            let volumes = report
                .volumes
                .iter()
                .map(|volume| VolumeRecordView {
                    id: volume.id.value(),
                    origin: match &volume.origin {
                        VolumeOrigin::WholeDevice => RemanenceVolumeOrigin::WholeDevice,
                        VolumeOrigin::Regions(_) => RemanenceVolumeOrigin::Regions,
                    },
                    origin_regions: match &volume.origin {
                        VolumeOrigin::WholeDevice => Vec::new(),
                        VolumeOrigin::Regions(regions) => {
                            regions.iter().map(|region| region.value()).collect()
                        }
                    },
                    start_bytes: volume.start_bytes,
                    length_bytes: volume.length_bytes,
                    evidence: evidence_views(&volume.evidence),
                })
                .collect();
            let filesystems = report
                .filesystems
                .iter()
                .map(|filesystem| FilesystemView {
                    id: filesystem.id.value(),
                    volume: filesystem.volume.value(),
                    kind: filesystem.kind.as_deref().map(to_cstring),
                    label: filesystem.label.as_ref().map(label_view),
                    cluster_bytes: filesystem.cluster_bytes,
                    cluster_count: filesystem.cluster_count,
                    sectors_per_track: filesystem.declared_geometry.sectors_per_track,
                    heads: filesystem.declared_geometry.heads,
                    cylinders: filesystem.declared_geometry.cylinders,
                    issues: filesystem.issues.iter().map(issue_view).collect(),
                })
                .collect();
            Box::into_raw(Box::new(RemanenceDiskReport {
                device_id: report.device.id,
                device_image_format: to_cstring(&report.device.image_format),
                device_length_bytes: report.device.length_bytes,
                device_media_type: to_cstring(&report.device.media_type),
            device_authoritative_layer: to_cstring(&report.device.authoritative_layer),
                device_active_layer: to_cstring(&report.device.active_layer),
                content,
                content_evidence,
                schema,
                regions,
                volumes,
                filesystems,
                source: report,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees an inspection report and everything borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_free(report: *mut RemanenceDiskReport) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

unsafe fn region_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a RegionView> {
    unsafe { report.as_ref() }?.regions.get(index)
}

unsafe fn volume_record_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a VolumeRecordView> {
    unsafe { report.as_ref() }?.volumes.get(index)
}

unsafe fn filesystem_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a FilesystemView> {
    unsafe { report.as_ref() }?.filesystems.get(index)
}

/// The device identity assigned by this loaded composition (P21), scoped
/// to the open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_id(report: *const RemanenceDiskReport) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.device_id)
}

/// The image format the artifact turned out to be.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_image_format(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_image_format.as_ptr())
}

/// The device's addressable length in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_length_bytes(
    report: *const RemanenceDiskReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.device_length_bytes)
}

/// The media type of the medium attached to the device (P14) — what
/// the medium is, said in the media-type catalog's own name for it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_media_type(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_media_type.as_ptr())
}

/// The layer the image is authoritative at (P13).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_authoritative_layer(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report.device_authoritative_layer.as_ptr()
    })
}

/// The layer active for this composition (P23).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_active_layer(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_active_layer.as_ptr())
}

/// What the device's leading structure turned out to be.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_content(
    report: *const RemanenceDiskReport,
) -> RemanenceDiskContent {
    unsafe { report.as_ref() }.map_or(RemanenceDiskContent::Blank, |report| report.content)
}

/// Why no adapter claimed the content, for the unknown-nonblank outcome;
/// null for every other outcome.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_content_evidence(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .content_evidence
            .as_ref()
            .map_or(ptr::null(), |evidence| evidence.as_ptr())
    })
}

/// Whether a partition schema was recognized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_has_partition_schema(
    report: *const RemanenceDiskReport,
) -> bool {
    unsafe { report.as_ref() }.is_some_and(|report| report.schema.is_some())
}

/// The recognized schema's kind, or null where none was recognized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_kind(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .schema
            .as_ref()
            .map_or(ptr::null(), |schema| schema.kind.as_ptr())
    })
}

/// How many evidence lines the schema recognition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_evidence_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| {
        report
            .schema
            .as_ref()
            .map_or(0, |schema| schema.evidence.len())
    })
}

/// One evidence line from the schema recognition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_evidence(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report.schema.as_ref().map_or(ptr::null(), |schema| {
            schema
                .evidence
                .get(index)
                .map_or(ptr::null(), |line| line.as_ptr())
        })
    })
}

/// How many regions the schema declares. Every declared region is
/// reported, refused ones included.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_count(report: *const RemanenceDiskReport) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.regions.len())
}

/// A region's opaque identity. Pass it back to the library; never parse
/// it, and never build one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.id)
}

/// The number the schema itself declared this region at.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_number(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u32 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.declared_number)
}

/// How the schema places this region in its own vocabulary: for MBR,
/// "primary" for one of the four slots and "logical" for an entry on the
/// extended chain. A different axis from the role: the extended partition
/// is a primary slot whose role is structural.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_placement(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }
        .map_or(ptr::null(), |region| region.declared_placement.as_ptr())
}

/// Whether the schema declares this region as data or as structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_role(
    report: *const RemanenceDiskReport,
    index: usize,
) -> RemanenceRegionRole {
    unsafe { region_view(report, index) }.map_or(RemanenceRegionRole::Data, |region| region.role)
}

/// The type value exactly as the schema records it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_type(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u8 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.declared_type)
}

/// What that value declares, in a sentence fit to quote in a refusal.
/// Present whether or not this release reads the type, and it describes
/// the declaration rather than the content.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_type_reading(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }
        .map_or(ptr::null(), |region| region.declared_type_reading.as_ptr())
}

/// Whether this release reads the declared type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_is_claimed(
    report: *const RemanenceDiskReport,
    index: usize,
) -> bool {
    unsafe { region_view(report, index) }.is_some_and(|region| region.claimed)
}

/// Where the region starts, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_start_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.start_bytes)
}

/// How long the region is, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_length_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.length_bytes)
}

/// The region's refusal category; false where the region reads cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_issue_category(
    report: *const RemanenceDiskReport,
    index: usize,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    let Some(issue) = (unsafe { region_view(report, index) }).and_then(|r| r.issue.as_ref()) else {
        return false;
    };
    if let Some(out) = unsafe { category_out.as_mut() } {
        *out = issue.category;
    }
    true
}

/// The region's refusal, or null where the region reads cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_issue(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }.map_or(ptr::null(), |region| {
        region
            .issue
            .as_ref()
            .map_or(ptr::null(), |issue| issue.message.as_ptr())
    })
}

/// How many volumes were composed, whatever was recognized on them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_count(report: *const RemanenceDiskReport) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.volumes.len())
}

/// How many volumes carry a filesystem the host actually read. Distinct
/// from the composed count on purpose: an unrecognized volume stays in the
/// report rather than vanishing to keep one number correct.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_readable_filesystem_volume_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| {
        report
            .filesystems
            .iter()
            .filter(|filesystem| filesystem.kind.is_some() && filesystem.issues.is_empty())
            .count()
    })
}

/// A volume's opaque identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.id)
}

/// What this volume was composed from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin(
    report: *const RemanenceDiskReport,
    index: usize,
) -> RemanenceVolumeOrigin {
    unsafe { volume_record_view(report, index) }
        .map_or(RemanenceVolumeOrigin::WholeDevice, |volume| volume.origin)
}

/// How many regions this volume was composed from; 0 for a whole-device
/// volume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin_region_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.origin_regions.len())
}

/// The identity of one region this volume was composed from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin_region_id(
    report: *const RemanenceDiskReport,
    index: usize,
    region_index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| {
        volume.origin_regions.get(region_index).copied().unwrap_or(0)
    })
}

/// Where the volume starts, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_start_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.start_bytes)
}

/// How long the volume is, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_length_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.length_bytes)
}

/// How many evidence lines this volume's composition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_evidence_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.evidence.len())
}

/// One evidence line from this volume's composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_evidence(
    report: *const RemanenceDiskReport,
    index: usize,
    evidence_index: usize,
) -> *const c_char {
    unsafe { volume_record_view(report, index) }.map_or(ptr::null(), |volume| {
        volume
            .evidence
            .get(evidence_index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many volumes filesystem recognition was attempted on. A refused
/// attempt is recorded here, at the seam that owns the refusal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.filesystems.len())
}

/// A filesystem's opaque identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.id)
}

/// The identity of the volume this recognition was attempted on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_volume_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.volume)
}

/// The recognized filesystem kind, or null where recognition was refused —
/// the issue then says why, and the volume still stands.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_kind(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .kind
            .as_ref()
            .map_or(ptr::null(), |kind| kind.as_ptr())
    })
}

/// Whether a filesystem answered the label question at all. False where
/// recognition was refused — there is then no filesystem to answer, which
/// is not the same as a volume that answered "unlabeled".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_answered(
    report: *const RemanenceDiskReport,
    index: usize,
) -> bool {
    unsafe { filesystem_view(report, index) }.is_some_and(|filesystem| filesystem.label.is_some())
}

/// The volume label, or null where the volume has none — the format's own
/// spelling of unlabeled already resolved, so no caller compares strings
/// to find that out. Null also where nothing answered;
/// `remanence_report_filesystem_label_answered` tells the two apart.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .label
            .as_ref()
            .and_then(|label| label.name.as_ref())
            .map_or(ptr::null(), |name| name.as_ptr())
    })
}

/// Which source decided the answer, or null where the volume carries no
/// such source at all. A source that exists and says unlabeled is named
/// here beside a null label.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_answered_by(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .label
            .as_ref()
            .and_then(|label| label.answered_by.as_ref())
            .map_or(ptr::null(), |source| source.as_ptr())
    })
}

/// How many sources this filesystem read for the label, kept beside the
/// answer as evidence (P4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| {
        filesystem
            .label
            .as_ref()
            .map_or(0, |label| label.readings.len())
    })
}

unsafe fn label_reading_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> Option<&'a LabelReadingView> {
    unsafe { filesystem_view(report, index) }?
        .label
        .as_ref()?
        .readings
        .get(reading_index)
}

/// One source's name, in the recognizing filesystem's own vocabulary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_source(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> *const c_char {
    unsafe { label_reading_view(report, index, reading_index) }
        .map_or(ptr::null(), |reading| reading.source.as_ptr())
}

/// Whether the format gives this volume that field at all. False is the
/// third state — no such field — and is distinct from a field that is
/// present and blank.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_present(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> bool {
    unsafe { label_reading_view(report, index, reading_index) }
        .is_some_and(|reading| reading.stored.is_some())
}

/// What that source holds, as stored and less the format's own
/// fixed-width padding: the empty string where it is present and blank,
/// and null where there is no such field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_stored(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> *const c_char {
    unsafe { label_reading_view(report, index, reading_index) }.map_or(ptr::null(), |reading| {
        reading
            .stored
            .as_ref()
            .map_or(ptr::null(), |stored| stored.as_ptr())
    })
}

/// The allocation unit size, where the filesystem states one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cluster_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_bytes)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// The allocation unit count, where the filesystem states one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cluster_count(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_count)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Sectors per track as the filesystem's own structures declare it. A
/// filesystem declaration, which manufactures no physical drive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_sectors_per_track(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u16,
) -> bool {
    let Some(value) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.sectors_per_track)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Heads as the filesystem's own structures declare it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_heads(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u16,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.heads) else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Cylinders, only where the derivation is exact. Never invented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cylinders(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cylinders)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// How many issues this recognition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.issues.len())
}

/// One issue's stable category.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue_category(
    report: *const RemanenceDiskReport,
    index: usize,
    issue_index: usize,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    let Some(issue) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.issues.get(issue_index))
    else {
        return false;
    };
    if let Some(out) = unsafe { category_out.as_mut() } {
        *out = issue.category;
    }
    true
}

/// One issue's diagnostic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue(
    report: *const RemanenceDiskReport,
    index: usize,
    issue_index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .issues
            .get(issue_index)
            .map_or(ptr::null(), |issue| issue.message.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// The DOS drive-letter composer: machine facts asserted by the caller, one
// named assignment rule, and the mapping it establishes. The machine keeps
// its own copy of every report asserted over it, so a report handle may be
// freed while the machine still stands.

use remanence::{DosAssignmentRule, DosMachine, LetterOutcome, ResidentCondition};

/// What one letter turned out to name.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceLetterOutcome {
    /// A volume on an asserted device, named by its report's identity.
    Volume = 0,
    /// A device the caller declared a resident driver placed here. The
    /// library composes no volume for it and invents no identity.
    DeclaredDevice = 1,
    /// DOS's phantom second floppy: the same drive as the letter before
    /// it, not a second volume.
    Phantom = 2,
    /// The claimed rules could not settle this letter.
    Undetermined = 3,
}

enum AssertedDevice {
    Floppy {
        slot: u32,
        report: remanence::DiskReport,
    },
    FixedDisk {
        order: u32,
        report: remanence::DiskReport,
    },
    CdRom {
        order: u32,
        driver_letter: Option<char>,
    },
}

/// The machine facts a caller asserts, in the order they were asserted.
pub struct RemanenceDosMachine {
    devices: Vec<AssertedDevice>,
    conditions: Vec<ResidentCondition>,
}

impl RemanenceDosMachine {
    /// Rebuilds the core machine over the stored facts. Every rule about
    /// what may be asserted lives in the core, so the assertions are
    /// replayed through it rather than re-checked here.
    fn build(&self) -> remanence::Result<DosMachine<'_>> {
        let mut machine = DosMachine::new();
        for device in &self.devices {
            match device {
                AssertedDevice::Floppy { slot, report } => machine.assert_floppy(*slot, report)?,
                AssertedDevice::FixedDisk { order, report } => {
                    machine.assert_fixed_disk(*order, report)?
                }
                AssertedDevice::CdRom {
                    order,
                    driver_letter,
                } => machine.assert_cdrom(*order, *driver_letter)?,
            }
        }
        for condition in &self.conditions {
            machine.declare(*condition);
        }
        Ok(machine)
    }
}

struct MappingView {
    letter: c_char,
    outcome: RemanenceLetterOutcome,
    device_kind: Option<CString>,
    device_index: Option<u32>,
    volume: Option<u64>,
    phantom_of: c_char,
    reason: Option<CString>,
}

/// A composed drive-letter mapping. Owned by the caller and released with
/// `remanence_drive_map_free`; every string reached through it is borrowed
/// from it and dies with it.
pub struct RemanenceDriveMap {
    applied_rules: Vec<CString>,
    mappings: Vec<MappingView>,
    provenance: Vec<CString>,
    established_count: usize,
}

fn ascii_letter(letter: char) -> c_char {
    c_char::try_from(u32::from(letter) as u8 as i8).unwrap_or(0)
}

/// Applies the assertion held in `machine`, rolling the fact back when the
/// core refuses it, so a refused assertion never half-lands.
unsafe fn assert_device(
    machine: *mut RemanenceDosMachine,
    device: AssertedDevice,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(machine) = (unsafe { machine.as_mut() }) else {
        return false;
    };
    machine.devices.push(device);
    match machine.build() {
        Ok(_) => true,
        Err(error) => {
            machine.devices.pop();
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// How many DOS assignment rules this release claims (P3).
#[unsafe(no_mangle)]
pub extern "C" fn remanence_dos_rule_count() -> usize {
    DosAssignmentRule::CLAIMED.len()
}

/// One claimed rule's stable name — the value passed to
/// `remanence_dos_machine_compose`. Null when the index is out of range.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_dos_rule_name(index: usize) -> *const c_char {
    match DosAssignmentRule::CLAIMED.get(index) {
        Some(DosAssignmentRule::MsDos4) => c"ms-dos-4".as_ptr(),
        Some(DosAssignmentRule::MsDos5) => c"ms-dos-5".as_ptr(),
        None => ptr::null(),
    }
}

/// What that rule says, in a sentence fit to show a user beside the
/// mapping it produced.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_dos_rule_reading(index: usize) -> *const c_char {
    match DosAssignmentRule::CLAIMED.get(index) {
        Some(DosAssignmentRule::MsDos4) => {
            c"MS-DOS 4.0 and 4.01: the first primary DOS partition of each disk in attachment order, then the logical drives of each disk's extended partition in the same order; a further primary DOS partition receives no letter".as_ptr()
        }
        Some(DosAssignmentRule::MsDos5) => {
            c"MS-DOS 5.0 through 6.22: the first primary DOS partition of each disk in attachment order, then the logical drives of each disk's extended partition in the same order, then each remaining primary DOS partition by disk in that order".as_ptr()
        }
        None => ptr::null(),
    }
}

/// A machine with nothing asserted about it yet. Free with
/// `remanence_dos_machine_free`.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_dos_machine_new() -> *mut RemanenceDosMachine {
    Box::into_raw(Box::new(RemanenceDosMachine {
        devices: Vec::new(),
        conditions: Vec::new(),
    }))
}

/// Frees a machine and the reports it copied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_dos_machine_free(machine: *mut RemanenceDosMachine) {
    if !machine.is_null() {
        drop(unsafe { Box::from_raw(machine) });
    }
}

/// Asserts that the medium `report` inspects occupies floppy slot `slot` —
/// 0 being `A:`. A slot above 1 and a slot already asserted are refused by
/// name. The report is copied; the handle may be freed afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_dos_machine_assert_floppy(
    machine: *mut RemanenceDosMachine,
    slot: u32,
    report: *const RemanenceDiskReport,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    let Some(report) = (unsafe { report.as_ref() }) else {
        return false;
    };
    unsafe {
        assert_device(
            machine,
            AssertedDevice::Floppy {
                slot,
                report: report.source.clone(),
            },
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Asserts that the medium `report` inspects is the fixed disk attached at
/// `order` — 0 being the first attached, which is the order DOS assigned
/// letters in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_dos_machine_assert_fixed_disk(
    machine: *mut RemanenceDosMachine,
    order: u32,
    report: *const RemanenceDiskReport,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    let Some(report) = (unsafe { report.as_ref() }) else {
        return false;
    };
    unsafe {
        assert_device(
            machine,
            AssertedDevice::FixedDisk {
                order,
                report: report.source.clone(),
            },
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Asserts a CD-ROM drive at attachment order `order`. `driver_letter` is
/// where the caller declares the resident driver placed it; `0` declares
/// no placement, and an undeclared CD-ROM takes no letter rather than a
/// guessed one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_dos_machine_assert_cdrom(
    machine: *mut RemanenceDosMachine,
    order: u32,
    driver_letter: c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    let driver_letter = if driver_letter == 0 {
        None
    } else {
        Some(char::from(driver_letter as u8))
    };
    unsafe {
        assert_device(
            machine,
            AssertedDevice::CdRom {
                order,
                driver_letter,
            },
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Declares a runtime condition outside every claimed rule, by its stable
/// spelling: `lastdrive=<letter>`, `subst`, `join`, `assign`,
/// `block-device-driver`, `network-redirector`. Anything else is refused by
/// name. The letters the condition could have changed come back
/// undetermined.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_dos_machine_declare_condition(
    machine: *mut RemanenceDosMachine,
    condition: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(machine) = (unsafe { machine.as_mut() }) else {
        return false;
    };
    let Some(condition) = (unsafe { utf8_arg(condition) }) else {
        let error = remanence::Error::io("null condition");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match ResidentCondition::parse(condition.as_ref()) {
        Ok(condition) => {
            if !machine.conditions.contains(&condition) {
                machine.conditions.push(condition);
            }
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Composes the mapping. `rule` names the variant the machine ran — one of
/// `remanence_dos_rule_name` — or is null where the caller states none, in
/// which case every claimed rule is applied and a letter they disagree on
/// comes back undetermined. Null on failure, with the category and message
/// written to the out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_dos_machine_compose(
    machine: *const RemanenceDosMachine,
    rule: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDriveMap {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(machine) = (unsafe { machine.as_ref() }) else {
        return ptr::null_mut();
    };

    let rule = if rule.is_null() {
        None
    } else {
        let Some(name) = (unsafe { utf8_arg(rule) }) else {
            return ptr::null_mut();
        };
        match DosAssignmentRule::from_name(name.as_ref()) {
            Ok(rule) => Some(rule),
            Err(error) => {
                unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
                return ptr::null_mut();
            }
        }
    };

    let composed = machine.build().and_then(|machine| machine.compose(rule));
    match composed {
        Ok(map) => Box::into_raw(Box::new(drive_map_view(&map))),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The C-side view of one composed mapping. Both composers — the asserted
/// machine and the machine that reads its own device set — answer with
/// the same records, because they answered with the same core type.
fn drive_map_view(map: &remanence::DriveMap) -> RemanenceDriveMap {
    let mappings = map
        .mappings
        .iter()
        .map(|mapping| {
            let (outcome, device, volume, phantom_of, reason) = match &mapping.outcome {
                LetterOutcome::Volume { device, volume } => (
                    RemanenceLetterOutcome::Volume,
                    Some(*device),
                    Some(volume.value()),
                    0,
                    None,
                ),
                LetterOutcome::DeclaredDevice { device } => (
                    RemanenceLetterOutcome::DeclaredDevice,
                    Some(*device),
                    None,
                    0,
                    None,
                ),
                LetterOutcome::Phantom { of } => (
                    RemanenceLetterOutcome::Phantom,
                    None,
                    None,
                    ascii_letter(*of),
                    None,
                ),
                LetterOutcome::Undetermined { reason } => (
                    RemanenceLetterOutcome::Undetermined,
                    None,
                    None,
                    0,
                    Some(to_cstring(reason)),
                ),
            };
            MappingView {
                letter: ascii_letter(mapping.letter),
                outcome,
                device_kind: device.map(|device| to_cstring(device.kind())),
                device_index: device.map(remanence::MachineDevice::index),
                volume,
                phantom_of,
                reason,
            }
        })
        .collect();
    RemanenceDriveMap {
        applied_rules: map
            .applied_rules
            .iter()
            .map(|rule| to_cstring(rule.name()))
            .collect(),
        established_count: map.established_count(),
        mappings,
        provenance: map.provenance.iter().map(|line| to_cstring(line)).collect(),
    }
}

/// Frees a composed mapping and everything borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_free(map: *mut RemanenceDriveMap) {
    if !map.is_null() {
        drop(unsafe { Box::from_raw(map) });
    }
}

unsafe fn mapping_view<'a>(map: *const RemanenceDriveMap, index: usize) -> Option<&'a MappingView> {
    unsafe { map.as_ref() }?.mappings.get(index)
}

/// How many rules were applied: one where the caller stated the variant,
/// and every claimed rule where it did not.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_applied_rule_count(
    map: *const RemanenceDriveMap,
) -> usize {
    unsafe { map.as_ref() }.map_or(0, |map| map.applied_rules.len())
}

/// One applied rule's stable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_applied_rule(
    map: *const RemanenceDriveMap,
    index: usize,
) -> *const c_char {
    unsafe { map.as_ref() }.map_or(ptr::null(), |map| {
        map.applied_rules
            .get(index)
            .map_or(ptr::null(), |rule| rule.as_ptr())
    })
}

/// How many letters the machine had a drive at. A letter absent from the
/// mapping is a letter the machine had no drive at, which is different from
/// one that exists and could not be settled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_count(map: *const RemanenceDriveMap) -> usize {
    unsafe { map.as_ref() }.map_or(0, |map| map.mappings.len())
}

/// How many letters the rules established — the count that excludes every
/// undetermined one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_established_count(
    map: *const RemanenceDriveMap,
) -> usize {
    unsafe { map.as_ref() }.map_or(0, |map| map.established_count)
}

/// The letter at `index`, without its colon. `0` when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_letter(
    map: *const RemanenceDriveMap,
    index: usize,
) -> c_char {
    unsafe { mapping_view(map, index) }.map_or(0, |mapping| mapping.letter)
}

/// Finds the entry for one letter, writing its index. False where the
/// machine had no drive at that letter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_find(
    map: *const RemanenceDriveMap,
    letter: c_char,
    index_out: *mut usize,
) -> bool {
    let Some(map) = (unsafe { map.as_ref() }) else {
        return false;
    };
    let wanted = (letter as u8).to_ascii_uppercase() as c_char;
    let Some(index) = map
        .mappings
        .iter()
        .position(|mapping| mapping.letter == wanted)
    else {
        return false;
    };
    if let Some(out) = unsafe { index_out.as_mut() } {
        *out = index;
    }
    true
}

/// What the letter at `index` turned out to name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_outcome(
    map: *const RemanenceDriveMap,
    index: usize,
) -> RemanenceLetterOutcome {
    unsafe { mapping_view(map, index) }
        .map_or(RemanenceLetterOutcome::Undetermined, |mapping| {
            mapping.outcome
        })
}

/// The asserted device this letter names — `floppy`, `fixed-disk` or
/// `cd-rom` — or null where the outcome names no device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_device_kind(
    map: *const RemanenceDriveMap,
    index: usize,
) -> *const c_char {
    unsafe { mapping_view(map, index) }.map_or(ptr::null(), |mapping| {
        mapping
            .device_kind
            .as_ref()
            .map_or(ptr::null(), |kind| kind.as_ptr())
    })
}

/// The slot or attachment order the caller asserted for that device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_device_index(
    map: *const RemanenceDriveMap,
    index: usize,
    value_out: *mut u32,
) -> bool {
    let value = unsafe { mapping_view(map, index) }.and_then(|mapping| mapping.device_index);
    unsafe { write_opt_u32(value, value_out) }
}

/// The volume this letter names, by the identity its own inspection report
/// issued — the value passed back into a file verb. False where the
/// outcome names no volume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_volume(
    map: *const RemanenceDriveMap,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let value = unsafe { mapping_view(map, index) }.and_then(|mapping| mapping.volume);
    unsafe { write_opt_u64(value, value_out) }
}

/// The letter a phantom drive stands for, or `0` where this outcome is not
/// a phantom.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_phantom_of(
    map: *const RemanenceDriveMap,
    index: usize,
) -> c_char {
    unsafe { mapping_view(map, index) }.map_or(0, |mapping| mapping.phantom_of)
}

/// Why the claimed rules could not settle this letter, or null where they
/// did.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_reason(
    map: *const RemanenceDriveMap,
    index: usize,
) -> *const c_char {
    unsafe { mapping_view(map, index) }.map_or(ptr::null(), |mapping| {
        mapping
            .reason
            .as_ref()
            .map_or(ptr::null(), |reason| reason.as_ptr())
    })
}

/// How many provenance lines the mapping carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_provenance_count(
    map: *const RemanenceDriveMap,
) -> usize {
    unsafe { map.as_ref() }.map_or(0, |map| map.provenance.len())
}

/// One provenance line: the asserted facts and the applied rules,
/// travelling with the answer. **This is not evidence** — nothing in it was
/// read off a disk.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_drive_map_provenance(
    map: *const RemanenceDriveMap,
    index: usize,
) -> *const c_char {
    unsafe { map.as_ref() }.map_or(ptr::null(), |map| {
        map.provenance
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A 1.44 MiB FAT12 floppy holding one file whose cluster chain runs
    /// past `keep` bytes, then truncated to `keep` — the shape P28's
    /// degraded reading is stated over.
    fn truncated_floppy(path: &std::path::Path, keep: u64) {
        const TOTAL_SECTORS: usize = 2880;
        let mut image = vec![0u8; TOTAL_SECTORS * 512];
        image[0] = 0xeb;
        image[1] = 0x3c;
        image[2] = 0x90;
        image[3..11].copy_from_slice(b"REMANENC");
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1u16.to_le_bytes());
        image[16] = 2;
        image[17..19].copy_from_slice(&224u16.to_le_bytes());
        image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
        image[21] = 0xf0;
        image[22..24].copy_from_slice(&9u16.to_le_bytes());
        image[24..26].copy_from_slice(&18u16.to_le_bytes());
        image[26..28].copy_from_slice(&2u16.to_le_bytes());
        image[510] = 0x55;
        image[511] = 0xaa;
        for fat in 0..2usize {
            let base = (1 + fat * 9) * 512;
            image[base] = 0xf0;
            image[base + 1] = 0xff;
            image[base + 2] = 0xff;
        }
        std::fs::write(path, image).expect("image writes");

        // The file is written through the library's own writer, so the
        // chain the truncation cuts is a real one.
        let mut session = remanence::Session::new();
        let medium = session
            .add_device(DeviceFamily::HARD_DISK)
            .expect("the drive is added");
        medium
            .load_media(path, AccessIntent::Write)
            .expect("the whole image loads");
        let content: Vec<u8> = (0..1_200_000u32).map(|n| (n % 241) as u8).collect();
        medium
            .filesystem()
            .expect("the floppy resolves to its one filesystem")
            .write_file("FAR.BIN", &content)
            .expect("writes");
        medium.commit().expect("commits");
        drop(session);

        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("opens for truncation")
            .set_len(keep)
            .expect("truncates");
    }

    /// The C presentation of P28 carries what Rust's does: the outcome,
    /// the condition, the ordered evidence, the exact readable extent,
    /// and the effective access mode — and a withheld write names the
    /// same condition as its rule (P5).
    #[test]
    fn the_c_surface_reports_a_degraded_medium_and_withholds_its_writes() {
        let path = std::env::temp_dir().join(format!(
            "remanence-ffi-degraded-{}.img",
            std::process::id()
        ));
        truncated_floppy(&path, 1_000_000);

        let session = unsafe { remanence_session_new() };
        let path_arg = to_cstring(&path.display().to_string());
        let family = to_cstring("hard-disk");
        let mut category = RemanenceErrorCategory::Io;
        let mut message = ptr::null_mut();
        let mut rule = ptr::null_mut();
        let device = unsafe {
            remanence_session_add_device(
                session,
                family.as_ptr(),
                &mut category,
                &mut message,
                &mut rule,
            )
        };
        assert!(!device.is_null(), "the drive is added");
        assert!(
            unsafe {
                remanence_device_load_media(
                    device,
                    path_arg.as_ptr(),
                    RemanenceAccessIntent::Write,
                    &mut category,
                    &mut message,
                    &mut rule,
                )
            },
            "a truncated source still loads, degraded"
        );

        let assurance = unsafe { remanence_device_assurance(device) };
        assert_eq!(
            unsafe { remanence_assurance_outcome(assurance) },
            RemanenceAssuranceOutcome::Degraded
        );
        assert_eq!(
            unsafe { CStr::from_ptr(remanence_assurance_condition(assurance)) }
                .to_str()
                .expect("UTF-8"),
            "source-truncated"
        );
        assert_eq!(
            unsafe { remanence_assurance_access_mode(assurance) },
            RemanenceAccessMode::ReadOnly
        );
        assert_eq!(
            unsafe { remanence_device_mode(device) },
            RemanenceAccessMode::ReadOnly,
            "the effective mode is the same answer read another way"
        );
        assert!(unsafe { remanence_assurance_evidence_count(assurance) } > 0);
        assert!(
            !unsafe { remanence_assurance_evidence(assurance, 0) }.is_null(),
            "the declaration leads the evidence"
        );
        assert!(unsafe { remanence_assurance_evidence(assurance, 99) }.is_null());

        let mut declared = 0u64;
        let mut observed = 0u64;
        let mut first_unavailable = 0u64;
        assert!(unsafe { remanence_assurance_declared_bytes(assurance, &mut declared) });
        assert!(unsafe { remanence_assurance_observed_bytes(assurance, &mut observed) });
        assert!(unsafe {
            remanence_assurance_first_unavailable_byte(assurance, &mut first_unavailable)
        });
        assert_eq!(declared, 1_474_560);
        assert_eq!(observed, 1_000_000);
        assert_eq!(first_unavailable, 1_000_000);

        assert_eq!(unsafe { remanence_assurance_readable_count(assurance) }, 1);
        let mut start = u64::MAX;
        let mut end = 0u64;
        assert!(unsafe { remanence_assurance_readable(assurance, 0, &mut start, &mut end) });
        assert_eq!((start, end), (0, 1_000_000));
        assert!(!unsafe { remanence_assurance_readable(assurance, 1, &mut start, &mut end) });
        unsafe { remanence_assurance_free(assurance) };

        // Every mutation path carries the condition as its rule.
        assert!(
            !unsafe {
                remanence_device_commit(device, &mut category, &mut message, &mut rule)
            },
            "commit is denied"
        );
        assert_eq!(category, RemanenceErrorCategory::ReadOnly);
        assert_eq!(
            unsafe { CStr::from_ptr(rule) }.to_str().expect("UTF-8"),
            "source-truncated"
        );
        unsafe { remanence_string_free(message) };
        unsafe { remanence_string_free(rule) };

        // The claimed condition set is readable without meeting one.
        assert_eq!(remanence_assurance_condition_count(), 2);
        assert_eq!(
            unsafe { CStr::from_ptr(remanence_assurance_condition_name(1)) }
                .to_str()
                .expect("UTF-8"),
            "evidence-conflict"
        );
        assert!(remanence_assurance_condition_name(2).is_null());

        unsafe { remanence_session_free(session) };
        std::fs::remove_file(&path).ok();
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
