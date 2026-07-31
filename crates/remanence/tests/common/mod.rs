// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

#![allow(dead_code)]

use std::path::PathBuf;

pub fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn repo_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir())
}

pub fn fixtures_dir() -> PathBuf {
    manifest_dir().join("tests/fixtures")
}

/// Returns the path to a requested test fixture from `tests/fixtures/`.
/// If the fixture is missing, panics with diagnostic instructions to run `python testing-prep/prep_fixtures.py`.
pub fn ensure_fixture(name: &str) -> PathBuf {
    let target = fixtures_dir().join(name);
    if target.exists() {
        return target;
    }

    panic!(
        "Missing required test fixture '{name}'.\n\
         Please run 'python testing-prep/prep_fixtures.py' (from the testing venv; see testing-prep/test-rigs/README.md)\n\
         to download or generate required test fixtures before running unit tests."
    );
}
