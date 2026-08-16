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

## P32 amendment — a machine is a named device set within a session

Pledged P32 makes the session itself the device set and states that
nothing groups sessions into a machine. This amendment would insert a
**machine** between them, without renaming anything: the **session**
keeps the meaning the principles already give it — the P7 claims, the
P27 cache budget and private session storage — and a **machine** is one
device set within it, owning the attachment identities and the
attachment order. What P32 says about devices is unchanged; only the
scope that holds them moves one level in.

**The layer earns itself where artifacts nest, and nowhere else has yet
asked for it.** An archive on the host was never part of the machine
whose disk it contains, so reading `games.zip/boot.h8d` need not put
both in one machine's configuration — and because every machine sits in
one session, a disk's medium may be source-backed through the claim the
session already holds, with no lifetime question between them. A machine
would reach its own devices and no others, so nothing reading one
machine's set could see a slot that belongs elsewhere.

**A machine would carry an identity, and the anonymous one would be
null.** A session would have one anonymous machine; devices could be
added to it directly, deterministically — one machine, not one conjured
per call — serving the caller who is opening artifacts rather than
reconstructing a machine. It would be the same kind of thing as a named
machine, holding no privileged position: not "machine zero", no
attachment order more meaningful than any other's, and moving a device
out of it a reconfiguration rather than a rename. Every verb a named
machine answered, the anonymous one would answer too (D23).

**This was built once and withdrawn, which is why it sits here rather
than in `pledged/`** (D58). Delivered alongside the DOS drive-letter
composer, it lost its only consumer when that composer did: nothing in
the library read a device set as a set or read attachment order at all,
and what remained was a scope whose every mechanism existed because
there could be more than one machine, with nothing needing two. The
nesting argument above is undamaged by that — it was never the reason
the tier was built — but it is an argument for work not yet done, and
this shelf is where an argument binding nothing belongs. Pledging it
again means answering the question the first attempt did not: what shape
does the tier take when read off a real nesting journey, rather than
guessed ahead of one?

**What it depends on.** Nesting resolves one level deep today and is
special-cased to ZIP and 7z by file extension; P25 (proposed) is the
recursion this amendment's own justification assumes. An amendment
pledged before that would be pledged on a prerequisite the project has
not agreed to, which is the flaw the reference rule names.

**The names `Machine` and `MachineView` are spent.** They were issued on
the first attempt and withdrawn with it; a tier that returns takes
whatever the journey shows it to be called.

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
