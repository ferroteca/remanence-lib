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

    /// The uniform geometry these records state for themselves, or the
    /// refusal naming what makes them non-uniform (D62).
    ///
    /// Every number is read off the claims rather than off the drive
    /// profile: a profile declares what the mechanism records, and this
    /// says what this disk holds.
    pub fn geometry(&self) -> Result<crate::flux::ibm::geometry::IbmGeometry> {
        crate::flux::ibm::geometry::geometry_of(self)
    }

    /// The **direct partition** over this recording — the library's own
    /// composition of the whole content, which is what a namespace above
    /// is reached through (P19).
    ///
    /// **Unlike a CBM DOS recording's, this partition is addressable**
    /// (D62). Its records state a cylinder, a head and a sector number,
    /// and those compose exactly the geometry ordering FAT, HDOS and
    /// CP/M were all written against — so a volume here opens through
    /// the same seam a hard-disk image opens through, with no flux
    /// vocabulary reaching the filesystem adapter and none of the
    /// filesystem's reaching the recording.
    ///
    /// The extent's length is the geometry's rather than the sum of what
    /// reads: a record the recording never stated, or one whose CRC
    /// disagrees, is a hole that still occupies its place. Reads that
    /// touch it are refused naming the address, and every other read
    /// answers — nothing is zeroed, because a zero here would be
    /// indistinguishable from one the recording holds.
    ///
    /// Refuses where the records compose no uniform image, which is
    /// [`IbmSectors::geometry`]'s refusal passed out unchanged.
    pub fn partition(&self) -> Result<IbmPartition<'_>> {
        Ok(IbmPartition {
            extent: crate::flux::ibm::geometry::IbmExtent::new(self)?,
        })
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

/// The addressed extent an FM or MFM recording composes, held so the
/// partition over it can borrow it mutably (D62).
///
/// It exists because the adapters read through a device by exclusive
/// reference while the sector layer is shared: this owns the device and
/// hands out the one borrow of it.
pub struct IbmPartition<'a> {
    extent: crate::flux::ibm::geometry::IbmExtent<'a>,
}

impl IbmPartition<'_> {
    /// The geometry the extent is ordered by.
    pub fn geometry(&self) -> crate::flux::ibm::geometry::IbmGeometry {
        self.extent.geometry()
    }

    /// The partition itself, and the namespace door onto it.
    pub fn view(&mut self) -> crate::PartitionView<'_> {
        let length = self.extent.geometry().length_bytes();
        crate::PartitionView::over_device(&mut self.extent, length)
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
    pub(super) const CELL_CYCLES: u64 = 32;
    pub(super) const CYCLES_PER_ROTATION: u64 = 3_200_000;

    pub(super) struct Bytes(pub(super) Vec<u8>);

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

#[cfg(test)]
mod reach {
    use super::tests::{Bytes, CELL_CYCLES, CYCLES_PER_ROTATION};
    use super::*;
    use crate::evidence::Provenance;
    use crate::flux::capture::TimeBase;
    use crate::flux::ibm::encoding::Encoding;
    use crate::flux::ibm::profiles::HEATH_H17_4_SOFT;
    use crate::flux::ibm::records::SectorAddress;
    use crate::flux::ibm::records::writing::TrackWriter;
    use crate::flux::medium::{
        Derivation, FluxMedium, LocationKey, MediumBuilder, OriginRule, OriginStatement, Pulse,
        RotationalFrame, Strength,
    };
    use crate::flux::presentation::materialize_bitstream;

    const CYLINDERS: u8 = 4;
    const HEADS: u8 = 2;
    const SECTORS: u8 = 9;
    const SECTOR_BYTES: usize = 512;
    /// 512 bytes is `128 << 2`.
    const SIZE_CODE: u8 = 2;

    const TOTAL_SECTORS: usize = CYLINDERS as usize * HEADS as usize * SECTORS as usize;

    const FILE_NAME: &str = "HELLO.TXT";
    const FILE_BODY: &[u8] = b"a filesystem on a flux recording\r\n";

