# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""PC-DOS 1.00, read from the release that wrote no parameter block.

The Python analog of `integration-tests/rust/tests/pcdos_files.rs`,
narrowed to the same media-descriptor-table claim: a 1.x boot sector
states nothing a reader can look up, so the FAT layout the library
applies is a claim about the world, and a real IBM distribution disk with
files whose contents say for themselves is what makes it falsifiable.
"""

from __future__ import annotations

import common
import remanence

FIXTURE = "pcdos-100-disk01.img"
DEVICE = "sector-floppy"

# The names IBM shipped on the 1.00 distribution disk, in the order its
# own root directory states them.
FIRST_NAMES = [
    "IBMBIO.COM",
    "IBMDOS.COM",
    "COMMAND.COM",
    "FORMAT.COM",
    "CHKDSK.COM",
]


def _load(session):
    path = common.ensure_fixture(FIXTURE)
    handle = open(path, "rb")
    medium = session.load_media(handle, "raw", device=DEVICE, block_bytes=512)
    return medium, handle


def test_a_pc_dos_1_volume_reads_without_any_parameter_block():
    with remanence.Session() as session:
        medium, handle = _load(session)
        with handle:
            filesystem = medium.partition(0).filesystem_as("fat")
            entries = filesystem.entries("")
            assert len(entries) == 40
            assert [entry.name for entry in entries[: len(FIRST_NAMES)]] == FIRST_NAMES


def test_the_files_come_back_at_the_sizes_and_contents_the_disk_states():
    with remanence.Session() as session:
        medium, handle = _load(session)
        with handle:
            filesystem = medium.partition(0).filesystem_as("fat")

            # Large enough to span many clusters, so this exercises the
            # chain rather than one entry, and an .EXE says what it is in
            # its first two bytes.
            link = filesystem.read_file("LINK.EXE")
            assert len(link) == 43_264
            assert link[:2] == b"MZ"

            # A .COM has no header to check, so the check is its exact
            # recorded length and that it is not all zeroes — what a
            # mis-walked cluster chain tends to return.
            command = filesystem.read_file("COMMAND.COM")
            assert len(command) == 3_231
            assert any(byte != 0 for byte in command)
