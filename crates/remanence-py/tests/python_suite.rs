// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The Python suite runs under `cargo test` (S3, D48, D51).
//!
//! D48 wrote the pytest suite and left running it to a person, behind
//! three commands: build a wheel, install it, run pytest. That is the
//! shape of a check that stops being run — the same objection D42 and
//! D43 answered for the stub, and D50 for the leak probe.
//!
//! **It needs no wheel.** A wheel is a `remanence/` package holding the
//! compiled module beside `__init__.py`, the stub and `py.typed`, and
//! that layout can be staged from what `cargo build` already produced —
//! the debug cdylib, renamed as the extension, and the three files from
//! `python/remanence/`. `PYTHONPATH` finds it, pytest imports it, and
//! the release build a wheel would need never happens.
//!
//! What that costs in fidelity is stated rather than hidden: this
//! exercises a **debug** build, staged by hand rather than installed by
//! a packaging tool. `uv build` remains what proves the artifact itself,
//! and D48's sdist check remains what proves the suite runs from an
//! unpacked source distribution.

mod common;

use common::{crate_dir, python, skipping, target_dir, workspace_dir, SKIP_PYTEST};

use std::path::PathBuf;
use std::process::Command;

/// The compiled module, under whichever name this platform builds.
fn built_module() -> PathBuf {
    let dir = target_dir();
    for name in ["remanence_py.dll", "libremanence_py.so", "libremanence_py.dylib"] {
        let path = dir.join(name);
        if path.exists() {
            return path;
        }
    }
    panic!(
        "no compiled Python module in {}.\n\n\
         `cargo test` does not build a cdylib and this crate is outside \
         the workspace's default members, so build it first:\n\n  \
         cargo build -p remanence-py\n  cargo test -p remanence-py\n\n\
         This does not run cargo itself: a nested build of *this* crate \
         would contend for the lock the current run already holds.",
        dir.display()
    );
}

/// Lays out a `remanence/` package the way a wheel does, from what is
/// already built. The extension's file name is what `__init__.py`
/// imports, so it is renamed rather than copied under its cargo name.
fn stage() -> PathBuf {
    let root = target_dir().join("py-stage");
    let package = root.join("remanence");
    std::fs::create_dir_all(&package).expect("the staging directory is writable");

    let extension = if cfg!(windows) {
        "remanence.pyd"
    } else {
        "remanence.so"
    };
    std::fs::copy(built_module(), package.join(extension)).expect("the module stages");

    let source = crate_dir().join("python/remanence");
    for name in ["__init__.py", "__init__.pyi", "py.typed"] {
        std::fs::copy(source.join(name), package.join(name))
            .unwrap_or_else(|error| panic!("{name} stages: {error}"));
    }
    root
}

#[test]
fn the_python_suite_passes_against_what_was_just_built() {
    if skipping(SKIP_PYTEST) {
        return;
    }
    let Some((label, argv)) = python::pytest() else {
        return;
    };
    let staged = stage();

    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .arg("-q")
        .env("PYTHONPATH", &staged)
        // From the crate root, so pyproject.toml's `testpaths` applies
        // and the suite is found the way a consumer finds it.
        .current_dir(crate_dir())
        .output()
        .expect("pytest runs");

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "the Python suite failed (pytest via {label}), against the module \
         staged from this build in {}:\n{text}",
        staged.display()
    );
    print!("{text}");
    let _ = workspace_dir();
}
