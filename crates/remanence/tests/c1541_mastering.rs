// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Mastering the prepared capture set into one 1541 flux medium.
//!
//! The claim under test is P29's: every reduction is a named policy
//! input, the plan computes the whole transformation and writes nothing,
//! the loss is declared before anything is produced in the source's own
//! terms, and the same sources and policy produce the same result.
//!
//! What is deliberately absent is a shortcut. The set holds locations
//! whose content their neighbour also holds, and the profile refuses
//! them until the caller declares which they are — so the first plan
//! below is a refusal, and the journey only proceeds once the policy
//! says something the evidence could not.

use std::sync::OnceLock;

use remanence::{
    CaptureSet, DuplicatePolicy, ErrorCategory, MasteringPolicy, ObservationPolicy, OriginPolicy,
    ProjectionPolicy, PulseStrengthPolicy,
};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";
/// The 1541's reference clock across one 300 RPM rotation.
const CYCLES_PER_ROTATION: u64 = 3_200_000;
/// 35 recorded tracks, less the two the caller left out below.
const MASTERED_LOCATIONS: usize = 33;

fn policy() -> MasteringPolicy {
    MasteringPolicy {
        side: 0,
        observation: ObservationPolicy::Selected { ordinal: 0 },
        // The set's duplicated locations are declared an unmoved
        // instrument and left out — which is a claim the caller makes,
        // not one the evidence supports on its own.
        duplicate: DuplicatePolicy::Omit,
        projection: ProjectionPolicy::DeclareLoss,
        pulse_strength: PulseStrengthPolicy::Declared { state: 2 },
        origin: OriginPolicy::Declared,
        seed: 0x0123_4567_89ab_cdef,
    }
}

struct Mastered {
    plan: remanence::MasteringPlanReport,
    medium_locations: u64,
    medium_backing: u64,
    resident: u64,
    /// The plan made twice from the same sources and policy.
    repeated: remanence::MasteringPlanReport,
    /// What the profile said before the caller declared anything.
    undeclared: (ErrorCategory, String),
    /// What it said when the policy named a side the capture does not
    /// hold.
    absent_side: String,
    /// And when it selected an observation no location holds.
    absent_observation: String,
}

fn mastered() -> &'static Mastered {
    static MASTERED: OnceLock<Mastered> = OnceLock::new();
    MASTERED.get_or_init(|| {
        let path = std::env::temp_dir()
            .join(format!("remanence-mastering-{}.7z", std::process::id()));
        std::fs::copy(common::ensure_fixture(ARCHIVE), &path).expect("fixture copies");
        let set = CaptureSet::open(&path).expect("the set opens");

        let undeclared = set
            .plan_c1541_mastering(MasteringPolicy {
                duplicate: DuplicatePolicy::Declared,
                ..policy()
            })
            .expect_err("the profile refuses a duplicate the caller has not declared");
        let absent_side = set
            .plan_c1541_mastering(MasteringPolicy { side: 7, ..policy() })
            .expect_err("a side the capture does not hold is refused")
            .to_string();
        let absent_observation = set
            .plan_c1541_mastering(MasteringPolicy {
                observation: ObservationPolicy::Selected { ordinal: 99 },
                ..policy()
            })
            .expect_err("an observation no location holds is refused")
            .to_string();

        let plan = set.plan_c1541_mastering(policy()).expect("the plan resolves");
        let repeated = set
            .plan_c1541_mastering(policy())
            .expect("the plan resolves again")
            .report()
            .clone();
        let report = plan.report().clone();
        let medium = plan.execute(1 << 20).expect("the medium is produced");

        let mastered = Mastered {
            plan: report,
            medium_locations: medium.locations(),
            medium_backing: medium.backing_bytes(),
            resident: medium.resident_bytes(),
            repeated,
            undeclared: (undeclared.category(), undeclared.to_string()),
            absent_side,
            absent_observation,
        };
        drop(medium);
        drop(set);
        std::fs::remove_file(&path).ok();
        mastered
    })
}

