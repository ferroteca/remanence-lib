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
                 = RF capture, then                      -> player presentation
                   corrected signal                      -> LV-ROM block view

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

## The signal seam holds two models

The magnetic family already met this shape and split it (D14): a **flux
capture** is what an instrument recorded, and a **flux medium** is what a drive
would read, derived from the capture under declared policy and never
constructible without it. The LaserDisc signal seam holds the same two objects
under different names, and naming only one of them would repeat the ambiguity
D14 found in P22.

A raw RF capture is instrument state — uniform amplitude samples on the
sampler's own clock, referred to nothing on the disc. A Domesday Duplicator
stream at 40 MSPS and ten bits is one such capture, and two captures of one
disc are not comparable sample for sample. Demodulating and time-base
correcting yields the second object: line-locked composite at four times the
colour subcarrier, organized into fields, with per-field dropout, blanking and
confidence observations beside it, which is what `ld-decode` writes as a `.tbc`
and its JSON companion. Two corrected readings of one disc *are* comparable,
field for field.

That comparability is the whole argument for a second model, and it is not a
new argument. It is why the Aaru format carries a bitstream block beside its
flux block: repeated dumps of one medium yield inconsistent flux, and the
decoded bitstream is where results become comparable regardless of whether
anything above it can be extracted. The corrected signal is that rung for this
family.

D14's test transfers without modification. **Disagreement across captures is a
capture fact; a corrected, addressable reading is a medium fact.** What
correction adds — the line and field frame, the subcarrier-locked clock, the
dropout vocabulary — is absent from the samples and supplied by a declared
decoder policy, exactly as a flux medium's rotational frame is supplied by a
P30 profile rather than found in the flux. D14's reason for refusing flux
capture an active-layer row applies here too: a write into a set of disagreeing
observations has no principled destination.

The corrected signal is therefore **not a third active representation**. P24's
rule stands unchanged — one optical active layer, signal or program — and the
two models sit behind the signal half of it. A report names which model it
speaks about, because "the signal" alone does not say.

## The layering, against the magnetic family

| magnetic | LaserDisc | what the rung is |
|---|---|---|
| flux capture | RF capture | instrument state; several passes, no common frame |
| flux medium | corrected signal | what a player would read; declared timebase and frame |
| CHS sectors | fields and frames | addressed units, reached only by decoding |
| files | program, EFM audio, LV-ROM blocks | the several things one disc carries at once |

The lower two rungs correspond closely, which is what makes the split above
transferable. The upper two do not, and the differences are the family's own.

## What the program layer addresses

A frame is addressed by the picture number or timecode carried in the disc's
own vertical-blanking data: CAV discs number frames, CLV discs carry a timecode
and chapter. Three things distinguish that from a CHS address, and each is a
claim the family has to make honestly rather than borrow.

**The address is wholly self-described.** CHS takes cylinder and head from the
mechanism and only the sector number from recorded data; a frame address exists
only inside the signal. The library must decode before it knows where it is, so
a capture whose blanking data is unreadable has no addressing at all rather
than a default one, and says so.

**The addressed unit is a window, not a record.** A sector is discrete and
checksummed and either reads or does not; a frame is a bounded segment of a
continuous signal with no bit-exact ground truth to compare against. Frames are
not reported as records that verified.

**One family carries two address spaces.** CAV gives one frame per revolution;
CLV gives time, with frames per revolution varying by radius. Which applies is
a fact about how the disc was mastered rather than about the player, so it is
read from the disc and reported, never configured.

The digital payloads riding the same signal keep their own addressing: EFM
audio decodes to bit-exact PCM, and an LV-ROM mapping presents blocks under the
rule below. One capture, several decode targets, only some of them exact —
which is precisely why the family cannot serve them all through one addressing
vocabulary.

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
- a signal-active report names which of the seam's two models it speaks about,
  and a corrected signal names the decoder policy that produced it;
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
- A third optical active representation for the corrected signal; it is a model
  within the signal seam, not a layer beside it.
- An addressing nature for signal or corrected-signal state, which is reached
  by position and time rather than by address (the proposed P32 amendment).
- Claims that raw RF and decoded A/V sources have equal preservation fidelity.
