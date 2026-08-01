<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# ARCHITECTURE (proposed)

> **Status:** proposed, not pledged. Nothing in this file is approved for
> implementation. Principle numbers record order of issue, not priority.

## P24 — Optical media has a family-owned active layer

An optical medium whose recorded structure is observable above a generic
logical-block device uses a family-owned **optical** durable active layer.
Its active representation is exactly one family-declared seam: a captured
signal representation when sampled channel or RF observations are the best
available evidence, or a recorded-program representation when the source
begins at decoded drive- or player-visible structure. The report names which
representation is active. Decoding a signal into a program view does not
create a second mutable durable peer.

For compact disc the recorded-program state can preserve ordered sessions,
tracks, indexes, gaps, lead-in and lead-out facts, disc-relative frames, track
modes, the 2,352-byte main channel, P–W subchannels, and per-field provenance.
A LaserDisc signal state can instead preserve time-indexed RF samples, sample
clock and capture provenance; its decoded program views can include video,
audio, vertical-blanking information, frame or chapter addressing, and mapped
digital data. Other optical families define their own applicable state at the
lowest useful evidenced seam. No one family's schema is imposed on DVD,
Blu-ray, magneto-optical, or later media whose observable structures differ.

Optical is neither block nor magnetic flux. At the signal seam it may claim
sampled channel or RF observations and their timebase, but never silently
promotes those measurements into original pits, lands, surface state, or
pickup physics. At the program seam it claims only the recorded units,
channels, addressing, and layout visible through the applicable drive or
player contract. Parallel channels and marker or layout facts are parts of
one optical active state, not additional active layers. Capture observations
such as retries, C2 reports, drive offset, RF capture chain, and conflicting
reads attach as evidence and never become deterministic recorded state merely
because an image format stores them.

An optical-active disc may expose a derived block presentation only over a
family-declared recorded extent or channel mapping that defines logical user
data. The presentation declares its scope, block size, address mapping, and
the evidence-bearing derivation from optical state. A CD data track, a
Blu-ray logical data extent, and LV-ROM digital data carried in channels which
otherwise hold audio are different family mappings of this rule. Unmapped
audio, video, gaps, and optical-only regions have no block presentation. A
track or channel mapping does not thereby become a P16 partition, and a mixed
medium never becomes one whole geometry-opaque block device. Volumes,
filesystems, and P19 file containers may compose above an eligible block
extent while every other optical structure remains present.

A block-, sector-, or filesystem-authoritative source may enter optical only
through an explicit family composition which claims an optical profile and
mastering rules. The atomic **generate-optical** transition synthesizes the
most honest optical state those inputs permit, identifies every manufactured
layout, raw-frame, error-correction, gap, and subchannel fact as synthetic,
and refuses contradictory or incomplete rules rather than inventing
precision. An ISO can therefore remain block-active for ordinary data access
or become the source of a newly mastered synthetic data disc when optical
hardware service is explicitly requested. It can never recover absent audio,
protection, damage, original mastering, or subchannel evidence. A generic LBA
hard drive remains terminal and is never inferred to be optical from its
content.

Image formats remain P12 adapters at their recorded representation seams. A
compound CCD/IMG/SUB source, BIN/CUE plus a sparse subchannel overlay, raw
LaserDisc RF capture, decoded LaserDisc CHD, Aaru Image Format, or another
optical encoding can all materialize optical state at different seams and
fidelity. A decoded source does not imply recoverable RF; an RF source remains
re-decodable without pretending its samples are literal surface geometry.
Single-file packaging confers no higher truth, and multiple source files do
not make the image a P19 file container. Each adapter declares which facts
are captured, declared, decoded, synthesized, patched, ambiguous, invalid, or
absent; no conversion silently promotes one provenance class into another.

P15 projects this durable state through a typed optical hardware presentation
at the useful common drive- or player-visible seam. Depending on the family,
that seam may provide commands, tracks, sectors, audio, subchannels, video,
vertical-blanking data, frame or chapter addresses, or mapped digital data.
Playback and pickup position, seek continuation, CAV or CLV rotational
progress, controller continuation, and pending causal effects are ephemeral
hardware state. Writes through either the hardware presentation or a derived
higher view mutate the one optical active instance and remain subject to P2
and P13 representability at commit.

Pledging this principle requires amending P23's exact active-layer table with:

| Active layer | Durable session state | Claim |
|---|---|---|
| **optical** | one family-owned signal or recorded-program representation, with its timebase or recorded layout, units, channels, mappings, and provenance | no inferred pits, lands, surface state, pickup physics, firmware, or geometry-opaque whole-disc block claim |

