// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The enrolled families.
//!
//! One entry per family, and **each entry is declared whole in one
//! place**: its recognition half — stepping, rotation, the density map,
//! the shape of an encoding's landmarks — beside its materialization
//! half, which the flux-to-medium reduction reads. They are laid out
//! together deliberately, because these are facts about the same drive
//! and splitting them across two places is how two features come to
//! hold different answers about one of them.
//!
//! Every field carries the published description it came from. None is
//! a value a capture is permitted to establish.

use crate::model::media_profile::FLEXIBLE_5_25_SOFT;

use super::*;

/// The Commodore 1541, the first and only enrolled family.
pub(crate) static C1541: DriveProfile = DriveProfile {
    id: "c1541",
    name: "Commodore 1541",
    version: 1,
    provenance: "declared from the published 1541 recording conventions: two \
                 drive steps per track, 300 RPM against a 16 MHz reference, and \
                 the four documented speed zones with their track boundaries \
                 and sector counts",
    // Soft-sectored 5.25-inch media. The disk carries an index hole and
    // this drive has no sensor for it, which is why the two facts are
    // declared in two places rather than one standing in for the other.
    media: &FLEXIBLE_5_25_SOFT,
    stepping: Stepping {
        steps_per_location: 2,
        first_location: 1,
    },
    rotation: Rotation {
        nominal_numerator: 5,
        nominal_denominator: 1,
        reference_clock: 16_000_000,
        cycles_per_rotation: 3_200_000,
        // The 1541 has no index sensor at all.
        index_observed_by_drive: false,
    },
    surfaces: Surfaces { recorded: 1 },
    encoding: EncodingShape {
        // GCR admits runs of at most two zeros between ones, so an
        // interval is one, two or three cells and never more.
        cell_multiples: &[1, 2, 3],
        band_numerator: 3,
        band_denominator: 10,
        landmark: LandmarkShape {
            // A GCR sync is ten or more consecutive one bits, and a one
            // bit is a transition one cell after the last — so a sync is
            // a run of at least nine minimum-length intervals, locatable
            // without a clock, without the encoding table, and without
            // knowing what it introduces.
            multiple: 1,
            min_run: 9,
            // A header sync and a data sync to each sector.
            per_record: 2,
        },
    },
    density: &[
        DensityZone {
            first_location: 1,
            last_location: 17,
            rate_numerator: 16_000_000,
            rate_denominator: 52,
            records: 21,
        },
        DensityZone {
            first_location: 18,
            last_location: 24,
            rate_numerator: 16_000_000,
            rate_denominator: 56,
            records: 19,
        },
        DensityZone {
            first_location: 25,
            last_location: 30,
            rate_numerator: 16_000_000,
            rate_denominator: 60,
            records: 18,
        },
        DensityZone {
            first_location: 31,
            last_location: 35,
            rate_numerator: 16_000_000,
            rate_denominator: 64,
            records: 17,
        },
    ],
    materialization: Materialization {
        unmapped_source_position_refused: true,
        unrecorded: UnrecordedRule::Absent,
        duplicate: DuplicateRule::Refuse,
        // The drive never observes index, so the circle begins at the
        // seam the write splice leaves.
        origin: OriginDefault::LongestGap,
        selection: SelectionRule::Selected,
        span: SpanProjection::ScaleToNominal,
        density: DensityProjection::SnapToZoneNominal,
        strength_states: &["absent", "weak", "strong"],
    },
    presentation: Presentation {
        read_channel: ReadChannel {
            // The 1541's read channel restarts its cell counter at every
            // detected transition, which is what keeps a track readable
            // while the disk's speed wanders away from the nominal.
            resync_on_transition: true,
            // Half a cell either way: the channel locks onto the
            // recording's own phase rather than to the nominal one.
            window_numerator: 1,
            window_denominator: 2,
            // Ten consecutive one bits. GCR's own table cannot produce
            // more than four in a row across any pair of symbols, so ten
            // is a pattern the recording cannot mean as data.
            alignment_one_bits: 10,
        },
        codec: GroupCodec {
            id: "c1541-gcr",
            name: "Commodore group-coded recording",
            symbol_bits: 5,
            data_bits: 4,
            symbols: &C1541_GCR_SYMBOLS,
            provenance: "declared from the published Commodore GCR table: each four-bit \
                         value is recorded as one of sixteen five-bit symbols, chosen so \
                         that no symbol and no pair of them runs more than two zeros or \
                         four ones together",
        },
        record: RecordGrammar {
            id: "cbm-dos-record",
            name: "CBM DOS sector record",
            checksum: ChecksumRule::Xor,
            header: BlockShape {
                id: "header",
                mark: 0x08,
                bytes: 8,
                checksum_at: 1,
                checked_from: 2,
                checked_to: 6,
            },
            data: BlockShape {
                id: "data",
                mark: 0x07,
                bytes: 260,
                checksum_at: 257,
                checked_from: 1,
                checked_to: 257,
            },
            track_at: 3,
            sector_at: 2,
            id_high_at: 5,
            id_low_at: 4,
            payload_from: 1,
            payload_to: 257,
            provenance: "declared from the published CBM DOS recording conventions: a \
                         sector is written as an eight-byte header block opening 0x08, \
                         holding the sector, the track and the two disk-identity bytes \
                         with their checksum, and then — after a gap and a second sync — \
                         a 260-byte data block opening 0x07, holding 256 payload bytes \
                         and their checksum",
        },
        // The family's declared defaults for the presentation ladder
        // (P30 reached through the type). Every choice states loss
        // rather than stopping, because a protected or damaged disk is
        // still a recording and the account is the honest answer; the
        // one stochastic element is seeded with the profile's own
        // stated constant, so the same medium reads back the same bits
        // (P29).
        channel_policy: crate::flux::c1541::presentation::ReadChannelPolicy {
            // The zone the family's own density map declares.
            density: crate::flux::c1541::presentation::DensityPolicy::Declared,
            // A location no zone covers — a half-track between two of
            // them — is left out and counted: no published rate reaches
            // it, and a neighbour's would be an undeclared number.
            unzoned: crate::flux::c1541::presentation::UnzonedPolicy::Omit,
            // A pulse that does not read the same every time resolves
            // reproducibly from the declared seed and its own angle,
            // which is the one honest answer a convention can give
            // about a bit the medium itself leaves undecided.
            weak_pulse: crate::flux::c1541::presentation::WeakPulsePolicy::Seeded,
            seed: 0x1541_1541_1541_1541,
        },
        codec_policy: crate::flux::c1541::presentation::GcrCodecPolicy {
            // Framing begins at the family's declared landmark and
            // nowhere else: bits before the first sync are unframed
            // rather than guessed into bytes.
            alignment: crate::flux::c1541::presentation::AlignmentPolicy::Landmark,
            // A pattern the table does not assign keeps its own bits,
            // stated as unresolved and counted.
            unassigned_symbol:
                crate::flux::c1541::presentation::UnassignedSymbolPolicy::DeclareLoss,
        },
        sector_policy: crate::flux::c1541::sectors::SectorPolicy {
            checksum_failure: crate::flux::c1541::sectors::ChecksumFailurePolicy::DeclareLoss,
            unpaired_record: crate::flux::c1541::sectors::UnpairedRecordPolicy::DeclareLoss,
        },
    },
};

