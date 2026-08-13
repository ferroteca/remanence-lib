# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Refusals carry a category, and a rule where a set defines one (P3, P10).

A refusal is part of this library's surface rather than an accident at
the edge of it: the whole design refuses by name where evidence cannot
bear a reading. These tests assert the shape a caller can rely on — that
`remanence.Error` is raised, that `category` is always set, and that
`rule` is set where the seam has an enumerated rule set and `None` where
it does not.
"""

import pytest

import remanence


def test_a_refusal_is_one_exception_type_with_two_attributes():
    with pytest.raises(remanence.Error) as refusal:
        remanence.discover_media("no-such-artifact-anywhere.img", writable=False)

    error = refusal.value
    assert isinstance(error, Exception)
    assert error.category, "category is always set"
    assert error.category in {
        condition_free
        for condition_free in (
            "io",
            "not-found",
            "unavailable",
            "unsupported",
            "invalid-image",
            "read-only",
        )
    }, f"unexpected category {error.category!r}"
    # This refusal belongs to no enumerated rule set, and that absence is
    # the ordinary case rather than an omission.
    assert error.rule is None


def test_the_message_names_what_was_refused():
    with pytest.raises(remanence.Error) as refusal:
        remanence.discover_media("a-very-specific-missing-name.img", writable=False)
    assert "a-very-specific-missing-name.img" in str(refusal.value)


def test_a_format_that_records_several_devices_refuses_a_bare_declaration():
    several = [entry for entry in remanence.formats() if len(entry[2]) > 1]
    assert several, "nothing to exercise"
    identity = several[0][0]

    with remanence.Session() as session:
        with pytest.raises(remanence.Error) as refusal:
            # No device stated, and this format records more than one.
            session.load_media(_a_closed_handle(), identity)
    # It refuses before it ever reads, so the complaint is the source or
    # the declaration, never a parse failure.
    assert refusal.value.category


def test_an_unknown_device_type_is_refused_by_name():
    with remanence.Session() as session:
        with pytest.raises(remanence.Error) as refusal:
            session.add_device("no-such-drive")
    assert "no-such-drive" in str(refusal.value)


def test_an_unknown_partition_type_reading_is_refused_by_name():
    with remanence.Session() as session:
        blank = session.new_media(
            "chs-disk", cylinders=1, heads=1, sectors_per_track=1, sector_bytes=512
        )
        direct = blank.partition(0)
        assert direct is not None
        with pytest.raises(remanence.Error) as refusal:
            direct.check_type("no-such-partition-type")
    assert "no-such-partition-type" in str(refusal.value)


def test_a_partial_geometry_is_refused_because_it_is_whole_or_nothing():
    """Every part is stated, or the declaration addresses nothing."""
    whole = dict(cylinders=40, heads=2, sectors_per_track=9, sector_bytes=512)
    with remanence.Session() as session:
        for omitted in whole:
            partial = {key: value for key, value in whole.items() if key != omitted}
            with pytest.raises(remanence.Error) as refusal:
                session.new_media("chs-disk", **partial)
            assert refusal.value.category, f"omitting {omitted} was not refused by name"


def test_releasing_a_medium_that_is_not_pooled_is_refused():
    with remanence.Session() as session:
        with pytest.raises(remanence.Error):
            session.release_media(9999)


def test_an_authored_blank_is_refused_by_a_drive_that_records_media():
    """Insert checks device-type equality, and an authored blank has none."""
    with remanence.Session() as session:
        blank = session.new_media(
            "chs-disk", cylinders=40, heads=1, sectors_per_track=9, sector_bytes=512
        )
        slots = [slot for slot in remanence.device_slots() if slot.device_type]
        assert slots, "no device slot with a device type"
        device = session.add_device(slots[0].id)
        with pytest.raises(remanence.Error):
            device.insert(blank.id)


class _Closed:
    """A file object already closed, so a load refuses before reading."""

    def fileno(self):
        raise ValueError("I/O operation on closed file")


def _a_closed_handle():
    return _Closed()
