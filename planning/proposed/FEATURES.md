<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (proposed)

> **Status:** proposed, not pledged. Nothing in this file is approved for
> implementation. Feature numbers record order of issue, not work order or
> priority.

## F20 — Layered partition and volume inspection

Replace the current FAT-shaped `DiskGeometry` snapshot and `geometry()`
verb with one evidence-bearing disk inspection report aligned with the
pledged image, device, partition-schema, volume-composition, filesystem, and
active-layer seams. Preserve every behavior U4 claims today while making the
result capable of guiding later operations without conflating partitions,
volumes, filesystems, physical geometry, or generic containers.

One deep inspection operation returns the complete report. It names the
opened image and block-active device, any recognized partition schema and
every declared partition region, each volume actually composed from the
available regions, and each filesystem actually recognized on a volume.
Typed relationships join those results. Evidence, ambiguity, absence, and
recognized refusals remain attached to the seam which owns them; a failed
filesystem read does not erase its partition row or renumber later volumes.

The report supplies opaque library-owned identities suitable for selecting a
reported region, volume, or filesystem in a later operation. Their public
semantics preserve U4's cross-report stability for an unchanged single-disk
layout while P21's device identity remains scoped to the open composition.
Callers never manufacture identities from partition numbers, offsets, array
positions, labels, or filesystem kinds.

F20 replaces the public shape coherently across S1, S2, and S3. The Rust,
C, and Python presentations expose the same report graph, relationship and
identity semantics, optional facts, and structured issues. The pre-1.0
`DiskGeometry`/geometry surface is deleted rather than retained as a
flattened compatibility view. Existing file verbs move to the new opaque
volume identity without changing U3's file behavior.

F20 depends on pledged F19 for the adapter catalogs, authoritative and active
layer model, provenance, and library-assigned device identity. It composes
the MBR, direct-volume, and FAT adapters already required by F19; it neither
duplicates their recognition rules nor adds another orchestration path.

The feature is deliberately limited to the formats and compositions already
needed to preserve U4: raw and qcow2 block devices, MBR including extended
and logical entries, a partitionless direct volume, and FAT12/FAT16
filesystems. GPT, NTFS, multi-device opening, manual volume recipes, complex
volume managers, Windows namespace reconstruction, partition editing, and
new disk formats remain outside it.

Touches: S1, S2, S3. S4 is unaffected and is removed by F19 before this
dependent feature lands. Supports: U3, U4; P3–P5, P13, P16–P19, P21, P23.
Needs: F19 pledged and delivered.

Companion design:
[design/layered-partition-volume-inspection.md](design/layered-partition-volume-inspection.md).

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

Touches: S1, S2, S3. S4 is unaffected and is removed by F19 before this
dependent feature lands. Supports: U18; P3–P5, P12–P15, P19, P21, P23, P24.
Needs: F19 pledged and delivered. F20 is not a prerequisite; any shared report
vocabulary must converge before either surface lands.

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

Touches: S1, S2, S3. S4 is unaffected and is removed by F19 before this
dependent feature lands. Supports: U19; P3–P5, P12–P15, P19, P21, P23, P24.
Needs: F19 pledged and delivered. F21 is not a prerequisite; both features
share P24 and must converge on its optical identities and report vocabulary.

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
vetted and queued. F23 and F20 must converge on one P19 file-container
interface before either duplicates enumeration, identity, or read operations.

F23 adds `tape` to P23's exact vocabulary when pledged, using P26's
signal-or-recorded-object wording. Repeated reads and redundant KERNAL copies
remain evidence, not independent media snapshots.

Touches: S1, S2, S3. S4 is unaffected and is removed by F19 before this
dependent feature lands. Supports: U21; P3–P5, P12–P15, P19, P21, P23, P26.
Needs: F19 pledged and delivered. F20–F22 are not prerequisites, but F20 and
F23 share the P19 seam and cannot land incompatible public interfaces.

Companion design:
[design/computer-tape-representations.md](design/computer-tape-representations.md).
