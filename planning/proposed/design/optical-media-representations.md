<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Optical media representations

Design for [F21](../FEATURES.md#f21--mixed-mode-optical-media-and-presentations),
serving U18 and proposed P24 alongside pledged P12–P15, P19, P21, and P23.
This is a proposed destination, not approval to implement it. It specifies the
semantic seams and evidence floor; exact public type and method names remain
surface design for delivery.

## The two useful views are asymmetric

A mixed-mode compact disc is durably represented at the optical floor because
its audio tracks, data tracks, indexes, gaps, main-channel frames, and
subchannels coexist as one recorded program. A logical-block view is useful
only over a data-track extent whose mode defines user-data sectors. It is a
derived partial presentation of the optical state, never a second whole-disc
representation.

```text
compound or single-file optical image
                  |
                  v
        optical active state
          /               \
         v                 v
typed optical-drive     selected data-track
presentation            block presentation
                              |
                              v
                         volume/filesystem
                              |
                              v
                        P19 file container
```

This is the optical analogue of a flux-active protected floppy whose ordinary
sectors and files remain readable on only the regions where those higher
interpretations are valid. The analogy stops at the representation itself:
optical state is not magnetic flux, and F21's CD drive-visible program floor
does not infer physical marks or analog pickup behavior.

## The CD program floor

The CD-family state is semantically an addressed program of frames plus typed
layout and evidence:

```text
OpticalDisc
  profile
  sessions
  tracks
  lead-in and lead-out facts
  addressed frames
  capture and derivation provenance

OpticalTrack
  identity
  session identity
  number and control flags
  mode
  index points and gap extents
  addressed frame bounds
  evidence and issues

OpticalFrame
  disc-relative address
  track/index membership
  main channel: payload + provenance
  P-W subchannels: payloads + provenance
  validation observations and issues
```

For a raw CD frame, the main channel is 2,352 bytes. A complete raw P–W
subchannel sample contributes 96 more bytes. Those sizes describe a CD-family
encoding unit, not a universal optical block size and not a public promise
that every source supplies both. Cooked 2,048-byte Mode 1 user data is a
derived payload inside an eligible track, not the optical floor.

The model must preserve distinctions which affect what a caller may claim:

- **captured** — supplied as an observation by the source image;
- **declared** — stated by a descriptor such as a cue sheet;
- **decoded** — derived reversibly from captured or declared state;
- **synthesized** — manufactured under explicit mastering or channel rules;
- **patched** — supplied by an overlay such as sparse replacement Q data;
- **ambiguous** — several interpretations remain supported by the evidence;
- **invalid** — bytes exist but fail the applicable structural or checksum
  rule; and
- **absent** — the source makes no claim and no selected policy supplies one.

These classifications can apply independently to layout, main-channel data,
each subchannel, and validation observations. A cue-declared Q position does
not turn synthesized P–W bytes into captured subchannels. A full SUB stream
can carry the final observable Q bits represented by an SBI overlay, while
the overlay still contributes distinct provenance: it says which values were
replacements rather than original capture.

Drive model, read offset, retries, C2 reports, confidence, and conflicting
reads describe capture. They accompany the applicable frame or source as
evidence. A composition may use an explicit reconciliation policy to choose
state, but the observations are not themselves another active layer and are
not silently collapsed into one supposedly recovered byte sequence.

## Image adapters normalize encoding, not evidence

An image adapter owns every source artifact required by its encoding and
materializes the common optical state at the fidelity it can prove:

- BIN/CUE may supply raw or cooked main-channel track payloads plus declared
  track and index layout. It normally supplies no complete captured P–W
  stream; any generated subchannel remains synthetic.
- CCD/IMG/SUB supplies a descriptor, raw main-channel stream, and raw
  subchannel stream as one compound image. The source files are members of
  one image encoding, not P19 entries.
- A CHD or Aaru image can carry normalized optical state in one host file.
  One-file packaging changes claims about neither fidelity nor provenance.
- A sparse SBI-like source is an overlay adapter, not a complete disc image.
  It patches selected Q-channel observations over a compatible base and keeps
  the patch relationship in provenance.

The catalog enrolls each of these as behavior at its actual representation
seam. Central orchestration never branches on extensions to decide which
channels must exist. Missing source members, contradictory descriptors,
overlapping extents, impossible addresses, or unsupported modes are named by
the adapter which understands them.

Round-trip fidelity is judged against optical state and its claimed evidence,
not against incidental source packaging. A normalized single-file format may
preserve every observable frame while not preserving the spelling, filenames,
or split of a source CCD/IMG/SUB set. If exact source-byte reproduction is a
claimed capability, it is a separate adapter guarantee and never inferred
from optical equivalence.

## The typed optical-drive presentation

P15 already places modern optical media at the applicable common command,
track, sector, and subchannel seam. F21 supplies one typed presentation using
the common timed-causality lifecycle rather than exposing a public iterator of
frames or subchannel events. Semantically it provides the operations needed
to model the selected optical command family, including:

- disc, session, and track information;
- raw and decoded sector reads with requested channel selection;
- current-position and other applicable subchannel queries;
- seek, play, pause, stop, and audio observations; and
- named command completion, refusal, and timing behavior.

The consumer may emulate a host bus, controller registers, DMA, interrupts,
or a larger device around this seam. Remanence owns only what the selected
typed contract places beneath it. It does not expose SCSI/MMC universally if
another system's natural seam differs, and it does not descend into laser,
servo, decoder firmware, or microcode merely because a real drive contains
them.

Audio playback illustrates the durable/runtime split. Sample frames belong
to optical state. Playback cursor, seek latency, pending completion, and the
time at which samples become outward observations belong to ephemeral P15
hardware state. Pausing or seeking changes runtime state; writing recorded
audio changes the optical active layer.

## The partial block presentation

A caller first identifies a data track from the optical report, then asks the
optical family to open that track's eligible user-data view. The result names:

- the selected track identity and exact optical frame extent;
- the logical block size defined by the track mode;
- the mapping between presentation-relative LBA and disc-relative frame
  address;
- whether main-channel decoding, descrambling, EDC/ECC validation or
  correction is applied; and
- the evidence and refusals produced by that derivation.

The presentation's LBA zero is local to its declared extent unless the typed
contract explicitly says otherwise. Callers do not infer it from track
number, cue-file offsets, array position, or whole-disc addresses. Reads are
whole-block and all-or-error as in U17, but their backing active state remains
optical. A block write must be encoded back into the selected optical frames
before it becomes visible; the implementation cannot retain an independently
mutable cooked-sector copy.

This seam does not make tracks into partitions. It supplies an addressed
region suitable for P17 volume composition. A recognized ISO 9660 filesystem
can then expose a P19 file container. Another data track may expose a different
block size or no recognized filesystem, while audio tracks expose no block
presentation at all. Enumeration and errors preserve the optical tracks which
have no higher view.

## ISO has two honest openings

An ISO-like source used only for data access remains block-authoritative and
block-active. Its adapter or caller supplies the applicable logical block size,
and ordinary volume, filesystem, and file presentations operate without any
optical claim.

An explicit optical attachment asks for a different composition. A selected
CD-family profile and mastering policy must account for at least track mode,
frame construction, scrambling where applicable, EDC/ECC generation, track
placement, gaps, lead-in/lead-out declarations visible at the chosen seam, and
subchannel synthesis. The whole optical state is constructed and validated
before it atomically replaces block as the active layer. Every detail not
present in the ISO retains synthetic provenance.

This transition creates a plausible newly mastered data disc, not the disc
from which the ISO may once have been extracted. It cannot recover audio
tracks, extra sessions, hidden pregap data, invalid sectors, protection Q,
CD-Text, damage, or original mastering. A missing rule refuses optical service
rather than using a global conventional default. Content sniffing never sends
an ordinary hard-drive image down this path; the caller must select an optical
composition whose family admits the source.

## Mutation and commit

All presentations share one optical-active instance after optical composition:

- an optical command write changes its addressed recorded channels;
- a data-track block write re-encodes the applicable optical frame;
- a filesystem edit projects through the volume and block presentations into
  those frames; and
- subsequent reads at every presentation observe the same result.

The composition is writable only if each requested mutation class has an
honest reverse path to the source image's P13 authoritative layer and encoding.
A BIN/CUE adapter which cannot preserve arbitrary subchannel writes cannot be
opened for that write contract. A richer destination may be selected through
explicit conversion. Commit never flattens the disc to the highest
presentation, discards audio or subchannels, or silently turns patched and
synthetic evidence into capture.

## Proposed P23 amendment

P23 currently says its four active-layer kinds are exact. P24 cannot coexist
with that sentence while optical is absent. Pledging F21 therefore requires
one coherent vision act:

1. pledge P24 and U18;
2. add `optical` to P23's exact active-layer table and all exhaustive prose;
3. retain the one-active-layer invariant;
4. state that derived data-track blocks do not make block active; and
5. state that an ISO's explicit generate-optical transition changes the one
   active state rather than maintaining block and optical peers.

No decision entry is needed for that lifecycle act; the principle texts and
the moving commit are its record.

## Acceptance shape

F21 is complete only when the delivered cut can demonstrate that:

- a mixed audio/data image retains every track when only one data track opens
  as blocks or files;
- raw main and subchannel data remain distinguishable and provenance-bearing;
- at least two materially different image encodings feed the same optical
  family interface without central format branching;
- a sparse Q overlay changes only its named frames and remains identified as
  patched;
- the optical hardware and data-track block presentations observe one active
  state;
- an ISO stays block-active for ordinary access and enters optical only by an
  explicit, atomic, synthetic generate-optical composition;
- unsupported track modes, missing members, invalid mappings, absent mastering
  rules, and unrepresentable writes are named refusals; and
- no F21 CD interface or report claims pits, lands, analog RF, pickup physics,
  firmware internals, or a whole-disc block view of mixed-mode media.

## Deliberately absent

- A universal schema forced across CD, DVD, Blu-ray, magneto-optical, and
  future optical families.
- Raw RF or EFM channel-bit capture within F21; F22 owns the distinct
  LaserDisc signal seam. Pit/land geometry, pickup, focus, tracking,
  spindle-servo, firmware, and microcode emulation remain absent.
- Automatic inference that arbitrary blocks originated on optical media.
- A second mutable block copy beside optical state.
- Treating tracks as partitions or audio as files merely to fit existing
  seams.
- A promise that CHD, Aaru, CCD/IMG/SUB, BIN/CUE, or any named format is
  supported until its own adapter is delivered and tested.
- Exact source-wrapper round trips unless an adapter claims them separately
  from optical-state fidelity.
