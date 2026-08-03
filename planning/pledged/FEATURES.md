<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F33 — C1541 flux mastering profile

Reduce an opened capture set's `FluxCapture` to one circular, half-track-addressed 1541 `FluxMedium` under a declared mastering policy, and account for the reduction. The profile owns side selection, per-location observation selection and reconciliation, the source-position to 1541 half-track map, projection of each observation's exact `TimeBase` onto the drive's rotation-relative timebase, and the expression of disagreement, weakness, and absence as pulse strength. It produces a `FluxMedium` and a complete declared-loss account; it writes no artifact and encodes no container.

Mastering resolves in two stages: a plan which computes everything and writes nothing, and an execution which writes. A reduction that no policy input names is a named refusal, not a default. The same sources, policy, and seed produce the same mastered state.

Touches: S1, S2, S3. Supports: U23; P2, P3, P4, P6, P13, P14, P22, P23, P27, P29. Needs: the flux-capture foundation, its KryoFlux capture-set adapter, the flux-medium foundation and the drive-profile catalog, all delivered.

Companion design: [design/c1541-flux-mastering.md](design/c1541-flux-mastering.md).

## F34 — P64 image-format adapter

The P64 adapter at its representation seam: recognition and evidence, explicit version and structure validation with named refusals, decode of stored half-track pulse positions and strengths into `FluxMedium`, and encode of a mastered `FluxMedium` into a new artifact under its own claim. The adapter owns the container grammar and its capability claim; it owns no selection, reconciliation, or timing policy, and it never decodes GCR, sectors, or files.

P64's authoritative layer is a flux medium, so conformance is a same-layer round trip: a mastered fixture encodes, reopens through the adapter's own decode, and presents the same half-tracks, pulse positions, and strengths. An existing destination path is refused rather than overwritten, and an interrupted write leaves a complete artifact or none.

Touches: S1, S2, S3. Supports: U7, U23; P1, P3, P4, P6, P8, P9, P12, P13, P22, P29. Needs: the flux-medium foundation, which is delivered.

Companion design: [design/p64-image-adapter.md](design/p64-image-adapter.md).
