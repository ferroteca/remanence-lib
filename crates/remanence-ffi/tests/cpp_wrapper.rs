// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C++ presentation, exercised from C++ (S2, D53).
//!
//! `c_abi_boundary.rs` crosses the boundary as a C caller meets it.
//! This crosses it as a C++ caller meets it — through
//! `include/remanence.hpp`, the hand-maintained header that derives an
//! idiomatic surface from the same ABI. What it checks is that header's
//! own claims, none of which the C tests can reach: that a refusal
//! arrives as a typed exception carrying the delivered category, that a
//! destructor discharges every owned handle, that a moved-from wrapper
//! frees nothing, and that the ABI's honest absences stay absences
//! rather than becoming throws.
//!
//! `tests/c/wrapper.cpp` is the caller, one group per test here.
//!
//! **The groups need no fixture but one.** Every journey is made on a
//! medium the suite authors itself, which is what makes them run on a
//! fresh clone; `a_real_artifact_reports_and_reads` is the exception,
//! because a layered report and a namespace above it are answers only a
//! recording has.
//!
//! **These need the built library, which `cargo test` does not
//! produce.** `cargo build` does, and AGENTS.md orders it first.

mod common;

use common::{run_c, run_c_probe, skipping, workspace_dir};

/// The artifact the report group walks. A real one, because a schema,
/// composed volumes and a filesystem above them are what it is about.
const FIXTURE: &str = "crates/remanence/tests/fixtures/freedos-parttest.qcow2";

fn group(name: &str, args: &[&str]) {
    if skipping() {
        return;
    }
    print!("{}", run_c("wrapper", &[&[name], args].concat()));
}

#[test]
fn the_catalogs_cross_as_vectors_and_scoped_enums() {
    group("catalogs", &[]);
}

#[test]
fn a_refusal_arrives_as_a_typed_exception() {
    group("refusals", &[]);
}

#[test]
fn an_authored_medium_makes_the_whole_journey() {
    group("authorship", &[]);
}

#[test]
fn a_session_owned_view_owns_nothing_itself() {
    group("views", &[]);
}

#[test]
fn owned_handles_move_and_release_as_they_claim() {
    group("lifetimes", &[]);
}

#[test]
fn an_honest_absence_is_an_empty_optional() {
    group("absences", &[]);
}

#[test]
fn a_real_artifact_reports_and_reads() {
    if skipping() {
        return;
    }
    let fixture = workspace_dir().join(FIXTURE);
    assert!(
        fixture.exists(),
        "the fixture {} is missing, so the wrapper was not walked over anything \
         with a layout",
        fixture.display()
    );
    group("report", &[FIXTURE]);
}

/// The `_free` discipline, asserted for a caller who writes no frees.
///
/// This is what D47 said the leak probe would serve when this
/// presentation landed: RAII is exactly where a missing free becomes
/// invisible, there being no call site to inspect. The probe library is
/// built by the harness with `--features leak-probe` and never ships.
#[test]
fn destructors_give_back_what_constructors_took() {
    if skipping() {
        return;
    }
    print!("{}", run_c_probe("wrapper_leaks", &[]));
}
