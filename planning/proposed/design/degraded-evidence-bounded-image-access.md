<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Degraded, evidence-bounded image access

Design for [F27](../FEATURES.md#f27--degraded-evidence-bounded-image-access), serving U3 and proposed P28. This is a proposed destination and delivery cut, not approval to implement it.

## Outcome

The library distinguishes inability to establish *write authority* from inability to establish *any truthful read*. One session-wide assurance state is available immediately: verified, degraded, or refusal. P4's 0–100 recognition confidence remains match evidence and is not repurposed as this access gate. The deterministic verified/degraded threshold is whether all facts required to bound the requested interpretation and operation are available and coherent.

## Truncated explicit FAT

For an explicit raw 1.44 MiB FAT12 interpretation, 1,474,560 bytes are expected. A shorter source reports the declaration, observed size, first unavailable byte, and withheld mutation authority. FAT may proceed only through fully present boot data, FAT copies, root data, geometry, and every visited cluster. A directory can be listable while a file is unavailable; an extracted file is always whole. Missing essential metadata or contradictory metadata that defeats a safe bound is refused.

## Surface and conditions

The initial stable conditions are `source-truncated` and `evidence-conflict`. Each withheld operation also identifies its unavailable range. The cross-language assurance model contains outcome, optional condition, ordered evidence, exact readable ranges, and effective access. Refusal remains the normal error path, carrying the condition and diagnostic.

## Boundaries and acceptance

P2 still forbids mutation during reading. P6 stops an operation at the first unaccountable condition; it does not invalidate independently bounded prior data. P7 host-claim failure still fails open; a successful claim followed by insufficient content becomes explicitly read-only.

The gray rule belongs only inside a catalog adapter's work: determining its type and interpreting its reads or writes. It may report incomplete or conflicting *artifact* evidence when it can still state exact limits. It never covers the surrounding library machinery. A failed claim, cache or private-session-storage read/write, journal operation, allocation, or host I/O halts immediately under P6; no partial catalog result is returned as a substitute.

F27 is accepted only when a truncated explicit floppy notifies before use; wholly present directory and file data read; a crossing file is never clipped or zero-filled; unsafe metadata refuses; every write and commit is denied; and Rust, C, and Python agree on outcome, evidence, condition, and effective mode.

## Deliberately absent

No recovery, filling, repair, partial values, generic numeric safety score, or automatic qcow2/ZIP/HDOS/MBR degradation rule.