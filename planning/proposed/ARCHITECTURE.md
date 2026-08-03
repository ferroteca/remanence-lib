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


## P28 — Evidence may narrow authority without discarding readable evidence

Fail-closed is a rule about authority, not a command to discard every byte whose complete intended interpretation cannot be proved. An image may be recognizably incomplete, contradictory, or only caller-described, yet still contain a bounded region that the library can read without inventing bytes or concealing the defect. In that case the library retains the evidence and offers only the operations whose preconditions it can establish.

Every open therefore has one explicit **assurance outcome**: **verified**, where the selected interpretation and every bound needed by the requested operation are evidenced; **degraded**, where a material shortfall or contradiction is known but a truthful read-only interpretation of a bounded portion remains; or **refused**, where no bounded interpretation exists or an operation needs the missing or contradictory fact. The transition from verified to degraded is the confidence threshold. It is not a second arbitrary score beside P4's recognition confidence: it is a deterministic safety gate.

A declared size exceeding the source, contradictory required structure, a caller assertion the source disproves, or a read reaching an unavailable extent fails that gate. The report states the evidence, resulting bounds, and withheld operations. An explicit caller selection is an interpretation request, not a waiver of evidence. Thus a raw 1.44 MiB FAT12 floppy declaration over a shorter source enters degraded read-only mode: the library may list or extract only data whose directory traversal and full cluster chain remain in the source. A chain entering the absent tail is a named unavailable result, never zero-filled, shortened, or successful.

Degradation is not repair: the library does not fabricate missing sectors, skip damaged structures, choose an unresolved interpretation, or continue after it has lost the bounds that make a result meaningful. A malformed boot record that prevents a safe prefix from being addressed remains a refusal.

The degraded path is deliberately narrow: it applies only while determining a catalog type or reading or writing through an already selected catalog type. A catalog adapter may preserve uncertainty in the image, layout, volume, filesystem, or file operation it owns when the result remains bounded and evidenced. It does not apply to the library machinery around that interpretation. Failure to acquire or use the host claim, to read or write the session cache or private storage, to persist the commit journal, to allocate a required resource, or to perform host I/O is an immediate P6 failure. Such a failure cannot be re-described as imperfect media evidence or yield a partial answer.

Degraded state revokes mutation authority for the session. A write-intent open reports an evidence-driven effective read-only mode and a stable condition; every write, commit, and mutation-capable derived operation is refused with that condition. P7's no-silent-fallback rule still governs an inability to acquire host access — this is a distinct restriction after a safe claim has been made. A session never regains write authority without a new verified open.

P3 and P6 remain intact: the library refuses an unclaimed interpretation and stops the first operation it cannot account for. It does not turn a known, bounded deficiency into an all-or-nothing loss of independently readable evidence. P4 carries the reason, P10 carries the stable condition, and P5 requires equivalent assurance outcome, evidence, bounds, and effective mode in Rust, C, and Python.
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

## P22 amendment — the flux family holds two models, capture and medium

P22 already names both and gives them one word: "a capture adapter may
preserve several revolutions and their marker timing, while **a normalized
media model may define one circular revolution**." This amendment names the
second model, so that one word stops doing two jobs.

**Flux capture** is timed transition evidence as an instrument recorded it:
several capture runs and observations of one source location, the
instrument's own timebase, the source's own location identity, parallel
marker channels, and whatever else the capture container expressed. It
asserts nothing about which revolution the disk *was*, and P22's existing
refusal to average, deduplicate, or select inside it stands unchanged.

**Flux medium** is one circular pulse stream per location the family
addresses, expressed in a declared rotational frame against a declared
reference clock, with each pulse carrying the family's strength semantics,
beside the medium-level facts that are not per-pulse. It asserts exactly
what a drive would read.

**The boundary is one sentence: disagreement across observations is a
capture fact, and strength is a medium fact.** A capture records that three
passes differed. A medium records that a pulse is weak. Turning the first
into the second is a reduction governed by P29 and performed by neither
model on its own initiative.

