<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# One claim, one medium surface

> **Status:** delivered; the feature that carried it has been struck and
> its handle retired. This remains the written statement of the seam —
> the merged medium surface in `crates/remanence/src/disk.rs`, opened
> through `crates/remanence/src/source.rs` over the claimed range device
> in `crates/remanence/src/device.rs`. It serves U1, U3–U6 and U23
> alongside in-force P1, P2, P7, P9, P12, P13 and P27.
>
> One thing it lists is narrower in the delivery than the prose below
> once suggested, and deliberately: an archive entry is not writable, for
> the reason the acceptance list now states.

## The defect being fixed

`Session` and `Disk` are unrelated types over the same file. `Session`
identifies and reads bytes; `Disk` inspects, performs file verbs, commits
and rolls back. Neither mentions the other, and a caller wanting both on one
image has no way to get them — **each takes its own P7 claim, and the second
open loses**. Two entry points that structurally cannot both be used on one
artifact is a defect in the surface, not merely duplication, and it is what
stands between the library and any device tier above it (P32).

The merge is therefore forced by P7, independently of P32. It would be owed
even if no device tier were ever pledged.

## Neither path is a superset

This is the whole difficulty. The two opens grew different capabilities:

| | identification path (`ImageSource`) | disk path (`Disk`) |
|---|---|---|
| Claim | `open_locked`, degrades to read-only | `open_declared`, refuses by name |
| Backing | claimed file **or spool** — so `archive[/entry]` works | the claimed file only |
| Cache | session cache + predictive reader | session cache, write-buffering |
| Journal | none | recovery sidecar (P9) |
| Adapters | probes only, from a bounded prefix | `open_disk`, full format drivers |
| Commit | none | capture, apply, rollback |

An archive entry can be identified but never inspected; a qcow2 chain can be
committed but its container never reached through an archive. **No capability
on either side may be dropped to make them meet.**

## The decision: generalize the adapter seam

Every image adapter opens through
`fn open_disk(&self, file: FileDevice, path: &Path)` — a *concrete*
`FileDevice`, which is a whole claimed file and nothing else. That concrete
type is precisely why the disk path cannot serve an archive entry: an entry
is a *range* inside a claimed file, or a range inside a spool, and
`FileDevice` can express neither.

So the seam takes a **claimed medium device** instead: the claimed handle
(file or spool), a base offset and length so a range is addressable, the
declared access mode, and the capture machinery commit already needs. Every
adapter moves across. `FileRangeDevice` already exists for the read half,
which is evidence the shape is right rather than a new invention.

The rejected alternative was to keep `FileDevice` and spool every archive
entry out to a private file before opening it. It is less code and it is
wrong twice over: it makes the disk path pay a full extraction for an entry
that is stored uncompressed and could be read in place, and it puts the
archive special case at the call site, which is exactly where P12 says
format knowledge must not live. The seam is the honest place for the
generality, and P12's adapters are the things that already know what they
are opening.

Changing this seam is a P12 contract change and is why F43 exists as its own
feature rather than as preparation inside another.

## Access intent: the disk path's rule wins

One surface cannot hold both claim rules, so the merge chooses. **Intent is
declared at open**: a read open takes no stronger claim than it needs, a
write open that cannot secure its claim is refused by name, and the mode
holds for the medium's whole life. The identification path's quiet degrade
to read-only does not survive.

That direction is not arbitrary. In-force P7 already requires the claim to
be declared at open and never obtained by silent fallback, and root
ARCHITECTURE.md states it of the disk stack today. The identification path's
degrade is the older behavior, and it is the one that disagrees with the
principle — so the merge is also an alignment, not a trade.

## What must not be lost

The merge is complete only when all of these hold, and the first two are
combinations that were **impossible before** — they are what the merge
newly admits, and therefore what proves it:

- an archive entry (ZIP and 7z) identifies **and** inspects and reads
  files. It is **not** writable, and that is a decision rather than an
  omission: a write would have to be encoded back into the archive's own
  grammar before it meant anything, and no adapter claims that (P13), so
  a write open on an entry is refused by name rather than degraded;
- a qcow2 backing chain composes, commits and rolls back, reached the same
  way as any other medium;
- an uncompressed archive entry is still read in place rather than extracted
  whole (P27);
- the declared cache bound and the predictive reader still apply;
- an image past the HDOS bound is still refused by size, never loaded;
- the recovery journal still reconciles an interrupted commit at open.

## Deliberately absent

- Any device tier. F44 adds it; this feature delivers the one medium
  surface a device will hold.
- Any change to the report's content, its seams, or its opaque region,
  volume and filesystem identities.
- Any change to which formats or filesystems are claimed.
- A compatibility alias for either retired surface. Pre-1.0, the old shape
  is deleted.
