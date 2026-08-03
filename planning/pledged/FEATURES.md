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