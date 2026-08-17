// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The IBM family's bitstream-to-bytestream transition (F78, P23, P30).
//!
//! **Framing begins at an address mark and nowhere else.** The 1541's
//! codec frames on a run of one-bits and resolves groups through a
//! symbol table; this family has neither. Every cell pattern an FM or
//! MFM recording can write is a legal byte, so a field boundary cannot
//! be a reserved value — it is a byte whose *clock* pattern the encoding
//! forbids, and locating one is the only thing that establishes where a
//! byte starts.
//!
//! So the bytes this produces are the bytes each field carries, from its
//! mark forward. Cells before the first mark are unframed and are stated
//! as such rather than guessed into bytes, exactly as the 1541's are
//! before its first sync.
//!
//! **It assigns nothing above a byte.** Which of these bytes is an
//! address and which is payload is the record layer's reading, one rung
//! up; here a mark says only that a byte begins.

use crate::error::Result;
use crate::evidence::{LossAccount, Provenance};
use crate::flux::bytestream::{
    ByteOutcome, ByteRecord, BytestreamBuilder, BytestreamFact, BytestreamFactKind,
};
use crate::flux::capture::SessionBacking;
use crate::flux::ibm::encoding::{self, Encoding, MFM_A1_CELLS};
use crate::flux::presentation::{
    Bitstream, Bytestream, BytestreamLocation, BytestreamReport, refuse,
};

/// Two cells carry one data bit, in both encodings.
const CELLS_PER_BIT: u32 = 2;

/// The cells one byte occupies.
const CELLS_PER_BYTE: usize = 16;

/// The declarations this family reads, which are its own rather than the
/// shared profile's.
pub(crate) struct IbmCodec {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) encoding: Encoding,
    pub(crate) provenance: &'static str,
}

/// The MFM codec: IBM System 34, framed on the `A1` clock violation.
pub(crate) static MFM: IbmCodec = IbmCodec {
    id: "ibm-mfm",
    name: "IBM System 34 modified frequency modulation",
    encoding: Encoding::Mfm,
    provenance: "declared from the published System 34 conventions: two cells to a data \
                 bit with a clock cell only between two zeroes, and fields introduced by \
                 three A1 bytes recorded with one clock suppressed — a pattern no lawful \
                 encoding of any byte produces",
};

/// The FM codec: IBM System 3740, framed on the marks' own clocks.
pub(crate) static FM: IbmCodec = IbmCodec {
    id: "ibm-fm",
    name: "IBM System 3740 frequency modulation",
    encoding: Encoding::Fm,
    provenance: "declared from the published System 3740 conventions: two cells to a data \
                 bit with a clock cell before every one, and fields introduced by marks \
                 whose own clock cells are deliberately incomplete",
};

/// Where framing begins in one location's cells.
///
/// FM's marks are single bytes and MFM's are a run of three, so what a
/// mark *is* differs; that it is the only thing establishing a byte
/// boundary does not.
fn frames(bits: &[bool], encoding: Encoding) -> Vec<usize> {
    let mut at: Vec<usize> = match encoding {
        Encoding::Mfm => encoding::find_marks(bits, MFM_A1_CELLS),
        Encoding::Fm => {
            use crate::flux::ibm::encoding::{FM_DATA_CELLS, FM_DELETED_CELLS, FM_ID_CELLS};
            let mut found = Vec::new();
            for mark in [FM_ID_CELLS, FM_DATA_CELLS, FM_DELETED_CELLS] {
                found.extend(encoding::find_marks(bits, mark));
            }
            found.sort_unstable();
            found
        }
    };
    at.dedup();
    at
}

