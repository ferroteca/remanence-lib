// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C ABI crossed with a disk that has answers (S2, D45, D49).
//!
//! **Behind the `rigs` feature, and split out of `c_abi_boundary.rs` to
//! get there.** Every other group in that suite either asks the library
//! about itself or builds what it needs, so a fresh clone runs them with
//! nothing downloaded and nothing installed. This one wants a real disk
//! — partitions to discover, volumes to release — and the only artifact
//! here that is one is `freedos-parttest.qcow2`, which reliquary
//! generates by booting a machine and installing FreeDOS onto a disk.
//!
//! It ran ungated until this split, so `cargo test --workspace` failed
//! on any machine that had not built the rig — which is every fresh
//! clone, and was found on one. It passed everywhere the prep had
//! already run, which is why nothing local ever showed it: the same way
//! `media_sources` and `sevenzip_catalog` hid before their gates went
//! in. Those two were caught by `ensure_fixture` being gated on the
//! features; this test reached past that guard by checking the path
//! itself, so the declaration was the only thing left to get right.
//!
//! The C caller is the same `tests/c/abi_boundary.c` the rest of the
//! boundary uses, asked for its `discovery` group. Splitting the Rust
//! target is what carries the feature; the C side has one program.
//!
//! ```text
//! cargo test -p remanence-ffi --features rigs
//! ```

mod common;

use common::{run_c, skipping, workspace_dir};

/// The disk the discovery group claims. A real one, because the point
/// is to cross the boundary with something that has answers.
const FIXTURE: &str = "crates/remanence/tests/fixtures/freedos-parttest.qcow2";

#[test]
fn a_real_artifact_discovers_and_releases() {
    if skipping() {
        return;
    }
    let fixture = workspace_dir().join(FIXTURE);
    assert!(
        fixture.exists(),
        "the rig {} is missing, so the boundary was not crossed with \
         anything that has answers. Run:\n\n  \
         uv run --directory test-fixture-prep prep_fixtures.py\n",
        fixture.display()
    );
    print!("{}", run_c("abi_boundary", &["discovery", FIXTURE]));
}
