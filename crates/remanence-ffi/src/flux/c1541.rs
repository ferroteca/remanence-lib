// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C1541 sector layer: the recording's own records, recognized above
//! the encoded bytestream under the family's declared grammar. This is
//! the seam where the two layers below stop saying nothing about what
//! their bytes mean — and it ends by stating what it derives, with every
//! claim carrying its evidence and every sector that does not read
//! refusing by name.

use crate::abi::{RemanenceErrorCategory, clear_error, set_error, to_cstring};
use crate::flux::stream::{LayerView, RemanenceBytestream};
use crate::storage::partition::{RemanencePartition, partition_handle};
use remanence::C1541Sectors;
use std::ffi::{CString, c_char};
use std::ptr;

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
    pub(crate) sectors: C1541Sectors,
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
