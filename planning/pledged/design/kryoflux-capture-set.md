<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# KryoFlux capture-set adapter

> **Status:** delivered; the feature that carried it has been struck and its
> handle retired. This remains the written statement of the adapter —
> implemented in `crates/remanence/src/kryoflux.rs`, reached through
> `CaptureSet` on all three application surfaces. It specifies the model
> and the grammar rather than the surface, and no normative specification
> of S1–S3 has shipped — the defining code is still the norm — so it stays
> here rather than moving out (D11).
>
> The stream reading it rests on is recorded here because the code is
> where it is enforced and this is where it is argued. An index record
> names the stream position of the flux cell that was being measured when
> the pulse arrived and how many sample ticks into it the pulse arrived,
> so the index sits that far past the transition the cell *began* at. That
> reading was settled against the fixture's own independent index counter,
> which is what the alternatives disagree with. The device's sample clock
> is likewise the adapter's own exact rational and never the stream's
> rounded decimal, which is retained as the declared fact it is and
> checked against the claim only to the precision the stream itself
> stated.

The adapter consumes a catalog subtree, not a single image-like member. It enumerates the declared member grammar, validates the complete capture set, and records each member's catalog identity in source provenance. The 7z catalog is responsible only for obtaining bounded member sources; it has no KryoFlux knowledge. Members sharing one coded folder are obtained together, so a set costs its archive one decode rather than one per member.

Each raw stream decodes into a source-ordered capture run. Flux intervals, asynchronous index observations, control/OOB records, transfer result, and device information retain their separate meanings. Data before the first and after the last index remains part of the run. Channel identity remains a source fact: a capture set does not average, choose, or merge channels.

The initial admitted layout is the prepared two-sided fixture layout. Its narrow grammar is explicit: an absent, duplicate, malformed, or unrelated member refuses the set with the discovered catalog evidence. Generalizing the capture-set grammar is later adapter work, not a filename heuristic hidden in the fixture test.