<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F29 — Archive catalog foundation: ZIP and 7z

Establish the archive-catalog seam and isolate each archive grammar behind its own adapter. `ZipCatalog` owns ZIP parsing and entry sources; `SevenZipCatalog` owns 7z parsing, its supported compression methods, entry indexing, bounded extraction, and named refusals. A catalog reports ordered entries and produces bounded entry sources; it does not identify disk images or interpret media.

The existing ZIP resolver becomes the ZIP catalog adapter. 7z support is a library capability, not test-only fixture plumbing. Unsupported 7z variants are refused by name rather than delegated to an external program or silently unpacked whole.

Touches: S1, S2, S3. Supports: P3, P4, P12, P13, P19, P27. Needs: none.

## F30 — Private FluxLayer v1 foundation

Establish the private, durable flux media model and its bounded session backing: physical track keys, capture runs, circular observations, exact timebases, parallel marker channels, ordered evidence, provenance, and sparse section-addressable storage. It is the common target of admitted capture adapters, not a public flux iterator, interchange format, drive profile, or sector decoder.

Every source fact maps to a named layer fact, ordered foreign record, or a named refusal. Several observed revolutions remain distinct evidence: the model neither averages timings nor selects a cleanest pass. Unit tests build small synthetic layers and verify ordering, circular bounds, marker separation, and bounded reload without a drive interpretation.

Touches: none. Supports: P3, P4, P13, P22, P23, P27. Needs: P23 pledged.

Companion design: [design/flux-layer-foundation.md](design/flux-layer-foundation.md).

## F31 — KryoFlux capture-set catalog adapter

Recognize a declared KryoFlux capture set assembled from a catalog subtree and materialize its members collectively into `FluxLayer`. The adapter owns the capture-set grammar, member identity, track and side identity, capture-channel identity, stream ordering, index and control/OOB records, transfer results, and source provenance. An archive member is never silently treated as a disk when the logical capture requires the complete set.

The initial conformance fixture is the prepared paired-channel capture set. Tests open it through `SevenZipCatalog`, verify that all selected members form one capture set, and assert preservation of runs, markers, pre/post-index data, and separate channels. An incomplete, duplicate, or contradictory set is a named refusal. The adapter neither merges channels into an ideal disk nor materializes a bitstream, sector, or filesystem.

Touches: S1, S2, S3. Supports: U7; P3, P4, P12, P13, P15, P22, P23, P27. Needs: F29 and F30 pledged and delivered.

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