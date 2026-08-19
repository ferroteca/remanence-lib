// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Running a Python tool, and the paths the suites need.
//!
//! **uv chooses the interpreter, always** (D63). Nothing here selects
//! one, constrains one, or searches for one: the module is stable-ABI,
//! so any CPython 3.10 or later can import it and there is no choice to
//! make. A tool that is absent **fails**, with no way to skip past it —
//! the same policy the C tests follow — because a check that quietly
//! does not run reads exactly like one that passed (D64).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn workspace_dir() -> PathBuf {
    crate_dir()
        .join("../..")
        .canonicalize()
        .expect("the workspace root is reachable from the crate")
}

/// `target/<profile>`, from this test binary's own location rather than
/// assumed to be `debug`.
pub fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary knows its own path");
    exe.parent()
        .and_then(Path::parent)
        .expect("the test binary sits in target/<profile>/deps")
        .to_path_buf()
}

pub mod python {
    use super::Command;

    /// The one command that runs `tool`, for every tool and every build.
    ///
    /// There is no `--python`: uv picks, and what it picks is not this
    /// crate's business. `--with` fetches the tool, so no prior install
    /// is needed, and `--no-project` keeps uv from treating the crate as
    /// a Python project to build — a wheel is exactly what the suite
    /// exists to avoid needing.
    fn command(tool: &str) -> (String, Vec<String>) {
        (
            format!("uv run --with {tool} --no-project {tool}"),
            ["uv", "run", "--with", tool, "--no-project", tool]
                .iter()
                .map(|part| (*part).to_owned())
                .collect(),
        )
    }

    fn require(tool: &str) -> Option<(String, Vec<String>)> {
        let (label, argv) = command(tool);
        let reachable = Command::new(&argv[0])
            .args(&argv[1..])
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());

        assert!(
            reachable,
            "{tool} is not reachable as `{label}`, so that check went \
             unrun. This fails rather than skips, because a check that \
             quietly does not run reads exactly like a check that \
             passed.\n\n\
             This fixes it:\n  \
             - install uv (the project already builds its wheel with it), \
             and it fetches {tool} itself\n\n"
        );
        Some((label, argv))
    }

    pub fn pytest() -> Option<(String, Vec<String>)> {
        require("pytest")
    }

    pub fn mypy() -> Option<(String, Vec<String>)> {
        require("mypy")
    }
}
