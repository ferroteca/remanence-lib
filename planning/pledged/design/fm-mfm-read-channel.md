<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The FM/MFM read channel, and a ladder with two families in it

Design for [F77](../FEATURES.md#f77--hxc-mfm-read-to-the-bit-tier) and
[F78](../FEATURES.md#f78--the-fm-and-mfm-framing-and-its-sector-claims),
serving U1 and U2 under P5, P8, P12–P14, P16–P19, P21–P23, P27 and P30.
The third piece it was written for, F76, is delivered; the section below
that describes it is kept as the account of what was done and why, since
what it explains is now a property of the code these two build on.

Pledged, which means the project owes it. Public names below are delivery
surface design and are settled in the change that lands them.

## The absence being closed

Nothing in this release decodes FM or MFM. The flux ladder is complete in
shape — bits, bytes, sector claims, a filesystem — and every rung of it is
reached through the 1541's GCR channel and code table, because a disk's
type carries its rules and that is the only type whose rules are written
down here.

That is a much larger absence than one missing format. FM and MFM are what
the overwhelming majority of floppies ever written are recorded in, so
every `.mfm`, `.mfi`, `.hfe`, `.scp` and KryoFlux capture of a PC, CP/M or
soft-sectored Heathkit disk currently stops at bits nobody can frame.

## The blocking fact, which was structural rather than missing code

**This is what F76 removed, and it is recorded here because F77 and F78
stand on the answer.** Before it, `FluxState` named the 1541 in its own
fields:

```rust
bitstream: Option<C1541Bitstream>,
bytestream: Option<C1541Bytestream>,
sectors:   Option<C1541Sectors>,
```

and the medium's `bitstream` and `bytestream` verbs returned those
concrete types. Outward from there the same names appeared about three
hundred times across thirteen files — the core, the C ABI and its
generated header, the hand-maintained C++ wrapper and its leak tests, the
Python module, its hand-written stub, and the examples.

So a second family was never blocked by the decoding being hard. It was
blocked because there was nowhere for a non-1541 rung to be, which is why
that piece was a feature rather than a tidy-up: it changed S1, S2 and S3
together, and pre-1.0 rules meant the old shape was deleted in the same
change rather than aliased.

**Plural, not universal.** The rung a caller reaches is still one seam
(P5). What stopped being true is that the family is spelled into the type
of every rung. What survived untouched is that no caller supplies a
policy: being a medium of a declared family *is* the declaration of how it
is read (P30, reached through the type). A ladder that had started taking
a channel argument would have solved the wrong problem.

**Generalized to two, not to n.** A third family would be evidence about
where the seam belongs and it is not here yet, so the split landed exactly
where the two families differ: the phase-locked channel is shared and
reads its every number off the profile, while the bits-to-bytes transition
is the family's own behavior, enrolled on its profile. What F77 and F78
must not do is widen that seam further than they need — the alternative is
the universal image-format language P12 forbids, arriving one tier lower.

## Framing is by address mark, not by code table

This is the substantive difference between the existing channel and the
new one, and it shapes the bit tier rather than only the byte tier.

The 1541's channel frames on a group code with undefined patterns, and a
pattern the table does not define keeps its own bits rather than being
rounded to a legal neighbour. FM and MFM have no such table: every cell
pattern is legal. A byte is framed instead from a sync field and an
**address mark**, and an address mark is identified by a *deliberate
violation* of the encoding — a clock transition that the rule says should
be there and is not.

The consequence for F77's tier: a bit must be able to carry "this cell is
a deliberate violation" as a fact about the recording. It is not a
resolved bit, not an error, and not something to smooth. A bit tier that
can only say recorded-or-resolved cannot host this channel, so F77 sizes
its tier for F78 even though F77 itself frames nothing.

The consequences for F78's tier:

```text
sync + address mark -> what kind of field opens here
id field            -> the address the recording states for itself
data field          -> the payload, and whether its mark says deleted
CRC-16/CCITT        -> stated and computed, side by side, neither preferred
```

CRC-16/CCITT joins CRC-32 in `checksum.rs`, which is where the small
checks several formats share already live — implemented once, named once.

A deleted-data mark is a claim the recording makes, carried as a declared
fact on the sector claim. Nothing at this tier decides on a caller's
behalf whether such a sector counts, and nothing repairs a field whose
computed CRC disagrees with its stated one: both stand, as two disagreeing
geometry readings do.

## What it reaches, and what it must not invent

The payoff of F78 is reach, not a new door. A FAT or HDOS volume on an FM
or MFM recording opens through the partition and filesystem seam that a
hard-disk image already uses (P16–P19), with neither side learning about
the other. Demonstrating that is part of the feature; adding a file
interface for it would be building a second seam beside a working one.

> **Annotation (D62, 2026-08-17):** this landed, and the shape it took is
> worth recording. The reach was not free of a *decision*: the CBM DOS
> sector layer composes no addressed extent, and that had been standing
> in as a property of flux rather than of CBM DOS. An FM or MFM
> recording's records state a cylinder, a head and a sector number, which
> compose the geometry ordering the filesystems were written against — so
> the layer presents a `Device` and the adapters read it unchanged. What
> the design did not anticipate is that FAT alone had no device-backed
> catalog, reading instead through the medium that composed its
> partition; it has one now, which is what let all three of FAT, HDOS and
> CP/M arrive through one door rather than FAT through a second.

Two things stay refused throughout:

- **A soft-sectored Heathkit recording read this way is not the H17 hard-
  sectored path**, and must not be presented as one. Proposed U11 keeps
  H8D and H17Disk out of the flux tier deliberately; this design does not
  return them to it by another route.
- **Nothing synthesizes what the container never held.** An HxC MFM file
  carries no weak region, no density variation and no second observation
  of a location. Where a rung above would like one, its absence is stated
  (P13: synthesized detail is identified as synthetic and is never
  presented as recovered evidence).

## Order of work

F76 lands with or immediately before F77 — the ladder is made plural
because a second family is owed, not in advance of one. F77 then delivers
the container to the bit tier and stops there, which is a coherent rung in
this ladder rather than a half-delivery: the medium answers `bitstream`
and nothing above it. F78 adds the framing and the sector claims, and with
them the filesystem reach.

## What proves it

F76 was proved by the existing suite continuing to pass in the new shape,
across all three surfaces — including the C++ wrapper's total-coverage
rule (D54) and the Python stub, both of which are hand-maintained and
therefore where a lapse would hide. That standard was met and its number
has retired; what follows is what remains owed.

> **Annotation (F77 delivered, 2026-08-17):** the container reads, and
> what settled its shape is worth recording. The companion design says
> `.mfi` sits below the bit tier and `.mfm` sits at it, which raised the
> question of whether reading one required a bitstream-authoritative
> medium the library does not have. It did not: `Derivation::Synthetic`
> already means "synthesized downward from a higher layer", which is
> exactly what transitions restated from cells are. So the medium is an
> ordinary flux medium whose flux layer declares itself a restatement,
> and nothing above may present it as recovered timing. The artifact
> below is still owed — synthetic containers prove the reader's
> arithmetic, not that it agrees with the tool that writes them.

F77 and F78 need artifacts. Third-party fixtures are pinned in
`test-fixture-prep/prep_fixtures.py` as the existing ones are, and the set
wanted is specific: one `.mfm` of a disk whose filesystem this library
already reads, so the reach in F78 is provable end to end, and one
single-density (FM) recording, because a channel proved only on MFM has
not been proved.

Synthetic fixtures carry the rest, as `disk/fixtures.rs` already does for
the commit tests: encoding a known track and decoding it back is the
direct test of a channel, and the refusals — a truncated track table, a
declared rate the tier does not claim, an address mark with no field
behind it — are not artifacts to go looking for in the wild.

The exact field layout of the HxC MFM container is settled at
implementation against the format's own published description. A field
this document describes loosely is not thereby licensed to be guessed at.
