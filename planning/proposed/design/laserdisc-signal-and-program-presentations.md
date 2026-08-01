<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# LaserDisc signal and program presentations

Design for [F22](../FEATURES.md#f22--laserdisc-signal-and-program-presentations),
serving U19 and proposed P24 alongside pledged P12–P15, P19, P21, and P23.

## The representation graph

LaserDisc proves that the optical family cannot assume its preservation floor
is always a decoded drive-visible sector structure. Two honest sources enter
the same family at different durable seams:

```text
raw RF image  -> optical signal active -> family decoder -> program observations
                                                        -> player presentation
                                                        -> LV-ROM block view

decoded CHD   -> optical program active ----------------> player presentation
                                                        -> LV-ROM block view
```

Only one optical representation is active. With raw RF, decoded video, audio,
vertical-blanking data, addressing, and digital data are derived observations
which can be regenerated under declared decoder policy. With decoded CHD, the
program itself is durable and active; RF is absent rather than an implicit
lower layer. Caching a derived decode does not create a second commit target.

## Signal-active state

The signal seam records time-indexed samples and the evidence required to
interpret them: sample clock, channel scaling and encoding, capture hardware
and processing, discontinuities, calibration, capture extent, conflicts, and
uncertainty. The source adapter claims exactly the artifacts which constitute
the capture. It distinguishes observation from correction or synthesis.

Signal state is not a physical surface model. Sample values are observations
after a particular capture chain; they do not prove original pits, lands,
reflectivity, pickup transfer function, or servo behavior. Re-decoding is the
fidelity test. Byte-for-byte reproduction of a source wrapper is a separate
adapter capability, not the definition of optical equivalence.

## Program-active state

The program seam records the family-visible decoded content supplied by its
source: synchronized video and audio, field or frame timing, vertical-blanking
and address information, chapter markers, control facts, and mapped digital
data. Every field retains provenance. A source may omit analog audio, digital
audio, blanking data, or addresses; the adapter reports those absences rather
than filling them from a nominal title profile.

A decoded LaserDisc CHD is the reference single-file program source. It proves
the seam but does not define every future encoding. A raw-RF adapter and a
decoded-program adapter remain peers behind the family boundary; neither
branches the orchestration layer on a filename suffix.

## Family decoding

Decoding signal state is an explicit, evidence-bearing derivation. Decoder
identity, version or policy, corrections, dropouts, confidence, and conflicts
remain reportable. Alternative decodes may coexist as observations or caches,
but there is still one signal-active durable state. Choosing a decode for a
player session does not discard the RF or silently bless the decode as
captured program truth.

F22 does not require one complete archival decoder in its first cut merely to
name this seam. It does require the reference raw adapter to preserve enough
information for an independent decoder and the public model to avoid
foreclosing later decoded observations.

## The player presentation

The typed hardware presentation sits at a useful external player boundary,
not at a universal optical-drive command set. A named player profile may map
F-code, serial, SCSI, or another documented control protocol into the common
P15 lifecycle. The presentation exposes the audio/video and data outputs that
profile makes observable while retaining family-neutral operations for
causal advancement, reset, capability inquiry, and completion handling.

Commands schedule effects; they do not synchronously teleport the pickup.
Seek latency, CAV or CLV progression, frame availability, playback direction,
still frames, audio state, and pending completion are session-local hardware
state. A consumer advances time and observes causal results. These facts are
never durable media content.

Profiles describe externally visible behavior only. F22 does not model the
laser, pickup electronics, focus or tracking loop, spindle servo, firmware,
or microcode. A profile may refuse an operation which another player supports
without weakening the common lifecycle.

## LV-ROM block mapping

LV-ROM demonstrates why optical block access is a partial presentation, not a
whole-disc identity. Its digital data occupies a declared mapping in program
channels which otherwise serve audio. The family adapter reports the mapping,
its time or frame extent, block size, address conversion, interleaving or
decoding rule, and evidence. A caller selects that identity to open a bounded
block presentation.

The video program remains concurrently observable because both views derive
from the same optical-active state. The mapped data is not a P16 partition,
and unselected audio, video, blanking, lead-in, control, and damaged regions
do not become padding blocks. A volume and filesystem may compose above the
bounded view when recognized.

## Mutation and persistence

The first feature is read-only. Editing decoded program content while
retaining honest signal state would require a declared signal-generation or
mastering composition and a representable destination; F22 supplies neither.
A future writable feature must still obey P2 and P13 and must never present
synthetic RF as captured evidence.

Session caches, decoder output, pickup position, playback continuation, and
player state are disposable. The source's selected optical representation and
its provenance are durable. Derived exported audio, video, metadata, or block
content are explicit products, not quiet replacement media layers.

## Surface and inspection consequences

S1, S2, and S3 need equivalent identities and reports for:

- optical family and active representation (`signal` or `program`);
- claimed source artifacts and per-field provenance;
- decoded observation sets and decoder policy;
- player-profile capabilities and presentation identity;
- program channels, time/frame extents, and LV-ROM data mappings; and
- the relationship from a selected mapping to its block, volume, filesystem,
  and file presentations.

Opaque identities are assigned by the library. Callers do not manufacture
them from frame numbers, chapter labels, channel numbers, filenames, or CHD
stream order.

## Acceptance shape

F22 is architecturally complete when:

- one raw RF adapter and one decoded program adapter feed the same LaserDisc
  family without orchestration branching;
- the raw source remains independently re-decodable with its capture evidence;
- the decoded source works without invented RF or physical-surface claims;
- one player presentation advances commands, playback, and outputs through
  P15 timed causality from either active representation;
- an LV-ROM mapping exposes only its eligible blocks while video remains
  observable; and
- all three application surfaces report the same identities, provenance,
  absences, ambiguity, and named refusals.

## Deliberately absent

- LaserDisc mastering, writing, or RF synthesis.
- A physical pit/land, reflectivity, pickup, laser, servo, firmware, or
  microcode model.
- A universal player command protocol.
- A whole-disc block view or a partition invented from a program mapping.
- Promotion of a decode cache into a second mutable durable representation.
- Claims that raw RF and decoded A/V sources have equal preservation fidelity.
