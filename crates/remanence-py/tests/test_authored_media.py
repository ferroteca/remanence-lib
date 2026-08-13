# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Authorship, the third fact class — and the suite's only fixture.

Evidence is discovered onto media and declarations are configured onto
machines; `new_media` is neither. It creates a medium whole from what the
author states, which is what lets this suite exercise real journeys
**without shipping a single disk image**: the project's fixtures are
vintage third-party media it claims no copyright in, and they are not in
the repository at all.

So everything here is made, not found.
"""

import pytest

import remanence


@pytest.fixture
def session():
    with remanence.Session() as opened:
        yield opened


@pytest.fixture
def blank(session):
    """A CHS disk whose coordinates are the author's own."""
    return session.new_media(
        "chs-disk", cylinders=40, heads=2, sectors_per_track=9, sector_bytes=512
    )


def test_the_declaration_becomes_the_mediums_original_facts(blank):
    assert blank.article == "authored"
    assert blank.size == 40 * 2 * 9 * 512

    geometry = blank.geometry
    assert geometry.state == "determined"
    assert (geometry.cylinders, geometry.heads, geometry.sectors_per_track) == (40, 2, 9)
    assert geometry.sector_bytes == 512


def test_the_one_geometry_reading_is_authorship(blank):
    readings = blank.geometry.readings
    assert len(readings) == 1, "an authored medium's coordinates have one source"
    assert readings[0].source == "authorship"


def test_an_authored_blank_assumes_no_device(blank):
    # Nothing recorded it, so no drive takes one.
    assert blank.device_type is None
    assert blank.authored_as is not None


def test_nobody_opened_it_so_the_claim_is_authorship(blank):
    assurance = blank.assurance
    assert assurance.outcome == "verified"
    assert assurance.claim == "authored"


def test_a_sector_written_reads_back_after_commit(blank):
    payload = bytes(range(256)) * 2
    blank.write_sector(0, 0, 1, payload)
    blank.commit()
    assert blank.read_sector(0, 0, 1) == payload


def test_a_write_is_buffered_until_it_is_committed(blank):
    blank.write_sector(1, 0, 1, b"\xa5" * 512)
    assert blank.is_modified
    blank.rollback()
    assert blank.read_sector(1, 0, 1) == b"\x00" * 512


def test_a_sector_outside_the_authored_coordinates_is_refused(blank):
    with pytest.raises(remanence.Error) as refusal:
        blank.read_sector(999, 0, 1)
    assert refusal.value.category


def test_a_medium_recording_no_scheme_bears_the_direct_partition(blank):
    assert blank.partition_scheme is None
    pool = blank.partitions()
    assert len(pool) == 1

    direct = blank.partition(0)
    assert direct is not None
    assert direct.is_direct
    # It is the library's own composition, carried as provenance and
    # never as evidence, so it records no type to check a reading against.
    assert direct.type_byte is None
    with pytest.raises(remanence.Error):
        direct.check_type("dos-primary")


def test_the_blank_article_kinds_take_no_coordinates(session):
    kinds = [kind for kind in remanence.new_media_kinds() if not kind[3]]
    assert kinds, "no blank article kind to exercise"
    with pytest.raises(remanence.Error):
        session.new_media(kinds[0][0], cylinders=40)


def test_coordinates_that_address_nothing_are_refused_when_stated(session):
    with pytest.raises(remanence.Error):
        session.new_media("chs-disk", cylinders=0, heads=2, sectors_per_track=9)


def test_the_session_pools_it_and_release_is_the_one_destroying_verb(session, blank):
    media_id = blank.id
    assert media_id in session.media
    assert session.medium(media_id) is not None

    session.release_media(media_id)
    assert media_id not in session.media
    assert session.medium(media_id) is None


def test_an_unknown_kind_is_refused_by_name(session):
    with pytest.raises(remanence.Error) as refusal:
        session.new_media("no-such-kind")
    assert "no-such-kind" in str(refusal.value)
