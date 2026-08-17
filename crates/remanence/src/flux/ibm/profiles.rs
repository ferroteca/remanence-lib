// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The Heath soft-sectored families (F77), and which codec each reads
//! with.
//!
//! **Two mechanisms, two profiles.** Heath shipped the H-17-1 and the
//! H-17-4, and the difference between them is not a detail the same
//! declaration can carry: one records a single surface at 48 tracks to
//! the inch and the other two at 96. A profile pairs one mechanism with
//! the recording it is served, so a mechanism reading two kinds of media
//! is two profiles rather than one with a branch in it.
//!
//! **The rate is fixed and a mismatch is refused.** Both entries declare
//! the cell rate their family records at; an artifact declaring another
//! is refused by name showing both numbers, rather than being clocked at
//! a rate nobody stated. That is what makes a declared rate falsifiable:
//! a wrong one here is a loud refusal on the first artifact, not a
//! silently misread disk.

use crate::flux::drive_profile::{
    DensityZone, DriveProfile, DuplicateRule, EncodingShape, LandmarkShape, Materialization,
    OriginDefault, Presentation, ReadChannel, Rotation, SelectionRule, SpanProjection, Stepping,
    Surfaces, UnrecordedRule,
};
use crate::flux::ibm::presentation::{FM, IbmCodec, MFM};
use crate::model::media_profile::FLEXIBLE_5_25_SOFT;

/// The reference clock both families' cells are stated against.
///
/// It is a unit rather than a measurement: 16 MHz divides both declared
/// rates exactly, so every cell length below is a whole number of cycles
/// and no rounding enters the model. The 1541 is stated against the same
/// clock, which keeps one comparison possible across families.
const REFERENCE_CLOCK: u64 = 16_000_000;

/// 300 revolutions a minute is five a second, so a revolution is a fifth
/// of the reference clock's cycles.
const CYCLES_PER_ROTATION: u64 = REFERENCE_CLOCK / 5;

/// The channel both families are clocked by — the same phase-locked one
/// every enrolled family declares its numbers for.
const READ_CHANNEL: ReadChannel = ReadChannel {
    resync_on_transition: true,
    window_numerator: 1,
    window_denominator: 2,
    // Framing here is an encoding violation rather than a run of ones,
    // so this family declares no run-length landmark at all. The channel
    // reads the field only to describe it; the codec locates marks.
    alignment_one_bits: 0,
};

const MATERIALIZATION: Materialization = Materialization {
    unmapped_source_position_refused: true,
    unrecorded: UnrecordedRule::Absent,
    duplicate: DuplicateRule::Refuse,
    // These drives observe the index hole, so the circle begins there
    // rather than at a splice the reduction has to find.
    origin: OriginDefault::Index,
    selection: SelectionRule::Selected,
    span: SpanProjection::ScaleToNominal,
    density: crate::flux::drive_profile::DensityProjection::SnapToZoneNominal,
    strength_states: &["absent", "weak", "strong"],
};

/// MFM admits one, two or three zero cells between transitions, so an
/// interval is two, three or four cells.
const MFM_ENCODING: EncodingShape = EncodingShape {
    cell_multiples: &[2, 3, 4],
    band_numerator: 3,
    band_denominator: 10,
    landmark: LandmarkShape {
        // The mark is a clock violation rather than a run, so there is
        // no interval pattern to state. Zero says the family declares
        // none, which is a different fact from declaring a short one.
        multiple: 0,
        min_run: 0,
        per_record: 2,
    },
};

/// FM writes a clock before every data bit, so an interval is one cell
/// or two and never more.
const FM_ENCODING: EncodingShape = EncodingShape {
    cell_multiples: &[1, 2],
    band_numerator: 3,
    band_denominator: 10,
    landmark: LandmarkShape {
        multiple: 0,
        min_run: 0,
        per_record: 2,
    },
};

