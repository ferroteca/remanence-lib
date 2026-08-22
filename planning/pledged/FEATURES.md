<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged — owed by the project, and not yet delivered. This
> file says nothing about when, and the order below is the order the
> numbers were issued, not a schedule. Feature numbers evaporate on
> delivery and are never reissued.

Two bodies of pledged work are cut into features here.

F77 and F78 are what remains of a single proposed feature that did not
fit one sprint: reading an HxC `.mfm` artifact and building the FM/MFM
channel it needs. The split retired the parent number, which was F72,
and issued a fresh number to each piece. F77 delivers the format to the
bit tier; F78 carries it up to sectors, where the filesystem doors
already are. The third piece — making the ladder family-plural, so that
a medium of a second family has a rung to answer with at all — was F76,
and it is **delivered**: the rungs are `Bitstream` and `Bytestream`, the
channel takes the profile rather than assuming one, and a family enrols
its own bitstream-to-bytestream transition on its profile. Its number
has retired with it, and what specifies it now is the code. The ruling
it forced along the way is D59. Companion design:
[design/fm-mfm-read-channel.md](design/fm-mfm-read-channel.md).

U35 — a blank DOS floppy, formatted, filled and saved as a raw image —
was cut into three pieces and all three are **delivered**: F81
catalogued the PC articles, blank kinds and drive families; F82
delivered the arc that records a DOS layout onto a blank article; F83
delivered the raw rendition that writes a sector medium out. All three
numbers have retired with them, and U35 itself has moved to root
USE-CASES.md on that full delivery. What specifies all of it now is the
code.

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
