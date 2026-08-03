<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F30 — Private FluxLayer v1 foundation

Establish the private, durable flux media model and its bounded session backing: physical track keys, capture runs, circular observations, exact timebases, parallel marker channels, ordered evidence, provenance, and sparse section-addressable storage. It is the common target of admitted capture adapters, not a public flux iterator, interchange format, drive profile, or sector decoder.

Every source fact maps to a named layer fact, ordered foreign record, or a named refusal. Several observed revolutions remain distinct evidence: the model neither averages timings nor selects a cleanest pass. Unit tests build small synthetic layers and verify ordering, circular bounds, marker separation, and bounded reload without a drive interpretation.

Touches: none. Supports: P3, P4, P13, P22, P23, P27. Needs: P23 pledged.

Companion design: [design/flux-layer-foundation.md](design/flux-layer-foundation.md).

## F31 — KryoFlux capture-set catalog adapter

Recognize a declared KryoFlux capture set assembled from a catalog subtree and materialize its members collectively into `FluxLayer`. The adapter owns the capture-set grammar, member identity, track and side identity, capture-channel identity, stream ordering, index and control/OOB records, transfer results, and source provenance. An archive member is never silently treated as a disk when the logical capture requires the complete set.

The initial conformance fixture is the prepared paired-channel capture set. Tests open it through `SevenZipCatalog`, verify that all selected members form one capture set, and assert preservation of runs, markers, pre/post-index data, and separate channels. An incomplete, duplicate, or contradictory set is a named refusal. The adapter neither merges channels into an ideal disk nor materializes a bitstream, sector, or filesystem.

Touches: S1, S2, S3. Supports: U7; P3, P4, P12, P13, P15, P22, P23, P27. Needs: F30 pledged and delivered; the archive-catalog seam, which is delivered.

Companion design: [design/kryoflux-capture-set.md](design/kryoflux-capture-set.md).

## F33 — C1541 flux mastering profile

Reduce an opened capture set's `FluxLayer` to one circular, half-track-addressed 1541 medium under a declared mastering policy, and account for the reduction. The profile owns channel selection, per-location observation selection and reconciliation, the source-position to 1541 half-track map, projection of each observation's exact `TimeBase` onto the drive's rotation-relative timebase, and the expression of disagreement, weakness, and absence as pulse strength. It produces a mastered medium and a complete declared-loss account; it writes no artifact and encodes no container.

Mastering resolves in two stages: a plan which computes everything and writes nothing, and an execution which writes. A reduction that no policy input names is a named refusal, not a default. The same sources, policy, and seed produce the same mastered state.

Touches: S1, S2, S3. Supports: U23; P2, P3, P4, P6, P13, P14, P22, P23, P27, P29. Needs: F30 and F31 pledged and delivered.

Companion design: [design/c1541-flux-mastering.md](design/c1541-flux-mastering.md).

## F34 — P64 image-format adapter

The P64 adapter at its representation seam: recognition and evidence, explicit version and structure validation with named refusals, decode of stored half-track pulse positions and strengths into `FluxLayer`, and encode of a mastered medium into a new artifact under its own claim. The adapter owns the container grammar and its capability claim; it owns no selection, reconciliation, or timing policy, and it never decodes GCR, sectors, or files.

Conformance is round trip: a mastered fixture encodes, reopens through the adapter's own decode, and presents the same half-tracks, pulse positions, and strengths. An existing destination path is refused rather than overwritten, and an interrupted write leaves a complete artifact or none.

Touches: S1, S2, S3. Supports: U7, U23; P1, P3, P4, P6, P8, P9, P12, P13, P22, P29. Needs: F30 pledged and delivered.

Companion design: [design/p64-image-adapter.md](design/p64-image-adapter.md).

## F36 — Drive-profile catalog and flux recognition

Establish the P30 seam and its catalog: the profile descriptor, the probe over an opened `FluxLayer`, bounded and comparable confidence carrying the observations that produced it, ranked verdicts where more than one profile claims a capture, caller pinning and override, and a named refusal when nothing claims it. C1541 is the first and only enrolled profile — two drive steps per track, 300 RPM against a 16 MHz reference, the four-zone density map with its sector counts and track boundaries, and GCR synchronization recognized as a run of shortest intervals.

The probe reads flux interval lengths and the patterns they form, and nothing else: it resolves no bit, assembles no byte, names no sector, and validates no checksum. What leaves it is a count, a density, an angle, an absence, and its evidence.

Conformance is the prepared capture set: probing the data surface recovers all four speed zones at their documented track boundaries with their documented sector counts, and refuses the half-step positions on cross-pass reproducibility rather than on a tuned threshold. A capture the catalog cannot claim names the refusal.

Touches: S1, S2, S3. Supports: U23; P3, P4, P12, P22, P23, P27, P29, P30. Needs: F30 pledged and delivered; P30 pledged.

Companion design: [design/drive-profile-recognition.md](design/drive-profile-recognition.md).