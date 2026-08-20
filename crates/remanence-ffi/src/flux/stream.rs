// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The presentation rungs: the hardware bitstream a declared read channel
//! clocks out of a flux medium, and the encoded bytestream the family's
//! own declared code resolves out of that. Neither names a family — the
//! medium beneath them does, and the rules come from its profile. Neither
//! layer assigns synchronization, headers, sectors or files to what it
//! holds, and there is no way back down.

use crate::abi::{RemanenceErrorCategory, clear_error, set_error, to_cstring};
use crate::flux::image::RemanenceFluxImage;
use crate::session::{RemanenceMedium, RemanenceSession};
use remanence::{Bitstream, Bytestream, Location, MediaId};
use std::ffi::{CString, c_char};
use std::ptr;

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

pub(crate) struct LayerView {
    pub(crate) first: CString,
    pub(crate) second: CString,
    pub(crate) third: CString,
    pub(crate) loss_codes: Vec<CString>,
    pub(crate) loss_details: Vec<CString>,
    pub(crate) evidence: Vec<CString>,
}

impl LayerView {
    pub(crate) fn new(
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
pub(crate) enum BitstreamBacking {
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
pub(crate) enum BytestreamBacking {
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
    pub(crate) fn stream(&self) -> Option<&Bitstream> {
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
    pub(crate) fn stream(&self) -> Option<&Bytestream> {
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
pub(crate) fn bitstream_view(bitstream: &Bitstream) -> LayerView {
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
pub(crate) fn bytestream_view(bytestream: &Bytestream) -> LayerView {
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
