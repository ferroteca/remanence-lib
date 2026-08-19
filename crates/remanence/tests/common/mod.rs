// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

#![allow(dead_code)]

use std::fs::File;
use std::path::{Path, PathBuf};

/// The caller's own read-only open — the source shape `load_media` takes
/// (P7 as amended: whoever opens owns the lock).
pub fn open_read(path: impl AsRef<Path>) -> File {
    let path = path.as_ref();
    File::open(path).unwrap_or_else(|error| panic!("cannot open '{}': {error}", path.display()))
}

/// The caller's own read/write open, which is what affords the library a
/// write.
pub fn open_write(path: impl AsRef<Path>) -> File {
    let path = path.as_ref();
    File::options()
        .read(true)
        .write(true)
        .open(path)
        .unwrap_or_else(|error| panic!("cannot open '{}' for writing: {error}", path.display()))
}

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

/// The fixtures live outside every crate, at the repository root: they
/// are read by the Rust suites, by the C/C++ CTest suite and by the prep
/// script that writes them, so belonging to any one crate would be a
/// claim none of those three can honour. `crates/*/tests` holds Rust
/// tests and nothing else.
pub fn fixtures_dir() -> PathBuf {
    repo_root().join("integration-tests/fixtures")
}

/// Returns the path to a requested test fixture from
/// `integration-tests/fixtures/`.
/// If the fixture is missing, panics with diagnostic instructions to run
/// the prep script.
///
/// **Gated on the features that declare a fixture is wanted**, which is
/// what keeps a suite from reaching for one without saying so. The
/// panic below is the right answer for a target that declared `fixtures`
/// or `rigs` and has not run the prep, and the wrong one for a target
/// that never declared either — there it would fail on whichever machine
/// had not downloaded, which is invisible on every machine that had.
/// Two suites drifted that way. A target that calls this without the
/// declaration now fails to compile, in the default run, on the first
/// machine to build it.
#[cfg(any(feature = "fixtures", feature = "rigs"))]
pub fn ensure_fixture(name: &str) -> PathBuf {
    let target = fixtures_dir().join(name);
    if target.exists() {
        return target;
    }

    panic!(
        "Missing required test fixture '{name}'.\n\
         Please run 'uv run integration-tests/prep_fixtures.py' (see test-fixture-prep/test-rigs/README.md)\n\
         to download or generate required test fixtures before running unit tests."
    );
}
