# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Shared helpers for the fixture-gated Python integration suite.

The mirror of `integration-tests/rust/tests/common/mod.rs`: the fixtures
live beside this suite rather than inside it, read by the Rust
integration crate, the C/C++ CTest suite, and the prep script that writes
them, so belonging to any one of those three would be a claim none of the
others can honour. Python has no compile-time feature gate to enforce a
fixture declaration the way `fixtures`/`rigs` do on the Rust and CTest
suites, so `ensure_fixture` is the only line of defense here — every test
in this directory calls it before reading a fixture, and the panic below
names the same prep step the other two suites point to.
"""

from __future__ import annotations

from pathlib import Path


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def fixtures_dir() -> Path:
    return repo_root() / "integration-tests" / "fixtures"


def ensure_fixture(name: str) -> Path:
    """The path to `name` in `integration-tests/fixtures/`.

    Raises with diagnostic instructions to run the prep script when the
    fixture is missing, rather than failing on whatever error reading a
    missing file happens to raise.
    """
    target = fixtures_dir() / name
    if not target.exists():
        raise AssertionError(
            f"Missing required test fixture '{name}'.\n"
            "Please run 'uv run integration-tests/prep_fixtures.py' "
            "(see test-fixture-prep/test-rigs/README.md)\n"
            "to download or generate required test fixtures before "
            "running these tests."
        )
    return target
