// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Every handle gives back what its creation took (S2, D47, D50).
//!
//! **This runs by default**, which D47 could not manage and D50 fixes.
//! The obstacle was never the check — it was that the probe is a global
//! allocator and an exported symbol, and an extra `remanence_*` symbol
//! is an S2 change. Shipping it to make the test convenient would have
//! been a surface change bought with a test's convenience.
//!
//! The way out is that the probe does not have to be in the *shipped*
//! library, only in one a C caller can link. The harness builds it into
//! `target/leak-probe` with `--features leak-probe`, leaving
//! `target/<profile>` exactly as it was — cargo locks a target directory
//! rather than a workspace, so that build runs happily while this one
//! holds the other lock. Cold it costs about twenty seconds; warm, a
//! fifth of a second.
//!
//! What it asserts is the `_free` discipline, which no leak checker
//! outside the library can see: every handle and string the ABI hands
//! out is allocated by Rust inside the cdylib, so CppUTest's detector
//! and the sanitizers never observe it. The library counts its own live
//! blocks; this reads the count either side of repeated cycles.

mod common;

use common::{run_c_probe, workspace_dir};

const FIXTURE: &str = "crates/remanence/tests/fixtures/freedos-parttest.qcow2";

#[test]
fn handles_and_messages_give_back_what_they_took() {
    let fixture = workspace_dir().join(FIXTURE);
    assert!(
        fixture.exists(),
        "the fixture {} is missing, so nothing was cycled",
        fixture.display()
    );
    print!("{}", run_c_probe("abi_leaks", &[FIXTURE]));
}
