<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

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
