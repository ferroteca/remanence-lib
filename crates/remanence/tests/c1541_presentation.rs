// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Presenting the mastered capture as a 1541 hardware bitstream, and
//! then as the family's encoded bytestream.
//!
//! The claim under test is P23's and P30's together: the layers above a
//! flux medium are materialized under rules the drive profile declares,
//! each transition carries what produced everything beneath it, and
//! neither layer assigns synchronization, headers, sectors or files to
//! what it holds.
//!
//! The journey is the real one — a KryoFlux capture of a C64 disk,
//! mastered under a declared policy and then read by a declared drive —
//! so the numbers below are what a 1541 makes of an actual recording
//! rather than what a synthetic one was built to produce.

use std::sync::OnceLock;

use remanence::{
    AlignmentPolicy, BitstreamReport, BytestreamReport, CaptureSet, DensityPolicy,
    DuplicatePolicy, ErrorCategory, GcrCodecPolicy, MasteringPolicy, ObservationPolicy,
    OriginPolicy, ProjectionPolicy, PulseStrengthPolicy, ReadChannelPolicy,
    UnassignedSymbolPolicy, UnzonedPolicy, WeakPulsePolicy,
};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";
/// The 1541's reference clock across one 300 RPM rotation.
const CYCLES_PER_ROTATION: u64 = 3_200_000;
/// 35 recorded tracks, less the two the mastering policy left out.
const LOCATIONS: usize = 33;

fn mastering() -> MasteringPolicy {
    MasteringPolicy {
        side: 0,
        observation: ObservationPolicy::Selected { ordinal: 0 },
        duplicate: DuplicatePolicy::Omit,
        projection: ProjectionPolicy::DeclareLoss,
        pulse_strength: PulseStrengthPolicy::Declared { state: 2 },
        origin: OriginPolicy::Declared,
        seed: 0x0123_4567_89ab_cdef,
    }
}

fn channel() -> ReadChannelPolicy {
    ReadChannelPolicy {
        density: DensityPolicy::Declared,
        unzoned: UnzonedPolicy::Refuse,
        weak_pulse: WeakPulsePolicy::Seeded,
        seed: 0x0123_4567_89ab_cdef,
    }
}

fn codec() -> GcrCodecPolicy {
    GcrCodecPolicy {
        alignment: AlignmentPolicy::Landmark,
        unassigned_symbol: UnassignedSymbolPolicy::DeclareLoss,
    }
}

struct Presented {
    bitstream: BitstreamReport,
    /// The same transition run twice from the same medium and policy.
    repeated: BitstreamReport,
    bitstream_backing: u64,
    bitstream_resident: u64,
    bytestream: BytestreamReport,
    bytestream_backing: u64,
    bytestream_resident: u64,
    /// What the codec said when the policy refused a pattern the
    /// family's table does not assign.
    unassigned: Option<(ErrorCategory, String)>,
}

fn presented() -> &'static Presented {
    static PRESENTED: OnceLock<Presented> = OnceLock::new();
    PRESENTED.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("remanence-presentation-{}.7z", std::process::id()));
        std::fs::copy(common::ensure_fixture(ARCHIVE), &path).expect("fixture copies");
        let set = CaptureSet::open(&path).expect("the set opens");
        let medium = set
            .plan_c1541_mastering(mastering())
            .expect("the plan resolves")
            .execute(1 << 20)
            .expect("the medium is produced");

        let bitstream = medium
            .materialize_c1541_bitstream(channel(), 1 << 20)
            .expect("the channel clocks the medium");
        let repeated = medium
            .materialize_c1541_bitstream(channel(), 1 << 20)
            .expect("and clocks it again");
        let bytestream = bitstream
            .materialize_c1541_bytestream(codec(), 1 << 20)
            .expect("the codec resolves the bitstream");
        let unassigned = bitstream
            .materialize_c1541_bytestream(
                GcrCodecPolicy {
                    unassigned_symbol: UnassignedSymbolPolicy::Refuse,
                    ..codec()
                },
                1 << 20,
            )
            .err()
            .map(|error| (error.category(), error.to_string()));

        let presented = Presented {
            bitstream: bitstream.inspect().clone(),
            repeated: repeated.inspect().clone(),
            bitstream_backing: bitstream.backing_bytes(),
            bitstream_resident: bitstream.resident_bytes(),
            bytestream: bytestream.inspect().clone(),
            bytestream_backing: bytestream.backing_bytes(),
            bytestream_resident: bytestream.resident_bytes(),
            unassigned,
        };
        drop(bytestream);
        drop(repeated);
        drop(bitstream);
        drop(medium);
        drop(set);
        std::fs::remove_file(&path).ok();
        presented
    })
}

