<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Record-structured sector images

Design for
[F69](../FEATURES.md#f69--imagedisk-write),
[F70](../FEATURES.md#f70--h17disk-version-2-read) and
[F71](../FEATURES.md#f71--h17disk-version-2-write), serving U1 and U2 under
pledged P8, P12–P14, P27 and P28. This is proposed, not implementation
approval. Public names remain delivery surface design.

> **ImageDisk *reading* is delivered, and under a smaller cut than this
> design called for.** The cut turned on D60: with sector ordering
> resolved by the image format, a uniform ImageDisk is a linear extent and
> needs none of the seam below. The reader is `crates/remanence/src/image/imd.rs`,
> and its design was swept with its feature on delivery, as every
> delivered design is. What survives here is the case that still needs
> the seam — **writing**, where a record's encoded length changes under
> the caller's hand, and H17Disk, which has not been looked at against a
> real artifact yet. Read the sections below in that light: they were
> written before the reader was built, and the geometry section in
> particular is now F80's problem rather than this family's.

## What actually blocks these two formats

Not the parsing. The parsing is ordinary. What blocks them is that the
sector-image seam in this release presents **a flat byte device**, and a
coordinate is turned into a place in it by arithmetic:

```text
read_sector(c, h, s) -> sector_offset(c, h, s) -> read_at(offset)
```

Every adapter delivered so far can honour that. H8D is the file's bytes
in recording order. QCOW2 and VDI translate an offset to another offset.
The presented plane is linear in all three cases, so the medium can own
the addressing and the adapter can own only the translation.

ImageDisk and H17Disk break it in the same way and for the same reason:
they store a track as a header plus a run of **individually encoded
records**, so the *n*th sector's bytes are found by walking the records
before it, and a record may not be a run of bytes at all — ImageDisk
stores a uniform sector as one fill byte. There is no offset to compute.

This is P12 arriving rather than being violated: raw sectors and encoded
tracks are already named there as distinct families that "do not collapse
into one universal interface". The seam below has simply only ever had
one family in it.

## The seam

An adapter of this family answers for a **record**, not a byte:

```text
RecordImage
  tracks() -> ordered track descriptions
  record(coordinate) -> the stored record, or the named absence
  replace(coordinate, bytes) -> the re-encoded container (F69/F71)
```

and a track description carries what the track states for itself:

```text
TrackDescription
  cylinder, head
  encoding mode and data rate, where the format states them
  the ordered sector ids the track declares
  the sector size, where the track declares one for all of them
  per-record facts (below)
```

Two things follow that are worth stating rather than discovering.

**The record's identity is the id the track declares, not its position.**
An ImageDisk track may number its sectors 1–9, 0–8, or in interleave
order, and it may carry a cylinder or head map saying the recording's own
numbering differs from where the track sits. Addressing by position would
quietly renumber the disk. Where a map and its header disagree, both
readings stand and the disk is refused by name, exactly as two disagreeing
geometry readings are.

**A record can be absent without the track being damaged.** A track that
declares nine sectors and stores eight is a fact about the recording;
serving zeroes for the ninth would be the one thing this library never
does. The absence is named at the read.

## The geometry problem, which is now F80's

`RecordingGeometry` is one four-tuple, and the discovered-geometry seam
settles between readings that may disagree. A mixed-density ImageDisk
fits neither shape: a single-density track 0 with eight 512-byte sectors
followed by seventy-nine double-density tracks with nine is not two
sources disagreeing about one recording. It is one recording, read
correctly, that is **not uniform**.

Calling that `Undetermined` would be a lie of the useful kind — it would
make the sector verbs refuse, which is worse than wrong, because the disk
is perfectly addressable. Calling it uniform by taking the majority track
would be the other kind.

So the seam gains a third settled state beside determined and
undetermined: a recording whose coordinates are **stated per track**,
carrying the table rather than a tuple. `Undetermined` keeps its exact
present meaning — sources disagree — and nothing about the existing
readings changes. `read_sector` and `write_sector` address through the
table where one is present; a caller asking what the geometry is gets the
table and the honest statement that no single tuple describes the disk.

The alternative considered and rejected: presenting only the uniform
majority and refusing the odd track. It reads well in the common case and
it silently loses track 0 of most CP/M and DOS floppies ever written,
which is precisely the track that carries the boot record.

## What travels beside the payload

Both formats record facts about a sector that are not the sector. They
become declared facts and issues on the record, in the format's own terms
(`evidence.rs`), and they are never folded into the bytes:

| ImageDisk | H17Disk version 2 |
| --- | --- |
| track encoding mode and data rate | the medium's hard-sector facts |
| optional cylinder and head map | container-level disk metadata |
| per-record deleted-address mark | per-sector error records |
| per-record data-error mark | |
| stored compressed or literal | |

The two lists overlapping in shape but not in content is the finding F70
exists to produce. If one vocabulary carries both without either format's
facts being bent to fit, the seam generalizes. If it cannot, the honest
answer is two family-owned fact sets and a shared record interface above
them — and discovering that with the second format is cheap, while
discovering it with the fifth is not.

## Version claims

H17Disk is read at version 2 **exactly** (P8). Another version is refused
by name rather than attempted on the assumption that a layout held, and
the refusal states the version found. ImageDisk carries an ASCII banner
rather than a version field; what is claimed there is the record grammar,
and a sector-data type byte outside the claimed set is the same kind of
named refusal.

## Writing, and what writing is not

A write replaces one record. Because a record's encoded length depends on
its contents — a uniform record is one byte, and writing a non-uniform
sector into it makes it a literal run — everything after the record moves.
The commit therefore re-encodes and rewrites the container under P9's
existing journal, with the whole plan computed before the first byte moves
(P6). Nothing here needs a new durability mechanism; it needs the plan to
be complete, which the crash harness already tests for.

Three things are deliberately not claimed:

- **Formatting.** Adding a track, changing a track's mode, or changing a
  sector size is manufacturing a recording nobody made. Refused by name.
- **Silently normalizing a record's facts.** Writing into a record marked
  deleted, or one marked as having had a data error, must either preserve
  the mark or refuse — the caller's bytes say nothing about whether the
  original drive read it cleanly, and inventing a clean mark would be a
  claim about the medium.
- **Creating either format from nothing.** That is mastering (P29) and
  belongs with the mastering seam, not with an in-place write.

## What proves it

Fixtures are third-party artifacts this project does not distribute,
pinned in `test-fixture-prep/prep_fixtures.py` as the existing ones are.
The set needed is small and specific: an ImageDisk with a mixed-density
track 0, one with a cylinder or head map, one with a compressed record and
a deleted-address mark, and a version 2 H17Disk carrying per-sector error
records. A synthetic builder beside them covers the refusals, as
`disk/fixtures.rs` already does for the commit tests — an artifact with a
map contradicting its header is not something to go looking for in the
wild.
