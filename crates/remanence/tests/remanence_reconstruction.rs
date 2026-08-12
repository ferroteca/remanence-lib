// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The gap-first reconstruction, from outside the crate.
//!
//! A KryoFlux capture set reduces to one remanence image on the
//! strength of all the evidence rather than the choice of one
//! revolution: every revolution of every location aligned by gap
//! correspondence, the cell lattice measured from the intervals
//! themselves, the angles integrated gap-first, coherence decided per
//! transition, and the fat track merged under measured agreement.
//!
//! What is exercised here is the surface and the discipline, not the
//! arithmetic — the numerics are the crate's own and are tested where
//! they live. The claims made here are the ones a caller depends on:
//! the plan computes everything and writes nothing, the account is
//! complete before the image exists, executing adds nothing to it, and
//! what comes back is the family's ordinary physical stratum rather
//! than a second root beside it.
//!
//! The journey runs on the repository's own Pinball Construction Set
//! capture, which the fixture preparation produces.

use std::sync::OnceLock;

use remanence::{CaptureSet, ReconstructionPolicy, RecordingSelection};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";

/// The reduction is the expensive part of this file, so it runs once
/// and every test reads what it produced. A capture of one side of a
/// 5.25-inch disk is 84 step positions of several revolutions each.
struct Reduced {
    /// What the plan said it would produce, before anything existed.
    planned: remanence::ReconstructionReport,
    /// What the image it produced reports of itself.
    image: remanence::RemanenceImageReport,
    backing_bytes: u64,
    resident_bytes: u64,
    /// What the reduction said when asked for a side the capture does
    /// not hold.
    absent_side: String,
}

fn reduced() -> &'static Reduced {
    static REDUCED: OnceLock<Reduced> = OnceLock::new();
    REDUCED.get_or_init(|| {
        // A copy of its own, so the P7 claim this takes cannot collide
        // with another test binary reading the same fixture.
        let source = std::env::temp_dir().join(format!(
            "remanence-reconstruction-source-{}.7z",
            std::process::id()
        ));
        std::fs::copy(common::ensure_fixture(ARCHIVE), &source).expect("the fixture copies");
        let set = CaptureSet::open(&source).expect("the capture set opens");

        let plan = set
            .plan_reconstruction(&ReconstructionPolicy {
                side: 0,
                recordings: RecordingSelection::Measured,
            })
            .expect("the reduction plans");
        let planned = plan.report().clone();

        let absent_side = set
            .plan_reconstruction(&ReconstructionPolicy {
                side: 7,
                recordings: RecordingSelection::Measured,
            })
            .expect_err("a side the capture does not hold is refused")
            .to_string();

        // A tight bound, so the working set is visibly narrower than
        // the image (P27) rather than incidentally so.
        let image = plan.execute(1 << 20).expect("the plan executes");
        let reduced = Reduced {
            planned,
            image: image.inspect(),
            backing_bytes: image.backing_bytes(),
            resident_bytes: image.resident_bytes(),
            absent_side,
        };

        drop(image);
        drop(set);
        let _ = std::fs::remove_file(&source);
        reduced
    })
}

#[test]
fn the_plan_states_the_whole_reduction_before_the_image_exists() {
    let reduced = reduced();

    assert_eq!(reduced.planned.format_id, "remanence");
    assert_eq!(reduced.planned.side, 0);
    assert_eq!(
        reduced.planned.swept_positions, 84,
        "a 5.25-inch capture sweeps 84 step positions of the one side"
    );

    // The recordings are measured from the evidence rather than
    // asserted: a position records where its revolutions resolve the
    // same transitions.
    assert!(
        (30..=40).contains(&reduced.planned.recorded_positions.len()),
        "the reference disk records about 35 positions: {:?}",
        reduced.planned.recorded_positions
    );
    assert!(
        reduced
            .planned
            .recorded_positions
            .windows(2)
            .all(|pair| pair[0] < pair[1]),
        "the positions are reported in the instrument's own order"
    );

    // Every orbit the plan describes is described whole: where it was
    // read, where it sits, and what the evidence beneath it was.
    assert!(!reduced.planned.orbits.is_empty());
    for orbit in &reduced.planned.orbits {
        assert!(orbit.radius_microns > 0, "{orbit:?}");
        assert!(orbit.revolutions >= 1, "{orbit:?}");
        assert_eq!(
            orbit.transition_counts.len(),
            orbit.revolutions as usize,
            "each revolution's raw count is reported: {orbit:?}"
        );
        if orbit.admitted {
            assert!(orbit.points > 0, "{orbit:?}");
            assert!(orbit.coherent_points <= orbit.points, "{orbit:?}");
            assert!(
                orbit.implied_cell_millidivisions > 0,
                "an admitted orbit closes on a cell: {orbit:?}"
            );
        }
    }

    // A count is not an account: every entry says what was left behind.
    assert!(!reduced.planned.declared_loss.is_empty());
    for loss in &reduced.planned.declared_loss {
        assert!(!loss.code.is_empty(), "{loss:?}");
        assert!(loss.detail.len() > 20, "{loss:?}");
        assert!(loss.count > 0, "{loss:?}");
    }
    let by_code = |code: &str| {
        reduced
            .planned
            .declared_loss
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| {
                panic!(
                    "{code} is not accounted for: {:?}",
                    reduced.planned.declared_loss
                )
            })
    };
    // The unselected side is the whole other head; the markers and the
    // flux outside a bounded revolution are what the envelope held that
    // an image of the surfaces has no place for.
    by_code("unselected-side");
    by_code("marker-channel");
    by_code("outside-the-revolution");

    // And the claim itself is stated beside the account (P4).
    assert!(!reduced.planned.evidence.is_empty());
}

