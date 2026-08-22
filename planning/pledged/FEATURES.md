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

F81, F82 and F83 deliver U35 — a blank DOS floppy, formatted, filled and
saved as a raw image — in the three pieces its journey has: the articles
and drives it needs catalogued, the arc that records a DOS layout onto a
blank, and the rendition that writes a sector medium out. Each is a
sprint on its own; F82 and F83 each need F81 first, and are otherwise
independent of each other.

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

## F81 — The PC floppy articles, blank kinds and drive families

Catalogue what a PC's two high-density floppies *are*, so that U35 has
something to author and somewhere to seat it.

The article catalog gains `flexible-5.25-hd` — the 600-oersted, 96-tpi
5.25-inch disk a 1.2 MB drive is served, which is a different
manufactured thing from the double-density `flexible-5.25-soft` a 1541
or an H-37 takes — with its facts declared from the published media
class as every article's are (P14). `flexible-3.5-hd` is already
catalogued. Two blank article kinds join `NewMedia`, spelled by their
article as the existing ones are (P3): `Flexible525Hd` and
`Flexible35Hd`, each creating that substrate with nothing recorded on it
and stating no coordinates, exactly as the two blank kinds delivered by
F60 do.

The drive catalog gains the PC families — a 5.25-inch 1.2 MB drive and
a 3.5-inch 1.44 MB drive — with profiles declared the way the Commodore
and Heath families' are (P30): the rotation, the data rates the drive
is served at, the index it observes from the article, and the articles
it accepts. Nothing here reads or writes a recording; the families
exist so that a medium recorded for one can be inserted (D19 weighs the
recording at the edge) and so that a raw image of one can be loaded
under `Format::Raw { device: FloppyDrive::… }`.

Every addition reaches the C and Python surfaces through the enumerations
those surfaces already carry for articles, blank kinds and drives.

Touches: S1, S2, S3. Supports: U35 (pledged); P3, P12, P14, P21, P30.

## F82 — The DOS recording arc onto an authored blank

Deliver the authored-to-recorded arc F60 reserved and D36 declined to
fake: record a published DOS floppy layout onto a blank article, after
which the medium testifies for itself.

`Recording` is an enumerated claim (P3) of published layouts, and this
feature claims two: `Dos12` (80 × 2 × 15 × 512, media byte `0xF9`) onto
`flexible-5.25-hd`, and `Dos144` (80 × 2 × 18 × 512, media byte `0xF0`)
onto `flexible-3.5-hd`. A kind declares the article it records onto and
is refused by name on any other. `PartitionView::record_as(kind)` over
the direct partition of a blank article lays down precisely what
`FORMAT` does — the boot record with its BPB and signature and zero code
bytes, the FAT copies with their media byte and end-of-chain marks, and
the root directory — and nothing chosen on the author's behalf. A blank
already recorded onto, a `ChsDisk`, and a loaded medium all refuse:
the arc records onto a blank article, once.

After the arc the medium is recorded, not merely authored: its geometry
carries one reading whose source is the recording chosen (a source of
its own, beside `Authorship`, because the author chose a layout and did
not state coordinates); its device type is the PC family the layout is
recorded for, so a drive takes it; and `partition(0).filesystem()`
opens FAT12 over it by the evidence of the boot record just recorded —
through the seam U31 writes through, reusing the delivered FAT write
verbs without change. The commit point stays the ordinary one with no
journal beneath it (P2, D36).

Touches: S1, S2, S3. Supports: U35 (pledged); P2, P3, P6, P10, P14,
P16, P18, P19. F81 is a prerequisite.

## F83 — The raw rendition of a sector medium

Write a sector-addressed medium out as a raw image, paired with a verb
that computes everything and writes nothing.

`Medium::describe_raw` and `Medium::write_raw(path)` are the sector
medium's rendition, shaped as the C64 renditions are (P29): the content
in the recording's own sector order — cylinder-major, head-minor,
sectors from one — and nothing else, with a report stating what a raw
artifact cannot carry: the article and its facts, the authored or
recorded provenance, the recording kind. The file is built beside the
destination and moved into place whole (P9); an existing file at the
destination is a refusal, never an overwrite; a blank article never
recorded onto has no content to encode and says so; a medium whose
geometry is undetermined has no sector order to write in and says so.

The rendition is of the medium's committed state. Uncommitted writes are
not written out, and the report says how many extents were left behind,
so that a caller who forgot the commit point is told rather than
surprised.

Raw is the one encode this feature claims; ImageDisk write is F69 and the
flux masterings are F74, and both remain proposed.

Touches: S1, S2, S3. Supports: U35 (pledged); P2, P6, P9, P10, P29.
F81 is a prerequisite, for the device a loaded raw image is declared
under; F82 is not.
