<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Bitstream and cell floppy images

Design for
[F73](../FEATURES.md#f73--mame-floppy-image-read),
[F74](../FEATURES.md#f74--mastering-out-to-hxc-mfm-and-mame-floppy-image),
[F75](../FEATURES.md#f75--writing-in-place-at-a-bitstream-authoritative-layer)
and [F79](../FEATURES.md#f79--hxc-floppy-emulator-hfe-read), serving U1 and
U2 under pledged P12–P14, P22, P23, P27 and P29. This is proposed, not
implementation approval. Public names remain delivery surface design.

The ladder these sit on is pledged separately, and its design is
[pledged/design/fm-mfm-read-channel.md](../../pledged/design/fm-mfm-read-channel.md).
Everything here assumes a family-plural ladder and an FM/MFM channel exist,
and adds nothing to either.

## These artifacts are not all at the same level

They are usually named together and they are not the same kind of thing.

**HxC `.mfm`** — pledged as F77 — is a track of **already-framed cells**:
MFM bits at one declared rate, per track and side. The recording's timing
was decided before the file existed, and it carries no weak region, no
density variation and no second reading of a track.

**HxC `.hfe`** is the same house and a wider claim: a bitstream container
that declares an *interface mode* and can carry more than one encoding.
What it tests is whether the tier below assumes the encoding that happened
to be loaded first.

**MAME `.mfi`** is a track of **cell transitions around the revolution**:
positions as a fraction of a turn rather than as bits, each carrying what
kind of region it opens — magnetized one way, the other, unmagnetized, or
damaged — with the track stored deflate-compressed. It is angular where a
flux capture is timed, and it keeps distinctions a framed bitstream has
already thrown away.

So `.mfi` sits below the bit tier and the other two sit at it. That
difference is the reason to build more than one: it is what says whether
the channel is a channel or a container reader wearing one. Where the
representations differ, the difference is stated rather than normalized —
a cell stream can carry a density variation and a uniform bitstream
cannot, so what MFI knows is never levelled down to what MFM knows and
then presented as all there was.

None of them is a block image, and none becomes one. P13's prohibition
stands untouched: this is derivation *within* the magnetic family.

## Writing means two different things, and only one of them is safe

This is the part to settle before either write feature is pledged.

**Mastering out (F74)** is producing a new `.mfm` or `.mfi` from evidence
the session already holds. It is what the D64, G64 and P64 renditions
already do, and it inherits their discipline unchanged: a plan that
computes everything and produces nothing, an execution that produces the
result, and a declared-loss account stated in the source's own terms before
a byte is written. Each destination answers for itself — an MFM container
cannot hold a weak region, a density variation or a second observation of a
location; an MFI track can hold weakness and damage but is one revolution,
so several observations reduce under a policy the caller or the profile
names.

**Writing in place (F75)** is changing the loaded artifact. P13 permits it
only where every derivation on the path projects back to the authoritative
layer without unclaimed loss, and that test is passed and failed by
different acts at this tier:

| Act | Verdict |
| --- | --- |
| Replace a whole track's cells with cells the caller supplies | Projects exactly; the caller states the authoritative layer's own content |
| Replace a sector's payload, re-encoding into the surrounding track | Chooses cell timings, gap contents and a splice the source never stated |

The second is a reduction, and P29 is explicit that a reduction no policy
names is a refusal rather than a default. The plausible outcome of F75 is
therefore that the track-level write is offered and the sector-level write
is refused by name, the refusal stating that mastering out is the operation
which does this honestly. That is a delivery: a refusal which says what
would change the answer is what P3 is for.

The alternative — offering `write_sector` on a bitstream medium because the
medium above it happens to expose sectors — would be the library inventing
a recording and presenting it as the disk's. It is refused here by design
rather than found to be awkward later.

## Deflate is already ours

MFI's per-track compression is the deflate stream the library already
encodes and decodes in `codec/`, through its own LZ window into private
session storage. Nothing is added to the dependency-free claim (P1), and
P27's streaming discipline is inherited rather than re-argued: a track
decompresses when it is read, not when the image loads.

## What proves it

Fixtures are third-party and pinned in `test-fixture-prep/prep_fixtures.py`
as the existing ones are. The set is specific rather than large: one `.mfi`
and one `.mfm` of the *same* disk, so the two paths can be required to
produce the same sector claims; an `.mfi` carrying a weak or damaged
region, so the levelling-down refusal has something real to refuse; and an
`.hfe` whose declared interface mode this release does not decode, so that
refusal has one too.

The exact field layouts of these containers are settled at implementation
against each format's own published description, and a field this document
describes loosely is not thereby licensed to be guessed at.
