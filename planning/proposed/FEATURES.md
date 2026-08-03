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

The report states what the device's leading structure turned out to be as
one classified outcome — blank; a recognized partition schema, whether or
not any volume composed from it; a direct unpartitioned volume; or nonblank
content no adapter claims. That is a distinct value, not a flag beside two
lists which may each be empty for several different reasons: a consumer
reconstructing the state from those combinations is reimplementing a
judgement this library has already made.

Every declared region reports both its raw declaration — the type value
exactly as the schema records it — and a reading of what that value
declares, present whether or not the type is inside this feature's read
claim. The reading is fit to quote in a refusal a user will read: type
`0x07` reads as NTFS or exFAT, and `0xee` says the disk is GPT rather than
MBR. A kind tag alone does not meet that bar. Its point is that no consumer
keeps a second partition-type table in order to explain what this library
declined to read — the same reason P16 puts type interpretation inside the
schema adapter.

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

F20 depends on the in-force adapter architecture for the adapter catalogs,
authoritative and active
layer model, provenance, and library-assigned device identity. It composes
the MBR, direct-volume, and FAT adapters already provided by the built-in
catalogs; it neither
duplicates their recognition rules nor adds another orchestration path.

The feature is deliberately limited to the formats and compositions already
needed to preserve U4: raw and qcow2 block devices, MBR including extended
and logical entries, a partitionless direct volume, and FAT12/FAT16
filesystems. GPT, NTFS, multi-device opening, manual volume recipes, complex
volume managers, Windows namespace reconstruction, partition editing, and
new disk formats remain outside it.

Touches: S1, S2, S3. Supports: U3, U4; P3–P5, P13, P16–P19, P21, P23,
P27.

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

Touches: S1, S2, S3. Supports: U18; P3–P5, P12–P15, P19, P21, P23, P24,
P27. F20 is not a prerequisite; any shared
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
vetted and queued. F23 and F20 must converge on one P19 file-container
interface before either duplicates enumeration, identity, or read operations.

F23 adds `tape` to P23's exact vocabulary when pledged, using P26's
signal-or-recorded-object wording. Repeated reads and redundant KERNAL copies
remain evidence, not independent media snapshots.

Touches: S1, S2, S3. Supports: U21; P3–P5, P12–P15, P19, P21, P23, P26,
P27. F20–F22 are not prerequisites, but F20
and F23 share the P19 seam and cannot land incompatible public interfaces.

Companion design:
[design/computer-tape-representations.md](design/computer-tape-representations.md).

## F24 — The FAT label answer, whole, at the filesystem seam

Make a recognized FAT volume's label one complete answer: the label, or the
fact that the volume has none. `NO NAME` is the format's own spelling of
unlabeled, so it is absence — decided where the format is known rather than by
a string comparison in every consumer that displays a drive.

FAT records a label in two places, the boot record's field and the root
directory's volume-ID entry, and a volume may carry either, both, or
disagreeing values. Choosing between them is a policy about FAT, so the
filesystem adapter holds it and states it: the root-directory entry is the
label DOS itself displays and answers wherever it exists; the boot-record
field answers where it does not; `NO NAME` at either source is absence. Both
readings stay beside the answer as evidence (P4), so a caller which needs the
literal bytes has them without opening a sector, and no caller has to know
which of the two it should have looked at.

Nothing else may become a label. A directory name, a filesystem kind, a file
inside the volume, and the image's own filename are not evidence of one, and
an unlabeled volume is reported unlabeled rather than given a placeholder.

The label sits today on the volume record the disk report returns and, once
F20 has landed, on the filesystem record where that seam owns it. F24 lands on
whichever presentation is current when it is picked up; it neither waits for
F20 nor blocks it, and the answer it defines is the same either way.

Touches: S1, S2, S3. Supports: U2, U4, U22; P3, P4, P5, P18. Needs: nothing
pledged first.

## F25 — DOS 8.3 name rules owned at the file-access seam

Make every 8.3 name decision the file-access seam's own, and make each refusal
name the rule it broke.

Reads match without regard to case and return the name as stored. That is the
behavior today; this feature states it as a claim rather than leaving it a
property of the implementation, so a caller may rely on showing the user what
the directory actually holds. Writes validate and normalize at the same seam:
the caller supplies the name it has, and the library uppercases, pads, and
stores it. A caller uppercasing first is performing the library's rule in the
one place it cannot be checked against the format.

A name outside the namespace is refused with a rule identity from one
enumerated set, under the P10 amendment:

