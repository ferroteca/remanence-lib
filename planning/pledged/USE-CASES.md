# USE-CASES (pledged)

> **Status:** drafted 2026-07-30 and pledged 2026-07-31, both at
> the owner's direction, from demand raised by the downstream
> embedding consumer that U3 and U4 serve — raw intake arriving as
> conversation with the owner, the first lane in
> [README.md](../README.md). U6 reaches the root list on full
> delivery. Numbers come from the one global U-sequence and are
> never reused.

## U6 — Differencing images are first-class disks

A stopped machine's disk is often a qcow2 whose content lives
partly behind it: a backing file — raw or qcow2, named by a
relative path resolved from the containing image, possibly itself
backed, several levels deep. I open the top image and work exactly
as U3 describes, as if the chain were one disk: reads compose
through the chain, unallocated and zero clusters reading through to
the backing image where the format requires it, compressed clusters
decompressed wherever in the chain they sit. Writes allocate
copy-on-write into the top image only. A backing file is never
modified and the chain is never flattened: after commit, the
delivering hypervisor's own tooling still reports the same backing
relationship and reads the changed guest bytes. A missing backing
file, a cycle, a chain deeper than the claimed bound, encryption,
an external data file — each is a named refusal (P3), never a
partial interpretation.

*(Identification (U5) is deliberately untouched: a differencing
image identifies as the qcow2 container it is. This entry is about
the `Disk` surface reaching through the chain — the write half is
where the consumer's stopped-machine workflow lives today and
cannot move here without it.)*
