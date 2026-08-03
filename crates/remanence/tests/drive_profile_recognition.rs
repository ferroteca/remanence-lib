// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Drive-profile recognition over the prepared capture set.
//!
//! The claim under test is the zone table: probing the data surface
//! recovers all four documented speed zones at their documented track
//! boundaries with their documented sector counts, from interval
//! statistics alone. Nothing here decodes anything — what leaves the
//! probe is a count, a density, an angle and an absence.
//!
//! The refusals matter as much as the claims. The half-step positions,
//! the unrecorded surface, and the step positions past the last declared
//! zone are all refused, each naming the rule it broke, and none of them
//! on a threshold chosen to make the fixture pass.

use std::sync::OnceLock;

use remanence::{CaptureSet, ErrorCategory, Recognition};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";

/// The four documented 1541 speed zones: track range and sector count.
const ZONES: [(u64, u64, u32); 4] = [(1, 17, 21), (18, 24, 19), (25, 30, 18), (31, 35, 17)];
/// The 1541 records 35 tracks; the capture reached 42 step positions.
const DECLARED_LOCATIONS: u64 = 35;

/// Everything one probe of the fixture established.
struct Probed {
    ranked: Recognition,
    pinned: Recognition,
    unknown_profile: (ErrorCategory, String),
}

/// The whole set decoded and probed once, shared by every assertion
/// below.
///
/// The probe is read-only and mutates nothing (P2), so one run answers
/// every question these tests ask of it — and decoding the archive five
/// times over would be minutes spent proving nothing extra.
fn probed() -> &'static Probed {
    static PROBED: OnceLock<Probed> = OnceLock::new();
    PROBED.get_or_init(|| {
        let path = std::env::temp_dir()
            .join(format!("remanence-profile-{}.7z", std::process::id()));
        std::fs::copy(common::ensure_fixture(ARCHIVE), &path).expect("fixture copies");
        let set = CaptureSet::open(&path).expect("the set opens");
        let error = set
            .recognize_as("apple2")
            .expect_err("a profile this build does not enroll is refused by name");
        let probed = Probed {
            ranked: set.recognize().expect("a drive profile claims this capture"),
            pinned: set.recognize_as("c1541").expect("the pinned profile answers"),
            unknown_profile: (error.category(), error.to_string()),
        };
        drop(set);
        std::fs::remove_file(&path).ok();
        probed
    })
}

#[test]
fn probing_the_data_surface_recovers_all_four_documented_zones() {
    let recognition = &probed().ranked;
    assert_eq!(recognition.pinned, None);
    assert_eq!(recognition.verdicts.len(), 1);

    let verdict = &recognition.verdicts[0];
    assert_eq!(verdict.profile_id, "c1541");
    assert_eq!(verdict.profile_name, "Commodore 1541");
    assert_eq!(verdict.locations_declared, DECLARED_LOCATIONS);
    assert_eq!(
        u64::from(verdict.locations_claimed),
        DECLARED_LOCATIONS,
        "every declared location should be recovered: {:?}",
        verdict.evidence
    );
    assert_eq!(verdict.confidence, 100);

    // Every zone at its documented boundaries, fully recovered, and
    // every claimed location in it holding exactly the records the zone
    // claims. That agreement across four zones is the signature.
    assert_eq!(verdict.zones.len(), ZONES.len());
    for (zone, (first, last, records)) in verdict.zones.iter().zip(ZONES) {
        assert_eq!((zone.first_location, zone.last_location), (first, last));
        assert_eq!(zone.records_declared, records);
        assert_eq!(zone.locations_declared, last - first + 1);
        assert_eq!(
            zone.locations_claimed, zone.locations_declared,
            "zone {first}-{last} recovered {} of {}",
            zone.locations_claimed, zone.locations_declared
        );
    }

    for location in verdict.locations.iter().filter(|location| location.claimed) {
        let family = location
            .family_location
            .expect("a claimed location is addressed");
        let (_, _, records) = ZONES
            .iter()
            .copied()
            .find(|(first, last, _)| (*first..=*last).contains(&family))
            .expect("a claimed location sits in a zone");
        assert_eq!(location.records, records, "track {family}");
        // The spacing between records repeats to the bit, which is what
        // a recorded track does and a straddled step position does not.
        assert_eq!(location.record_bits_deviation, 0, "track {family}");
        assert!(location.record_bits.is_some(), "track {family}");
        // Counting is the discriminator; the projected rate is
        // corroboration, and it lands within a few per cent of what the
        // zone claims — the writing drive's own speed sits in that gap.
        let cell = location
            .cell_millicycles
            .expect("a claimed location derived a cell");
        let nominal = location
            .nominal_cell_millicycles
            .expect("its zone claims one");
        assert!(
            cell.abs_diff(nominal) * 20 < nominal,
            "track {family}: derived {cell} against a claimed {nominal}"
        );
        // And every observation of it agreed about what it holds.
        assert_eq!(
            location.observations_agreeing, location.observations,
            "track {family}"
        );
    }
}