#[test]
fn the_reduction_answers_with_the_family_s_own_image() {
    let reduced = reduced();

    // Not a second root: what comes back is the same report a
    // `.remanence` artifact opens to.
    assert_eq!(reduced.image.form_factor, "5.25-inch");
    assert_eq!(reduced.image.angular_divisions, 1 << 28);
    assert_eq!(reduced.image.surfaces, vec![0]);

    // Every admitted orbit, and nothing else, reached the image.
    let admitted: Vec<&remanence::ReconstructedOrbit> = reduced
        .planned
        .orbits
        .iter()
        .filter(|orbit| orbit.admitted)
        .collect();
    assert_eq!(reduced.image.orbits.len(), admitted.len());
    // The plan reports orbits outermost-first, the way the instrument
    // swept them; the image keys them by radius, innermost-first. The
    // radius is what both agree on, so it is what they are matched by.
    for planned in &admitted {
        let held = reduced
            .image
            .orbits
            .iter()
            .find(|orbit| orbit.radius_microns == planned.radius_microns)
            .unwrap_or_else(|| panic!("the image holds the admitted orbit {planned:?}"));
        assert_eq!(held.surface, 0);
        assert_eq!(planned.points, held.points);
        assert_eq!(planned.coherent_points, held.coherent_points);
        assert_eq!(planned.unaligned_spans, held.unaligned_spans);
    }

    // The orbits ascend by radius, which is the model's own key order,
    // and the fat track has been merged rather than asserted: a whole
    // side reads fewer orbits than the instrument swept positions.
    assert!(
        reduced
            .image
            .orbits
            .windows(2)
            .all(|pair| pair[0].radius_microns < pair[1].radius_microns)
    );
    assert!(
        (reduced.image.orbits.len() as u32) < reduced.planned.swept_positions,
        "the fat track merges: {} orbits from {} positions",
        reduced.image.orbits.len(),
        reduced.planned.swept_positions
    );

    // The image carries the reduction that produced it (P4) — the
    // declared policy first, then what the reduction measured.
    assert!(
        reduced
            .image
            .provenance
            .iter()
            .any(|note| note.contains("side: 0")),
        "{:?}",
        reduced.image.provenance
    );
    assert!(
        reduced
            .image
            .provenance
            .iter()
            .any(|note| note.contains("recordings: measured")),
        "{:?}",
        reduced.image.provenance
    );

    // The points are never held whole: the declared bound is what the
    // working set answers to, not the size of the image (P27).
    assert!(reduced.backing_bytes > reduced.resident_bytes);
}

#[test]
fn a_side_the_capture_does_not_hold_is_refused_naming_what_it_has() {
    let reduced = reduced();
    assert!(
        reduced.absent_side.contains("side 7"),
        "the refusal names the side asked for: {}",
        reduced.absent_side
    );
    assert!(
        reduced.absent_side.contains('0'),
        "and the sides the capture actually holds: {}",
        reduced.absent_side
    );
}

/// The declared tier: a caller who knows which positions carry
/// recordings says so, and the reduction honours it rather than
/// measuring. Cheap enough to run on its own capture, being two
/// positions rather than eighty-four.
#[test]
fn declared_recordings_are_honoured_rather_than_measured() {
    let source = std::env::temp_dir().join(format!(
        "remanence-reconstruction-declared-{}.7z",
        std::process::id()
    ));
    std::fs::copy(common::ensure_fixture(ARCHIVE), &source).expect("the fixture copies");
    let set = CaptureSet::open(&source).expect("the capture set opens");

    let declared = vec![0u64, 2, 4];
    let plan = set
        .plan_reconstruction(&ReconstructionPolicy {
            side: 0,
            recordings: RecordingSelection::Declared(declared.clone()),
        })
        .expect("the declared selection plans");
    let report = plan.report();

    assert_eq!(
        report.recorded_positions, declared,
        "what the caller declared is what the reduction used"
    );
    assert!(
        report.evidence.iter().any(|line| line.contains("declared")),
        "the reduction says the selection was the caller's: {:?}",
        report.evidence
    );
    // The image holds the declared positions' orbits, whatever the
    // count-spread discriminator would have said about them.
    let image = plan.execute(1 << 20).expect("the plan executes");
    assert!(!image.inspect().orbits.is_empty());

    drop(image);
    drop(set);
    let _ = std::fs::remove_file(&source);
}

/// A position the capture does not hold is a refusal rather than a
/// silently dropped assertion: the caller named something that is not
/// there and is told so.
#[test]
fn a_declared_position_the_capture_lacks_is_refused_by_name() {
    let source = std::env::temp_dir().join(format!(
        "remanence-reconstruction-absent-{}.7z",
        std::process::id()
    ));
    std::fs::copy(common::ensure_fixture(ARCHIVE), &source).expect("the fixture copies");
    let set = CaptureSet::open(&source).expect("the capture set opens");

    let refusal = set
        .plan_reconstruction(&ReconstructionPolicy {
            side: 0,
            recordings: RecordingSelection::Declared(vec![0, 900]),
        })
        .expect_err("a position the capture does not hold is refused")
        .to_string();
    assert!(
        refusal.contains("900"),
        "the refusal names the position: {refusal}"
    );

    drop(set);
    let _ = std::fs::remove_file(&source);
}
