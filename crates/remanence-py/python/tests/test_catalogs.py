# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""The enumerated claims a caller can hold without meeting one (P3).

Every catalog exists so a caller knows the whole set before it needs a
member of it. These tests assert the sets are non-empty, well-formed,
and — the part that matters — that the spellings they hand out are the
spellings the verbs accept back, which is the only thing that makes them
useful.
"""

import remanence


def test_the_module_states_its_version_and_cache_bound():
    assert isinstance(remanence.__version__, str)
    assert remanence.__version__
    assert remanence.DEFAULT_CACHE_BYTES > 0


def test_every_format_is_named_and_says_what_it_takes():
    formats = remanence.formats()
    assert formats, "the release claims no formats at all"
    for identity, name, devices, takes_block_bytes, takes_collection in formats:
        assert identity and isinstance(identity, str)
        assert name and isinstance(name, str)
        assert isinstance(devices, list)
        assert isinstance(takes_block_bytes, bool)
        assert isinstance(takes_collection, bool)


def test_a_format_recording_several_devices_names_them_all():
    several = [entry for entry in remanence.formats() if len(entry[2]) > 1]
    assert several, (
        "no format records more than one device type, so nothing exercises "
        "the declaration a load needs in that case"
    )
    known = {slot.device_type for slot in remanence.device_slots()}
    for _, _, devices, _, _ in several:
        for device in devices:
            assert device in known, (
                f"format device {device!r} is not a device type any slot "
                f"claims, so a caller reading one catalog cannot use the other"
            )


def test_every_new_media_kind_names_the_article_it_creates():
    kinds = remanence.new_media_kinds()
    assert kinds
    for identity, name, article, takes_geometry in kinds:
        assert identity and name and article
        assert isinstance(takes_geometry, bool)

    # Exactly one kind's facts *are* coordinates; the blank article kinds
    # take none and refuse them.
    with_geometry = [kind for kind in kinds if kind[3]]
    assert len(with_geometry) == 1, (
        "the authored kinds should have exactly one whose declaration "
        "carries coordinates"
    )


def test_every_recorded_layout_names_the_article_it_fits():
    layouts = remanence.recordings()
    assert layouts
    articles = {entry.article for entry in remanence.device_slots() if entry.article}
    for identity, name, article, geometry in layouts:
        assert identity and name and article
        cylinders, heads, sectors_per_track, sector_bytes = geometry
        assert cylinders and heads and sectors_per_track and sector_bytes
        assert article in articles, (
            f"layout {identity!r} records onto {article!r}, which no device "
            f"this release claims is served"
        )


def test_device_slots_carry_the_prefix_their_attachments_use():
    slots = remanence.device_slots()
    assert slots
    for slot in slots:
        assert slot.id and slot.name
        assert slot.slot_prefix, f"{slot.id} has no attachment prefix"


def test_the_optical_class_is_in_the_catalog_and_takes_its_own_bay():
    optical = [
        slot for slot in remanence.device_slots() if slot.device_class == "optical"
    ]
    assert optical, "no slot is an optical device, so a session cannot state one"
    for slot in optical:
        assert slot.addressing == "block", (
            f"{slot.id} is told a block number rather than a cylinder and a head"
        )
        assert slot.scheme is None, (
            f"{slot.id} bears the direct partition; a disc is mastered whole"
        )
        assert slot.article, f"{slot.id} names no article it is served"

    session = remanence.Session()
    device = session.add_device("cdrom")
    assert device.attachment == "cdrom0"
    assert not device.is_occupied, (
        "an empty drive is configuration in its own right — the machine "
        "held the drive whether or not a disc was in it"
    )


def test_the_archive_receiver_is_a_slot_that_is_no_device_type():
    archive = [slot for slot in remanence.device_slots() if slot.device_type is None]
    assert archive, (
        "no slot answers None for device_type, so nothing receives an "
        "archive — which was recorded by no device"
    )


def test_partition_schemes_and_types_are_paired_spellings():
    for catalog in (remanence.partition_schemes(), remanence.partition_types()):
        assert catalog
        for identity, name in catalog:
            assert identity and name
            assert identity == identity.lower(), (
                f"{identity!r} is a stable spelling and should be lower-case"
            )


def test_the_reading_catalogs_are_stated():
    assert remanence.geometry_sources()
    assert "authorship" in remanence.geometry_sources(), (
        "authorship is the one reading an authored medium's geometry has"
    )
    assert remanence.assurance_conditions()
