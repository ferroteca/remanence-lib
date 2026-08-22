// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The root of the storage-device surface (U3/U4): the session, the media
//! pool it owns, and the loads that populate that pool — a raw, qcow2 or
//! VDI image taken under the P7 claim, an authored medium created whole,
//! or a discovery already made.
//!
//! Everything a caller reaches afterwards hangs off a medium in this pool,
//! and the two borrowed views the session hands out — [`RemanenceMedium`]
//! here and [`RemanenceDevice`](crate::device::RemanenceDevice) beside it —
//! name their subject by pool identity rather than by pointer, so a later
//! load can never make one point at a stranger.

use crate::abi::{
    RemanenceErrorCategory, clear_error, file_from_raw, set_error, to_cstring, utf8_arg,
};
use crate::catalog::declared_format;
use crate::device::RemanenceDevice;
use crate::discovery::RemanenceDiscovery;
use crate::storage::file::{RemanenceFileSource, RemanenceFileSourceList};
use remanence::{DeviceType, MediaId, NewMedia, Session};
use std::ffi::{CString, c_char};
use std::ptr;

/// An open session: the claim and cache scope, holding the devices
/// within it (P32).
pub struct RemanenceSession {
    pub(crate) session: Session,
    /// Borrowed device views handed to callers. Owned here so their
    /// strings outlive the call that produced them, and freed with the
    /// session.
    pub(crate) views: Vec<Box<RemanenceDevice>>,
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
    pub(crate) session: *mut RemanenceSession,
    pub(crate) id: MediaId,
    /// The artifact's names, absent where the caller's handle has none.
    pub(crate) path: Option<CString>,
    pub(crate) image_path: Option<CString>,
    /// The article and the device that recorded it.
    ///
    /// **The device type is not settled for good at the load**, which
    /// the names are: an authored blank binds one when a layout is
    /// recorded onto it, so the view is restated after that act as well
    /// as when it is minted.
    pub(crate) article: Option<CString>,
    pub(crate) device_type: Option<CString>,
}

impl RemanenceMedium {
    /// The medium this view names, or `None` once it is released.
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn medium(&self) -> Option<&mut remanence::Medium> {
        let session = unsafe { &mut (*self.session).session };
        session.medium_mut(self.id)
    }

    /// Restates what the view caches, from the medium it names.
    ///
    /// It runs when the view is minted and again after the
    /// authored-to-recorded arc, which is the one act that changes any
    /// of it: everything else here is settled at the load.
    pub(crate) fn refresh(&mut self) {
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
pub(crate) unsafe fn medium_view(
    session: *mut RemanenceSession,
    id: MediaId,
) -> *mut RemanenceMedium {
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
