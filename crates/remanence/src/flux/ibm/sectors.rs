// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The IBM record rung (F78): the records a recording states, reached
//! from a loaded medium.
//!
//! **It reads the bytes and the marks together, and it must.** A data
//! field whose payload happens to hold `A1 A1 A1 FE` is not a field
//! boundary — at the byte tier nothing distinguishes it from one, and
//! the thing that does is the deliberate clock violation, which the
//! layer below recorded as an alignment fact when it framed there. So
//! this reads each location's bytes *and* where framing began, and a
//! byte that merely looks like a mark opens nothing.
//!
//! That is the one place this family's rung differs in kind from the
//! 1541's, which finds its own block marks among ordinary bytes because
//! its group code has patterns that cannot be data.

use crate::error::{Error, ErrorCategory, Result};
use crate::evidence::{DeclaredLoss, LossAccount};
use crate::flux::bytestream::{ByteRecord, BytestreamFactKind};
use crate::flux::ibm::encoding::Encoding;
use crate::flux::ibm::encoding::MFM_A1;
use crate::flux::ibm::records::{DAM, DELETED_DAM, IDAM, MFM_SYNC_MARKS, checksum};
use crate::flux::presentation::{Bytestream, Sectors, refuse};

/// One record as this family states it.
///
/// It is the IBM vocabulary and only that: a cylinder, a head, a sector
/// and a size code, with sixteen-bit checks. A CBM DOS claim carries a
/// track, a sector, two disk-identity bytes and an exclusive-or, and
/// neither set is the other with its fields renamed — which is why the
/// rung above holds whichever the family made rather than one struct
/// wide enough for both and empty in half its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IbmSectorClaim {
    /// The location of the family's own addressing this record sits on.
    pub location: u64,
    pub surface: Option<u64>,
    /// The byte of that location's own bytestream the id field's mark
    /// sits at.
    pub at_byte: u64,
    /// The address the id field states for itself, as recorded.
    pub cylinder: u8,
    pub head: u8,
    pub sector: u8,
    /// The size code as recorded; the field it declares is `128 << code`
    /// bytes, and the code is kept because it is what was written.
    pub size_code: u8,
    /// The check the id field states, and the one its own bytes compute.
    pub header_checksum_stated: u16,
    pub header_checksum_computed: u16,
    /// Whether a data field followed the id field, and where it sat.
    pub has_data: bool,
    pub data_at_byte: u64,
    /// Whether the mark opening the data field was the deleted-data one.
    /// It is what the recording says, carried rather than judged.
    pub data_deleted: bool,
    pub data_checksum_stated: u16,
    pub data_checksum_computed: u16,
}

impl IbmSectorClaim {
    /// Whether both fields' checks agree with their own bytes.
    pub fn readable(&self) -> bool {
        self.header_checksum_stated == self.header_checksum_computed
            && self.has_data
            && self.data_checksum_stated == self.data_checksum_computed
    }

    /// The bytes the size code declares this record's data field holds.
    pub fn declared_bytes(&self) -> u64 {
        128u64 << self.size_code.min(7)
    }
}

/// What one IBM recognition produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IbmSectorReport {
    pub profile_id: String,
    pub encoding_id: String,
    pub claims: Vec<IbmSectorClaim>,
    pub declared_loss: Vec<DeclaredLoss>,
    pub evidence: Vec<String>,
}

/// The records an FM or MFM recording states, held in the session.
#[derive(Debug)]
pub struct IbmSectors {
    report: IbmSectorReport,
    payloads: Vec<Vec<u8>>,
}

impl IbmSectors {
    /// The recognition that produced these records, with every claim's
    /// evidence beside it.
    pub fn inspect(&self) -> &IbmSectorReport {
        &self.report
    }

    pub fn claim_count(&self) -> u64 {
        self.report.claims.len() as u64
    }

    /// One record's payload, by the address the recording states.
    ///
    /// Only a record whose checks both agree is served. One whose
    /// checksum disagrees holds what it holds and is reported with both
    /// numbers; serving it as though it read cleanly would answer a
    /// question the evidence does not.
    pub fn read_sector(&self, cylinder: u8, head: u8, sector: u8) -> Result<Vec<u8>> {
        let (at, claim) = self
            .report
            .claims
            .iter()
            .enumerate()
            .find(|(_, claim)| {
                claim.cylinder == cylinder && claim.head == head && claim.sector == sector
            })
            .ok_or_else(|| {
                Error::categorized_image(
                    ErrorCategory::NotFound,
                    "ibm",
                    format!(
                        "no record of this recording states cylinder {cylinder}, head \
                         {head}, sector {sector}"
                    ),
                )
            })?;
        if !claim.readable() {
            return Err(Error::categorized_image(
                ErrorCategory::Unavailable,
                "ibm",
                format!(
                    "cylinder {cylinder}, head {head}, sector {sector} is stated and not \
                     readable: its id check is {:#06x} against {:#06x} computed, and its \
                     data check {:#06x} against {:#06x}; nothing is repaired and nothing \
                     is filled in",
                    claim.header_checksum_stated,
                    claim.header_checksum_computed,
                    claim.data_checksum_stated,
                    claim.data_checksum_computed
                ),
            ));
        }
        Ok(self.payloads[at].clone())
    }
}

