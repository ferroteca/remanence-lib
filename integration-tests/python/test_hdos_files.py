# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""The HDOS namespace over the real HDOS 1.0 distribution disk.

The Python analog of `integration-tests/rust/tests/hdos_files.rs`,
narrowed to what proves the S3 binding reaches the same adapter: the
shippable `crates/remanence-py/python/tests/` suite is deliberately
fixture-free (D48), so nothing there opens a real HDOS disk.
"""

from __future__ import annotations

import common
import remanence

FIXTURE = "HDOS_1-0_Issue_#50-00-00_890-1.h8d"


def _load(session):
    path = common.ensure_fixture(FIXTURE)
    handle = open(path, "rb")
    medium = session.load_media(handle, "h8d")
    return medium, handle


def test_lists_files_from_the_hdos_fixture_image():
    with remanence.Session() as session:
        medium, handle = _load(session)
        with handle:
            filesystem = medium.partition(0).filesystem_as("hdos")
            entries = filesystem.entries("")
            assert len(entries) == 31
            assert entries[0].name == "HDOS.SYS"
            assert entries[0].kind == "file"
            assert entries[0].size_bytes == 24 * 256
            assert entries[-1].name == "DIRECT.SYS"


def test_reads_a_file_out_through_the_grt_chain():
    with remanence.Session() as session:
        medium, handle = _load(session)
        with handle:
            filesystem = medium.partition(0).filesystem_as("hdos")
            contents = filesystem.read_file("DEMO.BAS")
            # Sector-granular in HDOS terms, as the Rust suite states.
            assert contents
            assert len(contents) % 256 == 0
            # BASIC source: mostly printable ASCII plus CR/LF/NUL.
            printable = sum(
                1
                for byte in contents
                if byte in (0x00, 0x0D, 0x0A) or 0x20 <= byte < 0x7F
            )
            assert printable * 10 >= len(contents) * 9


def test_a_file_the_catalog_does_not_hold_is_refused_by_name():
    with remanence.Session() as session:
        medium, handle = _load(session)
        with handle:
            filesystem = medium.partition(0).filesystem_as("hdos")
            assert filesystem.stat("NOPE.NOP") is None
            try:
                filesystem.read_file("NOPE.NOP")
            except remanence.Error as refusal:
                assert refusal.category == "not-found"
            else:
                raise AssertionError("nothing is there")
