// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The IBM sector layer: the recording's records recognized above the
//! encoded bytestream under the IBM grammar, with the geometry they imply
//! and the claims that carry their evidence.

use crate::abi::{RemanenceErrorCategory, clear_error, set_error};
use crate::flux::stream::{LayerView, RemanenceBytestream};
use crate::storage::partition::{RemanencePartition, partition_handle};
use std::ffi::c_char;
use std::ptr;

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
    pub(crate) sectors: remanence::IbmSectors,
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