#[test]
fn every_refusal_names_the_rule_it_broke() {
    let verdict = &probed().ranked.verdicts[0];

    // Every position the capture supplied is accounted for: 84 step
    // positions by two heads, each either claimed or refused by name.
    assert_eq!(verdict.locations.len(), 168);
    for location in &verdict.locations {
        assert_eq!(
            location.claimed,
            location.refusal.is_none(),
            "{} is neither claimed nor refused",
            location.artifact
        );
    }

    // The unrecorded surface is refused everywhere. Which head carries
    // the recording is not this layer's to assume — it is what the
    // evidence said.
    for location in verdict.locations.iter().filter(|l| l.head == Some(1)) {
        assert!(!location.claimed, "{} was claimed", location.artifact);
    }

    // A half-step position is not one the family's addressing covers,
    // and that is a statement about addressing rather than about what
    // the position holds.
    let half = verdict
        .locations
        .iter()
        .find(|l| l.position.numerator == 1 && l.head == Some(0))
        .expect("step position 1 was captured");
    assert_eq!(half.family_location, None);
    assert!(
        half.refusal
            .as_deref()
            .expect("refused")
            .contains("addressing covers"),
        "{:?}",
        half.refusal
    );

    // A step position past the last declared zone is refused by
    // declaration: no threshold decides it, the density map simply does
    // not reach there.
    let past = verdict
        .locations
        .iter()
        .find(|l| l.position.numerator == 70 && l.head == Some(0))
        .expect("step position 70 was captured");
    assert_eq!(past.family_location, Some(36));
    assert!(
        past.refusal
            .as_deref()
            .expect("refused")
            .contains("outside every zone"),
        "{:?}",
        past.refusal
    );
}

#[test]
fn a_position_holding_its_neighbours_content_is_reported_not_resolved() {
    let verdict = &probed().ranked.verdicts[0];

    // Three consecutive step positions on the data surface carry the
    // same content. The middle one is a half-step the family's
    // addressing would not expect to hold a track at all, and it passes
    // every structural test — because it is reading a real track, just
    // not its own. Flux alone cannot tell that from an instrument that
    // did not move, so it is reported and never resolved.
    let at = |position: u64| {
        verdict
            .locations
            .iter()
            .find(|l| l.position.numerator == position && l.head == Some(0))
            .unwrap_or_else(|| panic!("step position {position} was captured"))
    };
    let middle = at(67);
    assert_eq!(middle.family_location, None, "step 67 is a half-step");
    assert!(
        middle.duplicate_of.is_some(),
        "step 67 duplicates a neighbour"
    );
    assert_eq!(at(66).duplicate_of.map(|p| p.numerator), Some(67));
    assert_eq!(at(68).duplicate_of.map(|p| p.numerator), Some(67));

    // It is reported beside the claim rather than instead of it: tracks
    // 34 and 35 are still claimed, and what the caller does about the
    // duplication is the caller's to declare.
    assert!(at(66).claimed && at(68).claimed);
    assert!(
        verdict
            .evidence
            .iter()
            .any(|line| line.contains("their neighbour also holds")),
        "{:?}",
        verdict.evidence
    );

    // And a distinct track is not reported as a duplicate of anything.
    assert_eq!(at(0).duplicate_of, None);
}

#[test]
fn the_verdict_carries_the_observations_that_produced_it() {
    let verdict = &probed().ranked.verdicts[0];

    // "C1541, confidence 100" is not an answer. The observations are.
    assert!(verdict.evidence.len() >= 5, "{:?}", verdict.evidence);
    assert!(
        verdict
            .evidence
            .iter()
            .any(|line| line.contains("claims 35 of the 35 locations")),
        "{:?}",
        verdict.evidence
    );
    assert!(
        verdict
            .evidence
            .iter()
            .any(|line| line.contains("21 records each")),
        "{:?}",
        verdict.evidence
    );
    // And the declared facts say they are the family's, not the
    // capture's.
    assert!(
        verdict
            .evidence
            .iter()
            .any(|line| line.contains("not the capture's")),
        "{:?}",
        verdict.evidence
    );

    // The seam is reported as an angle in the drive's own cycles — the
    // one departure from a spacing that otherwise repeats — never as
    // anything read out of the track.
    let seams = verdict
        .locations
        .iter()
        .filter(|l| l.claimed && l.seam_cycles.is_some())
        .count();
    assert!(seams > 0, "no claimed location located its seam");
    for location in verdict.locations.iter().filter(|l| l.claimed) {
        if let Some(seam) = location.seam_cycles {
            assert!(seam < 3_200_000, "a seam past one rotation: {seam}");
        }
    }
}

#[test]
fn a_caller_may_pin_a_profile_and_what_was_pinned_travels_with_the_result() {
    let recognition = &probed().pinned;
    assert_eq!(recognition.pinned.as_deref(), Some("c1541"));
    assert_eq!(recognition.verdicts.len(), 1);
    assert_eq!(recognition.verdicts[0].confidence, 100);
    assert!(
        recognition
            .evidence
            .iter()
            .any(|line| line.contains("pinned")),
        "{:?}",
        recognition.evidence
    );

    let (category, message) = &probed().unknown_profile;
    assert_eq!(*category, ErrorCategory::NotFound);
    assert!(message.contains("apple2"), "{message}");
}
