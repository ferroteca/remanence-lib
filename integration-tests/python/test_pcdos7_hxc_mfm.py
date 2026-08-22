# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""An IBM PC DOS 7 distribution disk, read from an HxC MFM container.

The Python analog of `integration-tests/rust/tests/pcdos7_hxc_mfm.rs`:
the flux ladder climbed from this surface — cells to bits to MFM bytes
to the recording's own sectors — and the FAT volume above them opened
through the partition the sector layer composes, with file contents
checked rather than names alone.
"""

from __future__ import annotations

import common
import remanence

FIXTURE = "pcdos-7-de-disk1.mfm"
DEVICE = "pc-3.5-hd"


def _load(session):
    path = common.ensure_fixture(FIXTURE)
    handle = open(path, "rb")
    medium = session.load_media(handle, "hxc-mfm", device=DEVICE)
    return medium, handle


def test_the_container_declares_what_hxc_actually_writes():
    with remanence.Session() as session:
        medium, handle = _load(session)
        with handle:
            assert medium.device_type == DEVICE
            assert medium.article == "flexible-3.5-hd"
            evidence = "\n".join(medium.assurance.evidence)
            assert "stating 501 kbit/s at 0 RPM" in evidence
            assert "a measured figure within one part in 50" in evidence
            assert "states no RPM" in evidence
            assert "state more cells than one revolution holds" in evidence
            assert "declared synthetic" in evidence


def test_every_sector_reads_and_the_fat_volume_opens_above_them():
    with remanence.Session() as session:
        medium, handle = _load(session)
        with handle:
            bits = medium.bitstream().inspect()
            assert bits.profile_id == "pc-3.5-hd"
            assert len(bits.locations) == 160

            sectors = medium.bytestream().recognize_ibm_sectors()
            assert sectors.claim_count == 80 * 2 * 18
            geometry = sectors.geometry()
            assert (
                geometry.cylinders,
                geometry.heads,
                geometry.sectors_per_track,
                geometry.sector_bytes,
            ) == (80, 2, 18, 512)

            filesystem = sectors.partition().filesystem_as("fat")
            entries = filesystem.entries("")
            assert len(entries) == 44
            assert [entry.name for entry in entries[:3]] == [
                "IBMBIO.COM",
                "IBMDOS.COM",
                "COMMAND.COM",
            ]

            bio = filesystem.read_file("IBMBIO.COM")
            assert len(bio) == 40_863
            assert bio[:3] == b"\xe9\x35\x01"
            command = filesystem.read_file("COMMAND.COM")
            assert len(command) == 55_377
            assert any(byte != 0 for byte in command)
