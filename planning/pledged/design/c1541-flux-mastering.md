<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# C1541 flux mastering profile

> **Status:** pledged, not delivered. This design serves F33 and authorizes no implementation outside that feature.

F33 consumes the `FluxCapture` produced by the delivered flux-capture layer and its KryoFlux capture-set adapter, and produces one circular, half-track-addressed 1541 `FluxMedium` — the delivered flux-medium layer — plus the declared-loss account P29 requires. It owns the reduction and nothing else: it does not read a container, does not encode one, and does not ascend to hardware bitstream, GCR, sectors, or files. It may recognize a family landmark in the interval domain to place the medium's origin (P30, D12) - interval lengths, never bit values; an angle, never a byte.

## The policy inputs

Each is supplied by the caller or declared by the profile, is reported in the plan, and travels into the result as provenance. None has a silent default; a reduction no input names is a named refusal.

- **Side selection.** Which captured side of the set supplies evidence for the family's recorded surface, named by the identity the capture-set adapter reported. The C1541 profile declares that the family records one surface (P30), so this input is answered by declaration rather than by choosing between two beliefs about one surface; a captured side the profile's surface mapping does not cover is refused. Sides are never merged or averaged.
- **Observation selection.** Which observation of a source location is used, and how several are reconciled when the rule admits more than one. A location whose observations disagree beyond what the rule resolves is a refusal, not a vote.
- **Half-track map and admission.** How the set's source drive-step positions map onto 1541 half-tracks, and which of them are admitted as recorded. The fixture supplies 84 positions per side; that the mapping is two steps per 1541 track is a declared fact of the capture, not arithmetic the profile performs unasked. Admission is decided per location from the evidence F36 reports, never from step parity (D12), and a source position no map covers is refused.
- **Timebase projection.** How an observation's exact `TimeBase` ticks and declared span project onto the medium's rotational frame — for a 1541, the drive's 16 MHz reference clock across one 300 RPM rotation. The projection is exact rational arithmetic against both declared bases; v1 introduces no floating point and no library-chosen sample rate. Resolution the destination cannot express is declared loss, never silent rounding.
- **Pulse strength.** How disagreement, weakness, absence, and contradiction across the selected evidence become the medium's strength vocabulary, with the seed that makes any stochastic element reproducible. This is the capture-fact-to-medium-fact conversion P22 names, and it happens here or nowhere.
- **Angular origin.** Where the medium's circle begins, under the profile's `OriginRule`. A 1541 drive never observes index, so the capture's index is a datum of the instrument rather than of the medium, and the C1541 profile defaults to the track's own seam. The rule applied and the evidence that located it travel into the medium's `OriginStatement`.

## What the plan reports

The plan computes the whole transformation before anything is written: the medium's half-tracks and their provenance, and the complete declared-loss account in the source's own terms — the unselected side, unselected observations, flux recorded before the first index and after the last, marker channels and control/OOB records with no destination expression, retained `ForeignRecord`s, capture metadata, transfer results, and unexpressible timing resolution. A count is not an account.

## What it does not own

The destination container's grammar, version claim, encoding, and refusals belong to the image-format adapter (F34, P12). The profile states how evidence is reduced; the adapter states what can be carried. Neither infers the other's answer, and neither is permitted to normalize what the other refused.
