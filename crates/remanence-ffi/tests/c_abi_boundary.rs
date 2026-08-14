// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C ABI, exercised from C (S2, D45, D46).
//!
//! The FFI crate's unit tests call the `extern "C"` functions **from
//! Rust**. That checks the logic behind them and nothing about the
//! boundary: no header is included, no C compiler sees a declaration, no
//! C calling convention is used, and a `#[repr(C)]` mistake cannot show.
//! D44 closed half the gap by compiling the header and the example; this
//! closes the other half by linking and running.
//!
//! `tests/c/abi_boundary.c` is the caller. It takes a group name, so
//! each group is a named test here rather than one pass-or-fail lump,
//! and it keeps going after a failed check so one run reports everything
//! wrong with a group.
//!
//! **These need the built library, which `cargo test` does not produce.**
//! `cargo build` does, and AGENTS.md orders it first, so the ordinary
//! flow satisfies it.

mod common;

use common::{run_c, skipping, workspace_dir};

/// The artifact the discovery group claims. A real one, because the
/// point is to cross the boundary with something that has answers.
const FIXTURE: &str = "crates/remanence/tests/fixtures/freedos-parttest.qcow2";

fn group(name: &str, args: &[&str]) {
    if skipping() {
        return;
    }
    print!("{}", run_c("abi_boundary", &[&[name], args].concat()));
}

#[test]
fn the_catalogs_answer_across_the_boundary() {
    group("catalogs", &[]);
}

#[test]
fn the_version_and_cache_bound_cross() {
    group("version", &[]);
}

#[test]
fn a_refusal_sets_its_out_parameters() {
    group("refusal", &[]);
}

#[test]
fn accessors_answer_on_a_null_handle() {
    group("nulls", &[]);
}

/// The machine reading, as a C caller meets it: a machine with nothing in
/// it reads and says nothing booted, a boot device it does not hold is
/// refused naming the attachment asked for, and every accessor answers on
/// a null report rather than dereferencing it.
///
/// The surface this replaced was never called from C at all — it compiled
/// and nothing exercised it — so this group exists to keep the new one
/// out of that position.
#[test]
fn a_machine_reads_across_the_boundary() {
    group("machine", &[]);
}

#[test]
fn a_real_artifact_discovers_and_releases() {
    if skipping() {
        return;
    }
    let fixture = workspace_dir().join(FIXTURE);
    assert!(
        fixture.exists(),
        "the fixture {} is missing, so the boundary was not crossed with \
         anything that has answers",
        fixture.display()
    );
    group("discovery", &[FIXTURE]);
}