/// The symbol each four-bit value is recorded as, indexed by the value.
///
/// It is a declared fact of the family in exactly the sense every other
/// field of the profile is: a published table, not a pattern any
/// recording is permitted to establish.
static C1541_GCR_SYMBOLS: [u16; 16] = [
    0b01010, 0b01011, 0b10010, 0b10011, 0b01110, 0b01111, 0b10110, 0b10111, 0b01001, 0b11001,
    0b11010, 0b11011, 0b01101, 0b11101, 0b11110, 0b10101,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_c1541_entry_declares_the_family_and_derives_nothing_from_a_capture() {
        // Every value here is a published fact of the drive. None is a
        // number a capture is permitted to establish.
        assert_eq!(C1541.stepping.steps_per_location, 2);
        assert_eq!(C1541.stepping.first_location, 1);
        assert_eq!(C1541.rotation.cycles_per_rotation, 3_200_000);
        assert!(!C1541.rotation.index_observed_by_drive);
        assert_eq!(C1541.surfaces.recorded, 1);
        assert_eq!(C1541.encoding.cell_multiples, &[1, 2, 3]);
        assert_eq!(C1541.encoding.landmark.min_run, 9);
        assert_eq!(C1541.encoding.landmark.per_record, 2);
        assert_eq!(C1541.declared_locations(), 35);

        // The four documented zones, at their documented boundaries.
        let zones: Vec<(u64, u64, u32)> = C1541
            .density
            .iter()
            .map(|zone| (zone.first_location, zone.last_location, zone.records))
            .collect();
        assert_eq!(
            zones,
            [(1, 17, 21), (18, 24, 19), (25, 30, 18), (31, 35, 17)]
        );

        // And each zone's cell follows from its rate and the clock,
        // exactly: 52, 56, 60 and 64 cycles.
        let cells: Vec<u128> = C1541
            .density
            .iter()
            .map(|zone| {
                let (numerator, denominator) = zone.nominal_cell(&C1541.rotation);
                numerator / denominator
            })
            .collect();
        assert_eq!(cells, [52, 56, 60, 64]);
    }

    #[test]
    fn the_drive_that_observes_no_index_does_not_inherit_the_captures() {
        // A 1541 has no index sensor, so a medium reduced for one cannot
        // honestly begin its circle at a datum the drive never sees.
        assert!(!C1541.rotation.index_observed_by_drive);
        assert_eq!(C1541.materialization.origin, OriginDefault::LongestGap);
    }

    #[test]
    fn the_family_declares_the_medium_it_is_served_without_absorbing_it() {
        // P14 and P30 are two declarations about one disk. The family
        // names the article it accepts; the article's own facts stay in
        // the article catalog, which knows nothing about this drive.
        assert_eq!(C1541.media.id, "flexible-5.25-soft");
        let medium = C1541
            .media
            .flexible_magnetic()
            .expect("a 1541 is served flexible magnetic media");

        // The clearest case of the two answering different questions:
        // the disk carries an index hole, and this drive has no sensor
        // for it. Neither fact stands in for the other, and a profile
        // that held both would have to choose which one it meant.
        assert_eq!(medium.index_holes, 1);
        assert!(!C1541.rotation.index_observed_by_drive);

        // Soft-sectored: the medium divides no revolution, so what a
        // location holds is entirely the family's declaration below.
        assert_eq!(medium.sectoring.sector_holes(), 0);
        assert!(C1541.density.iter().all(|zone| zone.records > 0));
    }

    #[test]
    fn a_duplicate_is_refused_by_declaration_rather_than_resolved() {
        assert_eq!(C1541.materialization.duplicate, DuplicateRule::Refuse);
        assert!(C1541.materialization.unmapped_source_position_refused);
    }

    #[test]
    fn addressing_maps_whole_steps_and_covers_no_half_step() {
        // Parity is a statement about the family's addressing. It is
        // never a statement about which source positions hold content.
        assert_eq!(C1541.stepping.location_of(0), Some(1));
        assert_eq!(C1541.stepping.location_of(2), Some(2));
        assert_eq!(C1541.stepping.location_of(68), Some(35));
        assert_eq!(C1541.stepping.location_of(1), None);
        assert_eq!(C1541.stepping.location_of(67), None);
    }

    #[test]
    fn a_zone_covers_what_it_declares_and_nothing_past_it() {
        assert_eq!(C1541.zone_for(1).map(|(at, _)| at), Some(0));
        assert_eq!(C1541.zone_for(17).map(|(at, _)| at), Some(0));
        assert_eq!(C1541.zone_for(18).map(|(at, _)| at), Some(1));
        assert_eq!(C1541.zone_for(35).map(|(at, _)| at), Some(3));
        // Tracks past 35 are outside every zone the family declares, so
        // a capture of them is refused by declaration rather than by a
        // threshold.
        assert!(C1541.zone_for(36).is_none());
    }

    #[test]
    fn the_landmark_is_one_convention_stated_at_two_layers_and_they_agree() {
        // A run of n shortest intervals is n + 1 one bits: the interval
        // shape the probe reads and the bit run the codec frames on are
        // the same convention seen from either side of the bitstream,
        // and the profile is where they are held to each other.
        assert_eq!(
            C1541.presentation.read_channel.alignment_one_bits,
            C1541.encoding.landmark.min_run + 1
        );
        assert!(C1541.presentation.read_channel.resync_on_transition);
    }

    #[test]
    fn the_group_code_is_a_declared_table_and_admits_no_pattern_outside_it() {
        let codec = &C1541.presentation.codec;
        assert_eq!(codec.symbol_bits, 5);
        assert_eq!(codec.data_bits, 4);
        // Two five-bit symbols to a byte, derived from the widths rather
        // than declared beside them.
        assert_eq!(codec.symbols_per_byte(), Some(2));
        assert_eq!(codec.symbols.len(), 16);

        // Every value round-trips, and the sixteen symbols are distinct.
        for value in 0..16u8 {
            assert_eq!(codec.value_of(codec.symbols[value as usize]), Some(value));
        }
        let mut sorted = codec.symbols.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 16);

        // A five-bit pattern the table does not hold is not a symbol,
        // and the codec says so rather than choosing the nearest entry.
        assert_eq!(codec.value_of(0b00000), None);
        assert_eq!(codec.value_of(0b11111), None);
        assert_eq!(codec.value_of(0b00111), None);
    }

    #[test]
    fn the_table_records_no_pattern_that_could_be_read_as_the_landmark() {
        // The whole reason ten one bits can mean "landmark" is that no
        // pair of symbols can produce a run that long. Checking it here
        // is what keeps the two declarations honest about each other.
        let codec = &C1541.presentation.codec;
        let mut longest = 0u32;
        for left in codec.symbols {
            for right in codec.symbols {
                let pair = (u64::from(*left) << codec.symbol_bits) | u64::from(*right);
                let (mut run, mut best) = (0u32, 0u32);
                for bit in (0..codec.symbol_bits * 2).rev() {
                    if pair >> bit & 1 == 1 {
                        run += 1;
                        best = best.max(run);
                    } else {
                        run = 0;
                    }
                }
                longest = longest.max(best);
            }
        }
        assert!(
            longest < C1541.presentation.read_channel.alignment_one_bits,
            "the table can record a run of {longest} ones, which the landmark claims \
             data cannot"
        );
    }

    #[test]
    fn the_record_grammar_is_declared_whole_and_states_its_own_spans() {
        // The layer above the bytestream reads sectors under these
        // numbers and holds none of its own. Each is a published fact of
        // CBM DOS, and the two marks are distinct because a byte opening
        // both blocks would make the grammar unreadable.
        let record = &C1541.presentation.record;
        assert_eq!(record.header.mark, 0x08);
        assert_eq!(record.data.mark, 0x07);
        assert_ne!(record.header.mark, record.data.mark);
        assert_eq!(record.block_of(0x08).map(|block| block.id), Some("header"));
        assert_eq!(record.block_of(0x07).map(|block| block.id), Some("data"));
        // A framed byte the grammar does not name opens no block, rather
        // than resolving to the nearer of the two marks.
        assert!(record.block_of(0x00).is_none());
        assert!(record.block_of(0x55).is_none());

        // Every declared span sits inside the block it is a span of, and
        // the payload is the 256 bytes CBM DOS carries.
        for block in [&record.header, &record.data] {
            assert!(block.checksum_at < block.bytes, "{block:?}");
            assert!(block.checked_from < block.checked_to, "{block:?}");
            assert!(block.checked_to <= block.bytes, "{block:?}");
        }
        for offset in [
            record.track_at,
            record.sector_at,
            record.id_high_at,
            record.id_low_at,
        ] {
            assert!(offset < record.header.bytes);
        }
        assert!(record.payload_to <= record.data.bytes);
        assert_eq!(record.payload_bytes(), 256);

        // The header's checksum covers exactly the four bytes it
        // addresses by, so a header that checksums is a header whose
        // address was read.
        assert_eq!(record.header.checked_from, 2);
        assert_eq!(record.header.checked_to, 6);
        assert_eq!(
            ChecksumRule::Xor.over(&[0x03, 0x12, 0x50, 0x43]),
            0x03 ^ 0x12 ^ 0x50 ^ 0x43
        );
    }

    #[test]
    fn a_half_track_between_two_zones_is_covered_by_neither() {
        // Whole tracks are covered exactly as before.
        assert_eq!(C1541.zone_for_ratio(1, 1).map(|(at, _)| at), Some(0));
        assert_eq!(C1541.zone_for_ratio(18, 1).map(|(at, _)| at), Some(1));
        // A half-track inside a zone's declared span is inside it.
        assert_eq!(C1541.zone_for_ratio(3, 2).map(|(at, _)| at), Some(0));
        assert_eq!(C1541.zone_for_ratio(33, 2).map(|(at, _)| at), Some(0));
        // And one between two zones is covered by neither: no published
        // rate reaches it, and a neighbour's would be an undeclared
        // number in the presentation.
        assert!(C1541.zone_for_ratio(35, 2).is_none());
        assert!(C1541.zone_for_ratio(71, 2).is_none());
    }
}
