<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged — owed by the project, and not yet delivered. This
> file says nothing about when, and the order below is the order the
> numbers were issued, not a schedule. Feature numbers evaporate on
> delivery and are never reissued.

The entries here are what remains of a single proposed feature that did
not fit one sprint: reading an HxC `.mfm` artifact and building the FM/MFM
channel it needs. The split retired the parent number, which was F72, and
issued a fresh number to each piece. F77 delivers the format to the bit
tier; F78 carries it up to sectors, where the filesystem doors already
are.

The third piece — making the ladder family-plural, so that a medium of a
second family has a rung to answer with at all — was F76, and it is
**delivered**: the rungs are `Bitstream` and `Bytestream`, the channel
takes the profile rather than assuming one, and a family enrols its own
bitstream-to-bytestream transition on its profile. Its number has retired
with it, and what specifies it now is the code. The ruling it forced along
the way is D59.

Companion design:
[design/fm-mfm-read-channel.md](design/fm-mfm-read-channel.md).

## F68 — ImageDisk read, presenting sectors in the order the recording numbers them

Read an ImageDisk (`.imd`) artifact: the header and comment, every track's
mode and data rate, its sector-id map and optional cylinder and head maps,
and all nine of the sector-data record types the format defines.

**The adapter presents sectors in stated-id order, and that is a ruling
rather than a convenience** (D60). An ImageDisk track stores its sectors in
the physical order they were recorded in and states separately which id
each one carries; a raw dump of the same disk is already in id order. Where
the ordering is resolved decides what every layer above has to know, and it
belongs to the image format, which is the only layer holding the evidence
for it. What the format states, the adapter applies; what it does not state
is somebody else's declaration.

That ruling is what lets this feature be small. With ids resolved, a
uniform ImageDisk *is* a linear extent, so it needs no new device seam —
the flat presentation every other sector image uses serves it exactly, and
the CP/M layouts read it with the same declared block they read a raw dump
with, minus the skew the format has already resolved.

**Non-uniform images are read, and are not addressed by coordinate.** A
disk whose track 0 differs from the rest is the ordinary case, not the
exotic one, and it has no single geometry tuple to declare. Rather than
invent one, the adapter declares none: byte access and every filesystem
above it work exactly as they do elsewhere, and `read_sector` refuses
through the geometry seam's existing rule, because the coordinate genuinely
is not established. Giving that case coordinates is F80 and is deliberately
not attempted here.

**A sector the artifact does not hold is not zeroes.** ImageDisk records
unavailable sectors explicitly, so the extents they occupy are excluded
from what the load establishes as readable and a read touching one is
refused with its range (P28) — the same machinery a short raw image already
uses. Deleted-address marks, data-error marks and compressed encoding are
counted into the load's evidence rather than flattened away.

An unrecognized sector-data type byte, a map disagreeing with its track
header, a duplicate id within a track, or a truncated final track is
refused by name (P3), never repaired.

Read only. The write direction is F69.

Touches: S1, S2, S3. Supports: U1, U2; P3–P5, P8, P10, P12–P14, P21, P27,
P28. F80 is not a prerequisite; it is what this feature deliberately
declines to do.

Companion design:
[design/imagedisk-read.md](design/imagedisk-read.md).

## F77 — HxC MFM read, to the bit tier

Read an HxC `.mfm` artifact as a flux-family medium and answer its
bitstream: the container's per-track, per-side MFM cell runs at the rate
the file declares, presented as the bit tier already presents the 1541's —
every bit saying whether it was recorded or resolved by a declared rule.

The version and grammar claim is exact (P8), and what the container does
not carry is stated rather than quietly supplied: an HxC MFM file holds no
weak region, no density variation and no second observation of a location,
so nothing at the tier above may later present one as recovered evidence.

The medium is claimed for the drive families the format's own declaration
supports, refusing by name where a caller declares one it does not (P12).
The artifact is read; nothing is written.

This stops at bits deliberately. Bits with no framing are a coherent rung
in this ladder — the medium answers `bitstream` and nothing above it — and
the framing is a body of work with its own refusals, which is F78.

Touches: S1, S2, S3. Supports: U1; P1, P3–P5, P8, P10, P12–P14, P21–P23,
P27, P30. F76 is a prerequisite.

## F78 — The FM and MFM framing and its sector claims

Carry an FM or MFM recording from bits to the recording's own sectors: the
address marks, the bytes they frame, and the sector claims above them.

Framing here is not the 1541's. That channel frames on a code table with
undefined patterns; FM and MFM have no such table, and a byte is framed
from the sync field and an address mark identified by a **deliberate
violation** of the encoding — a missing clock transition. The bit tier must
therefore be able to carry "this cell is a deliberate violation" as a fact
about the recording rather than an error resolved away, and F77's tier is
sized for that.

Above it the existing discipline is inherited rather than restated: the
address the recording states for itself, its stated and computed checksums
side by side, and a count of what could not be resolved. Nothing is
repaired and no field is filled in. A data field opened by a deleted-data
mark is what the recording says, carried as a declared fact — nothing here
decides on a caller's behalf whether such a sector counts.

CRC-16/CCITT joins CRC-32 in `checksum.rs` as a small check several formats
share, implemented once.

The payoff is that these sectors reach doors already built: a FAT or HDOS
volume on an MFM recording opens through the same partition and filesystem
seam a hard-disk image uses, with neither side learning about the other.
Delivering that reach is part of this feature; inventing a new file seam
for it is not.

Touches: S1, S2, S3. Supports: U1, U2; P3–P5, P10, P12, P13, P16–P19,
P21–P23, P27. F76 and F77 are prerequisites.
