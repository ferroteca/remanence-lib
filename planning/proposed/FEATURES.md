# FEATURES (proposed)

> **Status:** drafted 2026-07-30 at the owner's direction: the
> feature cut that would deliver reliquary's at-rest disk access
> (drafted [U3](USE-CASES.md), [U4](USE-CASES.md)) in this library.
> Nothing here binds; a feature is pledged by moving to `pledged/`,
> its number evaporates on delivery, and a split retires the parent
> number. Each feature is cut to one sprint (minutes to hours here).
> Per the standing invariant in [AGENTS.md](../../AGENTS.md), every
> feature lands with its C ABI and Python reflections in the same
> change — there is no separate "bindings" feature. References run
> sideways or down the lifecycle, never up.
>
> Deliberately absent: reliquary's NBD client and its qemu-nbd
> delegation. Drafted P1 forbids the helper process, and the native
> qcow2 driver (F2–F4) replaces both halves. Record the weighed
> alternative in the D-entry when these are adjudicated.
>
> Sources of truth for behavior: reliquary's `at_rest.py` defines
> the semantics being absorbed (partition walk, FAT rules, locking,
> refusals), but under the clean-room doctrine the *implementations*
> here are written from published format documentation — the qcow2
> spec (QEMU `docs/interop/qcow2`), the MBR and FAT layouts — plus
> reliquary's own code, which the owner owns outright. QEMU's block
> driver source is never read.

## F1 — The block device seam

A `Device` abstraction (length, `read_at`, `write_at`, `flush`) that
everything at-rest works over, plus the raw-image implementation:
opened where it lies under drafted P7's ladder — read/write with
writes denied to others (preferred); read-only with writes still
denied to others when our own write permission cannot be had,
`write_at` refused with a named reason; **fail fast when deny-write
cannot be obtained at all** (a running VM holding the image is the
designed refusal). On Windows that is share-mode opens
(`FILE_SHARE_READ`); on POSIX both modes take the exclusive
advisory `flock`/`fcntl` claim, `O_RDWR` vs `O_RDONLY` decided by
file permissions. The claim lives for the life of the handle. The
device reports which mode it holds, so a session can surface it.
Refusals name the path and the reason. Needs: nothing.

## F2 — qcow2 read

A native qcow2 driver, read side, presenting a qcow2 as a `Device`:
header and feature-bit validation, L1/L2 table walk, cluster
mapping, unallocated-reads-as-zero, and compressed-cluster reads
(the payload is DEFLATE — the library's own inflate does the work).
Claimed: qcow2 v2 and v3, standalone images. Named refusals, per
drafted P3: encryption, external backing files, external data
files, and any incompatible feature bit — each refusal naming the
feature. Needs: F1.

## F3 — qcow2 write

The write side of F2 on standalone images: cluster allocation,
refcount table maintenance, L1/L2 updates with copied-flag
handling, file growth, guest-cluster writes through the mapping.
Write access is a distinct open mode (drafted P2). Needs: F2.

## F4 — qcow2 internal snapshots as the commit point

Create, apply, and delete internal snapshots — snapshot table,
L1 copies, refcount adjustments — sufficient to reproduce
reliquary's undo protocol natively: snapshot before the first
write; roll back by apply-and-delete; commit by delete. This is
drafted P2's commit point for qcow2. (Raw images have no snapshot;
their undo story — a staged copy, as reliquary does today — stays
with the caller unless a later feature claims it.) Needs: F3.

## F5 — MBR partition discovery

Partition discovery over any `Device`: the MBR primary entries and
the extended-partition chain walk, partition types pinned value by
value, an unreadable entry refused rather than skipped (skipping
renumbers every volume after it), and a geometry report — the
partitions with their declared types and the per-volume facts —
matching what reliquary's `describe_drives` consumes. Needs: F1.

## F6 — FAT12/FAT16 volume read

FAT volume recognition and read over a partition or a whole device:
BPB plausibility checks, FAT width decided by cluster count
(the format's own rule, never the BPB label), root and
subdirectory walks, 8.3 names, volume label, entry listing with
kinds and sizes, and file copy-out. Claimed: FAT12, FAT16, FAT16B
over F5's partitioning or partitionless media. FAT32 and everything
else: named refusals. Needs: F5.

## F7 — FAT12/FAT16 volume write

The write side of F6: cluster claim and release, file store with
chain construction, directory-record creation with validated 8.3
names and timestamps, and directory creation. Validation precedes
the first mutating write (drafted P6) — an unexpected situation is
discovered while nothing has been written — and deliberate flush
ordering bounds what an interruption from outside can damage.
Needs: F6.

## F8 — At-rest layers join identification

qcow2 becomes a container format the registry knows; partitions and
FAT volumes become identification layers with evidence, reached
through the same `Session::identify` that reports an h8d today —
drafted U4's whole substance. The registry stays data-driven where
the format allows, code-backed where it does not (as HDOS detection
is today). Needs: F2, F5, F6.

## F9 — HDOS file extraction

Read a cataloged HDOS file's contents out through its GRT chain —
the walk `list_hdos_files` already performs for sizes, extended to
carry the sector bytes out to the caller. Serves drafted U2, not
U3; listed here so the vision draft is deliverable end to end.
Needs: nothing beyond the shipped code.
