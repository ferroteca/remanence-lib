<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# ARCHITECTURE (proposed)

> **Status:** proposed, not pledged. Nothing in this file is approved for
> implementation. Principle numbers record order of issue, not priority.
>
> Sections headed `P<n> amendment` are drafted changes to a principle the
> project already carries, pledged or in force. They keep that principle's
> number, consume none of their own, and fold into its text on delivery
> ([SURFACES.md](../SURFACES.md)).

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

## P26 — Computer tape has a family-owned active layer

A computer-tape capture uses exactly one durable active representation owned
by its media family. “Tape” does not imply one universal object schema. The
adapter selects only the representation its source actually records:

- a **signal representation** preserves a time base and ordered transitions,
  pulse intervals, samples, gaps, provenance, and issues; C64 TAP is in this
  class; or
- a **recorded-object representation** preserves ordered partitions, records,
  filemarks, setmarks, end observations, provenance, and issues where the
  source carries them; Aaru and record-oriented drive captures are in this
  class.

Neither representation is silently promoted into the other. Pulse intervals
are not records, records are not sampled signals, fixed-size records do not
make a disk, and a filename or container label supplies no missing fidelity.
Unreadable, truncated, conflicting, resumed, or inferred observations retain
their evidenced positions.

Decoders and media parsers derive higher interpretations over the active
state. A standard C64 KERNAL decoder can derive a P19 flat file container from
TAP pulses while those pulses remain active. A record selection can expose a
bounded byte view under explicit concatenation rules. Filesystems, file
containers, and child images never replace the tape evidence from which they
were derived.

T64 is a logical C64 file container, not a pulse or recorded-object tape
representation. It can be active at P19 without acquiring a tape layer.
Conversely, a custom-loader TAP remains an honest signal capture even when no
file container can be derived.

A future write journey must name the representation it changes and define a
separate generate-tape transition when logical contents are encoded as pulses
or records. It cannot mutate a derived file view and pretend the source
evidence was edited in place. Physical transport and drive emulation remain
outside P15 until a use case requires their runtime semantics.

Pledging this principle requires replacing P23's proposed tape row with:

| Active layer | Durable session state | Claim |
|---|---|---|
| **tape** | one family-owned signal or recorded-object representation, with exact ordering, provenance, and issues | captured tape evidence at its actual fidelity, not a universal record list, random-access disk, derived file container, transport mechanism, or firmware |

P23 otherwise remains unchanged. Derived signal decoding, record grouping,
byte, filesystem, and file-container views do not become active.

## P10 amendment — a refusal may also name the rule it broke

In-force P10 gives every refusal a stable category from one enumerated set,
so an embedder maps behavior without parsing text. That set is deliberately
cross-cutting and small: it answers *how should the caller behave*, and it
answers it for the whole library at once.

One question it cannot answer is *which rule did this input break*. Where a
format, namespace, or grammar defines a bounded set of rules an input must
satisfy — a DOS 8.3 name has six, and FAT is one filesystem of many — the
category is the same for every one of them, and the only difference between
them is the sentence. A caller that must act on the distinction, or state
it to a user in its own words, is then reduced to parsing the message no
release promises to keep, or to reimplementing the rule set to decide what
it would have said. Widening the category set instead would dissolve it:
the categories would grow one per format rule, and the small cross-cutting
mapping P10 exists to provide would be gone.

The amendment adds one field beside the category, not a second mapping:

Where a refusal is one of an enumerated set of rules defined by a format,
namespace, or grammar, the error also carries a **rule identity** — a
stable machine-readable value naming which rule was broken, from the set
owned by the seam that defines those rules. The category still says how to
behave and remains the interface an embedder maps onto; the rule identity
says which rule, and never substitutes for the category. A refusal
belonging to no such rule set carries none, and that absence is ordinary
rather than an omission. Each rule set is part of the surface that owns it
— adding a rule identity is a surface change, and rewording the diagnostic
that states it is not — and every presentation carries the same identities
(P5).

The rule identity is not a second diagnostic. It names the rule, and P6's
human diagnostic still says what was expected, what was found, and where.

U22's DOS 8.3 refusals are the first demand for this; nothing else the
library refuses today has a rule set behind it.

## P19 amendment — namespace composition may derive a mapping, not only consume one

Pledged P19 admits a namespace-composition adapter which "consume[s] file
containers plus explicit drive, mount, folder, or volume mappings and
expose[s] another file container". Both routes to that mapping assume it
already exists somewhere: recovered as evidence where an operating system
persisted it (U13, U16), or asserted outright by the caller.

A DOS machine persists no such mapping. Its drive letters were assigned at
boot by a rule over the machine's own configuration — which media occupied
which slots, in which order the disks were attached — and nothing on the
disks records the result. There is no evidence to read and nothing for the
caller to assert but the answer it came for. Under P19 as pledged, the only
remaining home for that rule is the caller, which is the one place it
cannot be checked against the volumes the library composed.

The amendment admits a third form at the same seam:

A **namespace-mapping composer** consumes composed volumes with their
identities, plus the machine facts its caller asserts, applies one named
assignment rule, and returns the mapping it establishes. Producing a
mapping and composing a file container over it are separate acts: the
mapping answers on its own, and a composer that can establish only part of
one still answers with that part.

Three constraints keep the derivation from becoming a guess:

- **The rule is an enumerated claim (P3).** The composer names the
  assignment rule it applied. Where variants of one system assign
  differently, it claims the variants it implements and refuses the rest by
  name; it does not average them or pick the most common.
- **Evidence outranks a rule.** Where a system persists its own mapping,
  that mapping governs and no rule may stand in for it. This form exists
  for systems which persist nothing, and it never becomes a fallback for a
  persisted mapping that could not be read — U13's and U16's refusal to
  invent `C:` is untouched.
- **A derived mapping is not evidence.** The asserted machine facts and the
  applied rule travel with the result as provenance, under the same
  discipline that keeps a caller-selected installation out of the evidence
  (U16). Whatever the rule cannot settle is reported undetermined, at the
  granularity of the mapping it failed to establish, and is never filled
  from position, size, order, label, or which volume happened to read
  cleanly.

The composer takes reports the caller already holds and returns a mapping;
it opens nothing. D5's deferral of multi-device topology, multi-device
volumes, and cross-source transactions is therefore untouched, and this
form requires none of the atomic multi-artifact open U16 proposes.
