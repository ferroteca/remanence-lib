// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Build-orchestration steps for `remanence-ffi`'s and `remanence-py`'s
//! checks, run explicitly by `Taskfile.yml`'s `test-ffi`/`test-py` tasks —
//! never by `cargo test`, and never automatically.
//!
//! Anything that needs argv-safe subprocess arguments (a list of `cmake`
//! flags, in particular) lives here in Rust rather than in the Taskfile's
//! shell script. Task's own bundled utilities on Windows are a small
//! file-operations set (`cp`, `mv`, `mkdir`) — not `sed`, `grep`, `xargs`,
//! or arrays — so reassembling a command line from parsed text in the
//! Taskfile would mean depending on external tools being on `PATH`,
//! which this project avoids. `std::process::Command` passes an argument
//! list correctly with no shell involved at all. Each subcommand here
//! does everything through to the point a human would want to watch (a
//! `cmake --build`, a `ctest` run), and prints exactly one line of
//! output — a path — for the Taskfile to hand to the next, simple step.
//!
//! Two subcommands:
//! - `xtask ffi` — see `ffi::run`.
//! - `xtask py-stage` — see `py::run`.
//!
//! Kept out of `remanence-ffi`/`remanence-py` themselves: both publish to
//! a registry and exclude their own `tests/**`/dev-only files on the
//! principle that a released artifact carries what a consumer runs and
//! nothing else. `xtask` is a workspace member that is never a default
//! one and is never published.

mod ffi;
mod py;

use std::path::PathBuf;

/// `canonicalize()` resolves `..` cleanly but prefixes the result with
/// `\\?\` on Windows — a verbatim path CMake's own tools tolerate for
/// most purposes but not all: MSVC's SARIF diagnostic output chokes on it
/// mid-`try_compile`, reporting `Invalid URI: The hostname could not be
/// parsed` for a source file that exists. `parent()` gives the same
/// answer (the manifest directory is already absolute) with no `..` left
/// to resolve and no prefix to trip over.
fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask's own crate directory has a parent")
        .to_path_buf()
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("ffi") => ffi::run(),
        Some("py-stage") => py::run(),
        other => {
            if let Some(other) = other {
                eprintln!("xtask: unknown subcommand `{other}`\n");
            }
            eprintln!(
                "usage: xtask <ffi|py-stage>\n\n  \
                 ffi        build the shipped/probe cdylibs, refuse a \
                 MinGW one, and configure+build\n             the CMake \
                 C/C++ test project — prints its build directory\n  \
                 py-stage   stage the compiled remanence-py module for \
                 pytest — prints the stage root\n             \
                 (for PYTHONPATH)"
            );
            std::process::exit(2);
        }
    }
}
