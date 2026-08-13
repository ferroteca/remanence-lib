// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Finding a Python tool, and the paths the suites need.
//!
//! The same policy the C tests follow: a tool is looked for where it is
//! likely to be, and its absence **fails** rather than skips, because a
//! check that quietly does not run reads exactly like one that passed.
//! `uv run --with` is the last resort precisely because it needs no
//! prior install — uv is already how this project builds its wheel.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Skips the mypy checks deliberately.
pub const SKIP_MYPY: &str = "REMANENCE_SKIP_MYPY";
/// Skips the pytest run deliberately.
pub const SKIP_PYTEST: &str = "REMANENCE_SKIP_PYTEST";

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

pub fn skipping(variable: &str) -> bool {
    if std::env::var_os(variable).is_some() {
        eprintln!("!! {variable} is set: that check did NOT run.");
        return true;
    }
    false
}

pub mod python {
    use super::Command;

    /// How a Python tool might be reachable, cheapest first.
    fn candidates(tool: &'static str) -> Vec<(String, Vec<String>)> {
        vec![
            (
                format!("python -m {tool}"),
                vec!["python".into(), "-m".into(), tool.into()],
            ),
            (tool.to_owned(), vec![tool.into()]),
            (
                format!("uv run --with {tool}"),
                vec![
                    "uv".into(),
                    "run".into(),
                    "--with".into(),
                    tool.into(),
                    "--no-project".into(),
                    tool.into(),
                ],
            ),
        ]
    }

    fn find(tool: &'static str) -> Option<(String, Vec<String>)> {
        for (label, argv) in candidates(tool) {
            let probe = Command::new(&argv[0])
                .args(&argv[1..])
                .arg("--version")
                .output();
            if probe.is_ok_and(|output| output.status.success()) {
                return Some((label, argv));
            }
        }
        None
    }

    fn require(tool: &'static str, skip: &str) -> Option<(String, Vec<String>)> {
        let found = find(tool);
        assert!(
            found.is_some(),
            "{tool} is not reachable, so that check went unrun. This fails \
             rather than skips, because a check that quietly does not run \
             reads exactly like a check that passed.\n\n\
             Any one of these fixes it:\n  \
             - install uv (the project already uses it to build the \
             wheel), and this fetches {tool} itself\n  \
             - pip install {tool} into the interpreter on PATH\n\n\
             To skip deliberately, set {skip}=1."
        );
        found
    }

    pub fn pytest() -> Option<(String, Vec<String>)> {
        require("pytest", super::SKIP_PYTEST)
    }

    pub fn mypy() -> Option<(String, Vec<String>)> {
        require("mypy", super::SKIP_MYPY)
    }
}
