# FEATURES (pledged)

> **Status:** drafted 2026-07-30 and pledged 2026-07-31, both at
> the owner's direction: the feature cut that would let the
> downstream embedding consumer that U3 and U4 serve delete its own
> disk-access implementation and stand wholly on this library,
> pinning one exact prerelease at a time. P10 is now in force at
> root [ARCHITECTURE.md](../../ARCHITECTURE.md); the remaining demand
> is pledged beside this file — [USE-CASES.md](USE-CASES.md) (U6 and
> the U3/U4 amendments) and [ARCHITECTURE.md](ARCHITECTURE.md) (P9
> and the P7 amendment).
> Every feature here is owed by the project, with no promise of
> order or time; a feature's number evaporates on delivery, and a
> split retires the parent number. Each feature is cut to one
> sprint. Per the standing
> invariant in [AGENTS.md](../../AGENTS.md), every feature lands
> with its C ABI and Python reflections in the same change — there
> is no separate "bindings" feature — and pre-1.0 the old spelling
> of a changed surface is deleted, never aliased. References run
> sideways or down the lifecycle, never up.
>
> **Most of the demand is already in force.** The shipped U3/U4
> stack covers the P7 claim with declared access intent at open,
> MBR discovery with the extended chain and pinned types, the
> complete geometry report (blank an answer distinct from
> unreadable, every declared row kept with a structured issue where
> it cannot be read, partition kinds, cylinders only where exact),
> FAT12/FAT16/FAT16B read and write with both FAT copies
> maintained, path semantics (`/` or `\`, case-insensitive DOS
> names, `.` ignored, `..` refused), the commit-point overlay with
> rollback, and the native qcow2 v2/v3 driver including
> compressed-cluster reads. The cut below is the delta, in
> dependency order — nothing about it is a schedule.
>
> Deliberately absent, named so their absence is a decision:
> **streaming file contents** (whole-`bytes` moves are what the
> consumer's delivered workflow needs; a streaming surface is not
> invented ahead of demand); **a byte-device presentation** of the
> open image (the semantic disk surface is the product; exposing
> bytes beside it would invite a second partition/FAT
> implementation above the library); **identification of backing
> chains** (U5 untouched, per U6's note).

## F14 — Stat, overwrite, recursive directories

The amended U3's verb completion. `stat` answers one path with its
entry or with an is-absent answer distinguished from failure.
`write_file` overwrites an existing file, shorter or longer,
releasing and reclaiming clusters, both FAT copies kept consistent.
`make_directory` creates missing parents and succeeds when the
directory already exists. Validation still precedes the first
mutating write (P6).

## F15 — qcow2 backing-chain read

U6's read half. The backing-file header fields are parsed; a
relative backing path resolves from the containing image; the chain
opens to a bounded maximum depth with cycle detection; every member
is claimed per the amended P7 — the top image per the declared
intent, every backing file immutable. Reads compose through the
chain: unallocated and zero clusters read through to the backing
image where v2/v3 semantics require it; compressed clusters
decompress wherever in the chain they sit. Raw and qcow2 backing
files are claimed. Named refusals, per P3 and P8: missing backing
file, cycle, depth beyond the claim, encryption, external data
files, unknown feature bits anywhere in the chain.

## F16 — Copy-on-write into the top image

U6's write half. Writing through a chain allocates into the top
image only; a backing file is never written and the chain is never
flattened; commit preserves the backing relationship. Evidence
includes hypervisor-authored fixtures: after commit, the
hypervisor's own tooling still reports the same backing file and
reads the changed guest bytes — the library's claim is only as wide
as what those fixtures exercise on the delivered host. Needs: F15.

## F17 — Durable commit

P9's mechanism. The overlay's write-through gains a durability
boundary — a durable undo journal or equivalent, beneath the commit
point D2 settled — such that interruption at any point leaves
state the next open reconciles before exposing the disk: wholly the
old image or wholly the committed new one. Recovery artifacts are
private transient state, no user-owned path and no cleanup verb.
Covers raw, standalone qcow2, and chains alike. Needs: P9 pledged;
composes with F16 where chains exist but does not wait for it.

## F18 — The crash harness

P9's evidence. A fault-injection harness terminates a separate
process after each durability boundary in commit and asserts the
next open reconciles to wholly-old or wholly-new — run for raw,
standalone qcow2, and backing-chain images. In-process rollback
tests are explicitly not evidence here. Needs: F17.
