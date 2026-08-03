<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F33 — C1541 flux mastering profile

Reduce an opened capture set's `FluxCapture` to one circular, half-track-addressed 1541 `FluxMedium` under a declared mastering policy, and account for the reduction. The profile owns side selection, per-location observation selection and reconciliation, the source-position to 1541 half-track map, projection of each observation's exact `TimeBase` onto the drive's rotation-relative timebase, and the expression of disagreement, weakness, and absence as pulse strength. It produces a `FluxMedium` and a complete declared-loss account; it writes no artifact and encodes no container.

Mastering resolves in two stages: a plan which computes everything and writes nothing, and an execution which writes. A reduction that no policy input names is a named refusal, not a default. The same sources, policy, and seed produce the same mastered state.

Touches: S1, S2, S3. Supports: U23; P2, P3, P4, P6, P13, P14, P22, P23, P27, P29. Needs: the flux-capture foundation and the KryoFlux capture-set adapter, both delivered; F37 pledged and delivered; F36 for the declarations it consumes.

Companion design: [design/c1541-flux-mastering.md](design/c1541-flux-mastering.md).

## F34 — P64 image-format adapter

The P64 adapter at its representation seam: recognition and evidence, explicit version and structure validation with named refusals, decode of stored half-track pulse positions and strengths into `FluxMedium`, and encode of a mastered `FluxMedium` into a new artifact under its own claim. The adapter owns the container grammar and its capability claim; it owns no selection, reconciliation, or timing policy, and it never decodes GCR, sectors, or files.

P64's authoritative layer is a flux medium, so conformance is a same-layer round trip: a mastered fixture encodes, reopens through the adapter's own decode, and presents the same half-tracks, pulse positions, and strengths. An existing destination path is refused rather than overwritten, and an interrupted write leaves a complete artifact or none.

Touches: S1, S2, S3. Supports: U7, U23; P1, P3, P4, P6, P8, P9, P12, P13, P22, P29. Needs: F37 pledged and delivered.

Companion design: [design/p64-image-adapter.md](design/p64-image-adapter.md).

## F36 — Drive-profile catalog and flux recognition

Establish the P30 seam and its catalog: the profile descriptor, the probe over an opened `FluxCapture`, bounded and comparable confidence carrying the observations that produced it, ranked verdicts where more than one profile claims a capture, caller pinning and override, and a named refusal when nothing claims it. C1541 is the first and only enrolled profile — two drive steps per track, 300 RPM against a 16 MHz reference, the four-zone density map with its sector counts and track boundaries, and GCR synchronization recognized as a run of shortest intervals.

The probe reads flux interval lengths and the patterns they form, and nothing else: it resolves no bit, assembles no byte, names no sector, and validates no checksum. What leaves it is a count, a density, an angle, an absence, and its evidence.

Conformance is the prepared capture set: probing the data surface recovers all four speed zones at their documented track boundaries with their documented sector counts, and refuses the half-step positions on cross-pass reproducibility rather than on a tuned threshold. A capture the catalog cannot claim names the refusal.

Touches: S1, S2, S3. Supports: U23; P3, P4, P12, P22, P23, P27, P29, P30. Needs: the flux-capture foundation, which is delivered; P30 pledged.

Companion design: [design/drive-profile-recognition.md](design/drive-profile-recognition.md).

## F37 — Flux medium v1 foundation

Establish the private durable flux-medium model beneath any drive interpretation and above flux capture: one circular pulse stream per family-addressed location, an exact rotational frame against a declared reference clock, per-pulse strength in the profile's declared vocabulary, the medium-level facts that are not per-pulse, and derived provenance on every part of it. It reuses the delivered flux-capture layer's bounded section-addressable backing rather than inventing a second one.

It is what a P64 decodes into, what a mastering reduction produces, and what a read channel projects from. It is not a public flux, pulse, or medium iterator, not an interchange format, and not a place any decoding happens: it holds no bitcell, recovered clock, synchronization, symbol, or byte.

Nothing in it carries recovered-evidence provenance. Every pulse is selected-and-projected or synthetic under P13 and P29, and a medium cannot be constructed except by a reduction that declared its policy. Unit tests build small synthetic media and verify circular bounds, exact-rational frame arithmetic, strength round-tripping, sparse locations, and bounded reload, with no capture, profile, or drive present.

Touches: none. Supports: P3, P4, P13, P22, P23, P27, P29, P30. Needs: the flux-capture foundation, which is delivered; the P22 and P23 amendments pledged.

Companion design: [design/flux-medium-foundation.md](design/flux-medium-foundation.md).
