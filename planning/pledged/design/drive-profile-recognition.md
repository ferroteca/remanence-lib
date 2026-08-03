<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Drive-profile catalog and flux recognition

> **Status:** pledged, not delivered. This design serves F36 and authorizes no implementation outside that feature.

## Purpose

F36 establishes the P30 seam: the place a drive family's recording
conventions are declared, and the place a capture is recognized as belonging
to that family. It exists because P22 and P23 both rest on a "media profile"
and a "hardware profile" without naming an owner for either, and because the
knowledge those principles assume — stepping, rotation, density map,
encoding landmarks — is precisely what a flux capture does not contain.

A capture records what the head saw. It does not record that the drive it
will be served to spins at 300 RPM, that two source positions make one
track, or that a run of shortest intervals is a synchronization mark. Every
one of those is family knowledge, and the seam is where it lives.

## What a profile declares

Each is a declared fact carrying its provenance, not a value inferred from a
capture and then treated as evidence:

- **Stepping and addressing.** How the source's step positions map onto the
  family's own location identity, and how many steps one location takes.
- **Rotation.** The family's nominal rate and its reference clock, which
  together define the rotation-relative timebase a served medium is
  expressed in.
- **Density map.** The zones or regions the family records at, each with its
  claimed rate and what that rate implies about capacity.
- **Encoding landmarks.** The timing shape of the family's synchronization
  and gap conventions — stated as interval patterns, never as symbols.
- **Surfaces.** Which surfaces the family records, so a capture of an unused
  one is recognized as unused rather than believed.
- **Revolution rule.** Whether the family's drive observes a selected
  revolution, a repeatable sequence, a seeded variation, or another
  specified behavior. F30 already refuses to normalize inside the layer;
  this is where the rule that resolves it is declared.

## Recognition

The probe is offered opened flux evidence and returns a verdict: the profile
claimed, a bounded and comparable confidence, and the observations that
produced it in terms a human can read (P4). "C1541, confidence 100" is not
an answer. The observations are.

Several profiles may claim one capture, and the result is ranked rather than
resolved by the catalog. A caller may pin a profile or override the ranking,
and whichever profile is used travels into the result as provenance. A
capture no profile claims is a named refusal (P3) — a single enrolled entry
never wins by being the only one.

### The recognition boundary

The probe reads interval lengths and the patterns they form. It may report a
count, a density, an angle, a location, and an absence. It may not resolve a
bit value, assemble a byte, name a sector, or validate a checksum: those are
the hardware bitstream and above, and reaching them here would make every
recognition depend on a clock-recovery model. **What leaves the probe is an
angle, never a byte.**

This is what makes a synchronization landmark admissible. A GCR sync is ten
or more consecutive `1` bits, and a `1` bit is a transition one cell after
the last, so a sync is a run of minimum-length intervals — recognizable
without a clock, without the encoding table, and without knowing what the
sync introduces. The profile locates it; it does not read it.

## The profile layout

One record states the family. Its **recognition half** is what the probe
reads and is delivered by this feature; its **materialization half** is what
the flux-to-medium reduction reads and is consumed by the feature that
performs that reduction, which this design does not authorize. Both halves
are laid out here for one reason: they are facts about the same family, and
splitting them across two documents is how two features come to hold
different answers about one drive.

Every field is a declared fact carrying the published description it came
from. None is a value a capture is permitted to establish.

```text
DriveProfile
  identity: ProfileId, display name, profile version
  provenance: the published description each declared fact derives from

  # recognition half — read by the probe
  stepping: Stepping
  rotation: Rotation
  surfaces: Surfaces
  encoding: EncodingShape
  density: ordered list<DensityZone>

  # materialization half — read by the reduction
  addressing: DestinationAddressing
  admission: AdmissionRule
  origin: OriginRule
  observation: ObservationRule
  projection: TimebaseProjection
  strength: StrengthVocabulary
```

```text
Stepping
  steps_per_location: u32          # how many source steps make one location
  first_location: the family location identity of source position zero
  location_order: the declared ordering of source positions

Rotation
  nominal: exact rational rotations per second
  reference_clock: exact rational Hz
  cycles_per_rotation: derived from the two above, exact
  index_observed_by_drive: bool    # whether the family's drive sees index

Surfaces
  recorded: u32                    # how many surfaces the family records
  identity: how a captured surface maps onto a family surface

EncodingShape
  cell_multiples: ordered set<u32> # the interval populations this encoding
                                   # produces, as multiples of the cell
  classification_band: rational    # admissible deviation from k * cell
  landmark: LandmarkShape

LandmarkShape
  multiple: u32                    # which multiple the landmark run is made of
  min_run: u32                     # the shortest run that counts as one
  per_record: u32                  # how many landmarks one record carries

DensityZone
  locations: inclusive family location range
  nominal_rate: exact rational bits per second
  records: u32                     # what this zone claims a location holds
  nominal_cell: derived from nominal_rate and reference_clock
```

