// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The two vantages a partition composes: the addressable volume and the
//! namespace-bearing filesystem, both reached through one handle.

use crate::abi::{
    RemanenceErrorCategory, clear_error, set_error, to_cstring, to_owned_c_char, utf8_arg,
};
use crate::flux::c1541::RemanenceC1541Sectors;
use crate::flux::ibm::RemanenceIbmSectors;
use crate::session::RemanenceSession;
use crate::storage::entries::RemanenceEntryKind;
use crate::storage::file::RemanenceFileSourceList;
use remanence::MediaId;
use std::ffi::{CString, c_char};
use std::ptr;

/// One space of a medium's content, composed over the partition that
/// bears it. Free with `remanence_space_free`.
pub struct RemanenceSpace {
    pub(crate) session: *mut RemanenceSession,
    /// The medium whose pool holds the partition this was composed over.
    /// `None` where no medium composed it at all.
    pub(crate) media: Option<MediaId>,
    /// The sector layer this namespace is presented over, where no
    /// medium composed it — the flux family is reached through its own
    /// types rather than through a device. Null for a medium-backed
    /// space, and borrowed from the handle that owns it either way.
    pub(crate) sectors: *const RemanenceC1541Sectors,
    /// The FM or MFM record layer it is composed over, where that is
    /// what composed it. Null otherwise, and never set alongside
    /// `sectors`: a recording belongs to one family.
    pub(crate) ibm_sectors: *const RemanenceIbmSectors,
    /// The scheme's own ordinal of the partition that composed it, which
    /// is what re-resolution looks the partition up by. `None` where no
    /// pool named it.
    pub(crate) ordinal: Option<u32>,
    /// The namespace reading a caller declared to mint it, where one did.
    /// It is carried so re-resolution declares the same thing again
    /// rather than falling back on what the pool records.
    pub(crate) declared: Option<CString>,
    /// Whether the composed space carries the addressable vantage.
    pub(crate) addressable: bool,
    /// The identity the inspection report issued for the volume composed
    /// over the partition, absent where it issued none.
    pub(crate) volume: Option<u64>,
    pub(crate) start_bytes: u64,
    pub(crate) length_bytes: u64,
    /// The namespace kind, where it has the namespace vantage.
    pub(crate) kind: Option<CString>,
}

/// One file, named by the filesystem that holds it. Free with
/// `remanence_file_free`.
pub struct RemanenceFile {
    pub(crate) session: *mut RemanenceSession,
    pub(crate) media: Option<MediaId>,
    /// As on `RemanenceSpace`: the sector layer a flux-family namespace
    /// is presented over, or null.
    pub(crate) sectors: *const RemanenceC1541Sectors,
    /// The FM or MFM record layer it is composed over, where that is
    /// what composed it. Null otherwise, and never set alongside
    /// `sectors`: a recording belongs to one family.
    pub(crate) ibm_sectors: *const RemanenceIbmSectors,
    pub(crate) ordinal: Option<u32>,
    pub(crate) declared: Option<CString>,
    pub(crate) path: CString,
    pub(crate) name: CString,
    pub(crate) kind: RemanenceEntryKind,
    pub(crate) size_bytes: u64,
}

/// What a space or a file re-composes itself from: the provider it was
/// minted through, the partition within it, and the reading declared over
/// that partition where one was.
///
/// It is the whole of what either handle knows about where it came from,
/// named once so both carry the same thing rather than two spellings of
/// it.
#[derive(Clone, Copy)]
pub(crate) struct SpaceOrigin<'a> {
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
    pub(crate) fn origin(&self) -> SpaceOrigin<'_> {
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
    pub(crate) fn origin(&self) -> SpaceOrigin<'_> {
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
pub(crate) unsafe fn with_space<T>(
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
