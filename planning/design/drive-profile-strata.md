<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# What a drive profile is, and where it is still 1541-shaped

An open design problem belonging to no single feature, which is why it
sits here rather than beside one. It is forced by whichever drive family
arrives next, and every family so far has been able to ignore it.

The material is a 2026-08-06 design conversation, recorded 2026-08-16
after F76 and F77 walked into the first of its findings. The pitch
decision it turns on is [D61](../DECISIONS.md#d61--step-pitch-is-declared-as-a-rational-pair-and-consumers-never-divide-it).

## Three owners, and only one of them is clean

A flux reading involves three things that each declare facts, and the
profile currently speaks for all of them at once:

- **the media** — already separate and already clean, as `MediaProfile`:
  passive compatibility facts, no behavior, no recognition;
- **the target drive** — the mechanism and electronics the recording was
  made by and would be read back by;
- **the capture drive** — the instrument that actually read the
  artifact, which has *no declaration home at all*.

The third is the one to notice. A capture instrument has its own step
pitch, its own head width and its own rotational behavior, and none of
them are declared anywhere. Before F76 the 1541 profile's
`steps_per_location: 2` silently *was* a capture-instrument fact — a 96
TPI instrument over 48 TPI media — wearing the target drive's clothes.
D61 moved it into a named pair, which makes it visible without yet
making it owned.

**Cross-owner relations become derivations once the owners separate.**
Stepping is a ratio between capture pitch and recorded pitch. Bleed
geometry is a relation between head width and track pitch. Span
projection relates two rotational frames. Index inheritance asks whether
the *capture* drive observed index, not whether the target drive would
have. Each is arithmetic over two owners' declarations, and each is
currently a constant because there is only one owner to put it on.

## The profile also conflates two strata within one owner

`DriveProfile` holds hardware facts — stepping, rotation, surfaces,
encoding shape — beside CBM DOS *format convention*: 35 tracks, the zone
assignment, the record counts per zone. Those are not facts about a
drive. They are facts about how a filesystem laid a recording down on
one.

The consequence is visible in what the capture probe actually does: it
recognizes the **format**, not the drive. A 1541 mechanism carrying a
non-CBM recording would not be recognized as a 1541 by anything here,
and a profile split along this seam would say why.

## The simplifications, and the family that forces each

Five, each a place where the model is correct for the 1541 and
incomplete in general. None is a defect today; each becomes one on
arrival of the family named beside it.

| # | Simplification | Forced by |
|---|---|---|
| 1 | `Stepping` arithmetic is integer | a C8050 at 100 TPI read by a 96 TPI instrument — the ratio is 24/25 |
| 2 | one global rotational frame per profile | Mac GCR, whose rotation is zoned |
| 3 | surfaces is a scalar count | any family needing a head→surface map rather than a number |
| 4 | write width is undeclared | anything reasoning about fat tracks or pitch mismatch |
| 5 | read/write head width is undeclared | the same, and it is what *derives* 4's consequence |

Four and five are the same region seen twice: the profile declares
`DuplicateRule` — the *consequence* of a head wider than the track pitch
— without declaring the width that produces it. So the model states an
outcome and not the fact it follows from, which is why a second family
cannot compute its own outcome.

## The first one is done

D61 is delivered in full. `Stepping` reduces its pair by the common
divisor and reports a **cadence** — how many steps the mechanism takes,
and how many recorded tracks those steps cover — rather than a single
count that only ever answered for one pairing:

| Mechanism over recording | Cadence | Reading |
|---|---|---|
| 96 over 48 (the 1541) | 2 steps, 1 track | every other step lands, the count it replaced |
| 48 over 48 | 1 step, 1 track | every step lands |
| 48 over 96 | 1 step, 2 tracks | reaches the even tracks, skips the odd |
| 96 over 100 | 24 steps, 25 tracks | every twenty-fourth step, advancing twenty-five |

Two things fell out that the integer form had got wrong, and both were
wrong in the same direction — refusing something real.

**A coarser mechanism addresses tracks.** A 48 TPI head over 96 TPI
media moves two whole track pitches per step, so it lands on the even
tracks. The old code answered zero steps and refused every location, and
a comment justified it as a drive that "cannot physically address a
track". Half of that was true — it cannot reach the odd ones — and the
conclusion did not follow.

**A ratio that does not divide is not a ratio that fails.** 96 over 100
is twenty-four steps across twenty-five tracks, which is exactly the
case D61 was decided for and exactly the case the integer form could not
express.

The division that remains is exact, and the admission check ahead of it
is what makes it so: a position is tested for divisibility first, and
only one that passes is divided. Nothing rounds, and nothing compares
within an epsilon — which is the whole of what "consumers never divide"
was protecting.

## What this does not decide

It does not propose the owner split, pledge the strata separation, or
choose a shape for any of the five. It records what was settled, what
was identified, and where the code currently stands, so that the next
family's implementer meets a written finding rather than a surprise.
Whatever eventually restructures the profile is a change to a private
contract and needs no surface amendment — which is why none of this has
a use case or a principle behind it, and why it waited.
