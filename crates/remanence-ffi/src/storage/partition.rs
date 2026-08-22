// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The partition surface (P16, P17, P19): content is reached through the
//! partition that composes it.
//!
//! A medium carries no file verbs at all. It carries its pool — the scheme
//! it was populated under, and every partition in it — and the vantage
//! doors live on the partition: `remanence_partition_volume` for the
//! addressable vantage, `remanence_partition_filesystem` for the namespace
//! one, and `remanence_partition_filesystem_as` where nothing determines a
//! namespace and the caller declares the reading. **Both doors compose the
//! same node**, so which one was opened changes nothing about what comes
//! back — only which question was asked of it.
//!
//! The pool is established when the medium is loaded and is evidence from
//! then on, so the doors are lookups rather than probes: a vantage the
//! partition does not have is null with the error outs untouched, and only
//! a composition that was attempted and refused writes them.
//!
//! The handles below name their provider — session and medium, or the
//! record layer a recording's sectors are held behind — rather than
//! holding a borrow, and re-resolve on every call: a medium that has been
//! released answers by name instead of reaching state that has left.

use crate::abi::{
    RemanenceErrorCategory, clear_error, set_error, to_cstring, utf8_arg, write_opt_u32,
    write_opt_u64,
};
use crate::catalog::scheme_spellings;
use crate::flux::c1541::RemanenceC1541Sectors;
use crate::flux::ibm::RemanenceIbmSectors;
use crate::report::{IssueView, RemanenceRegionRole, evidence_views, issue_view};
use crate::session::{RemanenceMedium, RemanenceSession};
use crate::storage::space::RemanenceSpace;
use remanence::{
    MediaId, Partition, PartitionScheme, PartitionType, PartitionView, RegionRole, VolumeId,
};
use std::ffi::{CString, c_char};
use std::ptr;

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
pub(crate) fn partition_handle(
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
pub(crate) unsafe fn with_partition<T>(
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
pub(crate) enum Door<'a> {
    /// The addressable vantage the pool records.
    Addressable,
    /// The namespace vantage the pool records.
    Namespace,
    /// The namespace vantage the caller declares, where nothing
    /// determines one (P18).
    Declared(&'a str),
}

/// What a composed space carries across the boundary.
pub(crate) struct SpaceFacts {
    volume: Option<u64>,
    addressable: bool,
    start_bytes: u64,
    length_bytes: u64,
    kind: Option<CString>,
}

pub(crate) fn space_facts(space: &remanence::StorageSpace<'_>) -> SpaceFacts {
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
pub(crate) fn open_door(
    view: PartitionView<'_>,
    door: Door<'_>,
) -> remanence::Result<Option<SpaceFacts>> {
    Ok(match door {
        Door::Addressable => view.volume().map(|space| space_facts(&space)),
        Door::Namespace => view.filesystem().map(|space| space_facts(&space)),
        Door::Declared(id) => Some(space_facts(&view.filesystem_as(id)?)),
    })
}

pub(crate) unsafe fn partition_space(
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

/// Records a published DOS layout onto the blank article this partition
/// is composed over — the **authored-to-recorded arc**.
///
/// This is what `FORMAT` does to a new disk: the boot record with its
/// parameter block, the FAT copies with their media descriptor, and an
/// empty root directory — precisely the layout named, and nothing chosen
/// on the caller's behalf. Afterwards the medium is a recording:
/// `remanence_medium_geometry` answers the layout's coordinates,
/// `remanence_medium_device_type` answers the drive it is recorded for so
/// a drive takes it, and `remanence_partition_filesystem` opens FAT12
/// over it by the evidence of the boot record just written.
///
/// `layout` is one of the spellings `remanence_recording_id` enumerates;
/// any other is refused naming what this release records. It refuses by
/// name on a medium loaded from an artifact, on an authored medium whose
/// coordinates its author stated, on one already recorded onto, and where
/// the layout's article is not the article this medium is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_partition_record_as(
    partition: *const RemanencePartition,
    layout: *const c_char,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> bool {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { partition.as_ref() }) else {
        return false;
    };
    let Some(layout) = (unsafe { utf8_arg(layout) }) else {
        let error = remanence::Error::io("null layout");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return false;
    };
    let declared = match remanence::Recording::declared(layout.as_ref()) {
        Ok(declared) => declared,
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            return false;
        }
    };
    match unsafe { with_partition(handle, |view| view.record_as(declared)) } {
        Ok(()) => {
            // The medium bound a device type in that act, and a view
            // minted before it caches the absence. Restate it, so a
            // handle the caller already holds answers what the medium
            // now says.
            if let Some(media) = handle.media {
                unsafe { crate::session::medium_view(handle.session, media) };
            }
            true
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            false
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
