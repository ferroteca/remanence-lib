// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C header and its example still compile (S2, D44).
//!
//! `include/remanence.h` regenerates on every build, so it cannot fall
//! behind the Rust — but `examples/identify.c` can, and did so silently
//! until a person remembered to recompile it by hand. These tests are
//! that step, run by `cargo test`.
//!
//! **Compiling is the whole check; linking would add nothing.** A header
//! generated from the `extern "C"` signatures cannot declare a symbol
//! the library does not export, so the failure linking would catch does
//! not exist here. What *can* drift is the example calling something the
//! header no longer declares, and that is a compile error. Dropping the
//! link step drops the need for a built cdylib with it, so these tests
//! need nothing but a compiler.
//!
//! Three things are checked, and the third is a claim nothing verified
//! before:
//!
//! 1. The header is self-contained — a translation unit that includes it
//!    and nothing else compiles.
//! 2. `examples/identify.c` compiles against it, warnings included.
//! 3. The header compiles as **C++**, which `cbindgen.toml` asserts with
//!    `cpp_compat = true` and no one had ever tested.

use std::path::PathBuf;

mod common;

use common::{compile, crate_dir, require, scratch, skipping, CC_OVERRIDE, CXX_OVERRIDE};

/// Writes a translation unit that includes the header and does nothing
/// else, which is how "self-contained" is asked.
fn only_includes_the_header(name: &str, main: &str) -> PathBuf {
    let path = scratch().join(name);
    std::fs::write(
        &path,
        format!("#include <remanence.h>\n\nint main({main}) {{\n    return 0;\n}}\n"),
    )
    .expect("the scratch translation unit is writable");
    path
}

#[test]
fn the_header_stands_alone_in_c() {
    if skipping() {
        return;
    }
    let Some(compiler) = require(CC_OVERRIDE, &["cc", "gcc", "clang"], "C") else {
        return;
    };
    let unit = only_includes_the_header("selfcontained.c", "void");
    if let Err(report) = compile(&compiler, &unit, "selfcontained.o", &[]) {
        panic!(
            "remanence.h does not compile on its own. It is generated, so \
             this is cbindgen's output or its configuration, not a hand \
             edit:\n{report}"
        );
    }
}

#[test]
fn the_example_compiles_against_the_header() {
    if skipping() {
        return;
    }
    let Some(compiler) = require(CC_OVERRIDE, &["cc", "gcc", "clang"], "C") else {
        return;
    };
    let example = crate_dir().join("examples/identify.c");
    if let Err(report) = compile(&compiler, &example, "identify.o", &[]) {
        panic!(
            "examples/identify.c no longer compiles against the generated \
             header. The header regenerates from the Rust on every build, \
             so the example is what moved out from under the surface — \
             the C ABI changed and the example did not follow \
             it:\n{report}"
        );
    }
}

#[test]
fn the_header_compiles_as_cpp_because_it_claims_to() {
    if skipping() {
        return;
    }
    let Some(compiler) = require(CXX_OVERRIDE, &["c++", "g++", "clang++"], "C++") else {
        return;
    };
    let unit = only_includes_the_header("selfcontained.cpp", "");
    if let Err(report) = compile(&compiler, &unit, "selfcontained_cpp.o", &[]) {
        panic!(
            "remanence.h does not compile as C++, which `cpp_compat = \
             true` in cbindgen.toml promises it does. Either the promise \
             or the configuration has to give:\n{report}"
        );
    }
}
