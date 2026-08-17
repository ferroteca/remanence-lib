<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (proposed)

> **Status:** proposed, not pledged. Nothing in this file is approved for
> implementation. Feature numbers record order of issue, not work order or
> priority.

## F21 — Mixed-mode optical media and presentations

Introduce the P24 optical active layer and the compositions required by U18:
one drive-visible optical state capable of preserving mixed audio/data track
layout, raw main-channel frames, P–W subchannels, and provenance; a typed
optical hardware presentation over that state; and bounded block
presentations over only the eligible data-track extents.

The feature must prove that at least two materially different image adapters
can load the same optical family interface without central format branching.
A compound raw-main/raw-subchannel source and a normalized single-file
optical source are the reference shapes; the eventual pledge may split their
individual adapters into smaller features if the implementation cut would
otherwise exceed one sprint. Format support is claimed only for adapters
actually delivered, never inferred from these reference names.

F21 adds an explicit generate-optical composition for a source such as an ISO
whose block or filesystem content can be mastered into a declared optical
profile. The transition is atomic, provenance-bearing, and opt-in. It does
not generalize into arbitrary block-to-optical conversion, and it cannot
recover mixed-mode content or physical evidence absent from the source.

The optical hardware presentation uses P15's common timed-causality
lifecycle at the applicable drive-visible command, track, sector, audio, and
subchannel seam. It does not model pickup physics or integrated-drive
internals. The derived block presentation is selected by reported data-track
identity and cannot cover audio or optical-only regions. Both presentations
operate on one optical-active state and share P2 commit and rollback.

F21 requires a coherent amendment to P23 when pledged: `optical` joins the
exact active-layer vocabulary, while block remains the active layer for an
ordinary ISO or other block-only open. It also requires the inspection graph
eventually serving these compositions to distinguish optical tracks from
P16 partitions and to preserve the relationship from an eligible data track
to its derived block extent and volume.

Touches: S1, S2, S3. Supports: U18; P3–P5, P12–P15, P19, P21, P23, P24,
P27. F38 is not a prerequisite; any shared
report vocabulary must converge before either surface lands.

Companion design:
[design/optical-media-representations.md](design/optical-media-representations.md).

## F22 — LaserDisc signal and program presentations

Introduce the LaserDisc-family forms of P24 required by U19: a raw sampled-RF
image adapter at the optical signal seam, a decoded audio/video image adapter
at the optical program seam, and one typed player presentation which can be
served honestly from either active representation. The two adapters prove
that the family interface does not depend on one storage encoding or force a
decoded source to invent its absent RF.

The raw path preserves the capture timebase, RF samples, capture-chain
provenance, discontinuities, and ambiguity needed for later re-decoding. Its
video, audio, vertical-blanking, frame or chapter, and digital-data outputs are
derived observations over the signal-active state. The decoded path begins at
the program-active seam and preserves only the channels and addressing its
source actually supplies. It cannot claim signal-level round trips.

The player presentation uses P15 timed causality at a family-appropriate
control and output boundary, including CAV/CLV behavior, seek and playback
continuations, still and directional playback where supported, audio/video
observations, and mapped digital-data service. F22 also supplies a bounded
block presentation for LV-ROM data carried in the applicable program-channel
mapping. That view cannot flatten the analog video program, cannot cover
unmapped extents, and does not reinterpret the mapping as a partition.

The initial feature is read-only. It does not master LaserDisc, synthesize RF
from decoded program content, emulate laser, pickup, focus, tracking, servo,
firmware, or physical pits and lands, or promise one universal command set for
every LaserDisc player. It may support named player profiles behind P15's
common lifecycle without moving their policy into the image adapters.

Touches: S1, S2, S3. Supports: U19; P3–P5, P12–P15, P19, P21, P23, P24,
P27. F21 is not a prerequisite; both
features share P24 and must converge on its optical identities and report
vocabulary.

Companion design:
[design/laserdisc-signal-and-program-presentations.md](design/laserdisc-signal-and-program-presentations.md).

## F23 — C64 tape file recovery and tape-family seams

Deliver U21 first as one vertical read-only slice: parse C64 TAP version 0 and
1 into a family-owned pulse representation, inspect the exact timing evidence,
decode standard KERNAL headers and data, reconcile their redundant copies, and
expose successful candidates through the common P19 file-container interface.