P23's one-active-layer rule otherwise remains unchanged. Block and optical are
different active representations, not concurrently mutable peers. A derived
eligible block presentation over optical state does not make block active;
an ISO opened only as blocks does not make optical active.

## P25 — Artifact mappings make nesting recursive

Any recognized structure may expose an evidence-bearing **artifact mapping**
from part of its state to a possible child artifact. The mapping is the one
general recursion mechanism whether the child bytes come from a P19
file-container entry, a filesystem file, an optical boot-catalog extent, a
partition or volume region whose format defines an embedded image, or another
typed range declared by a recognized standard. Nesting is not a special
property of ZIP, ISO, partitions, or any one format family.

An artifact mapping is an edge in the inspection and composition graph, not a
durable layer, partition, volume, filesystem, file container, or claim that
the child has been recognized. It names the parent identity, source extent or
byte projection, applicable standard semantics, evidence, access limits, and
the path by which a representable child change would return to the parent.
Opening that source invokes P12 image adapters normally. Successful
recognition can materialize a child; opening it as an independent state
instance gives it its own P13 authoritative layer and P23 active layer.

Thus a ZIP may remain file-container-active while its ISO entry is an
optical-active child; an El Torito boot entry in that disc may open a
CHS- or block-active boot-disk child; and a P64 stored as a file in the ISO
filesystem may instead open a flux-active child. These layers coexist because
they belong to different state instances. They are never multiple active
copies of one instance, and no parent-to-child mapping converts block into
flux or optical into block.

Discovery is explicit and lazy. Inspection reports mappings and their
relationships without recursively opening every candidate, selecting a
preferred boot entry, or guessing which embedded image the caller wants.
The caller selects a reported mapping and requests recognition of its child.
Unsupported, ambiguous, cyclic, excessively deep, or resource-hostile paths
are bounded and refused with their evidence preserved rather than silently
skipped or flattened.

Mappings may alias or overlap: an El Torito image can also be a named ISO file,
and hybrid structures can assign several meanings to the same bytes. Reports
preserve that identity and overlap. Two paths to the same mutable child must
share one child state, or conflicting writable composition is refused before
mutation; independent mutable copies over aliased bytes are forbidden.

Nested commit proceeds from child to parent. Every child result is first
validated against its image adapter and mapping, then encoded into the parent
state, continuing outward until the root source is representable. P2 commits
the validated composition atomically and P7 holds the necessary claims for
the whole graph. Failure at any seam writes nothing and names the exact child,
mapping, and representation which could not be encoded.

## P26 — Computer tape has a family-owned sequential active layer

A computer data tape whose ordering is observable above flat bytes uses a
family-owned **tape** durable active layer. It is an ordered sequence of typed
objects within tape partitions: records with preserved lengths, filemarks,
setmarks where applicable, end observations, provenance, and issues. Equal
record sizes do not make a disk.

Ordering and position are load-bearing. An adapter never deletes an unreadable
object and renumbers what follows, silently concatenates records, encodes marks
as payload, or fills absent structure. Retries, conflicts, drive responses,
resume history, and inferred positions remain evidence, not snapshots.

Tape partitions are family-owned divisions, not P16 layouts. Tape files are
mark-delimited extents, not P19 entries. A selected range supplies a derived
byte view only under explicit boundary and concatenation rules. Higher
interpretations compose above it without replacing tape-active state.

Image formats remain P12 adapters at the seam they record. Aaru may carry
partitions, tape files, records, marks, device responses, and provenance;
another encoding may carry only records and marks or flat bytes. Packaging
and format names confer no fidelity.

P15 projects tape state through a typed family drive presentation. Read,
rewind, space, locate, and position obey its contract. Position, motion,
buffering, continuation, latency, and status are ephemeral; contents, marks,
partitions, and evidenced structure are durable. U21 is initially read-only;
writable tape use must separately define mutation semantics.

Pledging this principle requires adding this row to P23:

| Active layer | Durable session state | Claim |
|---|---|---|
| **tape** | family-owned partitions containing ordered typed records, marks, end observations, and provenance | sequential recorded structure, not a random-access disk, sampled signal, transport mechanism, or firmware |

P23 otherwise remains unchanged. Derived byte, filesystem, file-container, or
fixed-record views do not become active. Flat bytes are not promoted to tape
because their name or contents suggest tape origin.
