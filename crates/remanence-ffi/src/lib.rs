// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! C ABI for the Remanence disk image analysis library.
//!
//! Conventions:
//! - Handles (`RemanenceIdentification`, `RemanencePartition`,
//!   `RemanenceSpace`, `RemanenceFile`, `RemanenceDiskReport`) are
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
    AttachmentId, DiskLayout, ErrorCategory, Format, Identification, Layer, LayerKind, LayerLayout,
    MediaId, PhysicalMediaLayout, SectorLayout, Session,
};

/// Counting live allocations, so a C caller can prove the `_free`
/// discipline (D47).
///
/// **Everything this ABI hands out is allocated by Rust inside this
/// cdylib** — `CString::into_raw` for strings, `Box::into_raw` for
/// handles — and freed by Rust when the matching `remanence_*_free`
/// runs. A C-side leak checker cannot see any of it: CppUTest, and the
/// sanitizers, instrument the *test binary's* allocator, which these
/// allocations never touch. So the count has to come from in here.
///
/// Off by default and absent from a released artifact: it is a global
/// allocator and an exported symbol, and an extra `remanence_*` symbol
/// would be an S2 change.
#[cfg(feature = "leak-probe")]
mod leak_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicI64, Ordering};

    static LIVE: AtomicI64 = AtomicI64::new(0);

    /// Counts blocks rather than bytes: the question is whether every
    /// allocation was given back, and a block is what a `_free` returns.
    pub struct Counting;

    // SAFETY: every method forwards to `System` unchanged and only adds
    // an atomic to the bookkeeping, so the allocator contract is
    // whatever `System` already satisfies.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if !pointer.is_null() {
                LIVE.fetch_add(1, Ordering::Relaxed);
            }
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            if !pointer.is_null() {
                LIVE.fetch_add(1, Ordering::Relaxed);
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            LIVE.fetch_sub(1, Ordering::Relaxed);
            unsafe { System.dealloc(pointer, layout) }
        }

        // `realloc` is deliberately left to the trait's default, which
        // allocates, copies and deallocates through the methods above —
        // so a growing buffer nets to zero rather than needing its own
        // rule.
    }

    pub fn live() -> i64 {
        LIVE.load(Ordering::Relaxed)
    }
}

#[cfg(feature = "leak-probe")]
#[global_allocator]
static LEAK_PROBE: leak_probe::Counting = leak_probe::Counting;

/// How many Rust allocations inside this library are live right now.
///
/// Test-only, and present only under the `leak-probe` feature — it is
/// deliberately **not** in the generated header, because it is not part
/// of S2. A caller declares it itself.
#[cfg(feature = "leak-probe")]
#[unsafe(no_mangle)]
pub extern "C" fn remanence_probe_live_allocations() -> i64 {
    leak_probe::live()
}

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
    article: CString,
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
            SectorLayout::Fixed { sectors_per_track } => (
                RemanenceSectorLayoutKind::Fixed,
                *sectors_per_track,
                Vec::new(),
            ),
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
            article: to_cstring(&layout.article),
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
        /// Where the archive sits, where its own handle could be named.
        path: Option<CString>,
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
                path: layout
                    .path
                    .as_ref()
                    .map(|path| to_cstring(&path.display().to_string())),
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

/// What this device is, by its stable spelling — a device type's own
/// (`mbr-block-hd`) or `archive`. Owned by the view; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_slot(device: *const RemanenceDevice) -> *const c_char {
    match unsafe { device.as_ref() } {
        Some(handle) => handle.slot_c.as_ptr(),
        None => ptr::null(),
    }
}

/// The recording device type this slot is typed by, or null for the
/// archive receiver — which records nothing, as the archive it holds was
/// recorded by nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_type(device: *const RemanenceDevice) -> *const c_char {
    match unsafe { device.as_ref() } {
        Some(handle) => handle
            .device_type_c
            .as_ref()
            .map_or(ptr::null(), |device| device.as_ptr()),
        None => ptr::null(),
    }
}

/// Whether a medium currently occupies this device's slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_is_occupied(device: *const RemanenceDevice) -> bool {
    match unsafe { device.as_ref() } {
        Some(handle) => handle.device().is_some_and(StorageDevice::is_occupied),
        None => false,
    }
}

/// The identity of the medium in this device's slot, or 0 while it is
/// empty. Read it beside `remanence_device_is_occupied`, which
/// distinguishes an empty slot from a pool identity of zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_media_id(device: *const RemanenceDevice) -> u64 {
    match unsafe { device.as_ref() } {
        Some(handle) => handle
            .device()
            .and_then(StorageDevice::media_id)
            .map_or(0, MediaId::value),
        None => 0,
    }
}

/// The medium in this device's slot, or null while it is empty — the
/// borrowed view every content verb answers on. The session owns it;
/// never free it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_medium(
    device: *mut RemanenceDevice,
) -> *mut RemanenceMedium {
    let Some(handle) = (unsafe { device.as_ref() }) else {
        return ptr::null_mut();
    };
    let Some(id) = handle.device().and_then(StorageDevice::media_id) else {
        return ptr::null_mut();
    };
    unsafe { medium_view(handle.session, id) }
}

/// Links the pooled medium `media_id` into this device's slot.
///
/// **The check is device-type equality** (P14): a medium carries the
/// device its content was recorded by, a slot is typed by the device
/// that fills it, and a medium belonging in another drive is refused
/// naming both sides. An
/// identity the pool does not hold, a slot already occupied, and a medium
/// another slot already holds are each refused by name. Returns false on
/// failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_device_insert(
    device: *mut RemanenceDevice,
    media_id: u64,
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
    let Some(mut target) = handle.view() else {
        let error = remanence::Error::io("this device was released from its machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match target.insert(MediaId::from_value(media_id)) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// **Severs the link and nothing more**: the device stays in its machine
/// and the medium stays in the session's pool, its claim, its assurance
/// and everything buffered intact.
///
/// Ejecting is not a commit point and never becomes one. Destroying a
/// medium's state is `remanence_session_release_media`, and it is the one
/// verb that does. Returns false when the slot was already empty.
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
    let Some(mut target) = handle.view() else {
        let error = remanence::Error::io("this device was released from its machine");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match target.eject() {
        Ok(_) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// One slot's strings, built once so every reader below answers with a
/// pointer the library owns and the caller never frees.
struct SlotView {
    id: CString,
    name: CString,
    provenance: Option<CString>,
    class: Option<CString>,
    article: Option<CString>,
    slot_prefix: CString,
    flux_path: Option<CString>,
    scheme: Option<CString>,
    addressing: Option<CString>,
}

/// Every slot a machine may hold a device in: one per device type, and
/// the archive receiver, which is no device type at all — its device
/// fields read null, exactly as a medium recorded by no device answers.
fn slots() -> &'static [SlotView] {
    static SLOTS: std::sync::OnceLock<Vec<SlotView>> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| {
        DeviceSlot::claimed()
            .into_iter()
            .map(|slot| SlotView {
                id: to_cstring(slot.id()),
                name: to_cstring(slot.name()),
                provenance: slot
                    .device_type()
                    .map(|device| to_cstring(device.provenance())),
                class: slot.device_type().map(|device| to_cstring(device.class())),
                article: slot
                    .device_type()
                    .map(|device| to_cstring(device.article())),
                slot_prefix: to_cstring(slot.slot_prefix()),
                flux_path: slot
                    .device_type()
                    .and_then(DeviceType::flux_path)
                    .map(to_cstring),
                scheme: slot
                    .device_type()
                    .and_then(DeviceType::scheme)
                    .map(to_cstring),
                addressing: slot
                    .device_type()
                    .map(|device| to_cstring(device.addressing())),
            })
            .collect()
    })
}

fn slot_string(index: usize, read: fn(&SlotView) -> Option<&CString>) -> *const c_char {
    slots()
        .get(index)
        .and_then(read)
        .map_or(ptr::null(), |value| value.as_ptr())
}

/// How many slots this release claims: one per device type in the
/// catalog (P14), plus the archive receiver.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_count() -> usize {
    slots().len()
}

/// The stable spelling of slot `index` — a device type's own (`c1541`,
/// `mbr-block-hd`) or `archive`, and the value
/// `remanence_session_add_device` takes. Null when out of range; owned by
/// the library and never freed.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_id(index: usize) -> *const c_char {
    slot_string(index, |slot| Some(&slot.id))
}

/// Slot `index`'s name, fit to show a user beside the bay it fills.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_name(index: usize) -> *const c_char {
    slot_string(index, |slot| Some(&slot.name))
}

/// Where slot `index`'s device-type declaration came from. Null for the
/// archive receiver, which declares no recording.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_provenance(index: usize) -> *const c_char {
    slot_string(index, |slot| slot.provenance.as_ref())
}

/// The class of slot `index`'s device type — `floppy`, `hard-drive` or
/// `optical`, the first of the catalog's two levels. Null for the
/// archive receiver.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_class(index: usize) -> *const c_char {
    slot_string(index, |slot| slot.class.as_ref())
}

/// The article slot `index`'s device type is served (P14), by stable
/// spelling. Null for the archive receiver.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_article(index: usize) -> *const c_char {
    slot_string(index, |slot| slot.article.as_ref())
}

/// The bay half of every attachment identity in slot `index` — `hdd` for
/// `hdd0`. Several device types share one where the machine does.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_prefix(index: usize) -> *const c_char {
    slot_string(index, |slot| Some(&slot.slot_prefix))
}

/// The drive profile slot `index`'s device type claims as its recording
/// path (P22), by stable spelling. Null where it claims none, which is
/// ordinary rather than deficient.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_flux_path(index: usize) -> *const c_char {
    slot_string(index, |slot| slot.flux_path.as_ref())
}

/// The partition scheme slot `index`'s device type lays its content out
/// under, by stable spelling — the hard-drive specs carry it. Null for
/// the schemeless types, whose media bear the direct partition.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_scheme(index: usize) -> *const c_char {
    slot_string(index, |slot| slot.scheme.as_ref())
}

/// How slot `index`'s device type addresses its recording — `sector` or
/// `block`. Every device type declares one; null for the archive
/// receiver, which is no device type at all.
///
/// A `sector` type is one whose medium answers
/// `remanence_medium_read_sector` and `remanence_medium_write_sector`, in
/// the coordinates that medium's own geometry established.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_device_slot_addressing(index: usize) -> *const c_char {
    slot_string(index, |slot| slot.addressing.as_ref())
}

/// What one artifact turned out to be, and the claim under which that was
/// established.
///
/// Free it with `remanence_discovery_free`, or hand it to
/// `remanence_session_load_discovery`, which consumes it. Every string it
/// returns is owned by it and freed with it.
pub struct RemanenceDiscovery {
    discovery: remanence::Discovery,
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
    fn new(discovery: remanence::Discovery) -> Self {
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

/// This medium's identity in its session's pool.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_id(medium: *const RemanenceMedium) -> u64 {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle.id.value(),
        None => 0,
    }
}

/// Whether a device currently links this medium. An unlinked medium is
/// ordinary rather than idle: it is loaded, claimed, and answering.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_is_linked(medium: *const RemanenceMedium) -> bool {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle.medium().is_some_and(|medium| medium.is_linked()),
        None => false,
    }
}

/// The article this medium is (P14), by the catalog's stable spelling —
/// the physical substrate. Owned by the library; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_article(medium: *const RemanenceMedium) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .article
            .as_ref()
            .map_or(ptr::null(), |article| article.as_ptr()),
        None => ptr::null(),
    }
}

/// The device this medium's content was recorded by, by the device
/// catalog's stable spelling — or null where no device recorded it,
/// which is an archive's honest answer rather than a gap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_device_type(
    medium: *const RemanenceMedium,
) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .device_type
            .as_ref()
            .map_or(ptr::null(), |device| device.as_ptr()),
        None => ptr::null(),
    }
}

/// The artifact the medium was loaded from (the archive itself for an
/// image loaded out of one).
///
/// **Null where the caller's handle has no recoverable name** — a name
/// serves location alone, and a nameless handle is served everywhere that
/// does not need a neighbourhood.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_path(medium: *const RemanenceMedium) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr()),
        None => ptr::null(),
    }
}

/// The resolved image path (the entry name for archive inputs), or null
/// as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_image_path(
    medium: *const RemanenceMedium,
) -> *const c_char {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .image_path
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr()),
        None => ptr::null(),
    }
}

/// The resolved image's size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_image_size_bytes(medium: *const RemanenceMedium) -> u64 {
    match unsafe { medium.as_ref() } {
        Some(handle) => handle
            .medium()
            .map(|medium| medium.image_size_bytes())
            .unwrap_or(0),
        None => 0,
    }
}

/// Reads `length` bytes of the resolved image at `offset` into
/// `buffer_out` — the bounded access form: the image streams from its
/// backing and is never resident whole. Returns false on failure and
/// stores a message in `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_read_at(
    medium: *const RemanenceMedium,
    offset: u64,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
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
pub unsafe extern "C" fn remanence_medium_identify(
    medium: *const RemanenceMedium,
) -> *mut RemanenceIdentification {
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        return ptr::null_mut();
    };
    let Identification {
        layers,
        modified,
        evidence,
    } = match handle.medium().map(|medium| medium.identify()) {
        Some(identification) => identification,
        None => return ptr::null_mut(),
    };

    Box::into_raw(Box::new(RemanenceIdentification {
        modified,
        layers: layers.iter().map(NestedLayerView::new).collect(),
        evidence: evidence.iter().map(|line| to_cstring(line)).collect(),
    }))
}

/// Frees an identification handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_identification_free(
    identification: *mut RemanenceIdentification,
) {
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
    unsafe { layer_view(identification, index) }.map_or(ptr::null(), |layer| layer.id.as_ptr())
}

/// The layer's human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_name(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { layer_view(identification, index) }.map_or(ptr::null(), |layer| layer.name.as_ptr())
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
        Some(LayoutView::Archive { path, .. }) => {
            path.as_ref().map_or(ptr::null(), |path| path.as_ptr())
        }
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

