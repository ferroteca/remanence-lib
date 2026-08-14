// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The flux ladder through the C++ presentation, over a real capture
//! (S2, D53, D54).
//!
//! **Behind the `fixtures` feature, and the only C++ test that is.**
//! Everything else `cpp_wrapper.rs` runs makes its own artifact — an
//! authored medium, or the remanence format's own worked example — so a
//! fresh clone checks the whole surface without downloading anything.
//! What cannot be made that way is a recording that actually frames
//! records: the sector layer needs one, and the worked example is two
//! points on one orbit, which is deliberately not a recording.
//!
//! So this walks a KryoFlux capture the prep script fetches: the
//! archive, the capture set gathered out of its namespace, the flux
//! medium loaded from that, and the bitstream, bytestream, sectors and
//! CBM DOS namespace above it. It takes a couple of minutes, which is
//! the gap-first reduction over eighty-four step positions doing its
//! work rather than the boundary being slow.
//!
//! ```text
//! cargo test -p remanence-ffi --features fixtures
//! ```

mod common;

use common::{run_c, skipping, workspace_dir};

/// The capture `test-fixture-prep/prep_fixtures.py` packages: one disk,
/// every step position of both heads, as a single 7z.
const FIXTURE: &str = "crates/remanence/tests/fixtures/\
                       Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";

#[test]
fn a_real_capture_climbs_the_whole_ladder() {
    if skipping() {
        return;
    }
    let fixture = workspace_dir().join(FIXTURE);
    assert!(
        fixture.exists(),
        "the capture {} is missing, so the sector layer was not walked over \
         anything that frames a record. Run:\n\n  \
         uv run --directory test-fixture-prep prep_fixtures.py\n",
        fixture.display()
    );
    print!("{}", run_c("wrapper", &["flux_capture", FIXTURE]));
}