#[test]
fn a_reduction_the_policy_does_not_name_is_refused_rather_than_defaulted() {
    let (category, message) = &mastered().undeclared;
    assert_eq!(*category, ErrorCategory::InvalidImage);
    // The refusal says what the evidence could not settle and whose
    // decision it is, rather than naming a symptom.
    assert!(
        message.contains("flux alone cannot tell"),
        "{message}"
    );
    assert!(message.contains("caller must declare"), "{message}");

    assert!(
        mastered().absent_side.contains("names side 7"),
        "{}",
        mastered().absent_side
    );
    assert!(
        mastered()
            .absent_observation
            .contains("the nearest one is not a substitute"),
        "{}",
        mastered().absent_observation
    );
}

#[test]
fn the_plan_states_the_frame_and_every_half_track_it_will_hold() {
    let plan = &mastered().plan;

    assert_eq!(plan.profile_id, "c1541");
    assert_eq!(plan.reference_clock_hz, 16_000_000);
    assert_eq!(plan.cycles_per_rotation, CYCLES_PER_ROTATION);
    // A 1541 drive never observes index, so the circle begins at the
    // track's own seam rather than at a datum of the instrument.
    assert_eq!(plan.origin_rule, "seam");

    assert_eq!(plan.locations.len(), MASTERED_LOCATIONS);
    for location in &plan.locations {
        // Half-track addressed: two source steps to a track, so step 0
        // is track 1 and the addressing is exact rather than rounded.
        assert_eq!(location.half_track_denominator, 1);
        assert_eq!(
            location.half_track_numerator,
            location.source_position.numerator / 2 + 1
        );
        assert_eq!(location.observation_ordinal, 0);
        assert!(location.pulses > 20_000, "{location:?}");
        assert!(location.origin_cycles < CYCLES_PER_ROTATION);
        assert!(location.seam_cycles.is_some(), "{location:?}");
        // Every pulse carries the strength the policy declared, and no
        // pulse is weakened by a rule nobody named.
        assert_eq!(location.strong_pulses, location.pulses);
        assert_eq!(location.weak_pulses, 0);
    }

    // Tracks 1 to 35 less the two the caller left out.
    let tracks: Vec<u64> = plan
        .locations
        .iter()
        .map(|location| location.half_track_numerator)
        .collect();
    assert_eq!(tracks.first(), Some(&1));
    assert_eq!(tracks.last(), Some(&33));
    assert!(tracks.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn the_loss_is_declared_before_anything_is_produced_and_in_the_sources_terms() {
    let plan = &mastered().plan;

    // A count is not an account: every entry says what was lost, in the
    // terms the source recorded it, beside how much of it there was.
    assert!(plan.declared_loss.len() >= 6, "{:?}", plan.declared_loss);
    for loss in &plan.declared_loss {
        assert!(!loss.code.is_empty());
        assert!(loss.detail.len() > 20, "{loss:?}");
        assert!(loss.count > 0, "{loss:?}");
    }

    let by_code = |code: &str| {
        plan.declared_loss
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code} is not accounted for: {:?}", plan.declared_loss))
    };
    // The unselected side, the unselected observations, the flux
    // outside the bounded revolutions, the markers the drive never
    // sees, the capture's own metadata and its transfer results.
    by_code("unselected-side");
    by_code("unselected-observation");
    by_code("outside-the-revolution");
    by_code("marker-channel");
    by_code("capture-metadata");
    by_code("transfer-result");
    // And the two locations the caller declared an unmoved instrument.
    assert_eq!(by_code("omitted-duplicate").count, 2);
    // The half-steps and the positions past the last declared zone.
    assert!(by_code("unadmitted-location").count > 40);

    // The policy that produced it all is stated in full beside it.
    assert_eq!(plan.evidence.len(), 6, "{:?}", plan.evidence);
    assert!(plan.evidence[0].contains("side 0"), "{:?}", plan.evidence);
}

#[test]
fn the_same_sources_and_policy_produce_the_same_plan() {
    // Reproducibility is the claim P29 makes, and it is testable
    // exactly: the plan is a value, and two of them compare equal.
    assert_eq!(mastered().plan, mastered().repeated);
}

#[test]
fn executing_produces_the_medium_the_plan_described() {
    let mastered = mastered();

    assert_eq!(mastered.medium_locations as usize, MASTERED_LOCATIONS);
    assert!(mastered.medium_backing > 0);
    // The medium is addressed out of private session storage rather
    // than held: nothing about producing it made it resident.
    assert!(
        mastered.resident <= 1 << 20,
        "{} bytes resident",
        mastered.resident
    );
}
