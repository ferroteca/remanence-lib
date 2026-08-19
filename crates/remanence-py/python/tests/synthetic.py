# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Disk images built byte by byte, so the suite needs no disk images.

Every artifact this project tests against is third-party media it does
not distribute and git does not track, which is why the shipped suite
opens none of them. This module is the way past that: an MBR and a FAT12
volume are small, wholly specified structures, and building them here
costs a few hundred lines and buys the partition and filesystem doors —
the largest part of the surface authored blanks alone cannot reach.

Constructing them rather than downloading them is also **stronger** in
one way that matters: the expected reading is known because the bytes
were chosen, so a test can assert what the library *should* say, and can
bend one field at a time to see a refusal arrive by name.

Nothing here is a general-purpose formatter. It writes the one shape the
tests need, correctly, and says where it is cutting a corner.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field

SECTOR = 512

#: The MBR type byte for a FAT12 primary partition.
FAT12_TYPE = 0x01
#: Where the partition starts. Not 63 or 2048 — those are conventions
#: for real hardware, and a smaller number keeps the image small.
PARTITION_LBA = 64

# ------------------------------------------------------------------ FAT12


@dataclass
class Fat12Layout:
    """Everything the BPB states, kept so tests can assert against it."""

    sectors: int = 1024
    bytes_per_sector: int = SECTOR
    sectors_per_cluster: int = 1
    reserved_sectors: int = 1
    fat_count: int = 2
    root_entries: int = 64
    media_descriptor: int = 0xF8
    sectors_per_fat: int = 3
    sectors_per_track: int = 32
    heads: int = 2
    hidden_sectors: int = PARTITION_LBA
    label: str = "SYNTHETIC "
    volume_id: int = 0x1234ABCD

    @property
    def root_sectors(self) -> int:
        # 32 bytes per directory entry, rounded up to whole sectors.
        return (self.root_entries * 32 + self.bytes_per_sector - 1) // self.bytes_per_sector

    @property
    def first_data_sector(self) -> int:
        return (
            self.reserved_sectors
            + self.fat_count * self.sectors_per_fat
            + self.root_sectors
        )

    @property
    def cluster_count(self) -> int:
        data = self.sectors - self.first_data_sector
        return data // self.sectors_per_cluster


@dataclass
class File:
    """One 8.3 file to place in the root directory."""

    name: str  # "HELLO" — the stem, up to 8 characters
    extension: str  # "TXT" — up to 3
    content: bytes
    attributes: int = 0x20  # archive

    @property
    def dos_name(self) -> str:
        return f"{self.name}.{self.extension}" if self.extension else self.name


def _boot_sector(layout: Fat12Layout) -> bytes:
    """The BPB, which is where the library reads the volume's own claims."""
    sector = bytearray(layout.bytes_per_sector)
    sector[0:3] = b"\xeb\x3c\x90"  # a short jump, as a real boot sector has
    sector[3:11] = b"MSWIN4.1"  # OEM name
    struct.pack_into(
        "<HBHBHHBHHHII",
        sector,
        11,
        layout.bytes_per_sector,
        layout.sectors_per_cluster,
        layout.reserved_sectors,
        layout.fat_count,
        layout.root_entries,
        layout.sectors if layout.sectors < 0x10000 else 0,
        layout.media_descriptor,
        layout.sectors_per_fat,
        layout.sectors_per_track,
        layout.heads,
        layout.hidden_sectors,
        0 if layout.sectors < 0x10000 else layout.sectors,
    )
    sector[36] = 0x80  # drive number
    sector[38] = 0x29  # extended boot signature: the three fields follow
    struct.pack_into("<I", sector, 39, layout.volume_id)
    sector[43:54] = layout.label.ljust(11).encode("ascii")[:11]
    sector[54:62] = b"FAT12   "
    sector[510:512] = b"\x55\xaa"
    return bytes(sector)


def _fat(layout: Fat12Layout, chains: list[list[int]]) -> bytes:
    """One FAT, packed 12 bits per entry, two entries per three bytes."""
    entries = [0] * layout.cluster_count + [0] * 2
    entries[0] = 0xF00 | layout.media_descriptor
    entries[1] = 0xFFF

    for chain in chains:
        for at, cluster in enumerate(chain):
            last = at == len(chain) - 1
            entries[cluster] = 0xFFF if last else chain[at + 1]

    packed = bytearray(layout.sectors_per_fat * layout.bytes_per_sector)
    for index in range(0, len(entries) - 1, 2):
        low, high = entries[index], entries[index + 1]
        offset = index * 3 // 2
        if offset + 2 >= len(packed):
            break
        packed[offset] = low & 0xFF
        packed[offset + 1] = ((low >> 8) & 0x0F) | ((high & 0x0F) << 4)
        packed[offset + 2] = (high >> 4) & 0xFF
    return bytes(packed)