/// Disk layout: the article the image format names for its medium
/// (e.g. "logical-block-512"); null when the layer has no disk
/// layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_layer_disk_article(
    identification: *const RemanenceIdentification,
    index: usize,
) -> *const c_char {
    unsafe { disk_view(identification, index) }.map_or(ptr::null(), |disk| disk.article.as_ptr())
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
    unsafe { disk_view(identification, index) }.map_or(RemanenceSectorLayoutKind::Unknown, |disk| {
        disk.sector_layout
    })
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
    AccessIntent, AccessMode, DeviceSlot, DeviceType, DiskContent, DiskFormat, Entry, EntryKind,
    NewMedia, RegionRole, StorageDevice, VolumeId, VolumeOrigin,
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
    Imd,
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

/// An open session: the claim and cache scope, holding the devices
/// within it (P32).
pub struct RemanenceSession {
    session: Session,
    /// Borrowed device views handed to callers. Owned here so their
    /// strings outlive the call that produced them, and freed with the
    /// session.
    views: Vec<Box<RemanenceDevice>>,
    /// Borrowed medium views handed to callers, owned here for the same
    /// reason and freed with the session.
    media: Vec<Box<RemanenceMedium>>,
}

/// A borrowed view of one medium in a session's media pool — the content
/// handle, and where every content verb lives.
///
/// **The session owns this; never free it.** It stays valid until the
/// medium is released or the session is freed, and it names the medium by
/// session and pool identity rather than by pointer, so a later load can
/// never make it point at a stranger.
pub struct RemanenceMedium {
    session: *mut RemanenceSession,
    id: MediaId,
    /// The artifact's names, absent where the caller's handle has none.
    path: Option<CString>,
    image_path: Option<CString>,
    /// The article and the device that recorded it, settled at the load
    /// like the names.
    article: Option<CString>,
    device_type: Option<CString>,
}

impl RemanenceMedium {
    /// The medium this view names, or `None` once it is released.
    #[allow(clippy::mut_from_ref)]
    fn medium(&self) -> Option<&mut remanence::Medium> {
        let session = unsafe { &mut (*self.session).session };
        session.medium_mut(self.id)
    }

    /// Restates the artifact's names. They are settled at the load and
    /// never change after it, so this runs once when the view is minted.
    fn refresh(&mut self) {
        let (path, image_path, article, device_type) = match self.medium() {
            Some(medium) => (
                medium.path().map(to_cstring),
                medium
                    .image_path()
                    .map(|path| to_cstring(&path.display().to_string())),
                Some(to_cstring(medium.article())),
                medium.device_type().map(|device| to_cstring(device.id())),
            ),
            None => (None, None, None, None),
        };
        self.path = path;
        self.image_path = image_path;
        self.article = article;
        self.device_type = device_type;
    }
}

/// Mints — or re-uses — the borrowed view of `id` in `session`.
unsafe fn medium_view(session: *mut RemanenceSession, id: MediaId) -> *mut RemanenceMedium {
    let handle = unsafe { &mut *session };
    if let Some(at) = handle.media.iter().position(|view| view.id == id) {
        handle.media[at].refresh();
        return handle.media[at].as_mut() as *mut RemanenceMedium;
    }
    let mut view = Box::new(RemanenceMedium {
        session,
        id,
        path: None,
        image_path: None,
        article: None,
        device_type: None,
    });
    view.refresh();
    handle.media.push(view);
    handle.media.last_mut().expect("just pushed").as_mut() as *mut RemanenceMedium
}

/// A borrowed view of one storage device — the slot, what it is, and
/// the state of the medium in it.
///
/// **The session owns this; never free it.** It stays valid until the
/// device is released or the session is freed.
///
/// It names the device by session and attachment identity rather than
/// by pointer, and re-resolves on every call. That is deliberate: a
/// later attach may reallocate the session's device storage, so a
/// cached pointer to the device itself would dangle silently.
pub struct RemanenceDevice {
    session: *mut RemanenceSession,
    attachment: AttachmentId,
    /// The slot-side facts, which do not change while the device exists.
    /// **They are all a device has**: every content-side fact answers on
    /// the medium, which the pool holds and this slot merely links.
    attachment_c: CString,
    slot_c: CString,
    device_type_c: Option<CString>,
}

impl RemanenceDevice {
    /// The device this view names, with its session's media pool beside
    /// it — the handle the edge verbs live on. `None` once the device is
    /// removed.
    fn view(&self) -> Option<remanence::DeviceView<'_>> {
        let session = unsafe { &mut (*self.session).session };
        session.device_mut(self.attachment)
    }

    /// The device as configuration, for the slot-side readers.
    fn device(&self) -> Option<&StorageDevice> {
        let session = unsafe { &*self.session };
        session.session.device(self.attachment)
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

/// Whose open a medium's P7 claim is.
///
/// In-force P7 makes denying writes to every other process mandatory
/// **where the library opens**, and leaves the claim to the caller where
/// the caller opened. A third answer exists because a third fact class
/// does: nobody opened an authored medium.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemanenceClaim {
    /// The library opened the artifact and holds P7's denial itself —
    /// the discovery path, and every artifact reached by name.
    LibraryOpened = 0,
    /// The caller opened the artifact and handed the handle over. What
    /// that handle affords is the whole of what the session has.
    CallerOpened = 1,
    /// Nobody opened anything: the medium was created whole by the
    /// author (`remanence_session_new_media`), and there is no artifact
    /// for a claim to be over.
    Authored = 2,
}

fn claim_class(claim: remanence::Claim) -> RemanenceClaim {
    match claim {
        remanence::Claim::LibraryOpened => RemanenceClaim::LibraryOpened,
        remanence::Claim::CallerOpened => RemanenceClaim::CallerOpened,
        remanence::Claim::Authored => RemanenceClaim::Authored,
    }
}

/// Whose open this medium's claim is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_assurance_claim(
    assurance: *const RemanenceAssurance,
) -> RemanenceClaim {
    unsafe { assurance.as_ref() }.map_or(RemanenceClaim::LibraryOpened, |assurance| assurance.claim)
}

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

struct FormatView {
    id: CString,
    name: CString,
    /// The device types this format's adapter records — what a
    /// declaration may name, and what a refusal quotes back.
    devices: Vec<CString>,
    block_bytes: bool,
    collection: bool,
}