#[test]
fn every_location_is_clocked_at_the_cell_its_declared_zone_states() {
    let report = &presented().bitstream;

    assert_eq!(report.profile_id, "c1541");
    assert_eq!(report.profile_name, "Commodore 1541");
    assert_eq!(report.reference_clock_hz, 16_000_000);
    assert_eq!(report.cycles_per_rotation, CYCLES_PER_ROTATION);
    assert_eq!(report.locations.len(), LOCATIONS);

    // The four documented zones, at their documented cells: the tracks
    // this capture holds fall in the first two, and each is clocked at
    // the rate its own zone declares rather than at one rate throughout.
    let cells: Vec<(u32, u64)> = {
        let mut cells: Vec<(u32, u64)> = report
            .locations
            .iter()
            .map(|location| (location.zone, location.cell_cycles_numerator))
            .collect();
        cells.sort_unstable();
        cells.dedup();
        cells
    };
    assert_eq!(cells, [(0, 52), (1, 56), (2, 60), (3, 64)]);

    for location in &report.locations {
        assert_eq!(location.cell_cycles_denominator, 1);
        assert_eq!(location.half_track_denominator, 1);
        assert_eq!(location.surface, Some(0));
        // About one rotation's worth of cells — and only about, which is
        // the point: the channel locks onto the recording's own phase,
        // so a disk written a little fast holds a few more bits than the
        // nominal rate states rather than being read as though it did
        // not.
        let nominal = CYCLES_PER_ROTATION / location.cell_cycles_numerator;
        assert!(
            location.cells * 20 > nominal * 19 && location.cells * 20 < nominal * 21,
            "{location:?} against a nominal {nominal}"
        );
        assert!(location.longest_zero_run >= 1, "{location:?}");
        assert_eq!(location.recorded_bits + location.resolved_bits, location.cells);
        // The medium's pulses all carry the strength its policy declared,
        // so nothing here was resolved by a rule instead of read.
        assert_eq!(location.resolved_bits, 0, "{location:?}");
        assert!(location.one_bits > 10_000, "{location:?}");
        // And the circle does not divide into cells: what is left over
        // is stated rather than rounded into a bit.
        assert!(
            location.wrap_slack_numerator < location.cell_cycles_numerator,
            "{location:?}"
        );
    }

    // GCR admits at most two zeros between transitions, so a disk whose
    // clocked tracks held longer runs everywhere would be saying the
    // channel was reading at the wrong cell. Nearly every location here
    // holds exactly the runs the encoding permits — and a location that
    // departs from it is reported as the number it is rather than
    // smoothed into the others.
    let departing: Vec<u64> = report
        .locations
        .iter()
        .filter(|location| location.longest_zero_run > 2)
        .map(|location| location.half_track_numerator)
        .collect();
    assert!(
        departing.len() * 4 < report.locations.len(),
        "{departing:?} of {} locations depart from the encoding",
        report.locations.len()
    );
}