    /// A FAT12 floppy image with one file in it, laid out for the
    /// geometry above.
    ///
    /// It is built here rather than loaded because the point of the test
    /// is the path from cells to files: a fixture would prove the FAT
    /// reader works, which is already proved elsewhere, and not that this
    /// recording reaches it.
    fn fat12_image() -> Vec<u8> {
        const RESERVED: usize = 1;
        const FATS: usize = 2;
        const SECTORS_PER_FAT: usize = 1;
        const ROOT_ENTRIES: usize = 16;

        let mut image = vec![0u8; TOTAL_SECTORS * SECTOR_BYTES];

        // The boot record, which is what the reader recognizes.
        image[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
        image[3..11].copy_from_slice(b"REMANENC");
        image[11..13].copy_from_slice(&(SECTOR_BYTES as u16).to_le_bytes());
        image[13] = 1; // one sector to a cluster
        image[14..16].copy_from_slice(&(RESERVED as u16).to_le_bytes());
        image[16] = FATS as u8;
        image[17..19].copy_from_slice(&(ROOT_ENTRIES as u16).to_le_bytes());
        image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
        image[21] = 0xf9;
        image[22..24].copy_from_slice(&(SECTORS_PER_FAT as u16).to_le_bytes());
        image[24..26].copy_from_slice(&u16::from(SECTORS).to_le_bytes());
        image[26..28].copy_from_slice(&u16::from(HEADS).to_le_bytes());
        image[510] = 0x55;
        image[511] = 0xaa;

        // Both copies of the FAT. The file is one cluster long, so
        // cluster 2 is the end of its own chain.
        for fat in 0..FATS {
            let base = (RESERVED + fat * SECTORS_PER_FAT) * SECTOR_BYTES;
            image[base] = 0xf9;
            image[base + 1] = 0xff;
            image[base + 2] = 0xff;
            // Cluster 2's 12-bit entry is the low nibble of byte 4 and
            // all of byte 3: 0xfff ends the chain.
            image[base + 3] = 0xff;
            image[base + 4] = 0x0f;
        }

        // The root directory, and the one file in it.
        let root = (RESERVED + FATS * SECTORS_PER_FAT) * SECTOR_BYTES;
        image[root..root + 8].copy_from_slice(b"HELLO   ");
        image[root + 8..root + 11].copy_from_slice(b"TXT");
        image[root + 11] = 0x20;
        image[root + 26..root + 28].copy_from_slice(&2u16.to_le_bytes());
        image[root + 28..root + 32].copy_from_slice(&(FILE_BODY.len() as u32).to_le_bytes());

        // The file's one cluster. Cluster 2 is the first data cluster.
        let data = (RESERVED + FATS * SECTORS_PER_FAT + 1) * SECTOR_BYTES;
        image[data..data + FILE_BODY.len()].copy_from_slice(FILE_BODY);

        image
    }

    /// Where one record's data field sits in the linear image, under the
    /// same ordering the extent composes.
    fn image_offset(cylinder: u8, head: u8, sector: u8) -> usize {
        let block = (usize::from(cylinder) * usize::from(HEADS) + usize::from(head))
            * usize::from(SECTORS)
            + usize::from(sector - 1);
        block * SECTOR_BYTES
    }

    /// The whole image written out as MFM cells, one track per location.
    fn recording(image: &[u8], damage: Option<(u8, u8, u8)>) -> Vec<(LocationKey, Vec<bool>)> {
        let mut tracks = Vec::new();
        for cylinder in 0..CYLINDERS {
            for head in 0..HEADS {
                let mut track = TrackWriter::new(Encoding::Mfm);
                for sector in 1..=SECTORS {
                    let at = image_offset(cylinder, head, sector);
                    track.sector(
                        SectorAddress {
                            cylinder,
                            head,
                            sector,
                            size_code: SIZE_CODE,
                        },
                        &image[at..at + SECTOR_BYTES],
                        false,
                    );
                }
                let mut bits = track.bits.clone();
                if damage == Some((cylinder, head, 0)) {
                    // Break one data cell in the first record of this
                    // track. MFM writes a clock cell before every data
                    // cell, so the data cells are the odd ones.
                    let flip = 701;
                    bits[flip] = !bits[flip];
                }
                tracks.push((
                    LocationKey::new(HEATH_H17_4_SOFT.id, u64::from(cylinder), u64::from(head)),
                    bits,
                ));
            }
        }
        tracks
    }

    fn medium_of(tracks: &[(LocationKey, Vec<bool>)]) -> FluxMedium {
        let profile = &HEATH_H17_4_SOFT;
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
        for (location, bits) in tracks {
            let mut pulses = Vec::new();
            let mut at = 0u64;
            for bit in bits {
                at += CELL_CYCLES;
                if *bit {
                    pulses.push(Pulse::new(at, Strength::certain(2)));
                }
            }
            builder
                .add_location(
                    location.clone(),
                    &pulses,
                    &[],
                    Provenance::new(profile.id).note("reduced for the test"),
                )
                .expect("the location");
        }
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 22);
        medium
    }

