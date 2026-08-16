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

    /// How a Python tool might be reachable, cheapest first. `interpreter`,
    /// when known, pins the `uv` fallback to it: without this, `uv` picks
    /// an interpreter of its own for an auto-installed tool, which need
    /// not be the one a compiled extension under test was built against —
    /// the same mismatch a `DLL load failed` on import is a symptom of.
    fn candidates(tool: &'static str, interpreter: Option<&str>) -> Vec<(String, Vec<String>)> {
        let mut uv_argv = vec!["uv".into(), "run".into()];
        let mut uv_label = "uv run".to_owned();
        if let Some(interpreter) = interpreter {
            uv_argv.push("--python".into());
            uv_argv.push(interpreter.into());
            uv_label += &format!(" --python {interpreter}");
        }
        uv_argv.extend([
            "--with".into(),
            tool.into(),
            "--no-project".into(),
            tool.into(),
        ]);
        uv_label += &format!(" --with {tool}");

        vec![
            (
                format!("python -m {tool}"),
                vec!["python".into(), "-m".into(), tool.into()],
            ),
            (tool.to_owned(), vec![tool.into()]),
            (uv_label, uv_argv),
        ]
    }

    fn find(tool: &'static str, interpreter: Option<&str>) -> Option<(String, Vec<String>)> {
        for (label, argv) in candidates(tool, interpreter) {
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

    fn require(
        tool: &'static str,
        skip: &str,
        interpreter: Option<&str>,
    ) -> Option<(String, Vec<String>)> {
        let found = find(tool, interpreter);
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

    /// `interpreter` pins the `uv` fallback to the Python a compiled
    /// module under test was built against (see `candidates`); pass
    /// `REMANENCE_BUILD_INTERPRETER` where one is being tested.
    pub fn pytest(interpreter: Option<&str>) -> Option<(String, Vec<String>)> {
        require("pytest", super::SKIP_PYTEST, interpreter)
    }

    pub fn mypy() -> Option<(String, Vec<String>)> {
        require("mypy", super::SKIP_MYPY, None)
    }
}