def _directory_entry(entry: File, first_cluster: int) -> bytes:
    record = bytearray(32)
    record[0:8] = entry.name.upper().ljust(8).encode("ascii")[:8]
    record[8:11] = entry.extension.upper().ljust(3).encode("ascii")[:3]
    record[11] = entry.attributes
    struct.pack_into("<H", record, 22, 0x6000)  # time
    struct.pack_into("<H", record, 24, 0x5A21)  # date: 2025-01-01
    struct.pack_into("<H", record, 26, first_cluster)
    struct.pack_into("<I", record, 28, len(entry.content))
    return bytes(record)


def fat12_volume(files: list[File], layout: Fat12Layout | None = None) -> bytes:
    """A whole FAT12 volume holding `files` in its root directory."""
    layout = layout or Fat12Layout()
    volume = bytearray(layout.sectors * layout.bytes_per_sector)
    volume[0 : layout.bytes_per_sector] = _boot_sector(layout)

    cluster_bytes = layout.sectors_per_cluster * layout.bytes_per_sector
    next_free = 2  # clusters 0 and 1 are the two reserved FAT entries
    chains: list[list[int]] = []
    directory = bytearray()

    for entry in files:
        needed = max(1, (len(entry.content) + cluster_bytes - 1) // cluster_bytes)
        chain = list(range(next_free, next_free + needed))
        next_free += needed
        chains.append(chain)
        directory += _directory_entry(entry, chain[0])

        for at, cluster in enumerate(chain):
            sector = layout.first_data_sector + (cluster - 2) * layout.sectors_per_cluster
            start = sector * layout.bytes_per_sector
            piece = entry.content[at * cluster_bytes : (at + 1) * cluster_bytes]
            volume[start : start + len(piece)] = piece

    table = _fat(layout, chains)
    for copy in range(layout.fat_count):
        start = (layout.reserved_sectors + copy * layout.sectors_per_fat) * layout.bytes_per_sector
        volume[start : start + len(table)] = table

    root_start = (
        layout.reserved_sectors + layout.fat_count * layout.sectors_per_fat
    ) * layout.bytes_per_sector
    volume[root_start : root_start + len(directory)] = directory

    return bytes(volume)


# --------------------------------------------------------------------- MBR


def _chs(lba: int, heads: int, sectors_per_track: int) -> bytes:
    """The three CHS bytes an MBR entry carries beside its LBA fields.

    Capped at the classic 1023/254/63 ceiling, which is what a real table
    does for anything past it — these tests address by LBA, so the value
    is filled plausibly rather than relied upon.
    """
    cylinder, remainder = divmod(lba, heads * sectors_per_track)
    head, sector = divmod(remainder, sectors_per_track)
    if cylinder > 1023:
        cylinder, head, sector = 1023, heads - 1, sectors_per_track - 1
    return bytes([head, ((cylinder >> 2) & 0xC0) | ((sector + 1) & 0x3F), cylinder & 0xFF])


@dataclass
class Partition:
    """One MBR table entry."""

    start_lba: int
    sectors: int
    type_byte: int = FAT12_TYPE
    active: bool = True
    payload: bytes = b""


@dataclass
class Disk:
    """An MBR-schemed disk: a table, and whatever the entries point at."""

    partitions: list[Partition] = field(default_factory=list)
    total_sectors: int = 2048
    heads: int = 2
    sectors_per_track: int = 32
    signature: int = 0xDEADBEEF
    boot_signature: bytes = b"\x55\xaa"

    def to_bytes(self) -> bytes:
        image = bytearray(self.total_sectors * SECTOR)
        table = bytearray(SECTOR)
        struct.pack_into("<I", table, 440, self.signature)

        for index, part in enumerate(self.partitions):
            offset = 446 + index * 16
            table[offset] = 0x80 if part.active else 0x00
            table[offset + 1 : offset + 4] = _chs(
                part.start_lba, self.heads, self.sectors_per_track
            )
            table[offset + 4] = part.type_byte
            table[offset + 5 : offset + 8] = _chs(
                part.start_lba + part.sectors - 1, self.heads, self.sectors_per_track
            )
            struct.pack_into("<II", table, offset + 8, part.start_lba, part.sectors)

            start = part.start_lba * SECTOR
            image[start : start + len(part.payload)] = part.payload

        table[510:512] = self.boot_signature
        image[0:SECTOR] = table
        return bytes(image)


def mbr_with_one_fat12(files: list[File] | None = None) -> tuple[bytes, Fat12Layout]:
    """The shape the tests use: one active FAT12 primary, and nothing else."""
    files = files or [File("HELLO", "TXT", b"synthesised, not downloaded\n")]
    layout = Fat12Layout()
    volume = fat12_volume(files, layout)
    disk = Disk(
        partitions=[
            Partition(start_lba=PARTITION_LBA, sectors=layout.sectors, payload=volume)
        ],
        total_sectors=PARTITION_LBA + layout.sectors,
    )
    return disk.to_bytes(), layout
