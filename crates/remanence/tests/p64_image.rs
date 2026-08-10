// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Saving the prepared capture set as a P64, and opening the result.
//!
//! End to end: the capture set is opened, the C1541 profile reduces it
//! to one flux medium under a declared policy, and the P64 adapter
//! reports what its container will and will not carry before it writes
//! anything. Then the artifact is reopened through the adapter's own
//! decode, which is the conformance claim — both ends are a flux
//! medium, so the comparison is a same-layer one.
//!
//! This is the journey U23 asks for, reached through the surface that
//! exists for captures alone rather than the media-first one the
//! withdrawn entry is owed in (D28): the capture set is its own root,
//! the reduction's policy is the caller's in full, and the write verb
//! belongs to the mastered medium. What is checked here is what that
//! surface does today — both accounts before the write, the loss named,
//! the round trip lossless, the refusals by name — all of which the
//! pledged entry still demands of whatever shape replaces it.
//!
//! What is deliberately absent is a pulse iterator. The transformation
//! is the surface, and everything below is compared by what the two
//! reports say about the same half-tracks.

use std::path::PathBuf;
use std::sync::OnceLock;

use remanence::{
    CaptureSet, DuplicatePolicy, MasteringPolicy, ObservationPolicy, OriginPolicy, P64Image,
    P64Report, ProjectionPolicy, PulseStrengthPolicy,
};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";
/// The 1541's reference clock across one 300 RPM rotation.
const CYCLES_PER_ROTATION: u64 = 3_200_000;
/// 35 recorded tracks, less the two the caller leaves out below.
const HALF_TRACKS: usize = 33;

fn policy() -> MasteringPolicy {
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

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("remanence-p64-{tag}-{}.p64", std::process::id()))
}

struct Saved {
    /// What the adapter said it would carry, before anything existed.
    claimed: P64Report,
    /// What it said it had carried, once the file was there.
    written: P64Report,
    /// The artifact, reopened through the adapter's own decode.
    reopened: P64Report,
    artifact_bytes: u64,
    resident: u64,
    /// What it said when the destination was already occupied.
    occupied: String,
    /// The destination that was occupied, and its content afterwards.
    survivor: Vec<u8>,
}

fn saved() -> &'static Saved {
    static SAVED: OnceLock<Saved> = OnceLock::new();
    SAVED.get_or_init(|| {
        let source = std::env::temp_dir().join(format!(
            "remanence-p64-source-{}.7z",
            std::process::id()
        ));
        std::fs::copy(common::ensure_fixture(ARCHIVE), &source).expect("fixture copies");
        let set = CaptureSet::open(&source).expect("the set opens");
        let mastered = set
            .plan_c1541_mastering(policy())
            .expect("the plan resolves")
            .execute(1 << 20)
            .expect("the medium is produced");

        let claimed = mastered.describe_p64().expect("the claim is computed");

        let destination = scratch("saved");
        std::fs::remove_file(&destination).ok();
        let written = mastered.write_p64(&destination).expect("the artifact writes");

        let occupied_path = scratch("occupied");
        std::fs::write(&occupied_path, b"someone else's file").expect("it is occupied");
        let occupied = mastered
            .write_p64(&occupied_path)
            .expect_err("an existing destination is refused")
            .to_string();
        let survivor = std::fs::read(&occupied_path).expect("it is still there");

        let image = P64Image::open_with_cache(&destination, 1 << 20).expect("the artifact reopens");
        let saved = Saved {
            claimed,
            written,
            reopened: image.inspect().clone(),
            artifact_bytes: std::fs::metadata(&destination).expect("it is there").len(),
            resident: image.resident_bytes(),
            occupied,
            survivor,
        };

        drop(image);
        drop(mastered);
        drop(set);
        std::fs::remove_file(&destination).ok();
        std::fs::remove_file(&occupied_path).ok();
        std::fs::remove_file(&source).ok();
        saved
    })
}

