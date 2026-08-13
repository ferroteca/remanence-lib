// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Every handle gives back what its creation took (S2, D47).
//!
//! **Opt-in, and the whole file vanishes without the feature.** The
//! probe is a global allocator and an exported symbol; carrying either
//! in a released artifact would add a `remanence_*` symbol, which is an
//! S2 change. So it lives behind `leak-probe` and the check runs under
//! an explicit command:
//!
//! ```text
//! cargo build -p remanence-ffi --features leak-probe
//! cargo test  -p remanence-ffi --features leak-probe
//! ```
//!
//! That is a real departure from the fail-rather-than-skip rule the
//! other C tests follow (D44, D45), and it is worth naming rather than
//! glossing: with the feature off this binary reports "0 tests", which
//! reads like nothing to check. The rule is kept where it can be — a
//! missing CMake or compiler still fails — and given up here, because
//! the alternative is shipping the probe. AGENTS.md carries the command
//! so it is a step someone runs, not a thing that quietly never happens.
//!
//! Both builds need the same feature: the first produces the cdylib the
//! C caller links, the second decides whether this file exists at all.
//! Mismatch them and CMake refuses to build the target, rather than
//! anything subtler.

#![cfg(feature = "leak-probe")]

mod common;

use common::{run_c, skipping, workspace_dir};

const FIXTURE: &str = "crates/remanence/tests/fixtures/freedos-parttest.qcow2";

#[test]
fn handles_and_messages_give_back_what_they_took() {
    if skipping() {
        return;
    }
    let fixture = workspace_dir().join(FIXTURE);
    assert!(
        fixture.exists(),
        "the fixture {} is missing, so nothing was cycled",
        fixture.display()
    );
    print!("{}", run_c("abi_leaks", &[FIXTURE]));
}
