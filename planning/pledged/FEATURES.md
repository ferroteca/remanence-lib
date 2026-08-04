<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F27 — Degraded, evidence-bounded image access

Replace all-or-nothing failure for a known deficient image with P28's verified, degraded, and refused outcomes. A degraded open preserves the observed deficiency and permits only reads whose complete interpretation is bounded by available evidence; it is irrevocably read-only. This is not recovery, repair, fabricated fill data, or a weaker way to accept an unsupported format.

The initial vertical slice covers caller-selected raw FAT12/FAT16 images and direct filesystem access. A declared image size larger than the source is reported as truncation with declared and observed bounds. The library may enumerate and extract only entries whose metadata and complete cluster chains lie inside the readable extent; a missing-range operation reports the same condition and location. Invalid or ambiguous metadata that prevents those bounds remains a refusal.

S1 exposes outcome, structured observations, readable bounds, effective access mode, and stable degraded condition; S2 and S3 mirror them. A write-intent open that becomes degraded reports its effective read-only mode and why; P7 host-access failure remains an open failure. Every mutation path, including commit, returns the degraded condition.

Touches: S1, S2, S3. Supports: U3; P2–P7, P10, P28. Needs: P28, pledged with it. The initial FAT slice must fit one sprint; qcow2, archives, HDOS, and later format-specific bounded-read rules are separate features.

Companion design: [design/degraded-evidence-bounded-image-access.md](design/degraded-evidence-bounded-image-access.md).

## F32 — C1541 drive and codec presentation

Materialize a C1541-family hardware bitstream from a `FluxMedium` under declared mechanics and read-channel rules, then materialize the family's encoded GCR bytestream without assigning synchronization, headers, sectors, or files. The P30 drive profile owns this detailed knowledge; neither a capture adapter nor either flux model decides what a drive observes.

Touches: S1, S2, S3. Supports: U7; P3–P5, P13, P15, P22, P23, P27. Needs: the flux-medium foundation, which is delivered, and the pledged P23 amendment, whose hardware-bitstream and encoded-bytestream layers this materializes. It does not promise sector recovery, a generic bitstream API, or every drive family.

Companion design: [design/flux-capture-and-hardware-bitstream.md](design/flux-capture-and-hardware-bitstream.md).

## F41 — VDI differencing chains

Make a VDI differencing image a first-class disk, as a qcow2 with a
backing file already is: the top image opens and the whole chain composes
as one disk, reads resolving through it block by block and writes
allocating copy-on-write into the top image only.

A differencing VDI names its parent by the parent's own identity rather
than by path alone, so resolution is checked rather than assumed: a
candidate parent whose identity does not match what the child declares is
a named refusal, not a silently accepted substitute. That is the
difference from qcow2's backing chain worth stating, because it is the one
place this format gives the library evidence qcow2 does not.

Every failure mode qcow2's chain already names is named here too, in the
same vocabulary and with the same refusal discipline (P3, P6): a missing
parent, a cycle, a chain deeper than the claimed bound, and a parent whose
own version or image type falls outside the claim. Each is a refusal at
the open, never a partial interpretation and never a fallback to reading
the top image alone.

A parent is claimed immutable for the session's life (P7) and is never
modified or flattened. After commit, the chain relationship stands and the
delivering hypervisor's own tooling reads the changed guest bytes — the
same guarantee U6 already makes for qcow2, which is what makes this a
first-class disk rather than a convenience.

Identification is deliberately untouched: a differencing VDI identifies as
the VDI container it is, exactly as U5 says of qcow2. On delivery U6's
wording moves to name the differencing formats the library then claims.

Touches: S1, S2, S3. Supports: U1, U3, U6; P1, P3–P8, P12, P13, P27.
Needs: nothing pledged first — the container, the version gate, and the
block map this composes through are delivered.