fn format_views() -> &'static [FormatView] {
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
unsafe fn declared_format(
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

struct NewMediaView {
    id: CString,
    name: CString,
    article: CString,
    geometry: bool,
}

fn new_media_views() -> &'static [NewMediaView] {
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

/// Creates blank media whole — **authorship, the third fact class** — and
/// answers with the medium, linked to nothing. The session owns the view;
/// never free it. Null on failure.
///
/// Nothing is discovered and nothing is opened, because there is no
/// artifact: the author declares one enumerated `kind` (a stable spelling
/// from `remanence_new_media_id`), and the facts that declaration states
/// become the medium's original facts — carried from creation as its
/// assurance provenance and, where the kind states coordinates, as its
/// `remanence_medium_geometry`, whose one reading is `authorship`.
///
/// `cylinders`, `heads`, `sectors_per_track` and `sector_bytes` are the
/// author's own coordinates, for the kind whose claim takes them
/// (`remanence_new_media_takes_geometry`); every other kind takes zeros
/// and refuses anything else by name. Coordinates that address nothing —
/// a zero in any part, or a product no medium could hold — are refused
/// here, when they are stated, which is the one moment authorship offers.
///
/// **An authored blank assumes no device**: `remanence_medium_device_type`
/// answers null, so no drive takes one and `remanence_device_insert`
/// refuses by name. It is session-backed until an explicit encode gives it
/// an artifact, and `remanence_medium_commit` is the ordinary commit point
/// over it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_new_media(
    session: *mut RemanenceSession,
    kind: *const c_char,
    cylinders: u32,
    heads: u32,
    sectors_per_track: u32,
    sector_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if session.is_null() {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let Some(kind) = (unsafe { utf8_arg(kind) }) else {
        let error = remanence::Error::io("null authored kind");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    // All-zero coordinates are "none stated", which is what a blank
    // article kind takes; anything else is the author speaking, and the
    // kind's own claim decides whether it may.
    let stated = cylinders != 0 || heads != 0 || sectors_per_track != 0 || sector_bytes != 0;
    let geometry = stated.then_some(remanence::RecordingGeometry {
        cylinders,
        heads,
        sectors_per_track,
        sector_bytes,
    });
    let kind = match NewMedia::declared(kind.as_ref(), geometry) {
        Ok(kind) => kind,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let handle = unsafe { &mut *session };
    match handle.session.new_media(kind) {
        Ok(medium) => {
            let id = medium.id();
            unsafe { medium_view(session, id) }
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Takes ownership of a caller-opened OS file handle.
///
/// On Windows this is a `HANDLE` from `CreateFile`; elsewhere it is a
/// file descriptor. The library owns it from here: it is closed when the
/// medium is released or the session is freed, and the caller must not
/// close it themselves.
#[cfg(windows)]
unsafe fn file_from_raw(source: isize) -> std::fs::File {
    use std::os::windows::io::FromRawHandle;
    unsafe { std::fs::File::from_raw_handle(source as *mut std::ffi::c_void) }
}

#[cfg(not(windows))]
unsafe fn file_from_raw(source: isize) -> std::fs::File {
    use std::os::fd::FromRawFd;
    unsafe { std::fs::File::from_raw_fd(source as i32) }
}

/// Loads the caller's own opened artifact as the format they **declare**
/// it to be, and answers with the medium — linked to nothing. The session
/// owns the view; never free it. Null on failure.
///
/// `source` is the caller's own OS file handle — a Windows `HANDLE`, a
/// POSIX file descriptor — and **the library takes ownership of it**:
/// closing it is the library's, at release or at session free.
///
/// **Whoever opens owns the lock** (P7 as amended). That open is the
/// claim: the library checks it for exactly one thing — may it write
/// through it? — honours the answer exactly, and never supplements it
/// with a lock of its own. A name is recovered from the handle for
/// location alone, under an identity check; a handle this host cannot
/// name serves everything but the commit journal and a backing chain's
/// parent, and refuses those two by name.
///
/// The declaration is checked by that one format's own adapter and
/// refused by name where the evidence cannot bear it. `format` is a
/// stable spelling from `remanence_format_id`.
///
/// **The declaration carries the device the content was recorded by.**
/// `device_type` is a stable spelling from `remanence_format_device`,
/// and may be null where the format records exactly one type and so
/// carries it bare. `block_bytes` is the raw reading's declared
/// addressable unit and is ignored — passed as zero — by every format
/// that records its own.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_media(
    session: *mut RemanenceSession,
    source: isize,
    format: *const c_char,
    device_type: *const c_char,
    block_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if session.is_null() {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let Some(format) = (unsafe { utf8_arg(format) }) else {
        let error = remanence::Error::io("null format");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let format = match unsafe { declared_format(format.as_ref(), device_type, block_bytes) } {
        Ok(format) => format,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    // Checked before the handle is adopted: an invalid one is the
    // caller's to close, and adopting it would close a handle we were
    // never given.
    if source == 0 || source == -1 {
        let error = remanence::Error::io("the source is not an open file handle");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let file = unsafe { file_from_raw(source) };
    let handle = unsafe { &mut *session };
    match handle.session.load_media(file, format) {
        Ok(medium) => {
            let id = medium.id();
            unsafe { medium_view(session, id) }
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Loads a collection of the caller's own opened artifacts as the format
/// they **declare** it to be — the collection shape of the load, which
/// only a format whose claim takes one reads
/// (`remanence_format_takes_collection`): a KryoFlux capture set is one
/// disk spread over a stream per head per drive-step position. Answers
/// with the medium — linked to nothing. The session owns the view; never
/// free it. Null on failure.
///
/// `sources` points at `source_count` OS file handles, each exactly what
/// `remanence_session_load_media` takes for one. Every member is checked
/// before any is adopted — a 0 or -1 among them refuses the whole load
/// and leaves every handle the caller's to close, mirroring the single
/// form's check — and once all are checked **the library takes ownership
/// of every one, whatever the outcome**: a refused load closes them, a
/// successful one closes them at release or at session free.
///
/// `format`, `device_type` and `block_bytes` are as
/// `remanence_session_load_media` takes them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_media_collection(
    session: *mut RemanenceSession,
    sources: *const isize,
    source_count: usize,
    format: *const c_char,
    device_type: *const c_char,
    block_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if session.is_null() {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let Some(format) = (unsafe { utf8_arg(format) }) else {
        let error = remanence::Error::io("null format");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let format = match unsafe { declared_format(format.as_ref(), device_type, block_bytes) } {
        Ok(format) => format,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    if sources.is_null() && source_count > 0 {
        let error = remanence::Error::io("null sources");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let raw = if source_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(sources, source_count) }
    };
    // Every member is checked before any is adopted: an invalid one is
    // the caller's to close, and adopting the rest would close handles
    // out of a collection the load never took.
    if raw.iter().any(|&source| source == 0 || source == -1) {
        let error = remanence::Error::io("a source is not an open file handle");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let files: Vec<std::fs::File> = raw
        .iter()
        .map(|&source| unsafe { file_from_raw(source) })
        .collect();
    let handle = unsafe { &mut *session };
    match handle.session.load_media(files, format) {
        Ok(medium) => {
            let id = medium.id();
            unsafe { medium_view(session, id) }
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Loads one file from another medium's namespace as the format the
/// caller declares — the namespace-source shape of the load, taking what
/// `remanence_file_source` produced.
///
/// **The source is consumed and freed whatever the outcome**, exactly as
/// a discovery is: the pointer must never be used or freed again after
/// this call, whatever it returns. The source is free-standing — it
/// rides the claim of the medium it came from, so the walk that named it
/// ended before this load begins and nothing is opened twice. `format`,
/// `device_type` and `block_bytes` are as `remanence_session_load_media`
/// takes them. The session owns the returned view; never free it. Null
/// on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_media_source(
    session: *mut RemanenceSession,
    source: *mut RemanenceFileSource,
    format: *const c_char,
    device_type: *const c_char,
    block_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if source.is_null() {
        let error = remanence::Error::io("null source");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    // Taken before anything can fail: the C contract is that this call
    // consumes the source whatever the outcome, so the two surfaces
    // cannot disagree about who holds its ride on the claim afterwards.
    let owned = unsafe { Box::from_raw(source) };
    if session.is_null() {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let Some(format) = (unsafe { utf8_arg(format) }) else {
        let error = remanence::Error::io("null format");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let format = match unsafe { declared_format(format.as_ref(), device_type, block_bytes) } {
        Ok(format) => format,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let RemanenceFileSource { source, .. } = *owned;
    let handle = unsafe { &mut *session };
    match handle.session.load_media(source, format) {
        Ok(medium) => {
            let id = medium.id();
            unsafe { medium_view(session, id) }
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Loads a collection of files gathered from another medium's namespace
/// as the format the caller declares — the collection beside
/// `remanence_session_load_media_source`, taking what
/// `remanence_space_files` gathered, for a format whose claim takes a
/// collection (`remanence_format_takes_collection`).
///
/// **The gathering is consumed and freed whatever the outcome**: the
/// pointer must never be used or freed again after this call, whatever
/// it returns. The session owns the returned view; never free it. Null
/// on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_media_sources(
    session: *mut RemanenceSession,
    sources: *mut RemanenceFileSourceList,
    format: *const c_char,
    device_type: *const c_char,
    block_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if sources.is_null() {
        let error = remanence::Error::io("null sources");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    // Taken before anything can fail, as the single source is taken:
    // the gathering is consumed whatever the outcome.
    let owned = unsafe { Box::from_raw(sources) };
    if session.is_null() {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let Some(format) = (unsafe { utf8_arg(format) }) else {
        let error = remanence::Error::io("null format");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let format = match unsafe { declared_format(format.as_ref(), device_type, block_bytes) } {
        Ok(format) => format,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let RemanenceFileSourceList { sources, .. } = *owned;
    let handle = unsafe { &mut *session };
    match handle.session.load_media(sources, format) {
        Ok(medium) => {
            let id = medium.id();
            unsafe { medium_view(session, id) }
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Loads the medium a discovery already opened into the session's pool,
/// **consuming and freeing the discovery**.
///
/// This is the load that runs nothing twice: the discovery holds the
/// claim taken when the artifact was identified and the work that
/// identification did, and the medium is built over that claim, so no
/// window exists between the question and the load in which the artifact
/// could change (P7). The intent and the assurance are the ones the
/// discovery established; the **cache bound is declared here**, because
/// this is where the medium comes into existence — discovery built
/// nothing, so it had nothing to bound (P27). This door takes the stated
/// default; `remanence_session_load_discovery_with_cache` takes the
/// caller's own.
///
/// **The discovery is freed either way** — a refused load releases its
/// claim with it — so the pointer must never be used or freed again
/// after this call, whatever it returns. Null on failure.
///
/// **This is the plain door, and it opens where the recognizing format
/// records exactly one device type.** Where it records several, nothing
/// in the artifact says which wrote it, and the refusal names them and
/// points at `remanence_session_load_discovery_as`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_discovery(
    session: *mut RemanenceSession,
    discovery: *mut RemanenceDiscovery,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe {
        remanence_session_load_discovery_with_cache(
            session,
            discovery,
            remanence::DEFAULT_CACHE_BYTES,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// `remanence_session_load_discovery` under a caller-declared session
/// cache bound (P27), which the medium this load creates keeps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_discovery_with_cache(
    session: *mut RemanenceSession,
    discovery: *mut RemanenceDiscovery,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if discovery.is_null() {
        let error = remanence::Error::io("null discovery");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    // Taken before anything can fail: the C contract is that this call
    // consumes the discovery whatever the outcome, exactly as the Rust
    // one does, so the two surfaces cannot disagree about who holds the
    // claim afterwards.
    let discovery = unsafe { Box::from_raw(discovery) };
    if session.is_null() {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let handle = unsafe { &mut *session };
    match handle
        .session
        .load_discovery_with_cache(discovery.discovery, cache_bytes)
    {
        Ok(medium) => {
            let id = medium.id();
            unsafe { medium_view(session, id) }
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Loads the medium a discovery already opened under the caller's own
/// declaration of the device that recorded it — the `_as` door, for a
/// format that records several device types and so asserts none.
///
/// `device_type` is a stable spelling from
/// `remanence_discovery_recorded_device`, and one the recognizing
/// format's adapter records; anything else is refused by name. The
/// discovery is consumed and freed exactly as the plain door consumes
/// it, whatever this returns. Null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_discovery_as(
    session: *mut RemanenceSession,
    discovery: *mut RemanenceDiscovery,
    device_type: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe {
        remanence_session_load_discovery_as_with_cache(
            session,
            discovery,
            device_type,
            remanence::DEFAULT_CACHE_BYTES,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// `remanence_session_load_discovery_as` under a caller-declared session
/// cache bound (P27).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_load_discovery_as_with_cache(
    session: *mut RemanenceSession,
    discovery: *mut RemanenceDiscovery,
    device_type: *const c_char,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceMedium {
    unsafe { clear_error(error_out, error_rule_out) };
    if discovery.is_null() {
        let error = remanence::Error::io("null discovery");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    // Taken before anything can fail, as the plain door takes it: the
    // discovery is consumed whatever the outcome.
    let discovery = unsafe { Box::from_raw(discovery) };
    if session.is_null() {
        let error = remanence::Error::io("null session");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    }
    let Some(device) = (unsafe { utf8_arg(device_type) }) else {
        let error = remanence::Error::io("null device type");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let device = match DeviceType::from_id(device.as_ref()) {
        Ok(device) => device,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let handle = unsafe { &mut *session };
    match handle
        .session
        .load_discovery_as_with_cache(discovery.discovery, device, cache_bytes)
    {
        Ok(medium) => {
            let id = medium.id();
            unsafe { medium_view(session, id) }
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// How many media this session holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_media_count(session: *const RemanenceSession) -> usize {
    match unsafe { session.as_ref() } {
        Some(handle) => handle.session.media().len(),
        None => 0,
    }
}

/// The identity of the medium at `index`, in the order they were loaded,
/// or 0 out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_media_id(
    session: *const RemanenceSession,
    index: usize,
) -> u64 {
    match unsafe { session.as_ref() } {
        Some(handle) => handle.session.media().get(index).map_or(0, |id| id.value()),
        None => 0,
    }
}

/// The medium `media_id` names, or **null where the pool holds none** —
/// absence is an answer, and the error outs are untouched. The session
/// owns the view; never free it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_medium(
    session: *mut RemanenceSession,
    media_id: u64,
) -> *mut RemanenceMedium {
    if session.is_null() {
        return ptr::null_mut();
    }
    let id = MediaId::from_value(media_id);
    let handle = unsafe { &mut *session };
    if handle.session.medium(id).is_none() {
        return ptr::null_mut();
    }
    unsafe { medium_view(session, id) }
}

/// **The one state-destroying verb.** It severs the medium's link if a
/// device holds it, ends the P7 claim — closing the handle the caller
/// handed over — and discards everything uncommitted.
///
/// Releasing is not a commit and never becomes one. Every borrowed view
/// of that medium, and every space and file resolved through it, stops
/// answering. Returns false when the pool holds no such medium.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_release_media(
    session: *mut RemanenceSession,
    media_id: u64,
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
    let id = MediaId::from_value(media_id);
    match handle.session.release_media(id) {
        Ok(()) => {
            handle.media.retain(|view| view.id != id);
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Opens an empty session — the claim and cache scope, holding nothing
/// at all. Devices and media are added over its life; neither set is
/// fixed at open. Free with `remanence_session_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_new() -> *mut RemanenceSession {
    Box::into_raw(Box::new(RemanenceSession {
        session: Session::new(),
        views: Vec::new(),
        media: Vec::new(),
    }))
}

/// Frees a session, dropping every device and releasing every P7 claim.
/// Every borrowed device view obtained from it becomes invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_free(session: *mut RemanenceSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Adds a device of `slot` (UTF-8, a stable spelling from
/// `remanence_device_slot_id` — a device type such as `mbr-block-hd`, or
/// `archive`) to the session, taking the lowest
/// free slot of that bay, and returns a **borrowed** view of it — empty,
/// until `remanence_device_insert` puts a medium in it.
///
/// The session owns the view; never free it. A
/// device this release does not claim is refused by name (P3). Returns
/// null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_add_device(
    session: *mut RemanenceSession,
    slot: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    unsafe {
        add_device(
            session,
            slot,
            None,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Adds a device of `slot` at index `index` of the session's anonymous
/// machine — `hdd1` being a hard drive at index 1. The caller chooses
/// the slot, never the name; a slot already taken is refused rather than
/// displaced, whatever device would fill it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_add_device_at(
    session: *mut RemanenceSession,
    slot: *const c_char,
    index: u32,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDevice {
    unsafe {
        add_device(
            session,
            slot,
            Some(index),
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Adds a device for the artifact at `path` (UTF-8) to the session — one
/// of the device type the artifact's format records — loads the medium
/// into it, and returns a **borrowed** view of it. A format recording
/// several device types is refused by name, toward the declared load.
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
            path,
            intent,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Releases the device at `attachment`, **ejecting first**: the link is
/// severed and the medium stays pooled with its claim and buffered
/// changes intact. Returns false when nothing is attached there.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_release_device(
    session: *mut RemanenceSession,
    attachment: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe {
        release_device(
            session,
            attachment,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// How many devices the session holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_device_count(session: *const RemanenceSession) -> usize {
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

/// A **borrowed** view of the device at `attachment`.
///
/// The session owns it; never free it. It stays valid until that device
/// is released or the session is freed. **Null where nothing is attached
/// there** — absence is an answer, and this takes no error outs to leave
/// untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_session_device(
    session: *mut RemanenceSession,
    attachment: *const c_char,
) -> *mut RemanenceDevice {
    unsafe { device_view(session, attachment) }
}

/// Adds a device to a session, and answers with the borrowed view of it.
unsafe fn add_device(
    session: *mut RemanenceSession,
    slot: *const c_char,
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
    let Some(slot) = (unsafe { utf8_arg(slot) }) else {
        let error = remanence::Error::io("null device type");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let slot = match DeviceSlot::from_id(slot.as_ref()) {
        Ok(slot) => slot,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let added = match index {
        Some(index) => handle.session.add_device_at(slot, index),
        None => handle.session.add_device(slot),
    };
    let attachment = match added {
        Ok(device) => device.attachment(),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let attachment = to_cstring(&attachment.to_string());
    unsafe { device_view(session, attachment.as_ptr()) }
}

/// Adds a device for one artifact in a session, and answers with the
/// borrowed view of it.
unsafe fn add_device_for(
    session: *mut RemanenceSession,
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
    let attachment = match handle
        .session
        .add_device_for(path.as_ref(), access_intent(intent))
    {
        Ok(device) => device.attachment(),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let attachment = to_cstring(&attachment.to_string());
    unsafe { device_view(session, attachment.as_ptr()) }
}

/// Releases a device from a session and invalidates every borrowed view
/// of it.
unsafe fn release_device(
    session: *mut RemanenceSession,
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
    match handle.session.release_device(attachment) {
        Ok(()) => {
            handle.views.retain(|view| view.attachment != attachment);
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// The borrowed device view for one attachment, reusing the one already
/// handed out where there is one.
unsafe fn device_view(
    session: *mut RemanenceSession,
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
        .position(|view| view.attachment == attachment)
    {
        return handle.views[at].as_mut() as *mut RemanenceDevice;
    }
    // The slot the device is typed by comes off the device itself: an
    // attachment identity names the bay, and several device types share
    // one, so the identity alone cannot say what is in it.
    let Some(slot) = handle
        .session
        .device(attachment)
        .map(|device| device.slot())
    else {
        return ptr::null_mut();
    };
    handle.views.push(Box::new(RemanenceDevice {
        session,
        attachment,
        attachment_c: to_cstring(&attachment.to_string()),
        slot_c: to_cstring(slot.id()),
        device_type_c: slot.device_type().map(|device| to_cstring(device.id())),
    }));
    handle.views.last_mut().expect("just pushed").as_mut() as *mut RemanenceDevice
}

/// The medium's **effective** access mode: the declared intent's
/// echo where the evidence supports it, and read-only where it does not
/// (P28). `remanence_assurance_access_mode` reports the same value beside
/// the reason for it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_mode(
    medium: *const RemanenceMedium,
) -> RemanenceAccessMode {
    unsafe { medium.as_ref() }.map_or(RemanenceAccessMode::ReadOnly, |handle| {
        access_mode(
            handle
                .medium()
                .map(|medium| medium.mode())
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
    claim: RemanenceClaim,
    declared_bytes: Option<u64>,
    observed_bytes: Option<u64>,
    first_unavailable_byte: Option<u64>,
}

/// The assurance of one open medium: what the open established, why, the
/// exact extents that read, and the access the evidence permits.
///
/// It is available before anything is read, so a caller meets a deficiency
/// by being told rather than by an operation failing halfway. Null only
/// once the medium itself has been released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_assurance(
    medium: *const RemanenceMedium,
) -> *mut RemanenceAssurance {
    let Some(medium) = (unsafe { medium.as_ref() }).and_then(RemanenceMedium::medium) else {
        return ptr::null_mut();
    };
    Box::into_raw(Box::new(assurance_view(medium.assurance())))
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
        claim: claim_class(assurance.claim),
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
    unsafe { assurance.as_ref() }.map_or(RemanenceAssuranceOutcome::Verified, |assurance| {
        assurance.outcome
    })
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
    let Some(range) =
        (unsafe { assurance.as_ref() }).and_then(|assurance| assurance.readable.get(index))
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
            assurance
                .as_ref()
                .and_then(|assurance| assurance.declared_bytes),
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
            assurance
                .as_ref()
                .and_then(|assurance| assurance.observed_bytes),
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
pub unsafe extern "C" fn remanence_medium_format(
    medium: *const RemanenceMedium,
    format_out: *mut RemanenceDiskFormat,
) -> bool {
    let Some(format) =
        (unsafe { medium.as_ref() }).and_then(|handle| handle.medium()?.format().ok())
    else {
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

/// The qcow2 version, or 0 for an image of any other format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_qcow2_version(medium: *const RemanenceMedium) -> u32 {
    match unsafe { medium.as_ref() }.and_then(|handle| handle.medium()?.format().ok()) {
        Some(DiskFormat::Qcow2 { version }) => version,
        _ => 0,
    }
}

/// The VDI version's major part, or 0 for an image of any other format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_vdi_version_major(medium: *const RemanenceMedium) -> u32 {
    match unsafe { medium.as_ref() }.and_then(|handle| handle.medium()?.format().ok()) {
        Some(DiskFormat::Vdi { major, .. }) => major,
        _ => 0,
    }
}

/// The VDI version's minor part, or 0 for an image of any other format.
/// Read it beside the major part: on its own, 0 is both "minor zero" and
/// "not a VDI".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_vdi_version_minor(medium: *const RemanenceMedium) -> u32 {
    match unsafe { medium.as_ref() }.and_then(|handle| handle.medium()?.format().ok()) {
        Some(DiskFormat::Vdi { minor, .. }) => minor,
        _ => 0,
    }
}

/// The virtual disk size in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_size(medium: *const RemanenceMedium) -> u64 {
    unsafe { medium.as_ref() }
        .and_then(|handle| handle.medium()?.size().ok())
        .unwrap_or(0)
}

/// Whether uncommitted changes exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_is_modified(medium: *const RemanenceMedium) -> bool {
    unsafe { medium.as_ref() }
        .and_then(|handle| handle.medium())
        .is_some_and(|medium| medium.is_modified())
}

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
struct GeometryReadingView {
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

fn reading_string(
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

fn reading_part(
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

/// Reads one whole sector in the recording's own coordinates into
/// `buffer_out`, which is exactly one sector of this recording.
///
/// Cylinders and heads number from zero and sectors from one. It answers
/// on a sector-addressed recording whose geometry the evidence
/// established and refuses by name otherwise, the rule identity in
/// `error_rule_out` naming which: `not-sector-addressed`,
/// `geometry-unstated`, `geometry-undetermined`, `outside-geometry` or
/// `partial-sector`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_read_sector(
    medium: *mut RemanenceMedium,
    cylinder: u32,
    head: u32,
    sector: u32,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    match medium.read_sector(cylinder, head, sector, buffer) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Writes one whole sector in the recording's own coordinates,
/// **buffered until `remanence_medium_commit`** like every other write
/// (P2), under the same rules `remanence_medium_read_sector` answers by.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_write_sector(
    medium: *mut RemanenceMedium,
    cylinder: u32,
    head: u32,
    sector: u32,
    data: *const u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_ref() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if data.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let data = unsafe { std::slice::from_raw_parts(data, length) };
    match medium.write_sector(cylinder, head, sector, data) {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

// ---------------------------------------------------------------------------
// The partition surface (P16, P17, P19): content is reached through the
// partition that composes it.
//
// A medium carries no file verbs at all. It carries its pool — the scheme
// it was populated under, and every partition in it — and the vantage
// doors live on the partition: `remanence_partition_volume` for the
// addressable vantage, `remanence_partition_filesystem` for the namespace
// one, and `remanence_partition_filesystem_as` where nothing determines a
// namespace and the caller declares the reading. **Both doors compose the
// same node**, so which one was opened changes nothing about what comes
// back — only which question was asked of it.
//
// The pool is established when the medium is loaded and is evidence from
// then on, so the doors are lookups rather than probes: a vantage the
// partition does not have is null with the error outs untouched, and only
// a composition that was attempted and refused writes them.
//
// The handles below name their provider — session and medium, or the
// record layer a recording's sectors are held behind — rather than
// holding a borrow, and re-resolve on every call: a medium that has been
// released answers by name instead of reaching state that has left.

use remanence::{Partition, PartitionScheme, PartitionType, PartitionView};

/// One space of a medium's content, composed over the partition that
/// bears it. Free with `remanence_space_free`.
pub struct RemanenceSpace {
    session: *mut RemanenceSession,
    /// The medium whose pool holds the partition this was composed over.
    /// `None` where no medium composed it at all.
    media: Option<MediaId>,
    /// The sector layer this namespace is presented over, where no
    /// medium composed it — the flux family is reached through its own
    /// types rather than through a device. Null for a medium-backed
    /// space, and borrowed from the handle that owns it either way.
    sectors: *const RemanenceC1541Sectors,
    /// The FM or MFM record layer it is composed over, where that is
    /// what composed it. Null otherwise, and never set alongside
    /// `sectors`: a recording belongs to one family.
    ibm_sectors: *const RemanenceIbmSectors,
    /// The scheme's own ordinal of the partition that composed it, which
    /// is what re-resolution looks the partition up by. `None` where no
    /// pool named it.
    ordinal: Option<u32>,
    /// The namespace reading a caller declared to mint it, where one did.
    /// It is carried so re-resolution declares the same thing again
    /// rather than falling back on what the pool records.
    declared: Option<CString>,
    /// Whether the composed space carries the addressable vantage.
    addressable: bool,
    /// The identity the inspection report issued for the volume composed
    /// over the partition, absent where it issued none.
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
    media: Option<MediaId>,
    /// As on `RemanenceSpace`: the sector layer a flux-family namespace
    /// is presented over, or null.
    sectors: *const RemanenceC1541Sectors,
    /// The FM or MFM record layer it is composed over, where that is
    /// what composed it. Null otherwise, and never set alongside
    /// `sectors`: a recording belongs to one family.
    ibm_sectors: *const RemanenceIbmSectors,
    ordinal: Option<u32>,
    declared: Option<CString>,
    path: CString,
    name: CString,
    kind: RemanenceEntryKind,
    size_bytes: u64,
}

/// What a space or a file re-composes itself from: the provider it was
/// minted through, the partition within it, and the reading declared over
/// that partition where one was.
///
/// It is the whole of what either handle knows about where it came from,
/// named once so both carry the same thing rather than two spellings of
/// it.
#[derive(Clone, Copy)]
struct SpaceOrigin<'a> {
    session: *mut RemanenceSession,
    media: Option<MediaId>,
    sectors: *const RemanenceC1541Sectors,
    /// The FM or MFM record layer, where that is what composed it. The
    /// two are never both set: a recording belongs to one family.
    ibm_sectors: *const RemanenceIbmSectors,
    ordinal: Option<u32>,
    declared: Option<&'a str>,
}

impl RemanenceSpace {
    /// Where this space came from, for the re-composition every verb on
    /// it passes through.
    fn origin(&self) -> SpaceOrigin<'_> {
        SpaceOrigin {
            session: self.session,
            media: self.media,
            sectors: self.sectors,
            ibm_sectors: self.ibm_sectors,
            ordinal: self.ordinal,
            declared: self.declared.as_deref().and_then(|id| id.to_str().ok()),
        }
    }
}

impl RemanenceFile {
    /// The same origin the space this file was named through carries: a
    /// file is a name within a namespace, never storage of its own.
    fn origin(&self) -> SpaceOrigin<'_> {
        SpaceOrigin {
            session: self.session,
            media: self.media,
            sectors: self.sectors,
            ibm_sectors: self.ibm_sectors,
            ordinal: self.ordinal,
            declared: self.declared.as_deref().and_then(|id| id.to_str().ok()),
        }
    }
}

/// Re-composes the space this origin names and runs `action` over it.
///
/// Every verb below passes through here, so the refusals a caller meets
/// are the library's own — the pool's where the partition it named has
/// left, the partition's where a declared reading does not hold, the
/// namespace's where it does.
///
/// **Both doors compose the same node**, so the space is reconstructed
/// the same way whichever door minted it: a declared reading is declared
/// again, and where the caller declared nothing the vantages the pool
/// records answer — the namespace where the partition bears one, the
/// addressable extent where it does not.
unsafe fn with_space<T>(
    origin: SpaceOrigin<'_>,
    action: impl FnOnce(&mut remanence::StorageSpace<'_>) -> remanence::Result<T>,
) -> remanence::Result<T> {
    // A namespace presented over a sector layer re-composes from that
    // layer, exactly as a medium-backed one re-composes from its
    // medium: the node is a view over what is beneath it and never an
    // instance, whichever seam that is (P13). A recording determines no
    // reading of its own, so the declaration stands again here — CBM DOS
    // where the mint made no other.
    if let Some(held) = unsafe { origin.sectors.as_ref() } {
        let mut space = held
            .sectors
            .partition()
            .filesystem_as(origin.declared.unwrap_or("cbmdos"))?;
        return action(&mut space);
    }
    // An FM or MFM recording composes an addressed extent instead
    // (D62), so its declaration reaches the ordinary adapters. There is
    // no default here: nothing about such a recording determines which
    // of FAT, HDOS or CP/M it holds, and picking one would be the guess
    // the declaration exists to prevent.
    if let Some(held) = unsafe { origin.ibm_sectors.as_ref() } {
        let declared = origin.declared.ok_or_else(|| {
            remanence::Error::io(
                "this partition is composed over an FM or MFM recording, which                  determines no reading of its own; declare one with                  remanence_partition_filesystem_as",
            )
        })?;
        let mut partition = held.sectors.partition()?;
        let mut space = partition.view().filesystem_as(declared)?;
        return action(&mut space);
    }
    let handle =
        unsafe { origin.session.as_mut() }.ok_or_else(|| remanence::Error::io("null session"))?;
    let media = origin
        .media
        .ok_or_else(|| remanence::Error::io("this space names no medium to be composed over"))?;
    let ordinal = origin
        .ordinal
        .ok_or_else(|| remanence::Error::io("this space names no partition to be composed over"))?;
    let medium = handle
        .session
        .medium_mut(media)
        .ok_or_else(|| remanence::Error::io("the medium this space reads was released"))?;
    let partition = medium.partition(ordinal).ok_or_else(|| {
        remanence::Error::io(format!(
            "the medium's partition pool holds no partition {ordinal}"
        ))
    })?;
    let bears_namespace = partition.partition().bears_namespace();
    let addressable = partition.partition().is_addressable();
    let mut space = match origin.declared {
        Some(id) => partition.filesystem_as(id)?,
        None if bears_namespace => partition
            .filesystem()
            .expect("the namespace vantage the pool records"),
        None if addressable => partition
            .volume()
            .expect("the addressable vantage the pool records"),
        None => {
            return Err(remanence::Error::io(format!(
                "partition {ordinal} composes neither the vantage this space \
                 was minted through nor any other"
            )));
        }
    };
    action(&mut space)
}

// ------------------------------------------------ the claimed vocabulary

/// A claimed enumerand's two spellings: the stable one that crosses the
/// boundary and the one fit to show a user.
struct SpellingView {
    id: CString,
    name: CString,
}

fn scheme_spellings() -> &'static [SpellingView] {
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

fn partition_type_spellings() -> &'static [SpellingView] {
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

// ----------------------------------------------------- the medium's pool

/// The stable spelling of the scheme this medium's content is laid out
/// under, or **null where it records none** — the direct partition stands
/// there, and that is an answer rather than a refusal. Owned by the
/// library; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_partition_scheme(
    medium: *const RemanenceMedium,
) -> *const c_char {
    let Some(scheme) = (unsafe { medium.as_ref() })
        .and_then(|handle| handle.medium())
        .and_then(|medium| medium.partition_scheme())
    else {
        return ptr::null();
    };
    PartitionScheme::ALL
        .iter()
        .position(|claimed| *claimed == scheme)
        .and_then(|at| scheme_spellings().get(at))
        .map_or(ptr::null(), |spelling| spelling.id.as_ptr())
}

/// How many partitions this medium's pool holds. A medium recording no
/// scheme answers 1 — the direct partition, which is what its content is
/// reached through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_partition_count(medium: *const RemanenceMedium) -> usize {
    unsafe { medium.as_ref() }
        .and_then(|handle| handle.medium())
        .map_or(0, |medium| medium.partitions().len())
}

/// The scheme's own ordinal of the partition at `index` in the pool's own
/// order. False past the end.
///
/// The two numbers differ on purpose: a partition carrying an issue keeps
/// its ordinal, so the partitions behind it never renumber (U4), and the
/// ordinal — never the index — is what `remanence_medium_partition`
/// takes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_partition_ordinal(
    medium: *const RemanenceMedium,
    index: usize,
    value_out: *mut u32,
) -> bool {
    let ordinal = unsafe { medium.as_ref() }
        .and_then(|handle| handle.medium())
        .and_then(|medium| {
            let partitions = medium.partitions();
            partitions.get(index).map(Partition::ordinal)
        });
    unsafe { write_opt_u32(ordinal, value_out) }
}

/// The partition the scheme's own ordinal names — MBR entry 1 is `1`, and
/// the direct partition is `0` — or **null where the pool holds none**.
///
/// It takes no error outs because absence is an answer here, exactly as
/// it is for `remanence_session_medium`: the pool was established when
/// the medium was loaded, so a number it does not hold is a fact about
/// the medium rather than a failure to look. Free with
/// `remanence_partition_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_partition(
    medium: *mut RemanenceMedium,
    ordinal: u32,
) -> *mut RemanencePartition {
    let Some(handle) = (unsafe { medium.as_mut() }) else {
        return ptr::null_mut();
    };
    let (session, media) = (handle.session, Some(handle.id));
    let Some(target) = handle.medium() else {
        return ptr::null_mut();
    };
    let Some(view) = target.partition(ordinal) else {
        return ptr::null_mut();
    };
    partition_handle(session, media, ptr::null(), ptr::null(), view.partition())
}

// ------------------------------------------------- the partition handle

/// One partition of a medium's evidence pool: what the scheme declared
/// about it, what the library composed over it, and the doors onto that
/// composition. Free with `remanence_partition_free`.
///
/// **It is a snapshot, not a borrow.** The record is a value on the Rust
/// side and it is one here too: the facts below are copied when the pool
/// answers, and every string reached through them is borrowed from this
/// handle and dies with it. The doors name their provider — session and
/// medium, or the record layer — the way `RemanenceDevice` does, and
/// re-resolve through it rather than holding state that may leave.
///
/// The Rust view is spent by opening a door; this handle is not, so a
/// caller may open both off one partition. That changes nothing about
/// what comes back: both doors compose the same node.
pub struct RemanencePartition {
    /// The session the medium is pooled in, null for a partition over a
    /// recording's own record layer.
    session: *mut RemanenceSession,
    /// The medium whose pool declared it, `None` for the same case.
    media: Option<MediaId>,
    /// The record layer it is composed over, where no medium composed it.
    /// Null for a pooled partition, and borrowed from the handle that
    /// owns it either way.
    sectors: *const RemanenceC1541Sectors,
    /// The FM or MFM record layer it is composed over, where that is
    /// what composed it. Null otherwise, and never set alongside
    /// `sectors`: a recording belongs to one family.
    ibm_sectors: *const RemanenceIbmSectors,
    ordinal: u32,
    direct: bool,
    active: bool,
    type_byte: Option<u8>,
    type_reading: Option<CString>,
    claimed: bool,
    placement: CString,
    role: RemanenceRegionRole,
    start_bytes: Option<u64>,
    length_bytes: Option<u64>,
    addressable: bool,
    bears_namespace: bool,
    volume: Option<u64>,
    issue: Option<IssueView>,
    evidence: Vec<CString>,
    provenance: Option<CString>,
}

/// Copies one partition record's facts across the boundary, beside the
/// provider its doors re-resolve through.
fn partition_handle(
    session: *mut RemanenceSession,
    media: Option<MediaId>,
    sectors: *const RemanenceC1541Sectors,
    ibm_sectors: *const RemanenceIbmSectors,
    partition: &Partition,
) -> *mut RemanencePartition {
    Box::into_raw(Box::new(RemanencePartition {
        session,
        media,
        sectors,
        ibm_sectors,
        ordinal: partition.ordinal(),
        direct: partition.is_direct(),
        active: partition.active(),
        type_byte: partition.type_byte(),
        type_reading: partition.type_reading().map(to_cstring),
        claimed: partition.is_claimed(),
        placement: to_cstring(partition.placement()),
        role: match partition.role() {
            RegionRole::Data => RemanenceRegionRole::Data,
            RegionRole::Structure => RemanenceRegionRole::Structure,
        },
        start_bytes: partition.start_bytes(),
        length_bytes: partition.length_bytes(),
        addressable: partition.is_addressable(),
        bears_namespace: partition.bears_namespace(),
        volume: partition.volume_id().map(VolumeId::value),
        issue: partition.issue().map(issue_view),
        evidence: evidence_views(partition.evidence()),
        provenance: partition.provenance().map(to_cstring),
    }))
}

/// Re-resolves the partition this handle names and runs `action` over the
/// view.
///
/// The pool is immutable for the session's life, so this answers the same
/// record every time — but it answers it *through* the provider, so a
/// medium that has been released refuses by name here rather than being
/// acted on from a copy of facts that have gone.
unsafe fn with_partition<T>(
    handle: &RemanencePartition,
    action: impl FnOnce(PartitionView<'_>) -> remanence::Result<T>,
) -> remanence::Result<T> {
    // A partition over a recording's own record layer re-resolves from
    // that layer, exactly as a pooled one re-resolves from its medium
    // (P13).
    if let Some(held) = unsafe { handle.sectors.as_ref() } {
        return action(held.sectors.partition());
    }
    if let Some(held) = unsafe { handle.ibm_sectors.as_ref() } {
        let mut partition = held.sectors.partition()?;
        return action(partition.view());
    }
    let session =
        unsafe { handle.session.as_mut() }.ok_or_else(|| remanence::Error::io("null session"))?;
    let media = handle.media.ok_or_else(|| {
        remanence::Error::io("this partition names no medium to be reached through")
    })?;
    let medium = session
        .session
        .medium_mut(media)
        .ok_or_else(|| remanence::Error::io("the medium this partition belongs to was released"))?;
    let view = medium.partition(handle.ordinal).ok_or_else(|| {
        remanence::Error::io(format!(
            "the medium's partition pool holds no partition {}",
            handle.ordinal
        ))
    })?;
    action(view)
}

/// Frees a partition handle. The medium and its pool are untouched, and
/// so is every space already composed through it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_free(partition: *mut RemanencePartition) {
    if !partition.is_null() {
        drop(unsafe { Box::from_raw(partition) });
    }
}

/// The scheme's own ordinal for this partition, or 0 for a null handle —
/// which is also the direct partition's own number, so
/// `remanence_partition_is_direct` is the question to ask about it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_ordinal(partition: *const RemanencePartition) -> u32 {
    unsafe { partition.as_ref() }.map_or(0, |partition| partition.ordinal)
}

/// Whether this is the direct partition — the library's own composition
/// of the whole content, which stands where the medium records no scheme.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_is_direct(
    partition: *const RemanencePartition,
) -> bool {
    unsafe { partition.as_ref() }.is_some_and(|partition| partition.direct)
}

/// Whether the scheme flags this partition active, as it records it. The
/// direct partition is flagged by nothing and answers false.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_active(partition: *const RemanencePartition) -> bool {
    unsafe { partition.as_ref() }.is_some_and(|partition| partition.active)
}

/// The type value exactly as the scheme records it. False for the direct
/// partition, which records none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_type_byte(
    partition: *const RemanencePartition,
    out: *mut u8,
) -> bool {
    let Some(type_byte) = (unsafe { partition.as_ref() }).and_then(|partition| partition.type_byte)
    else {
        return false;
    };
    if let Some(out) = unsafe { out.as_mut() } {
        *out = type_byte;
    }
    true
}

/// What that value *declares*, in a sentence fit to quote in a refusal a
/// user reads, or null for the direct partition.
///
/// It is present whether or not this release reads the type, because the
/// partition a caller most needs explained is the one the library
/// declines to read — and it describes the declaration, never the
/// content: an unread `0x07` partition is not thereby asserted to hold
/// NTFS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_type_reading(
    partition: *const RemanencePartition,
) -> *const c_char {
    unsafe { partition.as_ref() }
        .and_then(|partition| partition.type_reading.as_ref())
        .map_or(ptr::null(), |reading| reading.as_ptr())
}

/// Whether this release reads the declared type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_is_claimed(
    partition: *const RemanencePartition,
) -> bool {
    unsafe { partition.as_ref() }.is_some_and(|partition| partition.claimed)
}

/// How the scheme places this partition, in the scheme's own vocabulary:
/// for MBR, `"primary"` for one of the four slots and `"logical"` for an
/// entry on the extended chain. The direct partition answers `"direct"`.
///
/// This is a different axis from `remanence_partition_role` and neither
/// implies the other.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_placement(
    partition: *const RemanencePartition,
) -> *const c_char {
    unsafe { partition.as_ref() }.map_or(ptr::null(), |partition| partition.placement.as_ptr())
}

/// Whether the scheme declares this partition as data or as structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_role(
    partition: *const RemanencePartition,
) -> RemanenceRegionRole {
    unsafe { partition.as_ref() }.map_or(RemanenceRegionRole::Data, |partition| partition.role)
}

/// Where this partition starts in the presented content. False where it
/// has no addressed extent at all — the direct partition over a medium
/// whose native vantage is a namespace.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_start_bytes(
    partition: *const RemanencePartition,
    out: *mut u64,
) -> bool {
    unsafe {
        write_opt_u64(
            partition
                .as_ref()
                .and_then(|partition| partition.start_bytes),
            out,
        )
    }
}

/// How far this partition runs, under the same rule.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_length_bytes(
    partition: *const RemanencePartition,
    out: *mut u64,
) -> bool {
    unsafe {
        write_opt_u64(
            partition
                .as_ref()
                .and_then(|partition| partition.length_bytes),
            out,
        )
    }
}

/// Whether the addressable vantage opens — whether
/// `remanence_partition_volume` answers with a space rather than null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_is_addressable(
    partition: *const RemanencePartition,
) -> bool {
    unsafe { partition.as_ref() }.is_some_and(|partition| partition.addressable)
}

/// Whether the namespace vantage opens — whether
/// `remanence_partition_filesystem` answers with a space rather than
/// null. Where it does not, the namespace is declared with
/// `remanence_partition_filesystem_as`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_bears_namespace(
    partition: *const RemanencePartition,
) -> bool {
    unsafe { partition.as_ref() }.is_some_and(|partition| partition.bears_namespace)
}

/// The identity the inspection report issued for the volume composed over
/// this partition. False where it composed none, which an addressable
/// partition may still be.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_volume_id(
    partition: *const RemanencePartition,
    out: *mut u64,
) -> bool {
    unsafe {
        write_opt_u64(
            partition.as_ref().and_then(|partition| partition.volume),
            out,
        )
    }
}

/// The category of the refusal that keeps this partition in the pool when
/// its type is outside the claim or its chain could not be followed.
/// False where the partition reads cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_issue_category(
    partition: *const RemanencePartition,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    let Some(issue) =
        (unsafe { partition.as_ref() }).and_then(|partition| partition.issue.as_ref())
    else {
        return false;
    };
    if let Some(out) = unsafe { category_out.as_mut() } {
        *out = issue.category;
    }
    true
}

/// That refusal, or null where the partition reads cleanly. A partition
/// carrying one is still a partition and still keeps its place.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_issue(
    partition: *const RemanencePartition,
) -> *const c_char {
    unsafe { partition.as_ref() }.map_or(ptr::null(), |partition| {
        partition
            .issue
            .as_ref()
            .map_or(ptr::null(), |issue| issue.message.as_ptr())
    })
}

/// How many observations the scheme's adapter read to declare this
/// partition (P4). The direct partition read nothing and answers 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_evidence_count(
    partition: *const RemanencePartition,
) -> usize {
    unsafe { partition.as_ref() }.map_or(0, |partition| partition.evidence.len())
}

/// One of them, or null past the end. Borrowed from the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_evidence(
    partition: *const RemanencePartition,
    index: usize,
) -> *const c_char {
    unsafe { partition.as_ref() }.map_or(ptr::null(), |partition| {
        partition
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// The direct partition's account of itself: what the library composed
/// and why. Present for the direct partition and null for every partition
/// a scheme declared, which is the whole of the distinction — a
/// composition act is provenance, never evidence.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_provenance(
    partition: *const RemanencePartition,
) -> *const c_char {
    unsafe { partition.as_ref() }
        .and_then(|partition| partition.provenance.as_ref())
        .map_or(ptr::null(), |provenance| provenance.as_ptr())
}

/// The caller's own reading of the type, checked against the value the
/// scheme recorded (P3).
///
/// The declaration is the caller's and the check is the library's: a
/// reading the recorded byte does not bear is refused naming both sides,
/// and the direct partition — which records no type — refuses by name
/// rather than accepting a reading of nothing. `type_id` is one of the
/// spellings `remanence_partition_type_id` enumerates; any other is
/// refused naming what this release declares.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_check_type(
    partition: *const RemanencePartition,
    type_id: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { partition.as_ref() }) else {
        return false;
    };
    let Some(type_id) = (unsafe { utf8_arg(type_id) }) else {
        let error = remanence::Error::io("null partition type");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let declared = match PartitionType::from_id(type_id.as_ref()) {
        Ok(declared) => declared,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return false;
        }
    };
    match unsafe { with_partition(handle, |view| view.check_type(declared)) } {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

// -------------------------------------------------- the vantage doors

/// Which vantage a caller asked a partition for.
#[derive(Clone, Copy)]
enum Door<'a> {
    /// The addressable vantage the pool records.
    Addressable,
    /// The namespace vantage the pool records.
    Namespace,
    /// The namespace vantage the caller declares, where nothing
    /// determines one (P18).
    Declared(&'a str),
}

/// What a composed space carries across the boundary.
struct SpaceFacts {
    volume: Option<u64>,
    addressable: bool,
    start_bytes: u64,
    length_bytes: u64,
    kind: Option<CString>,
}

fn space_facts(space: &remanence::StorageSpace<'_>) -> SpaceFacts {
    SpaceFacts {
        volume: space.volume_id().map(VolumeId::value),
        addressable: space.is_addressable(),
        start_bytes: space.start_bytes().unwrap_or(0),
        length_bytes: space.length_bytes().unwrap_or(0),
        // A space bearing no namespace is an ordinary space, so the
        // absence travels on the handle rather than failing here.
        kind: space.kind().ok().map(to_cstring),
    }
}

/// Opens one door onto the node this partition composes. `Ok(None)` is
/// the vantage being absent rather than anything having failed.
fn open_door(view: PartitionView<'_>, door: Door<'_>) -> remanence::Result<Option<SpaceFacts>> {
    Ok(match door {
        Door::Addressable => view.volume().map(|space| space_facts(&space)),
        Door::Namespace => view.filesystem().map(|space| space_facts(&space)),
        Door::Declared(id) => Some(space_facts(&view.filesystem_as(id)?)),
    })
}

unsafe fn partition_space(
    partition: *const RemanencePartition,
    door: Door<'_>,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceSpace {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { partition.as_ref() }) else {
        return ptr::null_mut();
    };
    match unsafe { with_partition(handle, |view| open_door(view, door)) } {
        Ok(Some(facts)) => Box::into_raw(Box::new(RemanenceSpace {
            session: handle.session,
            media: handle.media,
            sectors: handle.sectors,
            ibm_sectors: handle.ibm_sectors,
            ordinal: Some(handle.ordinal),
            declared: match door {
                Door::Declared(id) => Some(to_cstring(id)),
                Door::Addressable | Door::Namespace => None,
            },
            addressable: facts.addressable,
            volume: facts.volume,
            start_bytes: facts.start_bytes,
            length_bytes: facts.length_bytes,
            kind: facts.kind,
        })),
        // The vantage is absent, which the pool settled when the medium
        // was loaded: null, and nothing written to the outs.
        Ok(None) => ptr::null_mut(),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The addressable vantage: the space this partition composes, read and
/// written **by position within the partition's own extent** — the
/// vantage that reaches a boot record, allocation metadata, or the
/// extents a filesystem calls free.
///
/// **Null means two different things and the caller can tell them
/// apart.** Null with the error outs untouched is the vantage being
/// absent — a structural region, a type this release will not read, or a
/// partition over content whose native vantage is a namespace — which
/// `remanence_partition_is_addressable` states in advance. Null with the
/// outs set is a composition that was attempted and refused. Free with
/// `remanence_space_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_volume(
    partition: *const RemanencePartition,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceSpace {
    unsafe {
        partition_space(
            partition,
            Door::Addressable,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// The namespace vantage: the same node, reached by the names it holds,
/// and where every `remanence_filesystem_*` verb lives.
///
/// Null with the error outs untouched is nothing determining a namespace
/// over this partition — the honest absence, which
/// `remanence_partition_bears_namespace` states in advance and
/// `remanence_partition_filesystem_as` is where a caller who knows says
/// so. Null with the outs set is a refusal. Free with
/// `remanence_space_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_filesystem(
    partition: *const RemanencePartition,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceSpace {
    unsafe {
        partition_space(
            partition,
            Door::Namespace,
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// The declared reading, where no partition type determines one: `"fat"`,
/// `"hdos"`, `"cpm"`, a `"cpm-*"` layout, or `"cbmdos"`.
///
/// **The reading is the caller's and the check is the library's.** The
/// adapter the declaration names is the one that reads it, and it reads
/// the evidence to verify the declaration rather than to pick one — a
/// declaration the content cannot bear is refused by that adapter, by
/// name, and a spelling this release does not read is refused naming what
/// it does (P3). Recognizing a format and reading it are separate claims,
/// so `"cpm"` still refuses at the open.
///
/// This door always attempted a composition, so null here always carries
/// the refusal. Free with `remanence_space_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_filesystem_as(
    partition: *const RemanencePartition,
    id: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceSpace {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(id) = (unsafe { utf8_arg(id) }) else {
        let error = remanence::Error::io("null namespace");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    unsafe {
        partition_space(
            partition,
            Door::Declared(id.as_ref()),
            error_category_out,
            error_out,
            error_rule_out,
        )
    }
}

/// Frees a space handle. The partition and its medium are untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_space_free(space: *mut RemanenceSpace) {
    if !space.is_null() {
        drop(unsafe { Box::from_raw(space) });
    }
}

/// Whether this space has the addressable vantage — an extent to read and
/// write by position. False where the partition composes none: an
/// archive's direct partition, a recording's, or a region a scheme
/// declares as structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_volume_is_addressable(space: *const RemanenceSpace) -> bool {
    unsafe { space.as_ref() }.is_some_and(|space| space.addressable)
}

/// This space's opaque volume identity, as the inspection report issued
/// it, or 0 where the report composed no volume for the partition — which
/// an addressable space may still be, a blank disk's direct partition
/// being the delivered case. `remanence_volume_is_addressable` is the
/// vantage question; this is the identity.
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
    match unsafe { with_space(handle.origin(), |space| space.read_at(offset, buf)) } {
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
    match unsafe { with_space(handle.origin(), |space| space.write_at(offset, bytes)) } {
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

/// The label the recognizing filesystem read, or null where this space
/// bears none — a namespace whose format has no such field, or one that
/// bears no namespace at all. Free with `remanence_string_free`.
///
/// Sets the error on a failure to read the namespace, which a caller
/// tells from an honest absence by whether `error_out` was written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_label(
    filesystem: *const RemanenceSpace,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return ptr::null_mut();
    };
    match unsafe { with_space(handle.origin(), |target| target.label()) } {
        Ok(label) => label
            .and_then(|label| label.name)
            .map_or(ptr::null_mut(), |name| to_owned_c_char(&name)),
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// How many readings the label answer holds, and each one's source and
/// stored value: the sources the recognizing filesystem consulted, in
/// the order its own policy consults them (P4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_label_reading_count(
    filesystem: *const RemanenceSpace,
) -> usize {
    unsafe { filesystem.as_ref() }.map_or(0, |handle| {
        unsafe { with_space(handle.origin(), |target| target.label()) }
            .ok()
            .flatten()
            .map_or(0, |label| label.readings.len())
    })
}

/// Writes reading `index`'s source and stored value, each freed with
/// `remanence_string_free`. Returns false past the end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_label_reading(
    filesystem: *const RemanenceSpace,
    index: usize,
    source_out: *mut *mut c_char,
    stored_out: *mut *mut c_char,
) -> bool {
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return false;
    };
    let Some(label) = (unsafe { with_space(handle.origin(), |target| target.label()) })
        .ok()
        .flatten()
    else {
        return false;
    };
    let Some(reading) = label.readings.get(index) else {
        return false;
    };
    if !source_out.is_null() {
        unsafe { *source_out = to_owned_c_char(&reading.source) };
    }
    if !stored_out.is_null() {
        unsafe {
            *stored_out = reading
                .stored
                .as_ref()
                .map_or(ptr::null_mut(), |stored| to_owned_c_char(stored));
        }
    }
    true
}

/// How many observations recognized this namespace (P4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_evidence_count(
    filesystem: *const RemanenceSpace,
) -> usize {
    unsafe { filesystem.as_ref() }.map_or(0, |handle| {
        unsafe { with_space(handle.origin(), |target| target.evidence()) }
            .map_or(0, |evidence| evidence.len())
    })
}

/// One of them, freed with `remanence_string_free`, or null past the end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_filesystem_evidence(
    filesystem: *const RemanenceSpace,
    index: usize,
) -> *mut c_char {
    let Some(handle) = (unsafe { filesystem.as_ref() }) else {
        return ptr::null_mut();
    };
    unsafe { with_space(handle.origin(), |target| target.evidence()) }
        .ok()
        .and_then(|evidence| evidence.get(index).map(|line| to_owned_c_char(line)))
        .unwrap_or(ptr::null_mut())
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

use remanence::FileSource;

/// One file taken from an archive medium's namespace as a load's source
/// — free-standing, riding the claim of the medium it came from. Free
/// with `remanence_file_source_free`, unless
/// `remanence_session_load_media_source` consumed it.
pub struct RemanenceFileSource {
    source: FileSource,
    name: CString,
}

/// Every file gathered under one namespace path as a load's sources.
/// Free with `remanence_file_source_list_free`, unless
/// `remanence_session_load_media_sources` consumed it.
pub struct RemanenceFileSourceList {
    sources: Vec<FileSource>,
    names: Vec<CString>,
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

/// Every file under `path` (`""` or null is the whole namespace),
/// gathered as a load's sources in one pass — what
/// `remanence_session_load_media_sources` consumes.
///
/// The sources are **free-standing** as the single form's is, and a
/// solid archive's coded stream decodes once for the whole gathering
/// (P27). This release gathers from an archive's namespace alone — a
/// volume-backed filesystem's files are read through the filesystem
/// that names them, and refuse here by name. Returns null on failure
/// and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_space_files(
    space: *mut RemanenceSpace,
    path: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceFileSourceList {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { space.as_ref() }) else {
        let error = remanence::Error::io("null space");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let path = unsafe { utf8_arg(path) }.unwrap_or_default();
    match unsafe { with_space(handle.origin(), |target| target.files(path.as_ref())) } {
        Ok(sources) => {
            let names = sources
                .iter()
                .map(|source| to_cstring(source.name()))
                .collect();
            Box::into_raw(Box::new(RemanenceFileSourceList { sources, names }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
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

/// The commit point (P2): everything buffered reaches the image, then a
/// flush. Until this call, nothing has touched the file. The commit is
/// durable (P9): a private recovery journal is armed before the first
/// byte of the file changes, so an interruption at any point leaves
/// state the next open reconciles to wholly the old image or wholly
/// the committed new one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_commit(
    medium: *mut RemanenceMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_mut() }) else {
        return false;
    };
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
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
pub unsafe extern "C" fn remanence_medium_rollback(medium: *mut RemanenceMedium) {
    if let Some(handle) = unsafe { medium.as_mut() } {
        if let Some(medium) = handle.medium() {
            let _ = medium.rollback();
        }
    }
}

// ---------------------------------------------------------------------------
// The P64 rendition: what a p64 container carries, or will carry, of
// one remanence image, under a claim stated before the file exists.
// Reading a P64 is a session load like any other medium's — the
// declared format "p64" — so the report below is the rendition
// direction's account and no root of its own.

use remanence::P64Report;

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
unsafe fn p64_reported<'a>(
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

// ---------------------------------------------------------------------------
// The presentation rungs: the hardware bitstream a declared read channel
// clocks out of a flux medium, and the encoded bytestream the family's
// own declared code resolves out of that. Neither names a family — the
// medium beneath them does, and the rules come from its profile. Neither
// layer assigns synchronization, headers, sectors or files to what it
// holds, and there is no way back down.

use remanence::{Bitstream, Bytestream, Location};

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

/// What a bitstream handle stands on: a stream of its own, or the one
/// cached in a pooled medium.
enum BitstreamBacking {
    /// The materialize door's: the handle owns the stream, and freeing
    /// it discards the stream's private session storage.
    Owned(Bitstream),
    /// The medium door's: the stream lives in the session's pool with
    /// its medium, named by session and pool identity and re-resolved
    /// on every call, so a released medium answers by name rather than
    /// through state that has left.
    Pooled {
        session: *mut RemanenceSession,
        media: MediaId,
    },
}

/// What a bytestream handle stands on, the same fork.
enum BytestreamBacking {
    Owned(Bytestream),
    Pooled {
        session: *mut RemanenceSession,
        media: MediaId,
    },
}

/// A hardware bitstream, held in the session. The bits stay behind this
/// handle; what it reports is the transition that produced them.
///
/// Two doors mint it: `remanence_flux_image_materialize_bitstream`,
/// whose handle owns the stream it materialized, and
/// `remanence_medium_bitstream`, whose handle is a view of the stream
/// cached in the pooled medium — that one **must not outlive its
/// session**, and stops answering once the medium is released.
pub struct RemanenceBitstream {
    backing: BitstreamBacking,
    view: LayerView,
}

impl RemanenceBitstream {
    /// The stream this handle names: its own for the materialize
    /// backing, the pooled medium's for the medium one — `None` once
    /// that medium is released.
    fn stream(&self) -> Option<&Bitstream> {
        match &self.backing {
            BitstreamBacking::Owned(bitstream) => Some(bitstream),
            BitstreamBacking::Pooled { session, media } => {
                let session = unsafe { &mut (**session).session };
                session.medium_mut(*media)?.bitstream().ok()
            }
        }
    }
}

/// An encoded bytestream, held in the session — minted by
/// `remanence_bitstream_materialize_bytestream`, whose handle owns
/// its stream, or by `remanence_medium_bytestream`, whose handle is a
/// view of the stream cached in the pooled medium and **must not outlive
/// its session**.
pub struct RemanenceBytestream {
    backing: BytestreamBacking,
    view: LayerView,
}

impl RemanenceBytestream {
    /// The stream this handle names, as the bitstream's resolves.
    fn stream(&self) -> Option<&Bytestream> {
        match &self.backing {
            BytestreamBacking::Owned(bytestream) => Some(bytestream),
            BytestreamBacking::Pooled { session, media } => {
                let session = unsafe { &mut (**session).session };
                session.medium_mut(*media)?.bytestream().ok()
            }
        }
    }
}

/// The report view a bitstream handle answers its strings from.
fn bitstream_view(bitstream: &Bitstream) -> LayerView {
    let report = bitstream.inspect();
    LayerView::new(
        &report.profile_id,
        &report.profile_name,
        "",
        &report.declared_loss,
        &report.evidence,
    )
}

/// The report view a bytestream handle answers its strings from.
fn bytestream_view(bytestream: &Bytestream) -> LayerView {
    let report = bytestream.inspect();
    LayerView::new(
        &report.profile_id,
        &report.codec_id,
        &report.codec_name,
        &report.declared_loss,
        &report.evidence,
    )
}

/// Materializes the family's hardware bitstream from what a remanence
/// image holds, under the profile's declared mechanics and read-channel
/// rules — it takes no policy because the type carries one (P30 reached
/// through the type), and `cache_bytes` is the P27 working-set bound.
///
/// The image carries no clock, so the ladder stands on the served
/// projection of it — one multiply per point, at the family's reference
/// frame — rather than on the image directly. The image is untouched.
/// The handle owns the stream; free it with
/// `remanence_bitstream_free`. Returns null on failure and stores
/// a message in `error_out` (free with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_flux_image_materialize_bitstream(
    image: *const RemanenceFluxImage,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceBitstream {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(image) = (unsafe { image.as_ref() }) else {
        let error = remanence::Error::io("null image");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match image.image.materialize_bitstream(cache_bytes) {
        Ok(bitstream) => {
            let view = bitstream_view(&bitstream);
            Box::into_raw(Box::new(RemanenceBitstream {
                backing: BitstreamBacking::Owned(bitstream),
                view,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The family's hardware bitstream over this medium's recording,
/// materialized once — lazily, into the pooled medium itself — and
/// answered from then on. It answers where the device type's profile
/// bears flux, and refuses by name everywhere else: a block medium's
/// recording is presented by its format adapter, and the two families
/// are disjoint (P13).
///
/// The handle is a view of the pooled stream, named by session and pool
/// identity like the medium's own view: it re-resolves on every call,
/// stops answering once the medium is released, and **must not outlive
/// the session**. Free it with `remanence_bitstream_free`, which
/// discards the view alone — the stream stays with its medium. Returns
/// null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_bitstream(
    medium: *mut RemanenceMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceBitstream {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_mut() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(pooled) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match pooled.bitstream() {
        Ok(bitstream) => {
            let view = bitstream_view(bitstream);
            Box::into_raw(Box::new(RemanenceBitstream {
                backing: BitstreamBacking::Pooled {
                    session: handle.session,
                    media: handle.id,
                },
                view,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a bitstream handle. A materialized stream's private session
/// storage goes with it; a pooled medium's stream stays with its
/// medium, and only the view is discarded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_free(bitstream: *mut RemanenceBitstream) {
    if !bitstream.is_null() {
        drop(unsafe { Box::from_raw(bitstream) });
    }
}

/// Materializes the family's encoded bytestream from a bitstream under
/// its declared group code — no policy, because the type carries one.
/// The bitstream is untouched, and the handle owns the stream it
/// answers. Returns null on failure and stores a message in
/// `error_out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_materialize_bytestream(
    bitstream: *const RemanenceBitstream,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceBytestream {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { bitstream.as_ref() }) else {
        let error = remanence::Error::io("null bitstream");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(stream) = held.stream() else {
        let error = remanence::Error::io("this bitstream's medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match stream.materialize_bytestream(cache_bytes) {
        Ok(bytestream) => {
            let view = bytestream_view(&bytestream);
            Box::into_raw(Box::new(RemanenceBytestream {
                backing: BytestreamBacking::Owned(bytestream),
                view,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// The family's encoded bytestream over this medium's recording — the
/// byte sequence the declared group code makes of the bitstream —
/// materialized once into the pooled medium and answered from then on,
/// refusing by name on non-flux media exactly as
/// `remanence_medium_bitstream` refuses.
///
/// The handle is a view of the pooled stream with the same contract as
/// the bitstream's: re-resolved per call, silent after release, never
/// to outlive the session, freed with
/// `remanence_bytestream_free` — which discards the view alone.
/// Returns null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_bytestream(
    medium: *mut RemanenceMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceBytestream {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_mut() }) else {
        let error = remanence::Error::io("null medium");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(pooled) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match pooled.bytestream() {
        Ok(bytestream) => {
            let view = bytestream_view(bytestream);
            Box::into_raw(Box::new(RemanenceBytestream {
                backing: BytestreamBacking::Pooled {
                    session: handle.session,
                    media: handle.id,
                },
                view,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a bytestream handle. A materialized stream's private session
/// storage goes with it; a pooled medium's stream stays with its
/// medium, and only the view is discarded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_free(bytestream: *mut RemanenceBytestream) {
    if !bytestream.is_null() {
        drop(unsafe { Box::from_raw(bytestream) });
    }
}

/// The profile the channel was declared by.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_profile_id(
    bitstream: *const RemanenceBitstream,
) -> *const c_char {
    unsafe { bitstream.as_ref() }.map_or(ptr::null(), |held| held.view.first.as_ptr())
}

/// Its human-readable name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_profile_name(
    bitstream: *const RemanenceBitstream,
) -> *const c_char {
    unsafe { bitstream.as_ref() }.map_or(ptr::null(), |held| held.view.second.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_profile_version(
    bitstream: *const RemanenceBitstream,
) -> u32 {
    unsafe { bitstream.as_ref() }
        .and_then(RemanenceBitstream::stream)
        .map_or(0, |stream| stream.inspect().profile_version)
}

/// The frame the cells are angles in, carried from the medium unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_reference_clock_hz(
    bitstream: *const RemanenceBitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }
        .and_then(RemanenceBitstream::stream)
        .map_or(0, |stream| stream.inspect().reference_clock_hz)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_cycles_per_rotation(
    bitstream: *const RemanenceBitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }
        .and_then(RemanenceBitstream::stream)
        .map_or(0, |stream| stream.inspect().cycles_per_rotation)
}

/// How many bytes of private session storage the bitstream occupies, and
/// how much of that is currently resident. It is never held whole (P27).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_backing_bytes(
    bitstream: *const RemanenceBitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }
        .and_then(RemanenceBitstream::stream)
        .map_or(0, |stream| stream.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_resident_bytes(
    bitstream: *const RemanenceBitstream,
) -> u64 {
    unsafe { bitstream.as_ref() }
        .and_then(RemanenceBitstream::stream)
        .map_or(0, |stream| stream.resident_bytes())
}

/// How many locations the bitstream claims.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_location_count(
    bitstream: *const RemanenceBitstream,
) -> usize {
    unsafe { bitstream.as_ref() }
        .and_then(RemanenceBitstream::stream)
        .map_or(0, |stream| stream.inspect().locations.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_location(
    bitstream: *const RemanenceBitstream,
    index: usize,
    out: *mut RemanenceBitstreamLocation,
) -> bool {
    let Some(stream) = (unsafe { bitstream.as_ref() }).and_then(RemanenceBitstream::stream) else {
        return false;
    };
    let Some(location) = stream.inspect().locations.get(index) else {
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
pub unsafe extern "C" fn remanence_bitstream_declared_loss_count(
    bitstream: *const RemanenceBitstream,
) -> usize {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.view.loss_codes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_declared_loss_code(
    bitstream: *const RemanenceBitstream,
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
pub unsafe extern "C" fn remanence_bitstream_declared_loss_detail(
    bitstream: *const RemanenceBitstream,
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
pub unsafe extern "C" fn remanence_bitstream_declared_loss_amount(
    bitstream: *const RemanenceBitstream,
    index: usize,
) -> u64 {
    unsafe { bitstream.as_ref() }
        .and_then(RemanenceBitstream::stream)
        .map_or(0, |stream| {
            stream
                .inspect()
                .declared_loss
                .get(index)
                .map_or(0, |loss| loss.count)
        })
}

/// The channel that produced the bitstream and the policy that produced
/// the medium, in that order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_evidence_count(
    bitstream: *const RemanenceBitstream,
) -> usize {
    unsafe { bitstream.as_ref() }.map_or(0, |held| held.view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bitstream_evidence(
    bitstream: *const RemanenceBitstream,
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
pub unsafe extern "C" fn remanence_bytestream_profile_id(
    bytestream: *const RemanenceBytestream,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| held.view.first.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_codec_id(
    bytestream: *const RemanenceBytestream,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| held.view.second.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_codec_name(
    bytestream: *const RemanenceBytestream,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| held.view.third.as_ptr())
}

/// How many bits of the recording carry how many bits of a byte, and how
/// many symbols make one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_symbol_bits(
    bytestream: *const RemanenceBytestream,
) -> u32 {
    unsafe { bytestream.as_ref() }
        .and_then(RemanenceBytestream::stream)
        .map_or(0, |stream| stream.inspect().symbol_bits)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_data_bits(
    bytestream: *const RemanenceBytestream,
) -> u32 {
    unsafe { bytestream.as_ref() }
        .and_then(RemanenceBytestream::stream)
        .map_or(0, |stream| stream.inspect().data_bits)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_symbols_per_byte(
    bytestream: *const RemanenceBytestream,
) -> u32 {
    unsafe { bytestream.as_ref() }
        .and_then(RemanenceBytestream::stream)
        .map_or(0, |stream| stream.inspect().symbols_per_byte)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_backing_bytes(
    bytestream: *const RemanenceBytestream,
) -> u64 {
    unsafe { bytestream.as_ref() }
        .and_then(RemanenceBytestream::stream)
        .map_or(0, |stream| stream.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_resident_bytes(
    bytestream: *const RemanenceBytestream,
) -> u64 {
    unsafe { bytestream.as_ref() }
        .and_then(RemanenceBytestream::stream)
        .map_or(0, |stream| stream.resident_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_location_count(
    bytestream: *const RemanenceBytestream,
) -> usize {
    unsafe { bytestream.as_ref() }
        .and_then(RemanenceBytestream::stream)
        .map_or(0, |stream| stream.inspect().locations.len())
}

/// One of them, written into `out`. Returns false when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_location(
    bytestream: *const RemanenceBytestream,
    index: usize,
    out: *mut RemanenceBytestreamLocation,
) -> bool {
    let Some(stream) = (unsafe { bytestream.as_ref() }).and_then(RemanenceBytestream::stream)
    else {
        return false;
    };
    let Some(location) = stream.inspect().locations.get(index) else {
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
pub unsafe extern "C" fn remanence_bytestream_declared_loss_count(
    bytestream: *const RemanenceBytestream,
) -> usize {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.view.loss_codes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_declared_loss_code(
    bytestream: *const RemanenceBytestream,
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
pub unsafe extern "C" fn remanence_bytestream_declared_loss_detail(
    bytestream: *const RemanenceBytestream,
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
pub unsafe extern "C" fn remanence_bytestream_declared_loss_amount(
    bytestream: *const RemanenceBytestream,
    index: usize,
) -> u64 {
    unsafe { bytestream.as_ref() }
        .and_then(RemanenceBytestream::stream)
        .map_or(0, |stream| {
            stream
                .inspect()
                .declared_loss
                .get(index)
                .map_or(0, |loss| loss.count)
        })
}

/// The codec, the channel beneath it and the medium policy beneath that,
/// in that order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_evidence_count(
    bytestream: *const RemanenceBytestream,
) -> usize {
    unsafe { bytestream.as_ref() }.map_or(0, |held| held.view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_evidence(
    bytestream: *const RemanenceBytestream,
    index: usize,
) -> *const c_char {
    unsafe { bytestream.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many framed bytes one location holds, addressed in the family's
/// own terms — the Commodore 1541 numbers its tracks from 1 — written
/// into `bytes_out`. This is the extent
/// `remanence_bytestream_location_read_at` reads within.
///
/// A track the stream does not hold is refused naming what it does
/// hold: the stream's locations are what the medium carried, and
/// nothing is manufactured to answer for a track that is not there.
/// Returns false on failure and stores a message in `error_out` (free
/// with `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_location_bytes(
    bytestream: *const RemanenceBytestream,
    track: u32,
    bytes_out: *mut u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { bytestream.as_ref() }) else {
        let error = remanence::Error::io("null bytestream");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let Some(stream) = held.stream() else {
        let error = remanence::Error::io("this bytestream's medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match stream.location(Location::track(track)) {
        Ok(location) => {
            if !bytes_out.is_null() {
                unsafe { *bytes_out = location.bytes() };
            }
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// Reads exactly `length` framed bytes at `offset` of one track into
/// `buffer_out`, whole or not at all. Bytes number from the first
/// framed byte, because nothing before sync is a byte at all; no byte
/// here is a header, a sector or a file, and the layers that assign
/// those sit above.
///
/// A byte whose recorded pattern the family's table does not assign has
/// no value to serve: a read that touches one is refused naming it
/// rather than answered with an invented value. Returns false on
/// failure and stores a message in `error_out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_location_read_at(
    bytestream: *const RemanenceBytestream,
    track: u32,
    offset: u64,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { bytestream.as_ref() }) else {
        let error = remanence::Error::io("null bytestream");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let Some(stream) = held.stream() else {
        let error = remanence::Error::io("this bytestream's medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
    match stream
        .location(Location::track(track))
        .and_then(|location| location.read_at(offset, buffer))
    {
        Ok(()) => true,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

// ---------------------------------------------------------------------------
// The C1541 sector layer: the recording's own records, recognized above
// the encoded bytestream under the family's declared grammar. This is
// the seam where the two layers below stop saying nothing about what
// their bytes mean — and it ends by stating what it derives, with every
// claim carrying its evidence and every sector that does not read
// refusing by name.

use remanence::C1541Sectors;

/// One location the sector layer read, and what it found there.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceSectorLocation {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub has_surface: bool,
    pub surface: u64,
    /// What the family's density map claims one location in this zone
    /// holds, where a declared zone covers it at all.
    pub has_records_declared: bool,
    pub records_declared: u32,
    pub headers: u64,
    pub records: u64,
    pub readable: u64,
    pub failed_checksum: u64,
    pub runs_without_a_record: u64,
}

/// One record the recognition read, and the evidence for every claim it
/// makes. The rule and the refusal behind an unreadable claim are read
/// with `remanence_c1541_sectors_claim_rule` and `..._claim_refusal`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceSectorClaim {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub has_surface: bool,
    pub surface: u64,
    /// The bit of the location's own bitstream the header opens at.
    pub at_bit: u64,
    /// The address the header states for itself, as recorded.
    pub track: u8,
    pub sector: u8,
    pub id_high: u8,
    pub id_low: u8,
    /// Stated beside computed, for each block the record holds.
    pub header_checksum_stated: u8,
    pub header_checksum_computed: u8,
    pub has_data: bool,
    pub data_at_bit: u64,
    pub data_checksum_stated: u8,
    pub data_checksum_computed: u8,
    /// Bytes of either block holding a pattern the family's table does
    /// not assign, which are not bytes.
    pub unresolved_bytes: u64,
    /// Whether the family's own declaration covers the address stated.
    pub within_declaration: bool,
    pub readable: bool,
}

/// One address more than one readable claim states.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RemanenceContestedAddress {
    pub track: u8,
    pub sector: u8,
    pub readable_claims: u32,
}

/// The recording's own sectors, held in the session. The payloads stay
/// behind this handle and are read by the address the recording states.
pub struct RemanenceC1541Sectors {
    sectors: C1541Sectors,
    view: LayerView,
    claim_rules: Vec<CString>,
    claim_refusals: Vec<CString>,
}

/// Recognizes the recording's own sectors out of a bytestream, under
/// the family's declared record grammar — no policy, because the
/// profile carries one; `cache_bytes` is the P27 working-set bound. The
/// bytestream is untouched, and either backing serves: a materialized
/// stream's handle or a pooled medium's view. Returns null on failure
/// and stores a message in `error_out` (free with
/// `remanence_string_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_recognize_sectors(
    bytestream: *const RemanenceBytestream,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceC1541Sectors {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { bytestream.as_ref() }) else {
        let error = remanence::Error::io("null bytestream");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(stream) = held.stream() else {
        let error = remanence::Error::io("this bytestream's medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match stream.recognize_sectors(cache_bytes) {
        Ok(sectors) => {
            let family = sectors.family().to_string();
            let Some(sectors) = sectors.into_c1541() else {
                // The rung is one and the reading is the family's. A
                // recording whose records are not CBM DOS sectors is
                // refused here by name rather than read as though they
                // were.
                let error = remanence::Error::io(format!(
                    "this recording's records were recognized by the '{}' family, whose                      claims are not CBM DOS sectors",
                    family
                ));
                unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
                return ptr::null_mut();
            };
            let report = sectors.inspect();
            let view = LayerView::new(
                &report.profile_id,
                &report.grammar_id,
                &report.grammar_name,
                &report.declared_loss,
                &report.evidence,
            );
            let claim_rules = report
                .claims
                .iter()
                .map(|claim| to_cstring(claim.rule.as_deref().unwrap_or("")))
                .collect();
            let claim_refusals = report
                .claims
                .iter()
                .map(|claim| to_cstring(claim.refusal.as_deref().unwrap_or("")))
                .collect();
            Box::into_raw(Box::new(RemanenceC1541Sectors {
                sectors,
                view,
                claim_rules,
                claim_refusals,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees a sector-layer handle, discarding its private session storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_free(sectors: *mut RemanenceC1541Sectors) {
    if !sectors.is_null() {
        drop(unsafe { Box::from_raw(sectors) });
    }
}

/// Reads one sector by the address the recording states for it, into
/// `buffer_out`, which must be `remanence_c1541_sectors_payload_bytes`
/// long.
///
/// It answers only where the recording is unambiguous: one readable
/// claim, or several holding the same bytes. Every other outcome is a
/// refusal naming its rule — an address no record states, an address no
/// claim of which reads, or one several readable claims disagree about.
/// Nothing is repaired and no block is filled in. Returns false on
/// failure and stores a message in `error_out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_read(
    sectors: *const RemanenceC1541Sectors,
    track: u8,
    sector: u8,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        let error = remanence::Error::io("null sector layer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    match held.sectors.read_sector(track, sector) {
        Ok(payload) => {
            if payload.len() != length {
                let error = remanence::Error::io(format!(
                    "the sector holds {} bytes and the buffer states {length}",
                    payload.len()
                ));
                unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
                return false;
            }
            let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
            buffer.copy_from_slice(&payload);
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// The **direct partition** over this recording — the library's own
/// composition of the whole content, which is what a namespace above is
/// reached through (P19). Null only for a null sector layer.
///
/// A recording records no partition scheme, so there is one member and it
/// is synthetic: its account is provenance and never evidence, and it
/// composes no addressed extent, because a recording's blocks are
/// addressed by the recording rather than by position. The addressable
/// vantage is therefore absent and the namespace vantage is *declared* —
/// `remanence_partition_filesystem_as` with `"cbmdos"` — because nothing
/// here determines a reading and this layer will not pick one.
///
/// **The sector layer carries no file verbs of its own**: it may be asked
/// what it composes — this — and may not be told to act as a namespace it
/// is not. The declaration's refusal is the seam that ran out of answers
/// stating it, and everything beneath stays readable either way: a disk
/// with no filesystem is still a recording, still a sector layer, and
/// still every claim this layer made about it.
///
/// The partition **borrows** the sector layer, and so does every space
/// composed through it: keep the sectors alive for as long as any of
/// them, and free them last — the partition with
/// `remanence_partition_free` and the space with `remanence_space_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_partition(
    sectors: *const RemanenceC1541Sectors,
) -> *mut RemanencePartition {
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        return ptr::null_mut();
    };
    let view = held.sectors.partition();
    partition_handle(
        ptr::null_mut(),
        None,
        sectors,
        ptr::null(),
        view.partition(),
    )
}

/// How many bytes of payload one sector carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_payload_bytes(
    sectors: *const RemanenceC1541Sectors,
) -> u32 {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.sectors.inspect().payload_bytes)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_profile_id(
    sectors: *const RemanenceC1541Sectors,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| held.view.first.as_ptr())
}

/// The record grammar every rule the recognition applied came from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_grammar_id(
    sectors: *const RemanenceC1541Sectors,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| held.view.second.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_grammar_name(
    sectors: *const RemanenceC1541Sectors,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| held.view.third.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_backing_bytes(
    sectors: *const RemanenceC1541Sectors,
) -> u64 {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.sectors.backing_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_resident_bytes(
    sectors: *const RemanenceC1541Sectors,
) -> u64 {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.sectors.resident_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_location_count(
    sectors: *const RemanenceC1541Sectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.sectors.inspect().locations.len())
}

/// Copies one location's counts into `out`. Returns false when `index`
/// is past the end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_location(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
    out: *mut RemanenceSectorLocation,
) -> bool {
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        return false;
    };
    let Some(location) = held.sectors.inspect().locations.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceSectorLocation {
                half_track_numerator: location.half_track_numerator,
                half_track_denominator: location.half_track_denominator,
                has_surface: location.surface.is_some(),
                surface: location.surface.unwrap_or(0),
                has_records_declared: location.records_declared.is_some(),
                records_declared: location.records_declared.unwrap_or(0),
                headers: location.headers,
                records: location.records,
                readable: location.readable,
                failed_checksum: location.failed_checksum,
                runs_without_a_record: location.runs_without_a_record,
            };
        }
    }
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_claim_count(
    sectors: *const RemanenceC1541Sectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.sectors.inspect().claims.len())
}

/// Copies one claim into `out`. Returns false when `index` is past the
/// end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_claim(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
    out: *mut RemanenceSectorClaim,
) -> bool {
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        return false;
    };
    let Some(claim) = held.sectors.inspect().claims.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceSectorClaim {
                half_track_numerator: claim.half_track_numerator,
                half_track_denominator: claim.half_track_denominator,
                has_surface: claim.surface.is_some(),
                surface: claim.surface.unwrap_or(0),
                at_bit: claim.at_bit,
                track: claim.track,
                sector: claim.sector,
                id_high: claim.id_high,
                id_low: claim.id_low,
                header_checksum_stated: claim.header_checksum_stated,
                header_checksum_computed: claim.header_checksum_computed,
                has_data: claim.has_data,
                data_at_bit: claim.data_at_bit,
                data_checksum_stated: claim.data_checksum_stated,
                data_checksum_computed: claim.data_checksum_computed,
                unresolved_bytes: claim.unresolved_bytes,
                within_declaration: claim.within_declaration,
                readable: claim.readable,
            };
        }
    }
    true
}

/// Which rule of the sector-layer set stands in the way of this claim,
/// or an empty string for one that reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_claim_rule(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
        held.claim_rules
            .get(index)
            .map_or(ptr::null(), |rule| rule.as_ptr())
    })
}

/// Why this claim does not read, in the layer's own terms, or an empty
/// string for one that does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_claim_refusal(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
        held.claim_refusals
            .get(index)
            .map_or(ptr::null(), |refusal| refusal.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_contested_count(
    sectors: *const RemanenceC1541Sectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.sectors.inspect().contested.len())
}

/// Copies one contested address into `out`. Returns false when `index`
/// is past the end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_contested(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
    out: *mut RemanenceContestedAddress,
) -> bool {
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        return false;
    };
    let Some(contested) = held.sectors.inspect().contested.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceContestedAddress {
                track: contested.track,
                sector: contested.sector,
                readable_claims: contested.readable_claims,
            };
        }
    }
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_declared_loss_count(
    sectors: *const RemanenceC1541Sectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.view.loss_codes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_declared_loss_code(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_declared_loss_detail(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_declared_loss_amount(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
) -> u64 {
    unsafe { sectors.as_ref() }.map_or(0, |held| {
        held.sectors
            .inspect()
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// The grammar and policy that produced it, and everything the
/// bytestream said beneath it, in that order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_evidence_count(
    sectors: *const RemanenceC1541Sectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_c1541_sectors_evidence(
    sectors: *const RemanenceC1541Sectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .evidence
            .get(index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

// ---------------------------------------------------------------------------
// The FM and MFM sector layer. It is a separate handle from the CBM DOS
// one rather than a shared one with a family tag, because the two claim
// vocabularies have nothing in common to share: one states a track and a
// sector under a one-byte exclusive-or, the other a cylinder, head and
// size code under a sixteen-bit CRC. A struct carrying both would be
// half-absent whichever recording it described.
// ---------------------------------------------------------------------------

/// One record an FM or MFM recording states, exactly as it states it.
///
/// Both checks are carried stated beside computed. Nothing is repaired,
/// and a record whose checks disagree is reported here and refused by
/// `remanence_ibm_sectors_read` rather than served as though it read.
#[repr(C)]
pub struct RemanenceIbmSectorClaim {
    /// Which location this record was found in, as the recording's own
    /// coordinate.
    pub location: u64,
    /// Which surface, where the family records more than one.
    pub has_surface: bool,
    pub surface: u64,
    /// Where the id field's mark opens, as a byte of the location.
    pub at_byte: u64,
    /// The address the record states for itself — not where it sits.
    pub cylinder: u8,
    pub head: u8,
    pub sector: u8,
    /// How large the record says its data field is, as the power-of-two
    /// code the family writes: 0 is 128 bytes, 1 is 256, and so on.
    pub size_code: u8,
    pub header_checksum_stated: u16,
    pub header_checksum_computed: u16,
    /// Whether a data field follows this id field at all. A recording
    /// may state an address and hold no data for it.
    pub has_data: bool,
    pub data_at_byte: u64,
    /// Whether the data field was opened by a deleted-data mark. That is
    /// what the recording says, carried as a fact: nothing here decides
    /// on a caller's behalf whether such a record counts.
    pub data_deleted: bool,
    pub data_checksum_stated: u16,
    pub data_checksum_computed: u16,
    /// Whether both checks agree. Only such a record is served.
    pub readable: bool,
}

/// An owned FM or MFM sector layer, holding its claims and their
/// payloads in private session storage.
pub struct RemanenceIbmSectors {
    sectors: remanence::IbmSectors,
    view: LayerView,
}

/// Recognizes the recording's own sectors out of a bytestream, under the
/// FM or MFM record grammar the profile enrols.
///
/// The rung beneath is one and the reading is the family's. A bytestream
/// whose records are not FM or MFM sectors is refused here by name
/// rather than read as though they were — use
/// `remanence_bytestream_recognize_sectors` for a CBM DOS recording.
///
/// Returns null and states the refusal on failure. Free the result with
/// `remanence_ibm_sectors_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_bytestream_recognize_ibm_sectors(
    bytestream: *const RemanenceBytestream,
    cache_bytes: u64,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceIbmSectors {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { bytestream.as_ref() }) else {
        let error = remanence::Error::io("null bytestream");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    let Some(stream) = held.stream() else {
        let error = remanence::Error::io("this bytestream's medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match stream.recognize_sectors(cache_bytes) {
        Ok(sectors) => {
            let family = sectors.family().to_string();
            let Some(sectors) = sectors.into_ibm() else {
                let error = remanence::Error::io(format!(
                    "this recording's records were recognized by the '{family}' family, \
                     whose claims are not FM or MFM sectors"
                ));
                unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
                return ptr::null_mut();
            };
            let report = sectors.inspect();
            let view = LayerView::new(
                &report.profile_id,
                &report.encoding_id,
                "",
                &report.declared_loss,
                &report.evidence,
            );
            Box::into_raw(Box::new(RemanenceIbmSectors { sectors, view }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees an FM or MFM sector layer, discarding its private session
/// storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_free(sectors: *mut RemanenceIbmSectors) {
    if !sectors.is_null() {
        drop(unsafe { Box::from_raw(sectors) });
    }
}

/// Copies one record's payload into `buffer_out`, by the address the
/// recording states for it.
///
/// Only a record whose checks both agree is served. One whose checksum
/// disagrees holds what it holds and is reported with both numbers by
/// `remanence_ibm_sectors_claim`; serving it as though it read cleanly
/// would answer a question the evidence does not.
///
/// `length` must be exactly what the record carries, which is what its
/// claim's size code states. Returns false and states the refusal
/// otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_read(
    sectors: *const RemanenceIbmSectors,
    cylinder: u8,
    head: u8,
    sector: u8,
    buffer_out: *mut u8,
    length: usize,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        let error = remanence::Error::io("null sector layer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    if buffer_out.is_null() {
        let error = remanence::Error::io("null buffer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    }
    match held.sectors.read_sector(cylinder, head, sector) {
        Ok(payload) => {
            if payload.len() != length {
                let error = remanence::Error::io(format!(
                    "the sector holds {} bytes and the buffer states {length}",
                    payload.len()
                ));
                unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
                return false;
            }
            let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_out, length) };
            buffer.copy_from_slice(&payload);
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// The drive profile that read the recording these records came off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_profile_id(
    sectors: *const RemanenceIbmSectors,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| held.view.first.as_ptr())
}

/// Which encoding framed these records — the FM or MFM codec the
/// profile enrols, by its own identifier.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_encoding_id(
    sectors: *const RemanenceIbmSectors,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| held.view.second.as_ptr())
}

/// How many records the recognition claims.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_claim_count(
    sectors: *const RemanenceIbmSectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.sectors.inspect().claims.len())
}

/// Copies one claim into `out`. Returns false when `index` is past the
/// end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_claim(
    sectors: *const RemanenceIbmSectors,
    index: usize,
    out: *mut RemanenceIbmSectorClaim,
) -> bool {
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        return false;
    };
    let Some(claim) = held.sectors.inspect().claims.get(index) else {
        return false;
    };
    if !out.is_null() {
        unsafe {
            *out = RemanenceIbmSectorClaim {
                location: claim.location,
                has_surface: claim.surface.is_some(),
                surface: claim.surface.unwrap_or(0),
                at_byte: claim.at_byte,
                cylinder: claim.cylinder,
                head: claim.head,
                sector: claim.sector,
                size_code: claim.size_code,
                header_checksum_stated: claim.header_checksum_stated,
                header_checksum_computed: claim.header_checksum_computed,
                has_data: claim.has_data,
                data_at_byte: claim.data_at_byte,
                data_deleted: claim.data_deleted,
                data_checksum_stated: claim.data_checksum_stated,
                data_checksum_computed: claim.data_checksum_computed,
                readable: claim.readable(),
            };
        }
    }
    true
}

/// The uniform geometry these records state for themselves.
///
/// Every number is read off the claims rather than off the drive
/// profile: a profile declares what the mechanism records, and this says
/// what this disk holds. Returns false and states the refusal where the
/// records compose no uniform image — more than one data-field size, or
/// a gap in the sector numbering.
#[repr(C)]
pub struct RemanenceIbmGeometry {
    pub cylinders: u32,
    pub heads: u32,
    pub sectors_per_track: u32,
    /// The lowest sector number the records state. IBM recordings
    /// conventionally number from one, but that is a convention and this
    /// reads what is there.
    pub first_sector: u8,
    pub sector_bytes: u32,
    /// What the whole extent spans, which is the geometry's rather than
    /// the sum of what reads: a hole still occupies its place.
    pub length_bytes: u64,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_geometry(
    sectors: *const RemanenceIbmSectors,
    out: *mut RemanenceIbmGeometry,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        let error = remanence::Error::io("null sector layer");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    match held.sectors.geometry() {
        Ok(geometry) => {
            if !out.is_null() {
                unsafe {
                    *out = RemanenceIbmGeometry {
                        cylinders: geometry.cylinders,
                        heads: geometry.heads,
                        sectors_per_track: geometry.sectors_per_track,
                        first_sector: geometry.first_sector,
                        sector_bytes: geometry.sector_bytes,
                        length_bytes: geometry.length_bytes(),
                    };
                }
            }
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
        }
    }
}

/// The **direct partition** over this recording — the library's own
/// composition of the whole content, which is what a namespace above is
/// reached through (P19).
///
/// **Unlike a CBM DOS recording's, this partition is addressable**
/// (D62). Its records state a cylinder, a head and a sector number, and
/// those compose exactly the geometry ordering FAT, HDOS and CP/M were
/// all written against — so a volume here opens through the same seam a
/// hard-disk image opens through, with no flux vocabulary reaching the
/// filesystem adapter and none of the filesystem's reaching the
/// recording.
///
/// The namespace vantage is *declared*: nothing about an FM or MFM
/// recording determines which of those it holds, and this layer will not
/// pick one. `remanence_partition_filesystem_as` with `"fat"`, `"hdos"`,
/// `"cpm"` or a `"cpm-*"` layout is the door; `"cbmdos"` is refused
/// here, because those blocks are addressed by the recording rather than
/// by position.
///
/// The extent's length is the geometry's rather than the sum of what
/// reads: a record the recording never stated, or one whose CRC
/// disagrees, is a hole that still occupies its place. Reads that touch
/// it are refused naming the address and every other read answers —
/// nothing is zeroed.
///
/// Null where the records compose no uniform image, with the refusal
/// stated. The partition **borrows** the sector layer, and so does every
/// space composed through it: keep the sectors alive for as long as any
/// of them, and free them last.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_partition(
    sectors: *const RemanenceIbmSectors,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanencePartition {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(held) = (unsafe { sectors.as_ref() }) else {
        return ptr::null_mut();
    };
    let mut partition = match held.sectors.partition() {
        Ok(partition) => partition,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return ptr::null_mut();
        }
    };
    let view = partition.view();
    partition_handle(
        ptr::null_mut(),
        None,
        ptr::null(),
        sectors,
        view.partition(),
    )
}

/// What this layer could not resolve, in its own terms, and how much of
/// it there was.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_declared_loss_count(
    sectors: *const RemanenceIbmSectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.view.loss_codes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_declared_loss_code(
    sectors: *const RemanenceIbmSectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_codes
            .get(index)
            .map_or(ptr::null(), |code| code.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_declared_loss_detail(
    sectors: *const RemanenceIbmSectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
        held.view
            .loss_details
            .get(index)
            .map_or(ptr::null(), |detail| detail.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_declared_loss_amount(
    sectors: *const RemanenceIbmSectors,
    index: usize,
) -> u64 {
    unsafe { sectors.as_ref() }.map_or(0, |held| {
        held.sectors
            .inspect()
            .declared_loss
            .get(index)
            .map_or(0, |loss| loss.count)
    })
}

/// The grammar that produced these records, and what it found, in that
/// order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_evidence_count(
    sectors: *const RemanenceIbmSectors,
) -> usize {
    unsafe { sectors.as_ref() }.map_or(0, |held| held.view.evidence.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_ibm_sectors_evidence(
    sectors: *const RemanenceIbmSectors,
    index: usize,
) -> *const c_char {
    unsafe { sectors.as_ref() }.map_or(ptr::null(), |held| {
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
//
// **It is a view derived from the partition pool, and nothing navigates
// through it.** Content is reached through `remanence_medium_partition`;
// the report is what a caller shows a user, and the volume identities in
// it are what a caller carries back into a verb. The direct partition is a composition act
// rather than something a scheme declared, so it never appears as a
// region here: a medium recording no scheme reports zero regions.

/// A snapshot of one disk's layered inspection. Owned by the caller and
/// released with `remanence_report_free`; every string and record
/// reached through it is borrowed from it and dies with it.
pub struct RemanenceDiskReport {
    device_id: u64,
    device_image_format: CString,
    device_length_bytes: u64,
    device_article: CString,
    device_device_type: Option<CString>,
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

/// Inspects the medium and returns its layered report, derived from the
/// pool the load established. Null on failure, with the category and
/// message written to the out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_inspect(
    medium: *mut RemanenceMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDiskReport {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
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
                device_article: to_cstring(&report.device.article),
                device_device_type: report.device.device_type.as_deref().map(to_cstring),
                device_authoritative_layer: to_cstring(&report.device.authoritative_layer),
                device_active_layer: to_cstring(&report.device.active_layer),
                content,
                content_evidence,
                schema,
                regions,
                volumes,
                filesystems,
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

/// The article of the medium attached to the device (P14) — the
/// substrate, said in the article catalog's own name for it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_article(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_article.as_ptr())
}

/// The device the medium's content was recorded by, by the device
/// catalog's stable spelling — null where no device recorded it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_type(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }
        .and_then(|report| report.device_device_type.as_ref())
        .map_or(ptr::null(), |device| device.as_ptr())
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
pub unsafe extern "C" fn remanence_report_region_count(
    report: *const RemanenceDiskReport,
) -> usize {
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
pub unsafe extern "C" fn remanence_report_volume_count(
    report: *const RemanenceDiskReport,
) -> usize {
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
        volume
            .origin_regions
            .get(region_index)
            .copied()
            .unwrap_or(0)
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
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_bytes)
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
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_count)
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
// The remanence image: the flux family's physical stratum, and the
// `.remanence` artifact it is read from and written to. The model
// beneath the root — orbits' points, magnetization, write geometry —
// does not cross this boundary; what crosses is the image's shape.

// The core's own name for the root is `FluxImage`; C prefixes it, as it
// prefixes every exported type, giving `RemanenceFluxImage`. The alias
// keeps both spellings honest inside this file.
use remanence::{FluxImage as PhysicalImage, FluxImageReport, FluxWriteReport};

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

struct ImageView {
    path: CString,
    format_id: CString,
    format_name: CString,
    form_factor: CString,
    provenance: Vec<CString>,
}

impl ImageView {
    fn new(image: &PhysicalImage, report: &FluxImageReport) -> Self {
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
    image: PhysicalImage,
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

unsafe fn open_remanence_image(
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

// -------------------------------------------------- the C64 renditions

use remanence::{D64Report, G64Report};

/// The strings a rendition report owns: where it was written, if it was,
/// and its declared-loss account.
struct RenditionView {
    path: Option<CString>,
    loss_codes: Vec<CString>,
    loss_details: Vec<CString>,
}

impl RenditionView {
    fn new(path: Option<&String>, loss: &[remanence::DeclaredLoss]) -> Self {
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

fn boxed_d64(report: D64Report) -> *mut RemanenceD64Report {
    let view = RenditionView::new(report.path.as_ref(), &report.declared_loss);
    Box::into_raw(Box::new(RemanenceD64Report { report, view }))
}

fn boxed_g64(report: G64Report) -> *mut RemanenceG64Report {
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

    /// The raw OS handle of a file the test hands to the library, which
    /// takes ownership of it from there.
    fn raw_source(file: std::fs::File) -> isize {
        #[cfg(windows)]
        {
            use std::os::windows::io::IntoRawHandle;
            file.into_raw_handle() as isize
        }
        #[cfg(not(windows))]
        {
            use std::os::fd::IntoRawFd;
            file.into_raw_fd() as isize
        }
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
        let source = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path)
            .expect("the caller's own writable open");
        let medium = session
            .load_media(
                source,
                Format::Raw {
                    device: remanence::HardDrive::MbrSector.into(),
                    block_bytes: 512,
                },
            )
            .expect("the whole image loads");
        let content: Vec<u8> = (0..1_200_000u32).map(|n| (n % 241) as u8).collect();
        medium
            .partition(0)
            .expect("a partitionless floppy bears its direct partition")
            .filesystem_as("fat")
            .expect("the declared reading the boot record bears out")
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
        let path =
            std::env::temp_dir().join(format!("remanence-ffi-degraded-{}.img", std::process::id()));
        truncated_floppy(&path, 1_000_000);

        let session = unsafe { remanence_session_new() };
        let format = to_cstring("raw");
        let device = to_cstring("mbr-sector-hd");
        let mut category = RemanenceErrorCategory::Io;
        let mut message = ptr::null_mut();
        let mut rule = ptr::null_mut();
        // The caller's own open, handed over: the library takes the
        // handle and asks it one question (P7 as amended).
        let source = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .expect("the caller's own writable open");
        let medium = unsafe {
            remanence_session_load_media(
                session,
                raw_source(source),
                format.as_ptr(),
                device.as_ptr(),
                512,
                &mut category,
                &mut message,
                &mut rule,
            )
        };
        assert!(
            !medium.is_null(),
            "a truncated source still loads, degraded"
        );

        let assurance = unsafe { remanence_medium_assurance(medium) };
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
            unsafe { remanence_medium_mode(medium) },
            RemanenceAccessMode::ReadOnly,
            "the effective mode is the same answer read another way"
        );
        assert_eq!(
            unsafe { remanence_assurance_claim(assurance) },
            RemanenceClaim::CallerOpened,
            "the claim's class travels beside the access it established"
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
            !unsafe { remanence_medium_commit(medium, &mut category, &mut message, &mut rule) },
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
