// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Which Python this module is built against: recorded for the suite
//! that tests it.
//!
//! **The claim is native CPython** (S3), and the wheels that carry it
//! are built by `uv build`, which hands maturin a native interpreter of
//! its own. No developer's `PATH` reaches a published artifact, so
//! nothing here needs to police one.
//!
//! **A local build against MSYS2's Python is therefore legitimate.**
//! MSYS2 puts its own Python first on `PATH`; pyo3 asks whichever
//! `python` it finds and names that interpreter's import library —
//! `libpython3` rather than `python3`, which takes it out of the subset
//! `raw-dylib` linking covers — so the module links `libpython3.dll` and
//! imports under MSYS2 and nowhere else. For someone working from an
//! MSYS2 shell that is the *correct* module: it is built for the
//! interpreter they are sitting in front of, and `cargo build` and
//! `cargo test` work there with nothing exported.
//!
//! **This script used to refuse that build (9c09da7).** The refusal was
//! redundant with the line below it. The suite runs against
//! `REMANENCE_BUILD_INTERPRETER` — the interpreter the module was
//! actually built for — so a MinGW build tests itself under MinGW and
//! cannot drift into the mismatch the refusal was watching for. What
//! shipped as 0.0.1a4 was a *published* wheel built that way, and
//! publishing does not run this script against a developer's `PATH`.

fn main() {
    // Our answer depends on which interpreter pyo3 resolved, and that
    // is the variable that decides it. pyo3's own script asks for the
    // same rerun; this one needs it on its own account.
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");

    let config = pyo3_build_config::get();
    let interpreter = config.executable().unwrap_or("<unstated>");

    // Named so a failing suite can say which Python the module it is
    // testing was actually built for. Every target of this package sees
    // it, tests included.
    println!("cargo:rustc-env=REMANENCE_BUILD_INTERPRETER={interpreter}");
}