- an empty base;
- a base longer than eight characters;
- an extension longer than three;
- more than one separator, or one where the format does not allow it;
- a character the format excludes, naming the character;
- a reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
  `LPT1`–`LPT9`), with or without an extension; and
- a leading or trailing space in a component.

The reserved-device rule is the one the code does not enforce at all today;
the others are enforced and refused with a single undifferentiated diagnostic,
which is what leaves a consumer reimplementing the set to say which rule was
broken. Nothing is truncated, transliterated, or repaired to fit — a refused
name is refused (P6), and the caller decides what to do about it.

The stored escape for a leading `0xe5` byte stays internal. It encodes a
stored name; it is not a rule a caller can break.

Touches: S1, S2, S3. Supports: U3, U22; P3, P5, P6, P10, P18, P19. Needs: the
P10 amendment ([ARCHITECTURE.md](ARCHITECTURE.md)) pledged, since the rule
identity is the half a category cannot carry.

## F26 — The DOS drive-letter composer

Deliver the namespace-mapping composer of the P19 amendment for DOS. Given the
machine facts the caller asserts — medium, slot, and attachment order — and the
volumes already composed from the images it inspected, return which volume each
drive letter names, as an answer built from a named rule rather than from the
order things happen to appear in.

Floppy slots take `A:` and `B:`, and a single-floppy machine's second letter is
the phantom-drive convention rather than a second volume. Hard-disk volumes take
letters from `C:` upward under the claimed rule. CD-ROM letters follow only
where the caller declares the resident driver's placement, because nothing on
the disks records it and the driver could put it anywhere.

The assignment rule is the substance of this feature and its whole risk. DOS
did not letter volumes in the order a report lists them: the usual rule takes
the first primary DOS partition of each disk in attachment order, then the
logical drives of the extended partitions across those disks in the same order,
then such remaining primaries as the variant assigns at all — and the variants
differ exactly there. F26 therefore claims named rules by variant (P3). Where
the caller states which variant the machine ran, the composer applies that
rule; where it does not, a letter on which the claimed variants disagree is
reported undetermined rather than settled by choosing the most common one.
`LASTDRIVE`, `SUBST`, `JOIN`, `ASSIGN`, a block-device driver, and a network
redirector are outside every claimed rule, and a mapping they would have
changed is undetermined, not approximated.

The composer answers with mappings: each established letter names a volume by
the identity its report issued, and every letter it could not establish says so
with the reason. It opens no artifact, takes the reports the caller already
holds, and composes no file container over the result — the letter is what a
consumer shows a user, and the identity is what it passes back into a file
verb. Composing a rooted namespace over the mapping is separately admitted by
P19 and is not this feature.

Touches: S1, S2, S3. Supports: U22; P3, P4, P5, P19, P21. Needs: F20 pledged
and delivered, for the stable volume identities and the composed-volume report
this maps over; and the P19 amendment
([ARCHITECTURE.md](ARCHITECTURE.md)) pledged, which is what admits a composer
that derives a mapping instead of consuming one. D5's deferral is untouched:
nothing here opens several artifacts together.

## F27 — Degraded, evidence-bounded image access

Replace all-or-nothing failure for a known deficient image with P28's verified, degraded, and refused outcomes. A degraded open preserves the observed deficiency and permits only reads whose complete interpretation is bounded by available evidence; it is irrevocably read-only. This is not recovery, repair, fabricated fill data, or a weaker way to accept an unsupported format.

The initial vertical slice covers caller-selected raw FAT12/FAT16 images and direct filesystem access. A declared image size larger than the source is reported as truncation with declared and observed bounds. The library may enumerate and extract only entries whose metadata and complete cluster chains lie inside the readable extent; a missing-range operation reports the same condition and location. Invalid or ambiguous metadata that prevents those bounds remains a refusal.

S1 exposes outcome, structured observations, readable bounds, effective access mode, and stable degraded condition; S2 and S3 mirror them. A write-intent open that becomes degraded reports its effective read-only mode and why; P7 host-access failure remains an open failure. Every mutation path, including commit, returns the degraded condition.

Touches: S1, S2, S3. Supports: U3; P2–P7, P10, P28. Needs: P28 pledged. The initial FAT slice must fit one sprint; qcow2, archives, HDOS, and later format-specific bounded-read rules are separate features.

Companion design: [design/degraded-evidence-bounded-image-access.md](design/degraded-evidence-bounded-image-access.md).