#[test]
fn the_container_states_what_it_will_carry_before_the_artifact_exists() {
    let saved = saved();

    // The claim computed before the write and the one reported after it
    // are the same claim: writing added nothing to the account.
    assert_eq!(saved.claimed.declared_loss, saved.written.declared_loss);
    assert_eq!(saved.claimed.half_tracks, saved.written.half_tracks);

    // A count is not an account, so every entry says what was lost.
    for loss in &saved.claimed.declared_loss {
        assert!(!loss.code.is_empty());
        assert!(loss.detail.len() > 20, "{loss:?}");
        assert!(loss.count > 0, "{loss:?}");
    }
    let by_code = |code: &str| {
        saved
            .claimed
            .declared_loss
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| {
                panic!(
                    "{code} is not accounted for: {:?}",
                    saved.claimed.declared_loss
                )
            })
    };
    // The whole declared policy, each half-track's own provenance, the
    // seam every one of them located, the rule that placed the circle's
    // start, and the medium's statement that it was derived at all.
    assert_eq!(by_code("reduction-policy").count, 6);
    assert_eq!(by_code("location-provenance").count, HALF_TRACKS as u64);
    assert_eq!(by_code("medium-fact").count, HALF_TRACKS as u64);
    by_code("located-origin");
    by_code("derivation");

    // And the claim itself is stated beside the account (P4).
    assert!(
        saved
            .claimed
            .evidence
            .iter()
            .any(|line| line.contains("index byte")),
        "{:?}",
        saved.claimed.evidence
    );
    assert!(
        saved
            .claimed
            .evidence
            .iter()
            .any(|line| line.contains("nothing is re-projected")),
        "{:?}",
        saved.claimed.evidence
    );
}

#[test]
fn the_artifact_reopens_as_the_same_half_tracks_it_was_written_from() {
    let saved = saved();

    assert_eq!(saved.written.half_tracks.len(), HALF_TRACKS);
    // The conformance claim: same addressing, same pulse counts, same
    // strengths, through the adapter's own decode.
    assert_eq!(saved.reopened.half_tracks, saved.written.half_tracks);

    // Tracks 1 to 35 less the two the caller left out, each addressed by
    // twice its family position with the side in bit 7.
    let first = &saved.reopened.half_tracks[0];
    assert_eq!(first.half_track_numerator, 1);
    assert_eq!(first.half_track_denominator, 1);
    assert_eq!(first.index, 2);
    assert_eq!(first.side, 0);
    assert_eq!(saved.reopened.half_tracks[HALF_TRACKS - 1].index, 66);
    for track in &saved.reopened.half_tracks {
        assert_eq!(u64::from(track.index), track.half_track_numerator * 2);
        assert!(track.pulses > 20_000, "{track:?}");
        // The policy declared one strength for every pulse, and the
        // crossing carried that rather than deciding something finer.
        assert_eq!(track.strong_pulses, track.pulses);
        assert_eq!(track.weak_pulses, 0);
        assert_eq!(track.absent_pulses, 0);
    }
}

#[test]
fn the_reopened_container_says_which_version_and_which_frame_it_is() {
    let saved = saved();

    assert_eq!(saved.reopened.format_id, "p64");
    assert_eq!(saved.reopened.version, 0);
    assert_eq!(saved.reopened.profile_id, "c1541");
    assert_eq!(saved.reopened.reference_clock_hz, 16_000_000);
    assert_eq!(saved.reopened.cycles_per_rotation, CYCLES_PER_ROTATION);
    // One recorded surface, so the container says single-sided, and the
    // medium made no write-protect claim for it to carry.
    assert!(!saved.reopened.double_sided);
    assert!(!saved.reopened.write_protected);

    // What the container never held is declared on the way out as well
    // as on the way in: a P64 records nothing of how its content came to
    // be, and the adapter says so rather than letting a reader assume.
    assert!(
        saved
            .reopened
            .declared_loss
            .iter()
            .any(|loss| loss.code == "container-provenance"),
        "{:?}",
        saved.reopened.declared_loss
    );
    assert!(
        saved
            .reopened
            .evidence
            .iter()
            .any(|line| line.contains("P64 signature")),
        "{:?}",
        saved.reopened.evidence
    );
}

#[test]
fn an_existing_destination_is_refused_and_left_exactly_as_it_was() {
    let saved = saved();

    assert!(saved.occupied.contains("already there"), "{}", saved.occupied);
    assert_eq!(saved.survivor, b"someone else's file");
}

#[test]
fn the_artifact_is_written_and_read_through_a_bounded_working_set() {
    let saved = saved();

    // Something over a megabyte of pulses, and neither writing it nor
    // reading it back held it (P27).
    assert!(saved.artifact_bytes > 100_000, "{}", saved.artifact_bytes);
    assert!(saved.resident <= 1 << 20, "{} bytes resident", saved.resident);
}
