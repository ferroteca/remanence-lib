// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C header and its example still compile (S2, D44, D46).
//!
//! `include/remanence.h` regenerates on every build, so it cannot fall
//! behind the Rust — but `examples/identify.c` can, and did so silently
//! until a person remembered to recompile it by hand.
//!
//! **Compiling is the whole of what these check; linking is
//! `c_abi_boundary.rs`'s job.** A header generated from the `extern "C"`
//! signatures cannot declare a symbol the library does not export, so
//! there is no drift here for a link step to catch that compiling does
//! not. What can drift is the example calling something the header no
//! longer declares.
//!
//! The three targets are object libraries in the CMake build (D46), so
//! these tests assert against one build rather than each shelling out to
//! a compiler of its own. A build failure reports the compiler's output,
//! which names the file that stopped compiling.

mod common;

use common::{build_dir, skipping};

fn built() {
    if skipping() {
        return;
    }
    build_dir();
}

#[test]
fn the_header_stands_alone_in_c() {
    // CMake target `header_selfcontained`: a translation unit that
    // includes the header and nothing else.
    built();
}

#[test]
fn the_example_compiles_against_the_header() {
    // CMake target `example_identify`.
    built();
}

#[test]
fn the_header_compiles_as_cpp_because_it_claims_to() {
    // CMake target `header_cpp` — `cpp_compat = true` in cbindgen.toml
    // claims this, and nothing tested it before D44.
    built();
}