The S1 surface introduces the concrete C64 entry point and report values
(`C64Tape`, `C64TapeReport`, `C64KernalFileSetInfo`,
`C64TapeFileInfo`, and `C64TapeHeaderKind`) while file enumeration and reads
use the general P19 `FileContainer` vocabulary. S2 and S3 mirror those
semantics with their normal ownership conventions; they do not invent a
binding-specific extraction path.

Aaru is the counterexample that keeps P26 honest: record-oriented captures need
a family-owned recorded-object representation rather than C64 pulse types.
F23 does not claim an Aaru adapter or force both representations behind a
universal tape-object API. T64 may later reuse P19 as a logical C64 container,
but it is not tape-active.

The first pledge must fit one sprint and may therefore cover only TAP parsing,
inspection, and standard KERNAL recovery. Any later Aaru adapter, custom-loader
decoder, pulse generation, write path, or drive presentation is separately
vetted and queued. F23 presents through the delivered P19 file-container
contract and must not duplicate its enumeration, identity, or read
operations; neither pledged disk feature presents a filesystem through that
seam, so nothing there constrains this one.

F23 adds `tape` to P23's exact vocabulary when pledged, using P26's
signal-or-recorded-object wording. Repeated reads and redundant KERNAL copies
remain evidence, not independent media snapshots.

Touches: S1, S2, S3. Supports: U21; P3–P5, P12–P15, P19, P21, P23, P26,
P27. F21, F22, F38 and F39 are not
prerequisites, and none of them presents a filesystem through the P19 seam.

Companion design:
[design/computer-tape-representations.md](design/computer-tape-representations.md).

## F80 — Coordinates for a recording that is not uniform

Give the discovered-geometry seam a way to state coordinates **per track**,
and make the sector verbs address through them.

A recording's coordinates are one four-tuple today. An ImageDisk whose
track 0 is single-density and whose remaining tracks are not — the ordinary
CP/M and DOS floppy, not an exotic one — has no single tuple to state, and
it is not `Undetermined` either: nothing disagrees, the recording simply is
not uniform. F68 declines the case honestly, declaring no geometry so that
bytes and filesystems read while `read_sector` refuses; this feature is
what gives that recording real coordinates.

The work is a third settled state beside determined and undetermined,
carrying the table rather than a tuple, without weakening what
`Undetermined` means — it stays exactly "two sources disagree". The sector
verbs then address through the table where one is present.

The alternative this must not become is declaring the majority track's
geometry and refusing the odd one. It reads well in the common case and it
silently loses track 0 of most CP/M and DOS floppies ever written, which is
the track carrying the boot record.

Touches: S1, S2, S3. Supports: U1, U2; P3, P4, P10, P13, P14. F68 is a
prerequisite — it is what produced the recordings this serves.

## F69 — ImageDisk write

Write a record back into an ImageDisk artifact, and commit the re-encoded
container durably.

The distinguishing problem is that a record's encoded length is not fixed:
a compressed record is one fill byte, and writing anything else into it
makes it a literal run. A write therefore relocates everything after it,
so the commit rewrites the container rather than patching an offset. P9's
journal already covers that; what is new is that the plan must be complete
before the first byte moves (P6), and that a write which would silently
drop a record's declared facts — its deleted mark, its error flag — is
refused rather than quietly normalized.

The feature claims writing an existing record in place. It does not claim
formatting: adding a track, changing a track's mode, or changing a sector
size is a different act with different evidence, and an adapter that
invented one would be manufacturing a recording nobody made.

Touches: S1, S2, S3. Supports: U1, U2; P2, P6, P7, P9, P10, P12, P13, P28.
F68 is a prerequisite.

Companion design:
[design/record-structured-sector-images.md](design/record-structured-sector-images.md).

## F70 — H17Disk version 2 read

Read a version 2 H17Disk artifact through the F68 seam. Its worth is
double: it is a format worth reading, and it is the second format that
proves the seam is not ImageDisk-shaped.

Its structure is a tagged container rather than a bare track run, and what
it carries beyond the payload is different in kind from ImageDisk's — disk
metadata, per-sector error records, and the hard-sector facts of the medium
the H17 wrote. That the two formats' extra facts land in one record
vocabulary, or that they demonstrably cannot, is the finding the feature
must produce.

The version claim is exact (P8): version 2 is read, and another version is
refused by name rather than attempted on the assumption that the layout
held.

