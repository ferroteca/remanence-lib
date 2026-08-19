# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""A consumer that must type-check clean against the stub.

This file is never executed — the paths in it do not exist and nothing
imports it. It is read by `mypy --strict` alone, and what it asserts is
that the stub's types are *usable*: that ordinary consumer code compiles
against them, and that each annotated binding below matches what the stub
promises. A binding whose declared type is wrong fails the check.

Annotations are written out rather than using `typing.assert_type`, which
arrived in 3.11 — the check runs at 3.10, the minimum the distribution
claims, so the stub is verified against the oldest Python it promises to
serve.
"""

from __future__ import annotations

import remanence

version: str = remanence.__version__
default_cache: int = remanence.DEFAULT_CACHE_BYTES

# --- catalogs ----------------------------------------------------------
for fmt_id, fmt_name, devices, takes_block, takes_collection in remanence.formats():
    ids: str = fmt_id
    names: str = fmt_name
    device_list: list[str] = devices
    block_flag: bool = takes_block
    collection_flag: bool = takes_collection

slots: list[remanence.DeviceSlot] = remanence.device_slots()
schemes: list[tuple[str, str]] = remanence.partition_schemes()
conditions: list[str] = remanence.assurance_conditions()

# --- discovery ---------------------------------------------------------
discovery = remanence.discover_media("disk.img", writable=False)
accepting: list[str] = discovery.accepting_devices
recorded: str | None = discovery.device_type
discovered_assurance: remanence.Assurance = discovery.assurance

# --- a session, a medium, a namespace ----------------------------------
with remanence.Session() as session, open("disk.qcow2", "rb") as handle:
    medium = session.load_media(handle, "qcow2", device="mbr-block-hd")
    medium_id: int = medium.id
    size: int = medium.size
    device_type: str | None = medium.device_type
    vdi: tuple[int, int] | None = medium.vdi_version
    geometry: remanence.Geometry = medium.geometry
    cylinders: int | None = geometry.cylinders
    readings: list[remanence.GeometryReading] = geometry.readings

    sector: bytes = medium.read_sector(0, 0, 1)
    medium.write_sector(0, 0, 1, b"\x00" * 512)

    report = medium.inspect()
    regions: list[remanence.RegionInfo] = report.regions
    volume_info: remanence.VolumeInfo | None = report.volume(report.volumes[0].id)
    filesystem_info: remanence.FilesystemInfo | None = report.filesystem_on(
        report.volumes[0].id
    )

    partition = medium.partition(1)
    if partition is not None:
        ordinal: int = partition.ordinal
        type_byte: int | None = partition.type_byte
        evidence: list[str] = partition.evidence
        partition.check_type("dos-primary")

        space = partition.filesystem()
        if space is not None:
            entries: list[remanence.Entry] = space.entries("")
            for entry in entries:
                entry_name: str = entry.name
                facts: list[remanence.EntryFact] = entry.declared
            contents: bytes = space.read_file("BOOT.SYS")
            space.write_file("OUT.TXT", b"written")
            handle_file = space.get_file("BOOT.SYS")
            payload: bytes = handle_file.bytes()
            chunk: bytes = handle_file.read_at(0, 16)
            nested: remanence.Discovery = handle_file.discover()

    medium.commit()

    # An authored blank takes coordinates and assumes no device.
    blank = session.new_media("chs-disk", cylinders=40, heads=1, sectors_per_track=9)
    authored: str | None = blank.authored_as

# --- the flux ladder ---------------------------------------------------
with remanence.FluxImage("disk.remanence", cache_bytes=1 << 20) as image:
    image_report: remanence.FluxImageReport = image.inspect()
    holes: list[remanence.FluxHole] = image_report.holes
    orbits: list[remanence.FluxOrbit] = image_report.orbits

    bitstream = image.materialize_bitstream()
    location_count: int = bitstream.location_count
    bitstream_report: remanence.BitstreamReport = bitstream.inspect()

    bytestream = bitstream.materialize_bytestream(cache_bytes=1 << 20)
    location = bytestream.location(18)
    location_bytes: int = location.bytes
    framed: bytes = location.read_at(0, 32)

    sectors = bytestream.recognize_sectors()
    claims: int = sectors.claim_count
    recovered: bytes = sectors.read_sector(18, 0)
    flux_partition: remanence.Partition = sectors.partition()

    d64: remanence.D64Report = image.describe_d64()
    missing: list[remanence.D64Block] = d64.missing
    written: remanence.FluxWriteReport = image.write("copy.remanence")

# --- the session's device set ------------------------------------------
seated: remanence.StorageDevice = session.add_device("mbr-sector-hd")
attachments: list[str] = session.devices
found: remanence.StorageDevice | None = session.device(seated.attachment)
session.release_device(seated.attachment)

# --- refusals ----------------------------------------------------------
try:
    remanence.discover_media("missing.img", writable=True)
except remanence.Error as refusal:
    category: str = refusal.category
    rule: str | None = refusal.rule
