// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C ABI, exercised from C (S2, D45).
//!
//! The FFI crate's unit tests call the `extern "C"` functions **from
//! Rust**. That checks the logic behind them and nothing about the
//! boundary: no header is included, no C compiler sees a declaration,
//! no C calling convention is used, and a `#[repr(C)]` mistake cannot
//! show. D44 closed half the gap by compiling the header and the
//! example; this closes the other half by linking and running.
//!
//! `tests/c/abi_boundary.c` is the caller. It takes a group name, so
//! each group is a named test here rather than one pass-or-fail lump,
//! and it keeps going after a failed check so one run reports
//! everything wrong with a group.
//!
//! **These need the built library, which `cargo test` does not produce.**
//! `cargo build` does, and AGENTS.md already orders it before
//! `cargo test`, so the ordinary flow satisfies this. When the library
//! is missing the tests say so and say what to run — they do not skip,
//! and they do not try to build it themselves: a nested `cargo` would
//! contend for the same lock this test is already running under.

mod common;

use common::{crate_dir, require, scratch, skipping, CC_OVERRIDE};

use std::path::{Path, PathBuf};
use std::process::Command;

/// The artifact the discovery group claims. A real one, because the
/// point is to cross the boundary with something that has answers.
const FIXTURE: &str = "crates/remanence/tests/fixtures/freedos-parttest.qcow2";

/// `target/<profile>`, found from this test binary's own location
/// (`target/<profile>/deps/<name>.exe`) rather than assumed to be
/// `debug`, so a `--release` run links the library it just built.
fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary knows its own path");
    exe.parent()
        .and_then(Path::parent)
        .expect("the test binary sits in target/<profile>/deps")
        .to_path_buf()
}

fn workspace_dir() -> PathBuf {
    crate_dir()
        .join("../..")
        .canonicalize()
        .expect("the workspace root is reachable from the crate")
}

/// The built cdylib, under whichever name this platform gives it.
fn library() -> PathBuf {
    let dir = target_dir();
    for name in [
        "remanence_ffi.dll",
        "libremanence_ffi.so",
        "libremanence_ffi.dylib",
    ] {
        let path = dir.join(name);
        if path.exists() {
            return path;
        }
    }
    panic!(
        "the built library is not in {}, so there is nothing for a C \
         caller to link against.\n\n\
         `cargo test` does not build a cdylib — `cargo build` does, and \
         AGENTS.md orders it first for exactly this reason:\n\n  \
         cargo build\n  cargo test\n\n\
         This test does not run cargo itself: a nested build would \
         contend for the lock the current run already holds.",
        dir.display()
    );
}

/// Builds the C caller once per group that needs it. Cheap enough that
/// building per test beats sharing state between them.
fn build_caller(compiler: &common::Compiler) -> PathBuf {
    let source = crate_dir().join("tests/c/abi_boundary.c");
    let exe = scratch().join(if cfg!(windows) {
        "abi_boundary.exe"
    } else {
        "abi_boundary"
    });

    let output = compiler
        .command()
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg("-I")
        .arg(crate_dir().join("include"))
        .arg("-o")
        .arg(&exe)
        .arg(&source)
        .arg(library())
        .output()
        .expect("the compiler runs");

    assert!(
        output.status.success(),
        "the C caller did not build against the header and the library:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Windows resolves the import at load time from beside the
    // executable, so the library goes there rather than onto PATH.
    if cfg!(windows) {
        let beside = scratch().join(
            library()
                .file_name()
                .expect("the library has a file name"),
        );
        std::fs::copy(library(), &beside).expect("the library copies beside the caller");
    }
    exe
}

/// The built caller and the directory its toolchain needs on `PATH`.
///
/// Built once for the whole binary: the harness runs these tests on
/// threads of one process, and five of them writing one executable —
/// while another is running it — is a file lock on Windows rather than
/// a race that merely wastes work.
static CALLER: std::sync::OnceLock<(PathBuf, Option<PathBuf>)> = std::sync::OnceLock::new();

fn caller() -> &'static (PathBuf, Option<PathBuf>) {
    CALLER.get_or_init(|| {
        let compiler = require(CC_OVERRIDE, &["cc", "gcc", "clang"], "C")
            .expect("require either answers or panics");
        (build_caller(&compiler), compiler.bin_dir.clone())
    })
}

/// Runs one group and reports its own output on failure — the C side
/// prints what it expected, which is the useful half of a failure.
fn run_group(group: &str, extra: &[&str]) {
    if skipping() {
        return;
    }
    let (exe, bin_dir) = caller();

    let mut command = Command::new(exe);
    command.arg(group).args(extra).current_dir(workspace_dir());
    if let Some(dir) = bin_dir {
        let existing = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{};{existing}", dir.display()));
    }
    let output = command.output().expect("the C caller runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        output.status.success(),
        "the C caller's `{group}` group failed. This is the ABI as C \
         meets it, so a failure here is a real boundary defect rather \
         than a test-harness one:\n{text}"
    );
    print!("{text}");
}

#[test]
fn the_catalogs_answer_across_the_boundary() {
    run_group("catalogs", &[]);
}

#[test]
fn the_version_and_cache_bound_cross() {
    run_group("version", &[]);
}

#[test]
fn a_refusal_sets_its_out_parameters() {
    run_group("refusal", &[]);
}

#[test]
fn accessors_answer_on_a_null_handle() {
    run_group("nulls", &[]);
}

#[test]
fn a_real_artifact_discovers_and_releases() {
    let fixture = workspace_dir().join(FIXTURE);
    assert!(
        fixture.exists(),
        "the fixture {} is missing, so the boundary was not crossed with \
         anything that has answers",
        fixture.display()
    );
    run_group("discovery", &[FIXTURE]);
}