What the medium adds is precisely what the flux does not contain — the
rotational frame, the family's addressing, the reference clock, the strength
vocabulary, and which surface is the disk. Every one of those is declared by
a P30 drive profile. That is why this is a second model and not a tidier
first one: the medium is where declared family knowledge and recorded
evidence combine, and a representation holding only one of the two cannot
stand in for it.

What the medium must **not** hold keeps it below the layer above: no
bitcell, no recovered clock, no synchronization, no symbol, no byte. Those
are hardware bitstream and above, and a medium that reached them would erase
the distinction between what a medium is and what a drive makes of it.

P64 is a flux medium. SCP, A2R and KryoFlux streams are flux captures. G64
is a hardware bitstream. Naming them locates them; it changes no support
claim, each of which remains enumerated under P3 and delivered by its own
adapter.

### What this folds into on delivery

In-force P13's authoritative-layer list carries both names rather than
"flux transitions" alone. Pledged P22's own text takes the vocabulary
throughout. F30 is renamed to the flux-capture foundation — its content is
already entirely the capture model, so this is a rename and not a split, and
its handle survives. F33 states that it reduces a capture to a medium, F34
that a P64 decodes into a medium and encodes from one, and proposed F32 that
it consumes a medium. F35's private `FloorAddressing::Flux` takes the
medium's spelling.

## P23 amendment — flux medium is the active layer, and flux capture is never one

P23's durable active-layer table already describes the medium in its `flux`
row: "circular track-relative flux transitions and strength semantics, with
marker/sensor channels and provenance — a modeled magnetic recording
surface". Singular, circular, carrying strength. That row is renamed **flux
medium**, and its description stands as written.

**Flux capture takes no row.** It is an authoritative image layer under P13,
which is a statement about what an artifact records, and it is read by
inspection and by mastering. It never carries a session's mutable truth.
P23's rule is scoped to "every independently mutable open state instance",
and a capture set opened to be inspected and mastered is not one; a writable
capture-editing session is claimed by nothing here and would need its own
proposal.

The reason is not bookkeeping. **A capture has no coherent answer to where a
write lands.** A drive writing to a flux capture would have to choose which
of several disagreeing observations to overwrite, and no answer to that is
better than another. A drive writes to a medium.

**Capture becomes medium by mastering, not by lowering.** It is a P29 act
with declared policy inputs, whether its destination is a new artifact — the
U23 journey — or an active layer inside the session, so that a drive can be
served over a capture. Only the destination differs; the inputs, the plan,
and the declared-loss account are the same. This supplies the mechanism the
pledged P15 clause assumes when it says a drive's floor may be "timed flux
for a P64 or a raw capture": a raw capture becomes a floor by being mastered
in session under declared policy, never by a normalization nobody named.

**Generate-flux is generate-medium.** In-force P23's explicit transition
below CHS synthesizes a medium and never a capture, because fabricating
instrument evidence from sectors would be a false claim about provenance in
the one clause most concerned with honest provenance. Every requirement in
it — preserve what is known, synthesize only what the lower model needs,
keep ambiguity ambiguous, refuse rather than invent — is unchanged.

The magnetic ladder therefore reads: flux capture → flux medium → hardware
bitstream → encoded bytestream → CHS → filesystem. Block stays terminal and
disjoint from all of it, and P23's prohibition on crossing between the block
and flux families is untouched in both directions.

### What this folds into on delivery

In-force P23's active-layer table, its generate-flux clause, and the tied-
caches sentence "a P64 source pins flux active", which becomes flux medium.
The pledged P23 amendment's ladder gains its lowest two rungs by name. The
pledged P27 tie is unchanged in substance: a medium derived in session from
a capture is the session's active state with its own cache, and the capture
beneath it is source-backed evidence, not a layer the session may write.