#[test]
fn the_bitstream_states_what_it_does_not_carry_of_the_medium() {
    let report = &presented().bitstream;

    // A count is not an account here either: every entry says what was
    // not carried, in the terms the medium stated it.
    for loss in &report.declared_loss {
        assert!(!loss.code.is_empty());
        assert!(loss.detail.len() > 20, "{loss:?}");
        assert!(loss.count > 0, "{loss:?}");
    }
    // The seam each location's reduction located is carried up as the
    // angle it is; a duplicate the caller admitted would not be, and
    // neither would the medium's write-protect state.
    assert!(
        report
            .declared_loss
            .iter()
            .all(|loss| loss.code != "unexpressed-pulse-strength"),
        "{:?}",
        report.declared_loss
    );

    // The channel that produced it and the policy that produced the
    // medium are both stated, in that order.
    assert!(report.evidence.len() >= 5, "{:?}", report.evidence);
    assert!(report.evidence[0].contains("Commodore 1541"), "{:?}", report.evidence);
    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("the medium beneath it")),
        "{:?}",
        report.evidence
    );
    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("restarts at every detected transition")
                && line.contains("1/2 of a cell")),
        "{:?}",
        report.evidence
    );
}

#[test]
fn the_same_medium_and_policy_produce_the_same_bitstream() {
    assert_eq!(presented().bitstream, presented().repeated);
}

#[test]
fn the_codec_frames_on_the_declared_landmark_and_claims_nothing_past_it() {
    let report = &presented().bytestream;

    assert_eq!(report.profile_id, "c1541");
    assert_eq!(report.codec_id, "c1541-gcr");
    assert_eq!(report.symbol_bits, 5);
    assert_eq!(report.data_bits, 4);
    assert_eq!(report.symbols_per_byte, 2);
    assert_eq!(report.locations.len(), LOCATIONS);

    for location in &report.locations {
        // A 1541 writes a sync ahead of each header and each data field,
        // so a track's landmark count runs with its sector count. What
        // the layer states is that it found them, and nothing about what
        // any of them introduces.
        assert!(location.alignments >= 30, "{location:?}");
        assert!(location.longest_landmark_bits >= 10, "{location:?}");
        assert!(location.bytes > 4_000, "{location:?}");
        assert_eq!(
            location.resolved_bytes + location.unassigned_groups,
            location.bytes
        );
        // Most of the recording resolves through the family's own table.
        assert!(
            location.resolved_bytes * 10 > location.bytes * 9,
            "{location:?}"
        );
    }
}

#[test]
fn a_pattern_the_family_assigns_nothing_to_is_refused_or_kept_as_its_bits() {
    // Which of the two happens is the caller's declaration, and the
    // refusing one names the location and the bit rather than reporting
    // a value the recording never held.
    let refused = presented()
        .unassigned
        .as_ref()
        .expect("a real recording holds patterns the table does not assign");
    assert_eq!(refused.0, ErrorCategory::InvalidImage);
    assert!(refused.1.contains("does not assign"), "{}", refused.1);
    assert!(refused.1.contains("never recorded"), "{}", refused.1);

    let report = &presented().bytestream;
    let by_code = |code: &str| {
        report
            .declared_loss
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code} is not accounted for: {:?}", report.declared_loss))
    };
    // The landmark's own bits frame the bytes rather than becoming one,
    // the bits no group covers are stated, and the groups the table
    // assigns nothing to keep what they held.
    by_code("alignment-landmark");
    by_code("unframed-bits");
    by_code("unassigned-symbol");

    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("no byte here is a header")),
        "{:?}",
        report.evidence
    );
    // And the whole chain beneath it is still readable two layers up.
    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("observation 0 of each location was selected")),
        "{:?}",
        report.evidence
    );
}

#[test]
fn neither_layer_is_held_whole_in_memory() {
    // Both are addressed out of private session storage under a declared
    // bound (P27), and producing them made neither resident.
    let presented = presented();
    assert!(presented.bitstream_backing > 0);
    assert!(presented.bytestream_backing > 0);
    assert!(
        presented.bitstream_resident <= 1 << 20,
        "{} bytes resident",
        presented.bitstream_resident
    );
    assert!(
        presented.bytestream_resident <= 1 << 20,
        "{} bytes resident",
        presented.bytestream_resident
    );
}
