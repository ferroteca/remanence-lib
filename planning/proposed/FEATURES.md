<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (proposed)

> **Status:** proposed, not pledged. Nothing in this file is approved for
> implementation. Feature numbers record order of issue, not work order or
> priority.

## F20 — Layered partition and volume inspection

Replace the current FAT-shaped `DiskGeometry` snapshot and `geometry()`
verb with one evidence-bearing disk inspection report aligned with the
pledged image, device, partition-schema, volume-composition, filesystem, and
active-layer seams. Preserve every behavior U4 claims today while making the
result capable of guiding later operations without conflating partitions,
volumes, filesystems, physical geometry, or generic containers.

One deep inspection operation returns the complete report. It names the
opened image and block-active device, any recognized partition schema and
every declared partition region, each volume actually composed from the
available regions, and each filesystem actually recognized on a volume.
Typed relationships join those results. Evidence, ambiguity, absence, and
recognized refusals remain attached to the seam which owns them; a failed
filesystem read does not erase its partition row or renumber later volumes.

The report supplies opaque library-owned identities suitable for selecting a
reported region, volume, or filesystem in a later operation. Their public
semantics preserve U4's cross-report stability for an unchanged single-disk
layout while P21's device identity remains scoped to the open composition.
Callers never manufacture identities from partition numbers, offsets, array
positions, labels, or filesystem kinds.

F20 replaces the public shape coherently across S1, S2, and S3. The Rust,
C, and Python presentations expose the same report graph, relationship and
identity semantics, optional facts, and structured issues. The pre-1.0
`DiskGeometry`/geometry surface is deleted rather than retained as a
flattened compatibility view. Existing file verbs move to the new opaque
volume identity without changing U3's file behavior.

F20 depends on pledged F19 for the adapter catalogs, authoritative and active
layer model, provenance, and library-assigned device identity. It composes
the MBR, direct-volume, and FAT adapters already required by F19; it neither
duplicates their recognition rules nor adds another orchestration path.

The feature is deliberately limited to the formats and compositions already
needed to preserve U4: raw and qcow2 block devices, MBR including extended
and logical entries, a partitionless direct volume, and FAT12/FAT16
filesystems. GPT, NTFS, multi-device opening, manual volume recipes, complex
volume managers, Windows namespace reconstruction, partition editing, and
new disk formats remain outside it.

Touches: S1, S2, S3. S4 is unaffected and is removed by F19 before this
dependent feature lands. Supports: U3, U4; P3–P5, P13, P16–P19, P21, P23.
Needs: F19 pledged and delivered.

Companion design:
[design/layered-partition-volume-inspection.md](design/layered-partition-volume-inspection.md).
