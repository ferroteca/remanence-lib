<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# C1541 flux mastering profile

> **Status:** pledged, not delivered. This design serves F33 and authorizes no implementation outside that feature.

F33 consumes the `FluxLayer` produced by F30 and F31 and produces one circular, half-track-addressed 1541 medium plus the declared-loss account P29 requires. It owns the reduction and nothing else: it does not read a container, does not encode one, and does not descend to hardware bitstream, GCR, sectors, or files.

## The policy inputs

Each is supplied by the caller or declared by the profile, is reported in the plan, and travels into the result as provenance. None has a silent default; a reduction no input names is a named refusal.

- **Channel selection.** Which capture channel of the set supplies evidence, named by the identity F31 reported. Channels are never merged or averaged.
- **Observation selection.** Which observation of a source location is used, and how several are reconciled when the rule admits more than one. A location whose observations disagree beyond what the rule resolves is a refusal, not a vote.
- **Half-track map.** How the set's source drive-step positions map onto 1541 half-tracks. The paired-channel fixture supplies 84 positions per channel; that the mapping is two steps per 1541 track is a declared fact of the capture, not arithmetic the profile performs unasked. A source position no map covers is refused.
- **Timebase projection.** How an observation's exact `TimeBase` ticks and declared span project onto the destination's rotation-relative timebase — for a 1541, the drive's 16 MHz reference clock across one 300 RPM rotation. The projection is exact rational arithmetic against both declared bases; v1 introduces no floating point and no library-chosen sample rate. Resolution the destination cannot express is declared loss, never silent rounding.
- **Pulse strength.** How disagreement, weakness, absence, and contradiction across the selected evidence become the destination's strength vocabulary, with the seed that makes any stochastic element reproducible.

## What the plan reports

The plan computes the whole transformation before anything is written: the mastered medium's half-tracks and their provenance, and the complete declared-loss account in the source's own terms — the unselected channel, unselected observations, flux recorded before the first index and after the last, marker channels and control/OOB records with no destination expression, retained `ForeignRecord`s, capture metadata, transfer results, and unexpressible timing resolution. A count is not an account.

## What it does not own

The destination container's grammar, version claim, encoding, and refusals belong to the image-format adapter (F34, P12). The profile states how evidence is reduced; the adapter states what can be carried. Neither infers the other's answer, and neither is permitted to normalize what the other refused.
