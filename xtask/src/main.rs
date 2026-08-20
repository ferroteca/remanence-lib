// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Build-orchestration for `remanence-ffi`'s C/C++ checks, run explicitly
//! by `Taskfile.yml`'s `test-ffi` task — never by `cargo test`, and never
//! automatically.
//!
//! This exists because CMake needs a variable-length list of `-D...`
//! flags assembled as a real argv (which files a specific `cargo build`
//! just wrote, classified by suffix), and Task's own bundled utilities on
//! Windows are a small file-operations set (`cp`, `mv`, `mkdir`) — not
//! `sed`, `grep`, `xargs`, or shell arrays — so reassembling a command
//! line from parsed text in the Taskfile would mean depending on external
//! tools being on `PATH`, which this project avoids.
//! `std::process::Command` passes an argument list correctly with no
//! shell involved at all. `ffi::run` does everything through to the point
//! a human would want to watch (a `cmake --build`), and prints exactly
//! one line of output — the build directory — for the Taskfile to hand to
//! `ctest`. Staging the compiled Python module (`xtask py-stage`,
//! formerly) needed none of this — a fixed set of file copies, no
//! argument list — and now runs directly in `Taskfile.yml`'s `test-py`/
//! `test-python` tasks (D73).
//!
//! Kept out of `remanence-ffi` itself: it publishes to a registry and
//! excludes its own `tests/**`/dev-only files on the principle that a
//! released artifact carries what a consumer runs and nothing else.
//! `xtask` is a workspace member that is never a default one and is
//! never published.

mod ffi;

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
        other => {
            if let Some(other) = other {
                eprintln!("xtask: unknown subcommand `{other}`\n");
            }
            eprintln!(
                "usage: xtask ffi\n\n  \
                 ffi        build the shipped/probe cdylibs, refuse a \
                 MinGW one, and configure+build\n             the CMake \
                 C/C++ test project — prints its build directory"
            );
            std::process::exit(2);
        }
    }
}