This does not supersede H8D, which already reads and writes today as a flat
CHS recording. The two carry different information floors and neither is
the other's improvement — the same distinction proposed U11 draws when it
keeps both out of the flux tier.

Touches: S1, S2, S3. Supports: U1, U2; P3–P5, P8, P10, P12–P14, P21, P27,
P28. F68 is a prerequisite.

Companion design:
[design/record-structured-sector-images.md](design/record-structured-sector-images.md).

## F71 — H17Disk version 2 write

Write a record back into a version 2 H17Disk artifact, under F69's rules
and with its own: the container's tags and metadata blocks must survive a
write that does not concern them, and a per-sector error record that no
longer describes what is now stored is a refusal rather than a stale fact
left behind.

Touches: S1, S2, S3. Supports: U1, U2; P2, P6, P7, P9, P10, P12, P13, P28.
F69 and F70 are prerequisites.

Companion design:
[design/record-structured-sector-images.md](design/record-structured-sector-images.md).

## F74 — Mastering out to HxC MFM and MAME floppy image

Write `.mfm` and `.mfi` artifacts from evidence the session already holds,
as the D64, G64 and P64 renditions are written: a plan that computes the
whole transformation and produces nothing, an execution that produces the
result, and a declared-loss account stated in the source's own terms before
a byte is written (P29).

This is the honest first meaning of "write" at this tier. Each destination
is asked what it can hold and answers for itself — an MFM container cannot
carry a density variation or a weak region, an MFI track cannot carry
several revolutions of the same location — and what would be lost is named
and counted rather than approximated. An existing file at the destination
is a refusal, and the artifact is built alongside and moved into place
whole.

Touches: S1, S2, S3. Supports: U1, U2; P6, P8, P9, P12, P13, P22, P29, P30.
F77 is a prerequisite for the MFM destination; the MFI reader that the
other destination writes back out is delivered.

Companion design:
[design/bitstream-and-cell-floppy-images.md](design/bitstream-and-cell-floppy-images.md).

## F75 — Writing in place at a bitstream authoritative layer

Change a loaded `.mfm` or `.mfi` artifact rather than producing a new one.

It is filed separately because it is the one piece of this group that the
principles may refuse. P13 offers a writable composition only where every
derivation on the path projects back to the authoritative layer without
unclaimed loss, and writing a *sector* into a cell stream chooses cell
timings the source never stated — a reduction, and under P29 a reduction
no policy names is a refusal rather than a default. The feature's first
job is therefore to establish whether an in-place write exists that is
honest at all: replacing a whole track's cells with cells the caller
supplies plainly is, and re-encoding one sector into a track the caller
did not otherwise touch plainly is not.

The outcome may be that the sector-level write is refused by name and only
the track-level one is offered. That is a delivery, not a failure — a
refusal that states what would change the answer is the point of P3.

Touches: S1, S2, S3. Supports: U1, U2; P2, P6, P7, P9, P10, P12, P13, P22,
P29. F77 is a prerequisite; F74 is not, and the two are alternatives at the
same seam rather than stages of one thing.

Companion design:
[design/bitstream-and-cell-floppy-images.md](design/bitstream-and-cell-floppy-images.md).

## F79 — HxC Floppy Emulator (HFE) read

Read an `.hfe` artifact through the ladder F76–F78 establish. It is the
same house as F77's `.mfm` and a materially different claim: HFE is a
bitstream container that declares an *interface mode* and can carry more
than one encoding, where the MFM container carries one and says so by
carrying nothing else.

That difference is the feature's whole argument. F77 proves a container
can be read into the bit tier; this proves the tier does not quietly
assume the encoding a caller happened to load first. Where the declared
mode names an encoding this release does not decode, the refusal names
the mode rather than reading it as MFM and hoping (P3, P8).

The version claim is exact, and the same limits stand: nothing above may
present as recovered evidence what the container never held.

Read only. Writing HFE belongs with F74's mastering seam if it is ever
argued, and is not claimed here.

Touches: S1, S2, S3. Supports: U1, U2; P3–P5, P8, P10, P12–P14, P21–P23,
P27, P30. F76 and F77 are prerequisites; F78 is a prerequisite for its
sector claims.

Companion design:
[design/bitstream-and-cell-floppy-images.md](design/bitstream-and-cell-floppy-images.md).
