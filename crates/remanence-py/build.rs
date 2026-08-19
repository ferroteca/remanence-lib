// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Which interpreter this module was built for, recorded so a failing
//! suite can explain itself.
//!
//! **Nothing selects on this.** uv chooses the interpreter that runs the
//! Python suite, always (D63), and the two need not agree: the shipped
//! module is stable-ABI, importable by any CPython 3.10 or later. This
//! variable exists for the one failure it explains — a module built
//! under MSYS2 links `libpython3.dll`, which lives only inside MSYS2, so
//! no interpreter uv can supply will import it, and the error is a bare
//! `DLL load failed` that says nothing about why. Naming the interpreter
//! it *was* built for is what turns that into a diagnosis.
//!
//! **The claim is native CPython** (S3), and the wheels that carry it
//! are built by `uv build`, which hands maturin a native interpreter of
//! its own. No developer's `PATH` reaches a published artifact, so
//! nothing here needs to police one — and a local MSYS2 build stays
//! legitimate for the person sitting in front of MSYS2, which is why
//! this script no longer refuses one (9c09da7 did; a58472c stopped).

fn main() {
    // Our answer depends on which interpreter pyo3 resolved, and that
    // is the variable that decides it. pyo3's own script asks for the
    // same rerun; this one needs it on its own account.
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");

    // abi3 lets a build resolve no interpreter at all — on Windows
    // `raw-dylib` needs no import library, so nothing forces the search
    // to succeed. Then there is nothing to name, and the variable goes
    // unset rather than carrying a placeholder that reads like a path.
    // Every target of this package sees it, tests included.
    if let Some(interpreter) = pyo3_build_config::get().executable() {
        println!("cargo:rustc-env=REMANENCE_BUILD_INTERPRETER={interpreter}");
    }
}