/// The family's declared recognition, as the profile enrols it.
pub(crate) fn recognize_declared(bytestream: &Bytestream, _cache_bytes: u64) -> Result<Sectors> {
    let profile = bytestream.profile();
    let codec = crate::flux::ibm::profiles::codec_of(profile).ok_or_else(|| {
        refuse(
            profile.id,
            "this profile enrols the IBM record grammar and declares no FM or MFM \
             encoding for it to read with",
        )
    })?;
    let sync = match codec.encoding {
        Encoding::Mfm => MFM_SYNC_MARKS,
        Encoding::Fm => 1,
    };
    let covered = |kind: u8| -> Vec<u8> {
        match codec.encoding {
            Encoding::Mfm => vec![MFM_A1, MFM_A1, MFM_A1, kind],
            Encoding::Fm => vec![kind],
        }
    };

    let inner = bytestream.inner();
    let mut claims = Vec::new();
    let mut payloads = Vec::new();
    let mut loss = LossAccount::new();
    let mut unreadable = 0u64;
    let mut deleted = 0u64;
    let mut unpaired = 0u64;

    for location in inner.locations() {
        let key = location.key();
        let (numerator, denominator) = key.position();
        let at_location = if denominator == 1 { numerator } else { 0 };

        let records = inner.bytes(location)?;
        let bytes: Vec<Option<u8>> = records.iter().map(ByteRecord::value).collect();

        // Where the layer below framed. A mark is one byte, so the
        // fact's bit position is the byte whose record opens there.
        let mut marks: Vec<usize> = Vec::new();
        for fact in inner.facts(location)? {
            if let BytestreamFactKind::Alignment { at_bit, .. } = fact.kind() {
                if let Some(at) = records.iter().position(|record| record.at_bit() == *at_bit) {
                    marks.push(at);
                }
            }
        }
        marks.sort_unstable();
        marks.dedup();

        // A field is introduced by a run of marks; the byte after the
        // run says which field it is.
        let mut fields: Vec<(usize, u8)> = Vec::new();
        let mut index = 0usize;
        while index < marks.len() {
            let start = marks[index];
            let mut run = 1usize;
            while index + run < marks.len() && marks[index + run] == start + run {
                run += 1;
            }
            index += run;
            if run < sync {
                continue;
            }
            let kind_at = start + sync;
            if let Some(Some(kind)) = bytes.get(kind_at).copied() {
                fields.push((kind_at, kind));
            }
        }

        for (position, (kind_at, kind)) in fields.iter().enumerate() {
            if *kind != IDAM {
                continue;
            }
            let read = |from: usize, count: usize| -> Option<Vec<u8>> {
                let mut out = Vec::with_capacity(count);
                for at in from..from + count {
                    out.push((*bytes.get(at)?)?);
                }
                Some(out)
            };
            let (Some(address), Some(stored)) = (read(kind_at + 1, 4), read(kind_at + 5, 2)) else {
                continue;
            };
            let size_code = address[3];
            let payload_bytes = 128usize << size_code.min(7);

            let mut has_data = false;
            let mut data_at = 0u64;
            let mut data_deleted = false;
            let mut data_stated = 0u16;
            let mut data_computed = 0u16;
            let mut payload = Vec::new();
            match fields.get(position + 1) {
                Some((next_at, next_kind)) if *next_kind == DAM || *next_kind == DELETED_DAM => {
                    if let (Some(field), Some(check)) = (
                        read(next_at + 1, payload_bytes),
                        read(next_at + 1 + payload_bytes, 2),
                    ) {
                        has_data = true;
                        data_at = *next_at as u64;
                        data_deleted = *next_kind == DELETED_DAM;
                        data_stated = u16::from_be_bytes([check[0], check[1]]);
                        data_computed = checksum(&covered(*next_kind), &field);
                        payload = field;
                    }
                }
                _ => unpaired += 1,
            }

            let claim = IbmSectorClaim {
                location: at_location,
                surface: key.surface(),
                at_byte: *kind_at as u64,
                cylinder: address[0],
                head: address[1],
                sector: address[2],
                size_code,
                header_checksum_stated: u16::from_be_bytes([stored[0], stored[1]]),
                header_checksum_computed: checksum(&covered(IDAM), &address),
                has_data,
                data_at_byte: data_at,
                data_deleted,
                data_checksum_stated: data_stated,
                data_checksum_computed: data_computed,
            };
            if data_deleted {
                deleted += 1;
            }
            if !claim.readable() {
                unreadable += 1;
            }
            claims.push(claim);
            payloads.push(payload);
        }
    }

    if unreadable > 0 {
        loss.add(
            "unreadable-record",
            "records the recording states whose own checks disagree with their bytes, \
             kept with both numbers because what is there is a fact",
            unreadable,
        );
    }
    if unpaired > 0 {
        loss.add(
            "unpaired-record",
            "id fields the recording does not follow with a data field, which hold no \
             data and say so",
            unpaired,
        );
    }
    if deleted > 0 {
        loss.add(
            "deleted-data-mark",
            "records whose data field was opened by a deleted-data mark, which is what \
             the recording states and not a judgement about whether they count",
            deleted,
        );
    }

    let report = IbmSectorReport {
        profile_id: profile.id.to_owned(),
        encoding_id: codec.id.to_owned(),
        evidence: vec![
            format!(
                "{} record(s) recognized under {}, framed at the marks the layer below \
                 located rather than at bytes that merely read like them",
                claims.len(),
                codec.name
            ),
            "every claim carries the address the recording states for itself and both \
             checks, stated beside computed; nothing is repaired and nothing is filled"
                .to_owned(),
        ],
        claims,
        declared_loss: loss.into_entries(),
    };
    Ok(Sectors::ibm_set(profile, IbmSectors { report, payloads }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::Provenance;
    use crate::flux::capture::ByteSource;
    use crate::flux::capture::TimeBase;
    use crate::flux::ibm::encoding::Encoding;
    use crate::flux::ibm::profiles::HEATH_H17_4_SOFT;
    use crate::flux::ibm::records::SectorAddress;
    use crate::flux::ibm::records::writing::TrackWriter;
    use crate::flux::medium::LocationKey;
    use crate::flux::medium::{
        Derivation, FluxMedium, MediumBuilder, OriginRule, OriginStatement, Pulse, RotationalFrame,
        Strength,
    };
    use crate::flux::presentation::materialize_bitstream;

    /// The H-17-4 records half a million cells a second against a
    /// sixteen-megahertz clock, so a cell is thirty-two cycles.
    const CELL_CYCLES: u64 = 32;
    const CYCLES_PER_ROTATION: u64 = 3_200_000;

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    fn address(cylinder: u8, head: u8, sector: u8) -> SectorAddress {
        SectorAddress {
            cylinder,
            head,
            sector,
            size_code: 1,
        }
    }

    /// A medium holding one location whose pulses are `bits` laid down
    /// at the family's declared cell.
    fn medium_of(location: LocationKey, bits: &[bool]) -> FluxMedium {
        let profile = &HEATH_H17_4_SOFT;
        let mut pulses = Vec::new();
        let mut at = 0u64;
        for bit in bits {
            at += CELL_CYCLES;
            if *bit {
                pulses.push(Pulse::new(at, Strength::certain(2)));
            }
        }
        let frame = RotationalFrame::new(
            profile.id,
            TimeBase::new(profile.id, 16_000_000, 1).expect("the rate is stated"),
            CYCLES_PER_ROTATION,
            OriginStatement::new(
                OriginRule::Index,
                Provenance::new(profile.id).note("placed at the index for the test"),
            ),
        )
        .expect("the frame states a circle");
        let mut builder = MediumBuilder::new(
            profile.id,
            profile.media,
            frame,
            Derivation::SelectedAndProjected,
            Provenance::new(profile.id).note("selected observation 0 of each location"),
            Vec::new(),
        )
        .expect("the policy is stated");
        builder
            .add_location(
                location,
                &pulses,
                // The circle already begins at the index, stated in the
                // frame's own origin rule, so this location carries no
                // fact of its own.
                &[],
                Provenance::new(profile.id).note("reduced for the test"),
            )
            .expect("the location");
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
        medium
    }

    /// The whole ladder, from pulses to the records this rung claims.
    fn read(location: LocationKey, bits: &[bool]) -> Result<Sectors> {
        let profile = &HEATH_H17_4_SOFT;
        let medium = medium_of(location, bits);
        let bitstream = materialize_bitstream(
            &medium,
            profile,
            profile.presentation.channel_policy,
            1 << 20,
        )
        .expect("the channel clocks it");
        let bytestream = bitstream
            .materialize_bytestream(1 << 20)
            .expect("the codec resolves it");
        recognize_declared(&bytestream, 1 << 20)
    }

    fn track_of(sectors: &[(u8, Vec<u8>)]) -> Vec<bool> {
        let mut track = TrackWriter::new(Encoding::Mfm);
        for (sector, data) in sectors {
            track.sector(address(0, 0, *sector), data, false);
        }
        track.bits.clone()
    }

    #[test]
    fn a_recorded_sector_is_read_by_the_address_it_states_for_itself() {
        let location = LocationKey::new(HEATH_H17_4_SOFT.id, 0, 0);
        let first = vec![0x10u8; 256];
        let second = vec![0x80u8; 256];
        let bits = track_of(&[(1, first.clone()), (2, second.clone())]);
        let sectors = read(location, &bits).expect("the grammar reads it");

        assert_eq!(sectors.family(), "ibm-id-data-record");
        assert_eq!(sectors.claim_count(), 2);

        let reading = sectors
            .ibm()
            .expect("an IBM recording answers the IBM reading");
        let report = reading.inspect();
        assert_eq!(report.profile_id, "heath-h17-4-soft");
        assert_eq!(report.encoding_id, "ibm-mfm");
        assert_eq!(report.claims.len(), 2);

        // Read by the address the record states, not by where it sits.
        assert_eq!(reading.read_sector(0, 0, 1).expect("sector 1"), first);
        assert_eq!(reading.read_sector(0, 0, 2).expect("sector 2"), second);

        // Both checks agree, stated beside computed.
        for claim in &report.claims {
            assert_eq!(claim.header_checksum_stated, claim.header_checksum_computed);
            assert_eq!(claim.data_checksum_stated, claim.data_checksum_computed);
            assert!(claim.has_data);
            assert!(!claim.data_deleted);
            assert_eq!(claim.size_code, 1);
        }
    }

    #[test]
    fn an_address_the_recording_never_stated_is_refused_rather_than_guessed() {
        let location = LocationKey::new(HEATH_H17_4_SOFT.id, 0, 0);
        let bits = track_of(&[(1, vec![0x10; 256])]);
        let sectors = read(location, &bits).expect("the grammar reads it");
        let reading = sectors.ibm().expect("the IBM reading");

        let error = reading
            .read_sector(0, 0, 9)
            .expect_err("no record of this track states sector 9");
        assert!(
            error.to_string().contains("sector 9"),
            "the refusal names what was asked: {error}"
        );
    }

    #[test]
    fn a_recording_of_one_family_holds_no_reading_of_another() {
        let location = LocationKey::new(HEATH_H17_4_SOFT.id, 0, 0);
        let bits = track_of(&[(1, vec![0x10; 256])]);
        let sectors = read(location, &bits).expect("the grammar reads it");

        // The rung is shared and the claims are the family's. Asking an
        // IBM recording for the CBM DOS reading answers the honest
        // absence rather than a bent claim, because the two vocabularies
        // state different addresses under different checks.
        assert!(sectors.c1541().is_none());
        assert!(sectors.ibm().is_some());
        assert!(sectors.into_c1541().is_none());
    }

    #[test]
    fn a_corrupted_data_field_is_reported_with_both_numbers_and_never_served() {
        let location = LocationKey::new(HEATH_H17_4_SOFT.id, 0, 0);
        let mut bits = track_of(&[(1, vec![0x10; 256]), (2, vec![0x80; 256])]);

        // Flip one data cell inside the first sector's payload. MFM
        // writes a clock cell before every data cell, so the data cells
        // are the odd ones; flipping an even one decodes the same byte
        // and would prove nothing.
        let flip = 701;
        bits[flip] = !bits[flip];

        let sectors = read(location, &bits).expect("the grammar still reads the track");
        let reading = sectors.ibm().expect("the IBM reading");
        let report = reading.inspect();

        // The damaged record is still claimed — dropping it would hide
        // that the recording states it at all.
        assert_eq!(report.claims.len(), 2);
        let damaged = report
            .claims
            .iter()
            .find(|claim| !claim.readable())
            .expect("one record's checks disagree");

        let error = reading
            .read_sector(damaged.cylinder, damaged.head, damaged.sector)
            .expect_err("a record whose checks disagree is not served");
        assert!(
            error.to_string().contains("not readable"),
            "the refusal says why: {error}"
        );

        // And the loss is declared rather than silently absorbed.
        assert!(
            !report.declared_loss.is_empty(),
            "a record that does not read is declared loss"
        );
    }
}
