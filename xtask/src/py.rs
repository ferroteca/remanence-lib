// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! `xtask py-stage`: stages the compiled `remanence-py` module (built
//! separately — see `Taskfile.yml`'s `test-py` task — by
//! `uv run -- cargo build -p remanence-py`, so pyo3 resolves the same
//! interpreter `uv` will later use to run pytest) into a `remanence/`
//! package layout pytest can import, printing only the stage root — for
//! `Taskfile.yml` to set as `PYTHONPATH`.
//!
//! Just the file-copying, deliberately: no test framework runs here, and
//! nothing in this file executes Python at all — `Taskfile.yml` runs
//! pytest itself, against the `PYTHONPATH` this prints.

use std::path::PathBuf;

use crate::workspace_dir;

/// The compiled module's cargo-produced name, and what it is staged as.
const CANDIDATES: [(&str, &str); 3] = [
    ("remanence_py.dll", "remanence.pyd"),
    ("libremanence_py.so", "remanence.so"),
    ("libremanence_py.dylib", "remanence.so"),
];

pub fn run() {
    let target_dir = workspace_dir().join("target").join("debug");
    let Some((built_name, staged_name)) = CANDIDATES
        .iter()
        .find(|(name, _)| target_dir.join(name).is_file())
    else {
        panic!(
            "no compiled Python module in {}.\n\nBuild it first: \
             uv run -- cargo build -p remanence-py",
            target_dir.display()
        );
    };

    let stage_root = target_dir.join("py-stage");
    let package_dir = stage_root.join("remanence");
    let _ = std::fs::remove_dir_all(&stage_root);
    std::fs::create_dir_all(&package_dir)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", package_dir.display()));

    let compiled = target_dir.join(built_name);
    std::fs::copy(&compiled, package_dir.join(staged_name))
        .unwrap_or_else(|error| panic!("cannot stage {}: {error}", compiled.display()));

    let source: PathBuf = workspace_dir()
        .join("crates")
        .join("remanence-py")
        .join("python")
        .join("src")
        .join("remanence");
    for file in ["__init__.py", "__init__.pyi", "py.typed"] {
        let from = source.join(file);
        std::fs::copy(&from, package_dir.join(file))
            .unwrap_or_else(|error| panic!("cannot stage {}: {error}", from.display()));
    }

    // The one line on stdout: the stage root, for `PYTHONPATH`.
    println!("{}", stage_root.display());
}
