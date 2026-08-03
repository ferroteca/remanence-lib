<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FluxMedium v1 foundation

> **Status:** proposed, not pledged. This design serves F37 and authorizes no implementation.

## Purpose

F37 establishes the second of the flux family's two models. `FluxCapture`
(F30, renamed by the P22 amendment) holds what an instrument recorded.
`FluxMedium` holds what a drive would read: one circular pulse stream per
location the family addresses, in that family's own frame.

It exists because the two are not the same object and the project has been
using one name for both. A capture has several observations per location and
no opinion about which was the disk; a medium has one, and its positions are
angles rather than offsets into a particular pass. Neither can stand in for
the other, and a single record with a mode discriminant would be the
kitchen-sink union D9 already declined at this layer.

## What the medium is, and what it refuses to be

The medium is **derived, always**. Nothing in it carries recovered-evidence
provenance: every pulse is selected-and-projected from a capture or
synthesized downward from a higher layer (P13, P29), and there is no
constructor that does not name the policy that produced it. A `FluxMedium`
cannot be built by opening a capture, only by reducing one.

It holds no bitcell, no recovered clock, no synchronization, no symbol and
no byte. A read channel projects those from it as rotation advances and they
are ephemeral, exactly as the pledged P15 clause says.

## The layout

```text
FluxMedium
  profile: the P30 drive-profile identity whose declarations set the frame
  frame: RotationalFrame
  locations: ordered map<LocationKey, Location>
  provenance: Provenance

RotationalFrame
  reference_clock: exact rational Hz
  cycles_per_rotation: exact positive integer
  origin: OriginStatement

OriginStatement
  rule: the OriginRule the reduction applied
  evidence: what located it — a seam angle, an index datum, a declaration
  # the circle has no natural start; this records the one it was given

Location
  key: LocationKey                 # the family's own addressing, not CHS
  pulses: ordered list<Pulse>
  facts: ordered list<MediumFact>
  provenance: Provenance

Pulse
  position: cycles                 # 0 <= position < cycles_per_rotation
  strength: Strength

Strength
  state: a value in the profile's declared strength vocabulary
  seed_domain: what makes a stochastic reading of it reproducible

MediumFact
  kind: WriteProtect | Unformatted | Seam
      | Duplicate { of: LocationKey }
      | SourceFact { namespace, code }
  payload: bytes
  provenance: Provenance
```

`LocationKey` is the family's addressing as the P30 profile declares it — a
1541 half-track, not a cylinder and head, and never renumbered into CHS. The
map is **sparse, and the two absences differ**: a missing key means the
medium claims nothing at that location, while a present location with no
pulses is a location the reduction claims is recorded-and-blank. A location
the capture showed as unrecorded is one or the other according to the
profile's `AdmissionRule`, never silently the first.

`position` is an exact integer cycle count from the frame's origin, strictly
less than `cycles_per_rotation`, and positions are strictly increasing. The
wrap from the last pulse to the first is implied by the frame, so no
duplicate boundary pulse exists — the same discipline F30 applies to an
observation's span. v1 introduces no floating point anywhere: the projection
from a capture's timebase into this frame is exact rational arithmetic
against both declared bases, and resolution the frame cannot express is
declared loss rather than silent rounding.

`Strength` is the profile's vocabulary rather than any container's. A
destination adapter states what it can carry and refuses what it cannot
(P12, P29); the medium does not pre-flatten itself into P64's spelling, or
WOZ's, or MFI's.

`MediumFact::Seam` records where the track's write splice sits when the
reduction located one, as an angle. `Duplicate` records that a location's
content matched an adjacent one, which the D12 evidence showed a capture
cannot disambiguate on its own — it is carried as a stated fact, not
resolved.

## Backing and residence

The backing is F30's, not a second mechanism: the same length-delimited
record stream and persistent ordered section index, keyed here by
`(LocationKey, SectionKind, ordinal)` with pulse and fact chunks splitting at
deterministic record-count boundaries. Pulse positions delta-code as unsigned
cycles inside their ordered chunks. A drive can load one location, or one
angular span of one location, without decoding another.

Under P27 the medium is normally **session-backed**: it is produced by a
reduction into private session storage and served from there through the
session cache. A source whose container can truthfully locate and decode one
location by key — a P64 — may be source-backed instead, and the choice is a
residence question that changes nothing about what the layer means.

When the medium is the active layer, its cache carries the session's mutable
truth under P27's two residency classes, and any capture beneath it is clean
source-backed evidence which the session never writes.

## Conformance

Unit tests build small synthetic media directly and verify circular bounds,
strictly increasing positions, exact-rational frame arithmetic, the sparse
map's two absences, strength round-tripping through the backing, seam and
duplicate facts, and bounded reload of one location. No capture, profile,
drive, or container appears in them: this feature is the model, and the
paths into and out of it are F33's, F34's and F32's.

## Outside the feature

The reduction that produces a medium from a capture, and the policy it
declares. Any container grammar. Drive profiles and their recognition. A
public iterator over pulses, locations or media. Everything above the
medium — bitstream, bytestream, sectors, filesystems, files — and everything
below it, which is nothing: this is the floor P22 declares.