    fn sectors_of(medium: &FluxMedium) -> IbmSectors {
        let profile = &HEATH_H17_4_SOFT;
        let bitstream = materialize_bitstream(
            medium,
            profile,
            profile.presentation.channel_policy,
            1 << 22,
        )
        .expect("the channel clocks it");
        let bytestream = bitstream
            .materialize_bytestream(1 << 22)
            .expect("the codec resolves it");
        recognize_declared(&bytestream, 1 << 22)
            .expect("the grammar reads it")
            .into_ibm()
            .expect("an IBM recording answers the IBM reading")
    }

    #[test]
    fn the_geometry_is_read_off_the_records_rather_than_off_the_profile() {
        let image = fat12_image();
        let medium = medium_of(&recording(&image, None));
        let sectors = sectors_of(&medium);

        let geometry = sectors.geometry().expect("the records compose one image");
        assert_eq!(geometry.cylinders, u32::from(CYLINDERS));
        assert_eq!(geometry.heads, u32::from(HEADS));
        assert_eq!(geometry.sectors_per_track, u32::from(SECTORS));
        assert_eq!(geometry.first_sector, 1);
        assert_eq!(geometry.sector_bytes, SECTOR_BYTES as u32);
        assert_eq!(geometry.length_bytes(), image.len() as u64);

        // The profile declares sixteen 256-byte records to a location.
        // None of that is what this disk holds, and the geometry is the
        // disk's rather than the profile's.
        assert_eq!(HEATH_H17_4_SOFT.density[0].records, 16);
        assert_ne!(
            geometry.sectors_per_track,
            HEATH_H17_4_SOFT.density[0].records
        );
    }

    #[test]
    fn the_extent_is_the_image_the_records_were_written_from() {
        let image = fat12_image();
        let medium = medium_of(&recording(&image, None));
        let sectors = sectors_of(&medium);
        let mut partition = sectors.partition().expect("the records compose an extent");

        let mut read = vec![0u8; image.len()];
        crate::io::device::Device::read_at(&mut partition.extent, 0, &mut read)
            .expect("every record reads");
        assert_eq!(read, image, "the extent is the image, byte for byte");
    }