/// Materializes the IBM bytestream a bitstream carries.
pub(crate) fn materialize_declared(bitstream: &Bitstream, cache_bytes: u64) -> Result<Bytestream> {
    let profile = bitstream.profile();
    let codec = codec_of(profile).ok_or_else(|| {
        refuse(
            profile.id,
            "this profile enrols the IBM codec and declares no FM or MFM encoding for it \
             to read with",
        )
    })?;

    let described = describe(profile.id, codec, bitstream.inner().provenance());
    let mut builder = BytestreamBuilder::new(
        profile.id,
        codec.id,
        described.clone(),
        SessionBacking::create()?,
    )?;
    let mut loss = LossAccount::new();
    let mut reported = Vec::new();
    let inner = bitstream.inner();

    for location in inner.locations() {
        let key = location.key();
        let (numerator, denominator) = key.position();

        let mut bits: Vec<bool> = Vec::with_capacity(location.cells() as usize);
        let mut unrecorded = 0u64;
        for ordinal in 0..inner.cell_chunks(location) {
            for cell in inner.cell_chunk(location, ordinal)? {
                if !cell.evidence().is_recorded() {
                    unrecorded += 1;
                }
                bits.push(cell.one());
            }
        }

        let marks = frames(&bits, codec.encoding);
        let mut records = Vec::new();
        let mut facts = Vec::new();
        let mut unframed_bits = 0u64;

        // Everything before the first mark is unframed: no byte boundary
        // has been established, so nothing there is a byte.
        if let Some(first) = marks.first() {
            if *first > 0 {
                unframed_bits += *first as u64;
                facts.push(BytestreamFact::new(
                    BytestreamFactKind::Unframed {
                        at_bit: 0,
                        bits: *first as u64,
                    },
                    Provenance::new(profile.id).note(
                        "cells before the first address mark, where no byte boundary has \
                         been established",
                    ),
                ));
            }
        } else {
            unframed_bits += bits.len() as u64;
            facts.push(BytestreamFact::new(
                BytestreamFactKind::Unframed {
                    at_bit: 0,
                    bits: bits.len() as u64,
                },
                Provenance::new(profile.id)
                    .note("this location carries no address mark, so nothing in it is framed"),
            ));
        }

        for (index, start) in marks.iter().enumerate() {
            facts.push(BytestreamFact::new(
                BytestreamFactKind::Alignment {
                    at_bit: *start as u64,
                    run_bits: CELLS_PER_BYTE as u64,
                },
                Provenance::new(profile.id).note(
                    "an address mark: a byte whose clock pattern the encoding forbids, \
                     which is what says a field begins here and nothing about what \
                     follows it",
                ),
            ));

            // Bytes run from the mark to the next mark, or to the end.
            let end = marks.get(index + 1).copied().unwrap_or(bits.len());
            let mut at = *start;
            while at + CELLS_PER_BYTE <= end {
                let cells = encoding::cells_at(&bits, at).expect("bounded above");
                records.push(ByteRecord::new(
                    at as u64,
                    ByteOutcome::Resolved(encoding::decode_byte(cells)),
                ));
                at += CELLS_PER_BYTE;
            }
            if at < end {
                unframed_bits += (end - at) as u64;
                facts.push(BytestreamFact::new(
                    BytestreamFactKind::Unframed {
                        at_bit: at as u64,
                        bits: (end - at) as u64,
                    },
                    Provenance::new(profile.id)
                        .note("cells no whole byte covers, stated rather than padded into one"),
                ));
            }
        }

        if unframed_bits > 0 {
            loss.add(
                "unframed-cells",
                "cells no framed byte covers, which no byte carries",
                unframed_bits,
            );
        }
        if unrecorded > 0 {
            loss.add(
                "resolved-bit-evidence",
                "cells a declared rule resolved rather than the medium recording them, \
                 whose rule a byte does not carry",
                unrecorded,
            );
        }

        reported.push(BytestreamLocation {
            half_track_numerator: numerator,
            half_track_denominator: denominator,
            surface: key.surface(),
            bytes: records.len() as u64,
            resolved_bytes: records.len() as u64,
            // Every cell pattern is a legal byte in these encodings, so
            // there is no such thing as a group the table does not
            // assign. The field is zero as a fact, not as a default.
            unassigned_groups: 0,
            alignments: marks.len() as u64,
            longest_landmark_bits: if marks.is_empty() {
                0
            } else {
                CELLS_PER_BYTE as u64
            },
            unframed_bits,
        });

        builder.add_location(
            key.clone(),
            &records,
            &facts,
            Provenance::new(profile.id).note(format!(
                "framed from {} address mark(s) in the location's {} clocked cells",
                marks.len(),
                bits.len()
            )),
        )?;
    }

    let (mut bytestream, sink, total) = builder.seal()?;
    bytestream.attach_backing(Box::new(sink.into_source()), total, cache_bytes);

    let report = BytestreamReport {
        profile_id: profile.id.to_owned(),
        codec_id: codec.id.to_owned(),
        codec_name: codec.name.to_owned(),
        symbol_bits: CELLS_PER_BIT,
        data_bits: 1,
        symbols_per_byte: 8,
        locations: reported,
        declared_loss: loss.into_entries(),
        evidence: described.notes,
    };
    Ok(Bytestream::new(bytestream, report, profile))
}

