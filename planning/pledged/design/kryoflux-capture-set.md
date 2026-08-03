<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# KryoFlux capture-set adapter

> **Status:** pledged, not delivered. This design serves F31 and authorizes no implementation outside that feature.

The adapter consumes a catalog subtree, not a single image-like member. It enumerates the declared member grammar, validates the complete capture set, and records each member's catalog identity in source provenance. The 7z catalog is responsible only for obtaining bounded member sources; it has no KryoFlux knowledge.

Each raw stream decodes into a source-ordered capture run. Flux intervals, asynchronous index observations, control/OOB records, transfer result, and device information retain their separate meanings. Data before the first and after the last index remains part of the run. Channel identity remains a source fact: a capture set does not average, choose, or merge channels.

The initial admitted layout is the prepared paired-channel fixture layout. Its narrow grammar is explicit: an absent, duplicate, malformed, or unrelated member refuses the set with the discovered catalog evidence. Generalizing the capture-set grammar is later adapter work, not a filename heuristic hidden in the fixture test.