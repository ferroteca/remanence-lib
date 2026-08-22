// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The FM and MFM families (F77) — the Heath soft-sectored ones and the
//! PC's high-density drive — and which codec each reads with.
//!
//! **Two mechanisms, two profiles.** Heath shipped the H-17-1 and the
//! H-17-4, and the difference between them is not a detail the same
//! declaration can carry: one records a single surface at 48 tracks to
//! the inch and the other two at 96. A profile pairs one mechanism with
//! the recording it is served, so a mechanism reading two kinds of media
//! is two profiles rather than one with a branch in it. The PC drive at
//! the end is the same rule applied to a third mechanism: 3.5-inch media
//! at 135 tracks to the inch, recorded at the high-density rate.
//!
//! **The rate is fixed and a mismatch is refused.** Every entry declares
//! the cell rate its family records at; an artifact declaring another
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
use crate::model::media_profile::{FLEXIBLE_3_5_HD, FLEXIBLE_5_25_SOFT};

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
        sectors: crate::flux::ibm::sectors::recognize_declared,
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
        sectors: crate::flux::ibm::sectors::recognize_declared,
    },
};

/// The H-17-1 mechanism reading a soft-sectored **double-density**
/// recording.
///
/// **This is the entry that was missing, and the assumption that hid
/// it.** The pair above read as though mechanism settled density —
/// single-sided 48 TPI meaning single density, double-sided 96 TPI
/// meaning double. It does not: the H-37 controller writes either
/// density to either mechanism, so the mechanisms and the densities are
/// independent axes and this is the third of their four combinations.
/// The fourth — the H-17-4 mechanism at single density — is not declared
/// here, because no artifact has shown one and this file has already
/// been wrong once about a combination nobody had read.
///
/// **Every number here was measured off a recording rather than
/// reasoned to.** A MAME image of this configuration states 40
/// cylinders and one head; its transitions fall on a 2000-unit
/// fundamental with intervals of two, three and four of them, which is
/// MFM at 500 kHz; and its tracks carry sixteen id fields each stating
/// size code 1. That is 160 KB, and it is what the entry declares.
pub(crate) static HEATH_H17_1_SOFT_DD: DriveProfile = DriveProfile {
    id: "heath-h17-1-soft-dd",
    name: "Heath H-17-1, soft-sectored double density",
    version: 1,
    provenance: "measured from a MAME recording of the configuration: 40 cylinders by                  one head, transitions on a 2000-unit fundamental at intervals of two,                  three and four — MFM at 500 kHz — and sixteen 256-byte records to a                  track, which is 160 KB",
    media: &FLEXIBLE_5_25_SOFT,
    stepping: Stepping {
        // The mechanism is the single-density one's: 48 tracks to the
        // inch, stepping one track at a time. Only the recording differs.
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
    encoding: MFM_ENCODING,
    density: &[DensityZone {
        first_location: 0,
        last_location: 39,
        // The same 500 kHz the double-sided unit records at: density is
        // the controller's business and not the mechanism's, which is
        // the whole point of this entry existing.
        rate_numerator: 500_000,
        rate_denominator: 1,
        records: 16,
    }],
    materialization: MATERIALIZATION,
    presentation: Presentation {
        read_channel: READ_CHANNEL,
        channel_policy: crate::flux::presentation::ReadChannelPolicy {
            density: crate::flux::presentation::DensityPolicy::Declared,
            unzoned: crate::flux::presentation::UnzonedPolicy::Omit,
            weak_pulse: crate::flux::presentation::WeakPulsePolicy::Seeded,
            seed: 0x0017_0001_0044_0044,
        },
        bytestream: crate::flux::ibm::presentation::materialize_declared,
        sectors: crate::flux::ibm::sectors::recognize_declared,
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
        id if id == HEATH_H17_1_SOFT_DD.id => Some(&MFM),
        id if id == HEATH_H17_4_SOFT.id => Some(&MFM),
        id if id == PC_3_5_HD.id => Some(&MFM),
        _ => None,
    }
}

/// The PC's 3.5-inch high-density drive reading a 1.44 MB recording.
///
/// **This is the one mechanism here that is not Heath's, and it was
/// enrolled for an artifact rather than ahead of one.** The standard PC
/// controller drives a two-head 135 TPI mechanism at 300 RPM and records
/// MFM at 500 kbit/s — a hundred thousand data bits, two hundred thousand
/// cells, to a revolution — with eighteen 512-byte records to a track
/// over eighty cylinders, which is the 1,474,560-byte disk everything
/// from DOS 3.3 on was distributed on.
///
/// **Every number was confirmed against a recording before it was
/// declared.** An IBM PC DOS 7 distribution disk, read as an HxC MFM
/// container, states two sides and a measured 501 kbit/s, holds eighteen
/// records on every one of its hundred and sixty tracks with every id
/// and data CRC agreeing, and carries four empty tracks past cylinder 79
/// where the capture drive kept stepping. That artifact is the fixture
/// the integration suite reads; the four extra tracks are what the
/// density zone's upper bound below is tested against.
pub(crate) static PC_3_5_HD: DriveProfile = DriveProfile {
    id: "pc-3.5-hd",
    name: "PC 3.5-inch high-density drive",
    version: 1,
    provenance: "declared from the PC floppy controller's published high-density \
                 conventions — two heads, 135 tracks to the inch, 300 RPM, MFM at \
                 500 kbit/s, eighteen 512-byte records to a track — and confirmed \
                 against an IBM PC DOS 7 distribution disk read as an HxC MFM \
                 container",
    media: &FLEXIBLE_3_5_HD,
    stepping: Stepping {
        // Mechanism and recording are at the same pitch, so one step
        // reaches one track.
        drive_tpi: 135,
        recorded_tpi: 135,
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
        // A megahertz of cells: a 16-cycle cell against the reference
        // clock, two hundred thousand of them to a revolution, carrying
        // the 500 kbit/s the PC calls high density — a cell being half
        // a data bit, exactly as the Heath double-density entry's 500 kHz
        // carries 250 kbit/s. The container's own track lengths say so:
        // 25,000 bytes of cells to a track, read off the fixture.
        rate_numerator: 1_000_000,
        rate_denominator: 1,
        records: 18,
    }],
    materialization: MATERIALIZATION,
    presentation: Presentation {
        read_channel: READ_CHANNEL,
        channel_policy: crate::flux::presentation::ReadChannelPolicy {
            density: crate::flux::presentation::DensityPolicy::Declared,
            unzoned: crate::flux::presentation::UnzonedPolicy::Omit,
            weak_pulse: crate::flux::presentation::WeakPulsePolicy::Seeded,
            seed: 0x0035_0012_0035_0012,
        },
        bytestream: crate::flux::ibm::presentation::materialize_declared,
        sectors: crate::flux::ibm::sectors::recognize_declared,
    },
};

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

    /// Mechanism and density are independent axes, and the entries here
    /// say so.
    ///
    /// **This is the assertion whose absence hid a whole family.** The
    /// first two entries were written as though the mechanism settled
    /// the density — single-sided 48 TPI meaning single density,
    /// double-sided 96 TPI meaning double — and nothing said otherwise,
    /// so nothing caught that the H-37 writes either density to either
    /// mechanism. A recording of the third combination read as forty
    /// tracks of nothing until the pairing was declared.
    ///
    /// So the test is not "each profile has the right numbers", which
    /// would have passed throughout. It is that the same mechanism
    /// appears at two densities and the same density at two mechanisms,
    /// which is only true if the axes really are independent.
    #[test]
    fn the_mechanism_and_the_density_vary_independently_of_each_other() {
        let single_sided_fm = &HEATH_H17_1_SOFT;
        let single_sided_mfm = &HEATH_H17_1_SOFT_DD;
        let double_sided_mfm = &HEATH_H17_4_SOFT;

        // One mechanism, two densities: same surfaces and same pitch,
        // different encoding and different rate.
        assert_eq!(
            single_sided_fm.surfaces.recorded,
            single_sided_mfm.surfaces.recorded
        );
        assert_eq!(
            single_sided_fm.stepping.drive_tpi,
            single_sided_mfm.stepping.drive_tpi
        );
        assert_eq!(
            codec_of(single_sided_fm).map(|codec| codec.id),
            Some("ibm-fm")
        );
        assert_eq!(
            codec_of(single_sided_mfm).map(|codec| codec.id),
            Some("ibm-mfm")
        );
        assert_ne!(
            single_sided_fm.density[0].rate_numerator,
            single_sided_mfm.density[0].rate_numerator
        );

        // One density, two mechanisms: same encoding and same rate,
        // different surfaces and different pitch.
        assert_eq!(
            codec_of(single_sided_mfm).map(|codec| codec.id),
            codec_of(double_sided_mfm).map(|codec| codec.id)
        );
        assert_eq!(
            single_sided_mfm.density[0].rate_numerator,
            double_sided_mfm.density[0].rate_numerator
        );
        assert_eq!(
            single_sided_mfm.density[0].records,
            double_sided_mfm.density[0].records
        );
        assert_ne!(
            single_sided_mfm.surfaces.recorded,
            double_sided_mfm.surfaces.recorded
        );
        assert_ne!(
            single_sided_mfm.stepping.drive_tpi,
            double_sided_mfm.stepping.drive_tpi
        );

        // And what each holds, which is what a caller sees: 100 KB,
        // 160 KB and 640 KB.
        for (profile, expected) in [
            (single_sided_fm, 100 * 1024u64),
            (single_sided_mfm, 160 * 1024),
            (double_sided_mfm, 640 * 1024),
        ] {
            let zone = &profile.density[0];
            let tracks = u64::from(zone.last_location - zone.first_location + 1);
            let held =
                tracks * u64::from(profile.surfaces.recorded) * u64::from(zone.records) * 256;
            assert_eq!(held, expected, "{} holds {held} bytes", profile.id);
        }
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