/// Which codec a profile enrolling this transition reads with.
fn codec_of(
    profile: &'static crate::flux::drive_profile::DriveProfile,
) -> Option<&'static IbmCodec> {
    crate::flux::ibm::profiles::codec_of(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::flux::capture::{ByteSource, TimeBase};
    use crate::flux::drive_profile::DriveProfile;
    use crate::flux::ibm::encoding::{encode, encode_mark};
    use crate::flux::ibm::profiles::{HEATH_H17_1_SOFT, HEATH_H17_4_SOFT};
    use crate::flux::ibm::records::{SectorAddress, recognize};
    use crate::flux::medium::{
        Derivation, FluxMedium, LocationKey, MediumBuilder, OriginRule, OriginStatement, Pulse,
        RotationalFrame, Strength,
    };
    use crate::flux::presentation::materialize_bitstream;

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    /// The cell this family clocks at, in reference cycles.
    fn cell_of(profile: &DriveProfile) -> u64 {
        let (numerator, denominator) = profile.density[0].nominal_cell(&profile.rotation);
        u64::try_from(numerator / denominator).expect("a whole cell")
    }

    /// A medium whose one location carries `cells`: a set cell is a
    /// transition one cell after the last, which is what a recording
    /// physically is.
    fn medium_of(profile: &'static DriveProfile, cells: &[bool]) -> FluxMedium {
        let cell = cell_of(profile);
        let mut pulses = Vec::new();
        let mut at = 0u64;
        for set in cells {
            at += cell;
            if *set {
                pulses.push(Pulse::new(at, Strength::certain(2)));
            }
        }

        let frame = RotationalFrame::new(
            profile.id,
            TimeBase::new(profile.id, profile.rotation.reference_clock, 1).expect("a rate"),
            profile.rotation.cycles_per_rotation,
            OriginStatement::new(
                OriginRule::Index,
                Provenance::new(profile.id).note("the drive observes index"),
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
                LocationKey::new(profile.id, 0, 0),
                &pulses,
                &[],
                Provenance::new(profile.id).note("written for the test"),
            )
            .expect("the location");
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
        medium
    }

    /// Writes one IBM sector's worth of cells: gap, id field, gap, data
    /// field, each field introduced by the marks its encoding uses.
    fn sector_cells(encoding: Encoding, address: SectorAddress, payload: &[u8]) -> Vec<bool> {
        use crate::checksum::Crc16Ccitt;
        use crate::flux::ibm::encoding::{FM_DATA_CELLS, FM_ID_CELLS, MFM_A1, MFM_A1_CELLS};
        const IDAM: u8 = 0xfe;
        const DAM: u8 = 0xfb;

        let mut cells = Vec::new();
        let mut last = false;
        let write = |bytes: &[u8], cells: &mut Vec<bool>, last: &mut bool| {
            let (bits, end) = encode(encoding, bytes, *last);
            cells.extend(bits);
            *last = end;
        };
        let marks = |kind: u8, cells: &mut Vec<bool>, last: &mut bool| match encoding {
            Encoding::Mfm => {
                for _ in 0..3 {
                    cells.extend(encode_mark(MFM_A1_CELLS));
                }
                *last = MFM_A1 & 1 == 1;
                let (bits, end) = encode(encoding, &[kind], *last);
                cells.extend(bits);
                *last = end;
            }
            Encoding::Fm => {
                cells.extend(encode_mark(if kind == IDAM {
                    FM_ID_CELLS
                } else {
                    FM_DATA_CELLS
                }));
                *last = kind & 1 == 1;
            }
        };
        let covered = |kind: u8| -> Vec<u8> {
            match encoding {
                Encoding::Mfm => vec![MFM_A1, MFM_A1, MFM_A1, kind],
                Encoding::Fm => vec![kind],
            }
        };

        write(&[0x4e; 16], &mut cells, &mut last);
        marks(IDAM, &mut cells, &mut last);
        let id = [
            address.cylinder,
            address.head,
            address.sector,
            address.size_code,
        ];
        write(&id, &mut cells, &mut last);
        let mut crc = Crc16Ccitt::new();
        crc.update(&covered(IDAM));
        crc.update(&id);
        write(&crc.finish().to_be_bytes(), &mut cells, &mut last);

        write(&[0x4e; 16], &mut cells, &mut last);
        marks(DAM, &mut cells, &mut last);
        write(payload, &mut cells, &mut last);
        let mut crc = Crc16Ccitt::new();
        crc.update(&covered(DAM));
        crc.update(payload);
        write(&crc.finish().to_be_bytes(), &mut cells, &mut last);
        cells
    }

    /// The whole ladder, through the code a load actually runs: pulses
    /// clocked into cells by the shared channel, cells framed into bytes
    /// by the family's own enrolled transition.
    fn read_back(profile: &'static DriveProfile, cells: &[bool]) -> Vec<u8> {
        let medium = medium_of(profile, cells);
        let bitstream = materialize_bitstream(
            &medium,
            profile,
            profile.presentation.channel_policy,
            1 << 20,
        )
        .expect("the channel clocks it");
        let bytestream = bitstream
            .materialize_bytestream(1 << 20)
            .expect("the family's own transition frames it");

        let inner = bytestream.inner();
        let held = inner
            .location(&LocationKey::new(profile.id, 0, 0))
            .expect("the location is held");
        inner
            .bytes(held)
            .expect("the bytes read")
            .iter()
            .filter_map(crate::flux::bytestream::ByteRecord::value)
            .collect()
    }

    #[test]
    fn an_fm_recording_survives_the_whole_ladder_and_states_its_sectors() {
        // The single-density family, end to end: a written track becomes
        // pulses, the shared channel clocks them, and the family's own
        // transition frames them at the marks.
        let address = SectorAddress {
            cylinder: 0,
            head: 0,
            sector: 1,
            size_code: 1,
        };
        let payload: Vec<u8> = (0..256).map(|at| (at % 251) as u8).collect();
        let cells = sector_cells(Encoding::Fm, address, &payload);

        let read = read_back(&HEATH_H17_1_SOFT, &cells);
        // Framing began at the id mark, so the first framed byte is the
        // mark itself and the address follows it.
        assert_eq!(read[0], 0xfe, "the id mark is the first framed byte");
        assert_eq!(&read[1..5], &[0, 0, 1, 1], "the address it states");

        // And the records the recording claims, read off those bytes.
        let records = recognize(&cells, Encoding::Fm);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].address, address);
        assert!(records[0].header.agree());
        let data = records[0].data.as_ref().expect("a data field followed");
        assert_eq!(data.bytes, payload);
        assert!(data.checksums.agree());
    }

    #[test]
    fn an_mfm_recording_survives_the_whole_ladder() {
        let address = SectorAddress {
            cylinder: 3,
            head: 1,
            sector: 9,
            size_code: 2,
        };
        let payload: Vec<u8> = (0..512).map(|at| (at % 253) as u8).collect();
        let cells = sector_cells(Encoding::Mfm, address, &payload);

        let read = read_back(&HEATH_H17_4_SOFT, &cells);
        // MFM's marks are three A1 bytes then the field's own.
        assert_eq!(&read[..4], &[0xa1, 0xa1, 0xa1, 0xfe]);
        assert_eq!(&read[4..8], &[3, 1, 9, 2]);

        let records = recognize(&cells, Encoding::Mfm);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].address, address);
        let data = records[0].data.as_ref().expect("a data field followed");
        assert_eq!(data.bytes, payload);
        assert!(data.checksums.agree());
    }

    #[test]
    fn cells_before_the_first_mark_are_unframed_rather_than_guessed_into_bytes() {
        let address = SectorAddress {
            cylinder: 0,
            head: 0,
            sector: 1,
            size_code: 0,
        };
        let cells = sector_cells(Encoding::Mfm, address, &[0u8; 128]);
        let medium = medium_of(&HEATH_H17_4_SOFT, &cells);
        let bitstream = materialize_bitstream(
            &medium,
            &HEATH_H17_4_SOFT,
            HEATH_H17_4_SOFT.presentation.channel_policy,
            1 << 20,
        )
        .expect("the channel clocks it");
        let bytestream = bitstream.materialize_bytestream(1 << 20).expect("framed");

        let location = &bytestream.inspect().locations[0];
        // The sixteen gap bytes ahead of the first mark are cells no
        // byte covers, and the account says so rather than the stream
        // starting at the circle's origin.
        assert!(location.unframed_bits >= 16 * 16);
        // Six, not two: MFM introduces each field with *three* A1 marks,
        // and this count is of marks rather than of fields. Framing
        // restarts at each one, which costs nothing — consecutive marks
        // are one byte apart, so the bytes come back A1 A1 A1 FE either
        // way — and counting fields here would mean this tier deciding
        // which runs of marks belong together, which is the rung above's
        // reading and not framing's.
        assert_eq!(location.alignments, 6, "three marks to each of two fields");
        assert_eq!(
            location.unassigned_groups, 0,
            "every cell pattern is a legal byte in this encoding"
        );
        assert!(
            bytestream
                .inspect()
                .declared_loss
                .iter()
                .any(|loss| loss.code == "unframed-cells")
        );
    }
}

fn describe(namespace: &'static str, codec: &IbmCodec, bitstream: &Provenance) -> Provenance {
    let mut provenance = Provenance::new(namespace)
        .note(format!("{}: {}", codec.name, codec.provenance))
        .note(
            "two cells of the recording carry one bit of a byte, and every cell pattern \
             is a legal value — so there is no illegal byte and framing cannot rest on \
             one"
            .to_owned(),
        )
        .note(
            "byte framing begins at an address mark and nowhere else; cells before the \
             first are stated as unframed rather than guessed into bytes"
                .to_owned(),
        )
        .note(
            "no byte here is a header, an address, a data field or a sector, and no mark \
             introduces one"
                .to_owned(),
        );
    for note in &bitstream.notes {
        provenance = provenance.note(format!("the bitstream beneath it: {note}"));
    }
    provenance
}