    #[test]
    fn a_fat_volume_on_an_mfm_recording_opens_through_the_ordinary_seam() {
        let image = fat12_image();
        let medium = medium_of(&recording(&image, None));
        let sectors = sectors_of(&medium);
        let mut partition = sectors.partition().expect("the records compose an extent");

        // This is the payoff: no flux vocabulary crosses into the
        // filesystem seam, and none of the filesystem's crosses back.
        let mut space = partition
            .view()
            .filesystem_as("fat")
            .expect("a FAT volume is declared and the content bears it");

        let entries = space.entries("/").expect("the root directory lists");
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].name, FILE_NAME);
        assert_eq!(entries[0].size_bytes, FILE_BODY.len() as u64);

        let body = space.read_file(FILE_NAME).expect("the file reads");
        assert_eq!(body, FILE_BODY, "the contents are what was recorded");
    }

    #[test]
    fn a_hole_refuses_the_reads_that_touch_it_and_no_others() {
        let image = fat12_image();
        // Damage the first record of the last track, which holds no
        // part of the boot record, the FAT, the root directory or the
        // file.
        let damaged = (CYLINDERS - 1, HEADS - 1, 0u8);
        let medium = medium_of(&recording(&image, Some(damaged)));
        let sectors = sectors_of(&medium);

        // The extent still composes at its full length: a hole occupies
        // its place rather than shortening the image.
        let mut partition = sectors
            .partition()
            .expect("the records still compose an extent");
        assert_eq!(
            partition.geometry().length_bytes(),
            image.len() as u64,
            "the length is the geometry's, not the sum of what reads"
        );

        // The read that touches the hole is refused, naming the address.
        let at = image_offset(damaged.0, damaged.1, 1) as u64;
        let mut buf = [0u8; 16];
        let error = crate::io::device::Device::read_at(&mut partition.extent, at, &mut buf)
            .expect_err("the damaged record does not read");
        let said = error.to_string();
        assert!(said.contains("not readable"), "{said}");
        assert!(
            buf.iter().all(|byte| *byte == 0),
            "nothing was filled in before the refusal"
        );

        // And every other read still answers — including the whole
        // filesystem, which lives nowhere near the damage.
        let mut space = partition
            .view()
            .filesystem_as("fat")
            .expect("the volume still opens");
        let entries = space.entries("/").expect("the root directory still lists");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            space.read_file(FILE_NAME).expect("the file still reads"),
            FILE_BODY
        );
    }

    #[test]
    fn the_reading_is_declared_and_the_wrong_family_is_refused_by_name() {
        let image = fat12_image();
        let medium = medium_of(&recording(&image, None));
        let sectors = sectors_of(&medium);
        let mut partition = sectors.partition().expect("the records compose an extent");

        // CBM DOS addresses its blocks by the recording rather than by
        // position, so it is not a reading of an addressed extent. The
        // refusal says which, rather than reading 256-byte blocks out of
        // 512-byte sectors and calling the result a directory.
        let error = partition
            .view()
            .filesystem_as("cbmdos")
            .expect_err("a CBM DOS reading of an IBM recording is refused");
        let said = error.to_string();
        assert!(
            said.contains("addresses its blocks by the recording"),
            "{said}"
        );

        // A declaration this release does not read is refused naming
        // what it does.
        let error = partition
            .view()
            .filesystem_as("ext2")
            .expect_err("an unclaimed namespace is refused");
        assert!(error.to_string().contains("fat"), "{error}");

        // And a claimed one reaches its own adapter, which reads the
        // evidence to verify the declaration rather than to pick one:
        // this content is FAT, so the HDOS adapter refuses it — and that
        // it refuses at all is the proof the adapter ran over the
        // recording's extent.
        let refused = partition.view().filesystem_as("hdos");
        assert!(
            refused.is_err(),
            "the HDOS adapter read FAT content without objecting"
        );
    }

    #[test]
    fn a_recording_of_more_than_one_data_field_size_composes_no_extent() {
        let image = fat12_image();
        let mut tracks = recording(&image, None);

        // Re-record one track with a different size code. A linear
        // extent has one block size, and this is what makes it not one.
        let mut odd = TrackWriter::new(Encoding::Mfm);
        odd.sector(
            SectorAddress {
                cylinder: 0,
                head: 0,
                sector: 1,
                size_code: 1,
            },
            &[0u8; 256],
            false,
        );
        tracks[0].1 = odd.bits.clone();

        let medium = medium_of(&tracks);
        let sectors = sectors_of(&medium);
        let error = sectors
            .geometry()
            .expect_err("the records state two data-field sizes");
        let said = error.to_string();
        assert!(said.contains("more than one data-field size"), "{said}");
        assert!(
            sectors.partition().is_err(),
            "and no extent composes over them"
        );
    }
}
