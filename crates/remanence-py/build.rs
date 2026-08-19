// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Refuses a build against a MinGW/MSYS2 Python (D66).
//!
//! **The claim is native CPython** (S3), and this project no longer
//! treats an MSYS2 build as a legitimate alternative — reversing
//! a58247c's stance, for the same reason `xtask` refuses one for
//! `remanence-ffi`'s C/C++ tests (D65): the two Windows toolchains
//! disagree, and MSYS2's is not the one this project ships.
//!
//! **Read from what pyo3 resolved, not guessed from the host.** pyo3
//! names a library's link name without a `lib` prefix by convention —
//! `python3`, the stable-ABI forwarder every native CPython ≥ 3.10 ships.
//! MSYS2's own Python breaks that convention: its import library is
//! literally named `libpython3`, because that is the file MSYS2 ships. A
//! `lib`-prefixed link name is therefore the claim itself, not an
//! inference from `PATH` or the shell the build happens to run in — and
//! it is read via `pyo3_build_config::get()`, which needs no interpreter
//! of its own: it only reads the config `pyo3-ffi`'s build script already
//! resolved and exported, guaranteed (by the `links`/`DEP_*` mechanism
//! cargo requires a direct dependency for) to run first.

fn main() {
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");

    let config = pyo3_build_config::get();
    if let Some(lib_name) = config.lib_name().filter(|name| name.starts_with("lib")) {
        panic!(
            "pyo3 resolved a MinGW/MSYS2 Python (link name `{lib_name}`), \
             which this project no longer supports building against. \
             Build with a native interpreter instead — `uv run -- cargo \
             build -p remanence-py` puts one first on `PATH` for pyo3 to \
             find."
        );
    }
}