```text
DestinationAddressing
  space: the family's own location identity set and its order
  from_source: the mapping from source step position onto that space
  unmapped_source_position: Refuse | Declared(rule)

AdmissionRule
  # what the medium holds where the family records but the capture does not
  unrecorded: Absent | CarriedAsWeak
  # what a location must satisfy before it is claimed as recorded
  claim_requires: landmark agreement with the location's zone,
                  landmark spacing regularity,
                  and agreement across the location's observations,
                  each reported as evidence rather than as a verdict

OriginRule
  default: LongestGap | Index | DeclaredAngle
  # where the medium's circle begins.  A family whose drive never observes
  # index cannot honestly inherit the capture's index as its origin.

ObservationRule
  selection: Selected(rule) | Sequence | SeededVariation(seed)
  reconciliation: how several observations of one location combine
  disagreement_beyond_rule: Refuse

TimebaseProjection
  span: ScaleToNominal | PreserveIntervals
  density: SnapToZoneNominal | PreserveMeasured
  # each choice that discards a measured quantity is declared loss

StrengthVocabulary
  states: the family's declared strength semantics
  from_evidence: how agreement, disagreement, absence and contradiction
                 across the selected observations map onto those states
  seed: what makes any stochastic element reproducible
```

Every field of the materialization half is a P29 policy input: the profile's
value is a declaration, the caller may supply its own, the plan reports which
was used, and it travels into the result as provenance. A field the profile
leaves unstated and the caller does not supply is a refusal, never a default.

## Recognition, in terms of the layout

The probe, per source position:

1. **Derive the cell** from the interval population, self-consistently.
2. **Classify** each interval against `cell_multiples` within
   `classification_band`, and report how much of the population resolves.
3. **Find landmarks**: runs of at least `min_run` consecutive intervals at
   `landmark.multiple`.
4. **Project** the derived cell onto `rotation.nominal`, which removes the
   capture drive's own speed, and compare it with the `nominal_cell` of the
   zone that `stepping` says this position belongs to.
5. **Compare** the landmark count against that zone's `records` ×
   `landmark.per_record`.
6. **Measure the spacing** between landmarks, report its regularity, and
   report the one distance that departs from it as an angle — that departure
   is the location's seam, and `OriginRule::LongestGap` consumes it.
7. **Compare the location's observations** with one another, and report the
   agreement.

Confidence is composed from those observations and every one of them is
reported beside it (P4).

**A declared fact guards the arithmetic.** Deriving a cell self-consistently
admits a spurious solution at half the true cell, where the shortest
multiple is simply unpopulated and every real interval classifies one step
too high. `cell_multiples` makes that detectable rather than plausible: a
solution leaving its shortest multiple empty is rejected and re-derived. A
confidence figure alone would have reported the failure as a finding about
the disk; the evidence beside it is what makes it a finding about the probe.

## The C1541 entry

The first and only enrolled profile, and the layout's concrete values:

| field | value |
|---|---|
| `stepping.steps_per_location` | 2 |
| `rotation.nominal` | 5 rotations per second |
| `rotation.reference_clock` | 16 MHz |
| `rotation.cycles_per_rotation` | 3,200,000 |
| `rotation.index_observed_by_drive` | false |
| `surfaces.recorded` | 1 |
| `encoding.cell_multiples` | {1, 2, 3} |
| `encoding.landmark` | multiple 1, min run 9, 2 per record |
| `density` | the four documented speed zones, with their track boundaries, rates and sector counts |
| `origin.default` | `LongestGap` — the drive never observes index |

The zone table is the family's strongest recognition evidence and the
reason discovery is worth having: sector count and projected rate agreeing
across four zones at their documented boundaries is a signature no other
family produces.

## Conformance

The prepared capture set. Probing the data surface recovers all four speed
zones at their documented boundaries with their documented sector counts,
and reports the landmark and seam evidence for each location. The half-step
positions and the unrecorded surface are refused on reproducibility, not on
a threshold chosen to make the fixture pass. A capture the catalog cannot
claim names its refusal and opens nothing.

The probe is read-only and mutates nothing (P2), reads the bounded evidence
its claim names rather than a whole capture (P27), and its result is
identical however many threads served it.

## Outside the feature

Any second drive family. The flux-to-medium reduction itself, which is F33's
under P29 — this feature supplies declarations to that plan and performs no
reduction. Hardware bitstream, GCR decoding, sectors, filesystems and files.
Hardware emulation and mechanism state, which are P15's. A public flux,
pulse, or capture-run iterator: the verdict and its evidence are the surface.
