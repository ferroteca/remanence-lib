// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Saving the prepared capture set as a P64, and opening the result.
//!
//! End to end: the capture set is opened, the gap-first reduction turns
//! it into a remanence image, and the P64 adapter reports what its
//! container will and will not carry before it writes anything. Then the
//! artifact is reopened through the adapter's own decode, which is the
//! conformance claim — both ends are a flux medium, so the comparison is
//! a same-layer one.
//!
//! This is the journey U23 asks for, reached through the surface that
//! exists today rather than the media-first one the withdrawn entry is
//! owed in (D28): the capture set is its own root and the write verb
//! belongs to the image. What is checked here is what that surface
//! does — both accounts before the write, the loss named, the round
//! trip lossless, the refusals by name — all of which the pledged entry
//! still demands of whatever shape replaces it.
//!
//! What is deliberately absent is a pulse iterator. The transformation
//! is the surface, and everything below is compared by what the two
//! reports say about the same half-tracks.

use std::path::PathBuf;
use std::sync::OnceLock;

use remanence::{CaptureSet, P64Image, P64Report, ReconstructionPolicy, RecordingSelection};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";
/// The 1541's reference clock across one 300 RPM rotation.
const CYCLES_PER_ROTATION: u64 = 3_200_000;
/// The recordings the reduction admits from a whole side, the fat track
/// merged rather than asserted.
const HALF_TRACKS: usize = 36;

fn policy() -> ReconstructionPolicy {
    ReconstructionPolicy {
        side: 0,
        recordings: RecordingSelection::Measured,
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
        let source =
            std::env::temp_dir().join(format!("remanence-p64-source-{}.7z", std::process::id()));
        std::fs::copy(common::ensure_fixture(ARCHIVE), &source).expect("fixture copies");
        let set = CaptureSet::open(&source).expect("the set opens");
        let image = set
            .plan_reconstruction(&policy())
            .expect("the reduction plans")
            .execute(1 << 20)
            .expect("the image is produced");

        let claimed = image.describe_p64().expect("the claim is computed");

        let destination = scratch("saved");
        std::fs::remove_file(&destination).ok();
        let written = image.write_p64(&destination).expect("the artifact writes");

        let occupied_path = scratch("occupied");
        std::fs::write(&occupied_path, b"someone else's file").expect("it is occupied");
        let occupied = image
            .write_p64(&occupied_path)
            .expect_err("an existing destination is refused")
            .to_string();
        let survivor = std::fs::read(&occupied_path).expect("it is still there");

        let container =
            P64Image::open_with_cache(&destination, 1 << 20).expect("the artifact reopens");
        let saved = Saved {
            claimed,
            written,
            reopened: container.inspect().clone(),
            artifact_bytes: std::fs::metadata(&destination).expect("it is there").len(),
            resident: container.resident_bytes(),
            occupied,
            survivor,
        };

        drop(container);
        drop(image);
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
    // The whole declared policy — the reduction's own account, which
    // travels into the medium ahead of the projection's two notes and
    // which a P64 records nothing of — each half-track's own provenance,
    // the rule that placed the circle's start, and the medium's
    // statement that it was derived at all.
    assert!(
        by_code("reduction-policy").count > 2,
        "the reduction's account reaches the medium and is declared lost: {:?}",
        by_code("reduction-policy")
    );
    assert_eq!(by_code("location-provenance").count, HALF_TRACKS as u64);
    by_code("located-origin");
    by_code("derivation");
    // The image's own facts the projection replaces: the centre radius
    // in microns becomes the half-track a 96 tpi drive would find it at,
    // and the write geometry has no field in a container of pulse
    // positions.
    by_code("measured-radius");
    by_code("write-geometry");
    // There is no `medium-fact` entry, and that is the honest answer
    // rather than a gap: a remanence image states no write protection or
    // other medium-level fact for the projection to carry or lose.
    assert!(
        !saved
            .claimed
            .declared_loss
            .iter()
            .any(|loss| loss.code == "medium-fact"),
        "{:?}",
        saved.claimed.declared_loss
    );

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

    // Every recording the reduction admitted, addressed by twice its
    // family position with the side in bit 7 — and the half-tracks
    // between the whole ones address as the odd indices, which is what
    // the fat track's fringe reads land on.
    let first = &saved.reopened.half_tracks[0];
    assert_eq!(first.half_track_numerator, 1);
    assert_eq!(first.half_track_denominator, 1);
    assert_eq!(first.index, 2);
    assert_eq!(first.side, 0);
    assert_eq!(saved.reopened.half_tracks[HALF_TRACKS - 1].index, 70);
    for track in &saved.reopened.half_tracks {
        assert_eq!(
            u64::from(track.index),
            track.half_track_numerator * 2 / track.half_track_denominator
        );
        assert!(track.pulses > 20_000, "{track:?}");
        // The image carries no per-pulse strength — uncertainty rides
        // the report instead — so the projection states one strength for
        // every pulse rather than inventing something finer.
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

    assert!(
        saved.occupied.contains("already there"),
        "{}",
        saved.occupied
    );
    assert_eq!(saved.survivor, b"someone else's file");
}

#[test]
fn the_artifact_is_written_and_read_through_a_bounded_working_set() {
    let saved = saved();

    // Something over a megabyte of pulses, and neither writing it nor
    // reading it back held it (P27).
    assert!(saved.artifact_bytes > 100_000, "{}", saved.artifact_bytes);
    assert!(
        saved.resident <= 1 << 20,
        "{} bytes resident",
        saved.resident
    );
}
