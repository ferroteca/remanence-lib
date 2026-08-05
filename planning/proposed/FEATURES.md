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

## F45 — An idiomatic C++ presentation, derived from the C ABI

Provide C++ consumers an idiomatic surface — RAII, namespaces, typed
errors — without the project acquiring a fourth application surface. The
wrapper is a single header-only layer over S2, and **S2 remains the
norm**: the C++ header is a derived representation of the C ABI exactly
as `include/remanence.h` is a derived representation of the Rust
`extern "C"` items, and it moves with S2 in the same change, never
independently. This feature deliberately amends nothing: P5's
three-presentation rule stands, no S-number is issued, and the wrapper
claims no capability the C ABI does not already provide — C++ programs
consume `remanence.h` today, and this adds ergonomics, not reach.

Shape: one move-only RAII class per node kind — the storage model's
`Machine`, `StorageDevice`, `Medium`, `Volume`, `Filesystem`, and `File`
— each owning its handle's lifetime through the ABI's free functions,
with families as enum values and every refusal surfacing as a typed
error carrying the delivered category and rule identity (P10). No
compiled C++ artifact exists — C++ has no stable cross-compiler ABI,
which is why the boundary stays C — so the deliverable is a header, its
tests, and a C++ example consumer beside the C one. The wrapper
documents view lifetimes — a filesystem borrowed from its medium, a
file from its filesystem — under the ABI's existing "borrowed, owned by
their handle" discipline; C++'s inability to enforce them is documented
rather than papered over.

Open to the pledge: whether errors present as exceptions, an
`expected`-style result, or both; whether the header is generated from
S2's shape or hand-maintained thin; and the header's name and install
path.

Touches: S2 only — a new derived representation beside the generated
header, with the ABI itself unchanged; S1 and S3 are unaffected because
nothing crosses the C boundary differently. Supports: S2, P5, P10 — no
U-number demands idiomatic C++ today, the demand being developer
experience at an existing surface; pledging may want a small use case
stating the C++ consumer's journey. Depends sideways on the storage
model's node-kind rule
([design/storage-model-and-vocabulary.md](design/storage-model-and-vocabulary.md));
if that model is not pledged, the wrapper wraps S2 as it stands.
