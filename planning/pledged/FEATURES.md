<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F38 — The layered disk inspection report

Add one evidence-bearing disk inspection operation whose result keeps the
pledged image, device, partition-schema, volume-composition, filesystem, and
active-layer seams distinct. The report names the opened image and
block-active device, states what the device's leading structure turned out to
be, and reports any recognized partition schema, every declared partition
region, each volume actually composed, and each filesystem actually
recognized on a volume. Typed relationships join those records, and evidence,
ambiguity, absence, and recognized refusals stay attached to the seam which
owns them.

The leading structure is one classified outcome the report *states* — blank; a
recognized partition schema, whether or not any volume composed from it; a
direct unpartitioned volume; or nonblank content no adapter claims — not a
flag beside two lists a consumer has to reconstruct the judgement from. The
last of those arms is a deliberate behavior change: unclaimed nonblank content
is a refusal from `discover` today, and becomes a reported outcome here.

Every declared region reports both its raw type value and a reading of what
that value declares, present whether or not the type is inside this feature's
read claim, and fit to quote in a refusal a user will read. The report supplies
opaque library-owned identities for its regions, volumes, and filesystems; a
public identity is a deterministic function of the layout's structure, never a
report index or a session counter, because U4's cross-open stability and P21's
opacity together admit nothing else.

F38 is additive. `DiskGeometry` and `geometry()` remain until F39 removes them,
so every presentation carries both models for exactly as long as the two
features are apart, and no presentation ever lags another. The Rust, C, and
Python surfaces expose the same report graph, relationship and identity
semantics, optional facts, and structured issues, and land together.

Scope is the formats U4 already needs: raw and qcow2 block devices, MBR
including extended and logical entries, a partitionless direct volume, and
FAT12/FAT16. F38 adds no format recognition and no orchestration path beside
the in-force adapter architecture's. It does **not** make FAT a P19
file-container provider — the delivered file-container contract holds
filesystem listings at their present shape until a feature presents them
through that seam, and that feature is neither of these.

Touches: S1, S2, S3. Supports: U4; P3–P5, P13, P16–P18, P21, P23, P27. Needs:
nothing pledged first; the adapter architecture it composes is in force. P19
is deliberately absent from that list: this feature reports a recognized
filesystem, and does not present one through the file-access seam.

Companion design:
[design/layered-disk-inspection-report.md](design/layered-disk-inspection-report.md).

## F39 — Opaque volume selection, and the end of the geometry surface

Retire the FAT-shaped disk surface F38 replaced. Volume-scoped file verbs stop
accepting a caller-parsed volume string and take the opaque volume identity the
inspection report issues; `DiskGeometry`, `geometry()`, and the flattened
partition and volume records are deleted from Rust, C, and Python together,
with the generated header committed. No compatibility alias or flattened view
of the old model survives.

The two surfaces cannot be separated by presentation: the C binding imports the
core's concrete geometry types directly, so deleting them is one change across
all three or it is not a change at all. What makes this feature separable from
F38 is order, not layering — F38 adds, F39 removes.

U3's file behavior is unchanged apart from how a volume is named. U4's wording
and every descriptive surface — examples, README, architecture, usage
documentation — move to the layered expression of the same stopped-machine,
stability, no-skipping, and known-cause guarantees, which is what arms U4
against the delivered surface.

Touches: S1, S2, S3. Supports: U3, U4; P5, P21. Needs: F38 delivered, for the
report and the identities this selects by.
