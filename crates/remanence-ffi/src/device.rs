// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The storage devices a session attaches, and the slot catalogue saying
//! what kinds there are. A device is a borrowed view the session owns;
//! every content-side fact answers on the medium instead.

use crate::abi::{
    RemanenceAccessIntent, RemanenceErrorCategory, access_intent, clear_error, set_error,
    to_cstring, to_owned_c_char, utf8_arg,
};
use crate::session::{RemanenceMedium, RemanenceSession, medium_view};
use remanence::{AttachmentId, DeviceSlot, DeviceType, MediaId, StorageDevice};
use std::ffi::{CString, c_char};
use std::ptr;

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
pub(crate) struct SlotView {
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
pub(crate) fn slots() -> &'static [SlotView] {
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

pub(crate) fn slot_string(index: usize, read: fn(&SlotView) -> Option<&CString>) -> *const c_char {
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
pub(crate) unsafe fn add_device(
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
pub(crate) unsafe fn add_device_for(
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
pub(crate) unsafe fn release_device(
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
pub(crate) unsafe fn device_view(
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
