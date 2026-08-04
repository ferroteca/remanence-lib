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

A signal seam may hold more than one model behind its single active layer, as
the magnetic family already does. A sampled capture and the corrected signal a
player would have read are different objects: the second carries a timebase and
frame the first does not, supplied by a declared decoder policy rather than
found in the samples. D14's test governs the boundary unchanged — disagreement
across observations is a capture fact, a corrected reading is a medium fact,
and neither becomes the other unasked. A report names which model it speaks
about. This creates no second active representation and no second commit
target.

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

## P32 amendment — A device declares an addressing nature, and no family owns one

A storage device declares an **addressing nature** when it is created: the
shape of the address a caller presents to it. Two are claimed — **CHS**, where
a declared geometry is observable and load-bearing, and **LBA**, where a flat
block number deliberately hides it. A third, **sequential**, is owed wherever a
tape family is claimed; it is the only one whose address cannot be formed
without the device's current position, so a nature is not always a pure
function from an address to a location.

Nature is a fact about the device. It is machine configuration the caller
supplies — the class this principle already places on the caller's side of the
line, beside the slot an attach lands in — and it never reaches the media
instance or its profile. P14 supplies the proof rather than the constraint:
media is the independent mutable state *between image formats and drives*, and
a medium carries no nature of its own. One 1.44 MiB disk is CHS-addressed in a
floppy-controller drive and LBA-addressed over the USB floppy command set, and
nothing about the disk changes when it moves between them.

**No family is confined to one nature, deliberately.** A hard drive answers
both CHS and LBA depending on the command issued, which is what makes nature a
choice rather than a constant. Nothing here is served by additionally ruling
which natures a floppy or optical family may take: such a rule would be a claim
about hardware that exists rather than about what this library implements,
which is the distinction P3 draws, and it would already be false, since ATAPI
and USB floppy drives are LBA-addressed over ordinary floppy media. If a source
addressed that way is ever claimed, it is claimed as LBA and this principle
does not amend.

**Nature constrains which layers can serve a device; it never selects one.**
P13 and P23 settle the active layer from the evidence, and nature is checked
against the result. An LBA device requires block state, which is why P23
already holds that an LBA device cannot be lowered merely because another
family knows CHS or flux; a CHS device requires a geometry, from evidence or
from configuration. **Where a nature is not native to the source, its mapping
is declared rather than assumed** — a CHS presentation over geometry-opaque
block state declares the geometry it translates through, and an LBA
presentation over CHS- or flux-active state declares the order that makes
blocks of records. That is the requirement proposed P24 already places on an
optical block presentation, stated once for every family instead of once per
family.

Nature also bounds the seams a device offers, without a further rule: an LBA
device presents no P15 low hardware seam, having no geometry to position.

**Not every way of reaching recorded state is an addressing, and nature names
only those that are.** A flux medium and a sampled signal are reached by
position and time — P15's seam applies timestamped control changes and returns
causally ordered transitions rather than answering reads at addresses — so they
carry no nature at all. They are active layers under P23 and proposed P24,
beneath a device whose nature describes the addressed presentation above them.