/// The H-17-1 mechanism reading a soft-sectored single-density recording.
///
/// **The drive's facts are the published ones**: one surface, 48 tracks
/// to the inch, forty of them. Heath's own peripherals catalogue
/// describes the cases these shipped in as single-sided, single-density,
/// 48 TPI.
///
/// **The rate is checked against artifacts rather than asserted.** A
/// quarter-megahertz cell at 300 RPM gives fifty thousand cells to a
/// revolution; the ten 256-byte sectors a Heath single-density track
/// carries are 40,960 cells of payload, leaving about 56 bytes a sector
/// for marks and gaps — which is what such a track holds. The
/// soft-sectored CP/M distribution imaged as ImageDisk states the same
/// rate in every track record, scaled for the drive it was read in.
pub(crate) static HEATH_H17_1_SOFT: DriveProfile = DriveProfile {
    id: "heath-h17-1-soft",
    name: "Heath H-17-1, soft-sectored single density",
    version: 1,
    provenance: "declared from Heath's published drive descriptions — single-sided, 48 \
                 tracks to the inch — with the cell rate checked against the sector \
                 geometry a single-density Heath track carries and against the rate the \
                 soft-sectored CP/M distribution's own track records state",
    media: &FLEXIBLE_5_25_SOFT,
    stepping: Stepping {
        // Mechanism and recording are at the same pitch, so one step
        // reaches one track.
        drive_tpi: 48,
        recorded_tpi: 48,
        first_location: 0,
    },
    rotation: Rotation {
        nominal_numerator: 5,
        nominal_denominator: 1,
        reference_clock: REFERENCE_CLOCK,
        cycles_per_rotation: CYCLES_PER_ROTATION,
        index_observed_by_drive: true,
    },
    surfaces: Surfaces { recorded: 1 },
    encoding: FM_ENCODING,
    density: &[DensityZone {
        first_location: 0,
        last_location: 39,
        // 250 kHz of cells: a 64-cycle cell against the reference clock.
        rate_numerator: 250_000,
        rate_denominator: 1,
        records: 10,
    }],
    materialization: MATERIALIZATION,
    presentation: Presentation {
        read_channel: READ_CHANNEL,
        channel_policy: crate::flux::presentation::ReadChannelPolicy {
            density: crate::flux::presentation::DensityPolicy::Declared,
            unzoned: crate::flux::presentation::UnzonedPolicy::Omit,
            weak_pulse: crate::flux::presentation::WeakPulsePolicy::Seeded,
            seed: 0x0017_0001_0017_0001,
        },
        bytestream: crate::flux::ibm::presentation::materialize_declared,
    },
};

/// The H-17-4 mechanism reading a soft-sectored double-density recording.
///
/// Two surfaces at 96 tracks to the inch, eighty of them, recorded MFM —
/// which is what Heath's catalogue describes the H-37 case as carrying.
/// The cell rate doubles with the density: MFM at 300 RPM puts a hundred
/// thousand cells on a revolution where single density puts fifty.
///
/// **An artifact of this family has now been read, and it corrected the
/// record count.** A double-density Heath image — 80 cylinders, two
/// heads, `525 DSQD` — confirms the geometry and the rate outright: its
/// transitions fall two, three and four cells apart and nowhere else,
/// which is the MFM population, and the cell those intervals imply puts
/// a hundred thousand of them on a revolution. At 300 RPM that is the
/// 500 kHz declared below.
///
/// What it refuted is the sector count. This entry declared nine records
/// to a track, taken from what "standard double density" usually means;
/// the recording holds **sixteen of 256 bytes**, which is 640 KB to the
/// disk and is Heath's own double-density format rather than the PC's.
/// Every one of its id and data fields checks: sixteen sectors a track
/// with both CRCs agreeing, on the first cylinder, the last, and both
/// heads.
///
/// The artifact is not redistributable and is therefore not a fixture,
/// so what it settles is recorded here rather than in a test. The
/// declaration stays answerable either way: an artifact stating another
/// rate, side or track count refuses by name showing both numbers.
pub(crate) static HEATH_H17_4_SOFT: DriveProfile = DriveProfile {
    id: "heath-h17-4-soft",
    name: "Heath H-17-4, soft-sectored double density",
    version: 1,
    provenance: "declared from Heath's published drive descriptions — double-sided, 96 \
                 tracks to the inch, double density — with the standard double-density \
                 cell rate; no artifact of this family has been read against it, and a \
                 mismatch is refused rather than accommodated",
    media: &FLEXIBLE_5_25_SOFT,
    stepping: Stepping {
        drive_tpi: 96,
        recorded_tpi: 96,
        first_location: 0,
    },
    rotation: Rotation {
        nominal_numerator: 5,
        nominal_denominator: 1,
        reference_clock: REFERENCE_CLOCK,
        cycles_per_rotation: CYCLES_PER_ROTATION,
        index_observed_by_drive: true,
    },
    surfaces: Surfaces { recorded: 2 },
    encoding: MFM_ENCODING,
    density: &[DensityZone {
        first_location: 0,
        last_location: 79,
        // 500 kHz of cells: a 32-cycle cell against the reference
        // clock, and the rate a read artifact's own intervals imply.
        rate_numerator: 500_000,
        rate_denominator: 1,
        // Sixteen 256-byte sectors, read off a double-density Heath
        // recording. Not the nine of a PC double-density track, which
        // is what this said before an artifact was read against it.
        records: 16,
    }],
    materialization: MATERIALIZATION,
    presentation: Presentation {
        read_channel: READ_CHANNEL,
        channel_policy: crate::flux::presentation::ReadChannelPolicy {
            density: crate::flux::presentation::DensityPolicy::Declared,
            unzoned: crate::flux::presentation::UnzonedPolicy::Omit,
            weak_pulse: crate::flux::presentation::WeakPulsePolicy::Seeded,
            seed: 0x0017_0004_0017_0004,
        },
        bytestream: crate::flux::ibm::presentation::materialize_declared,
    },
};

