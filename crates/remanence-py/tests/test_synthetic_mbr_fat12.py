# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""The partition and filesystem doors, over an image the suite built.

`test_authored_media.py` reaches everything a blank medium can answer,
and stops where a *recorded structure* begins: a partition table and a
filesystem are things found on media, and an authored blank has neither.
This file supplies them without shipping an artifact — `synthetic.py`
writes an MBR and a FAT12 volume byte by byte, and the tests load the
result as a raw image.

Because the bytes were chosen, the expected reading is known: these
assert the library says what the structure says, and — the half a
downloaded fixture cannot easily give — that bending one field produces
the refusal it should.
"""

from __future__ import annotations

import pytest

import remanence
import synthetic

DEVICE = "mbr-block-hd"


@pytest.fixture
def image(tmp_path):
    """The default shape: one active FAT12 primary holding one file."""
    payload, layout = synthetic.mbr_with_one_fat12()
    path = tmp_path / "synthetic.img"
    path.write_bytes(payload)
    return path, layout


def _load(session, path):
    handle = open(path, "rb")
    return session.load_media(
        handle, "raw", device=DEVICE, block_bytes=synthetic.SECTOR
    ), handle


def test_the_table_this_suite_wrote_is_the_scheme_the_library_reads(image):
    path, _ = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            assert medium.partition_scheme == "mbr"
            assert len(medium.partitions()) == 1


def test_the_entry_states_its_type_and_the_library_reads_it(image):
    path, layout = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            partition = medium.partition(1)
            assert partition is not None
            assert partition.type_byte == synthetic.FAT12_TYPE
            assert partition.type_reading == "FAT12"
            assert partition.active
            assert partition.bears_namespace
            assert partition.start_bytes == synthetic.PARTITION_LBA * synthetic.SECTOR
            assert partition.length_bytes == layout.sectors * synthetic.SECTOR


def test_a_declared_reading_is_checked_against_the_recorded_byte(image):
    path, _ = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            partition = medium.partition(1)
            partition.check_type("dos-primary")  # 0x01 bears it

            with pytest.raises(remanence.Error) as refusal:
                partition.check_type("dos-extended")
            # The refusal names both sides: what was recorded, what was read.
            assert "dos-extended" in str(refusal.value)


def test_the_filesystem_door_opens_on_the_declared_type(image):
    path, _ = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            space = medium.partition(1).filesystem()
            assert space is not None
            assert space.kind == "FAT12"
            assert space.is_addressable


def test_the_volume_label_is_read_from_the_boot_record(image):
    path, layout = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            label = medium.partition(1).filesystem().label()
            assert label is not None
            assert label.name == layout.label.strip()


def test_a_file_written_into_the_image_lists_and_reads_back(image):
    path, _ = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            space = medium.partition(1).filesystem()
            entries = space.entries("")
            assert [entry.name for entry in entries] == ["HELLO.TXT"]
            assert entries[0].kind == "file"
            assert entries[0].size_bytes == len(b"synthesised, not downloaded\n")
            assert space.read_file("HELLO.TXT") == b"synthesised, not downloaded\n"


def test_several_files_across_several_clusters_all_read_back(tmp_path):
    files = [
        synthetic.File("ONE", "TXT", b"a" * 10),
        synthetic.File("TWO", "BIN", bytes(range(256)) * 8),  # spans clusters
        synthetic.File("THREE", "DAT", b""),  # empty
    ]
    payload, _ = synthetic.mbr_with_one_fat12(files)
    path = tmp_path / "several.img"
    path.write_bytes(payload)

    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            space = medium.partition(1).filesystem()
            listed = {entry.name: entry.size_bytes for entry in space.entries("")}
            assert listed == {
                "ONE.TXT": 10,
                "TWO.BIN": 2048,
                "THREE.DAT": 0,
            }
            for entry in files:
                assert space.read_file(entry.dos_name) == entry.content


def test_a_file_the_directory_does_not_hold_is_refused_by_name(image):
    path, _ = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            space = medium.partition(1).filesystem()
            with pytest.raises(remanence.Error) as refusal:
                space.read_file("NOSUCH.TXT")
            assert "NOSUCH.TXT" in str(refusal.value).upper()


def test_the_geometry_carries_what_the_boot_record_states(image):
    path, layout = image
    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            geometry = medium.geometry
            sources = {reading.source for reading in geometry.readings}
            assert sources, "a loaded image should state where its coordinates came from"
            # The BPB's own heads and sectors-per-track are one reading of
            # the disk's coordinates, and the library keeps every reading
            # with where it came from.
            assert any(
                reading.heads == layout.heads
                and reading.sectors_per_track == layout.sectors_per_track
                for reading in geometry.readings
            ), f"no reading matches the BPB: {[(r.source, r.heads, r.sectors_per_track) for r in geometry.readings]}"


# --------------------------------------------------- one field bent at a time


def test_a_table_without_its_signature_records_no_scheme(tmp_path):
    """Chosen bytes make this cheap: a downloaded image cannot be bent."""
    payload, layout = synthetic.mbr_with_one_fat12()
    disk = synthetic.Disk(
        partitions=[
            synthetic.Partition(
                start_lba=synthetic.PARTITION_LBA,
                sectors=layout.sectors,
                payload=synthetic.fat12_volume(
                    [synthetic.File("HELLO", "TXT", b"x")], layout
                ),
            )
        ],
        total_sectors=synthetic.PARTITION_LBA + layout.sectors,
        boot_signature=b"\x00\x00",  # the one thing changed
    )
    path = tmp_path / "unsigned.img"
    path.write_bytes(disk.to_bytes())

    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            # No table was recorded, so the medium bears the direct
            # partition — the library's own composition — and no scheme.
            assert medium.partition_scheme is None
            direct = medium.partition(0)
            assert direct is not None and direct.is_direct


def test_an_entry_typed_outside_the_claim_keeps_its_place_and_composes_nothing(tmp_path):
    payload, layout = synthetic.mbr_with_one_fat12()
    disk = synthetic.Disk(
        partitions=[
            synthetic.Partition(
                start_lba=synthetic.PARTITION_LBA,
                sectors=layout.sectors,
                type_byte=0xA5,  # FreeBSD slice: recorded, outside the claim
                payload=synthetic.fat12_volume([], layout),
            )
        ],
        total_sectors=synthetic.PARTITION_LBA + layout.sectors,
    )
    path = tmp_path / "foreign.img"
    path.write_bytes(disk.to_bytes())

    with remanence.Session() as session:
        medium, handle = _load(session, path)
        with handle:
            partition = medium.partition(1)
            assert partition is not None, "the entry keeps its place in the pool"
            assert partition.type_byte == 0xA5
            # Nothing this release reads determines a namespace here.
            assert not partition.bears_namespace
