// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! What a medium's reading is worth (P28): the outcome, the condition that
//! produced it, the readable extent, and the access mode it leaves behind.

use crate::abi::{RemanenceAccessMode, access_mode, to_cstring, write_opt_u64};
use crate::report::evidence_views;
use crate::session::RemanenceMedium;
use std::ffi::{CString, c_char};
use std::ptr;

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

pub(crate) fn claim_class(claim: remanence::Claim) -> RemanenceClaim {
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
pub(crate) fn assurance_view(assurance: &remanence::Assurance) -> RemanenceAssurance {
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
