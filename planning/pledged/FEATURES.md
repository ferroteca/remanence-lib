<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F40 — The VDI image adapter

Claim the standalone VDI container as an ordinary block-family image
format: one adapter owning its recognition and evidence, its version gate,
its declared image types, its block map, and its read and write paths. A
VDI opens, identifies, inspects, and reads and writes files exactly as a
raw or qcow2 image does, through the same session and the same evidence
model, because the adapter is the only thing that knows it is a VDI.

The header's declared version is validated before anything else is touched
and a version above the claim fails immediately, naming what it found and
what is supported (P8). The declared image type is an enumerated claim
(P3): the fixed and dynamically allocated types this feature reads and
writes are claimed by name, and every other type the format defines —
differencing among them, which is F41 — is refused by name rather than
attempted. A block map entry marking a block unallocated reads as zeroes
where the format says so, and is never confused with a block that is
allocated and happens to be zero.

Writing follows the delivered disk stack unchanged: reads never alter the
image, writes buffer to the session cache under its declared bound, and
commit is the single durable moment with its recovery journal beneath it
(P2, P9, P27). Allocating a new block in a dynamically allocated image
happens inside commit, never during a read.

The work is what P12 says an ordinary image format costs: the module, its
tests, and one mechanical enrollment in the built-in catalog — plus the
one place the `Disk` surface selects a container by magic, which is a
second selection path and not the catalog. Nothing central learns a VDI
branch, and no shared module acquires a VDI parameter.

On delivery this feature widens a claim the descriptive surfaces state, so
U1's identification journey gains a format and U3's and U4's "qcow2 or
raw" wording moves to name what the library then claims. That is the
delivering feature's ordinary job, as the delivered device tier did for
U1, U3 and U4.

Touches: S1, S2, S3. Supports: U1, U3, U4; P1, P3–P5, P7–P9, P12, P13,
P27. Needs: nothing pledged first.

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
Needs: F40 delivered, for the container, the version gate, and the block
map this composes through.