/// Which codec an IBM-family profile reads with.
///
/// The pairing is a lookup over the enrolled entries rather than a
/// branch on a name: enrolling a family adds a line here and changes
/// nothing above.
pub(crate) fn codec_of(profile: &'static DriveProfile) -> Option<&'static IbmCodec> {
    match profile.id {
        id if id == HEATH_H17_1_SOFT.id => Some(&FM),
        id if id == HEATH_H17_4_SOFT.id => Some(&MFM),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mechanism_steps_once_over_the_recording_it_is_paired_with() {
        // Both are at their recording's own pitch, which the pair says
        // and a bare count could only have said for one of them.
        assert_eq!(HEATH_H17_1_SOFT.stepping.cadence(), Some((1, 1)));
        assert_eq!(HEATH_H17_4_SOFT.stepping.cadence(), Some((1, 1)));
        assert_eq!(HEATH_H17_1_SOFT.stepping.drive_tpi, 48);
        assert_eq!(HEATH_H17_4_SOFT.stepping.drive_tpi, 96);
    }

    #[test]
    fn the_declared_rates_divide_the_reference_clock_exactly() {
        // A cell that is not a whole number of cycles would put rounding
        // into the model, which is what stating the clock this way
        // exists to prevent.
        for profile in [&HEATH_H17_1_SOFT, &HEATH_H17_4_SOFT] {
            for zone in profile.density {
                let (numerator, denominator) = zone.nominal_cell(&profile.rotation);
                assert_eq!(
                    numerator % denominator,
                    0,
                    "{} states a fractional cell",
                    profile.id
                );
            }
        }
        // And they come to the documented cell lengths.
        let (cell, per) = HEATH_H17_1_SOFT.density[0].nominal_cell(&HEATH_H17_1_SOFT.rotation);
        assert_eq!(cell / per, 64, "single density is a 64-cycle cell");
        let (cell, per) = HEATH_H17_4_SOFT.density[0].nominal_cell(&HEATH_H17_4_SOFT.rotation);
        assert_eq!(cell / per, 32, "double density is half that");
    }

    #[test]
    fn a_single_density_revolution_holds_the_track_heath_records_on_it() {
        // The check that says the declared rate is the right one: ten
        // 256-byte sectors have to fit a revolution with room for their
        // marks and gaps, and not by much.
        let (cell, per) = HEATH_H17_1_SOFT.density[0].nominal_cell(&HEATH_H17_1_SOFT.rotation);
        let cells_per_revolution =
            HEATH_H17_1_SOFT.rotation.cycles_per_rotation / (cell / per) as u64;
        assert_eq!(cells_per_revolution, 50_000);

        // Ten sectors of 256 bytes, two cells to a bit.
        let payload_cells = 10 * 256 * 8 * 2;
        assert!(payload_cells < cells_per_revolution);
        let spare = cells_per_revolution - payload_cells;
        let spare_bytes_per_sector = spare / 16 / 10;
        assert!(
            (40..80).contains(&spare_bytes_per_sector),
            "each sector's marks and gaps come to {spare_bytes_per_sector} bytes, which \
             is the range a real track leaves"
        );
    }

    #[test]
    fn a_double_density_revolution_holds_the_track_heath_records_on_it() {
        // The same check as its single-density sibling, at the count a
        // real double-density Heath recording turned out to carry:
        // sixteen 256-byte sectors, not the nine a PC track holds.
        //
        // The artifact that settled it is not redistributable, so this
        // stands in for it — arithmetic the corrected declaration has to
        // satisfy, which the value it replaced does not.
        let zone = &HEATH_H17_4_SOFT.density[0];
        let (cell, per) = zone.nominal_cell(&HEATH_H17_4_SOFT.rotation);
        let cells_per_revolution =
            HEATH_H17_4_SOFT.rotation.cycles_per_rotation / (cell / per) as u64;
        assert_eq!(cells_per_revolution, 100_000, "twice single density's");

        // Two cells to a bit, so a 256-byte sector is 4,096 cells.
        let payload_cells = u64::from(zone.records) * 256 * 8 * 2;
        assert!(payload_cells < cells_per_revolution);
        let spare_bytes_per_sector =
            (cells_per_revolution - payload_cells) / 16 / u64::from(zone.records);
        assert!(
            (80..200).contains(&spare_bytes_per_sector),
            "each sector's marks and gaps come to {spare_bytes_per_sector} bytes, which \
             is the range an MFM track leaves"
        );

        // And the capacity that count implies is Heath's own figure for
        // the drive, which is the cross-check that it is not a PC
        // format wearing a Heath name.
        let bytes = u64::from(zone.records)
            * 256
            * u64::from(HEATH_H17_4_SOFT.surfaces.recorded)
            * (zone.last_location - zone.first_location + 1);
        assert_eq!(bytes, 640 * 1024);
    }

    #[test]
    fn each_family_reads_with_its_own_declared_encoding() {
        assert_eq!(
            codec_of(&HEATH_H17_1_SOFT).map(|codec| codec.id),
            Some("ibm-fm")
        );
        assert_eq!(
            codec_of(&HEATH_H17_4_SOFT).map(|codec| codec.id),
            Some("ibm-mfm")
        );
        // A family that does not enrol this transition reads with none
        // of them, rather than defaulting to one.
        assert!(codec_of(&crate::flux::drive_profile::C1541).is_none());
    }
}
