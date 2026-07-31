# USE-CASES (proposed)

> **Status:** drafted 2026-07-30 at the owner's direction, from
> demand raised by the downstream embedding consumer that U3 and U4
> serve — raw intake arriving as conversation with the owner, the
> first lane in [README.md](../README.md). Nothing here binds. A new
> entry is pledged by moving to `pledged/` and reaches the root list
> on full delivery; an amendment to an in-force entry is argued here
> and lands on the root list only through the surface-change rule
> ([SURFACES.md](../SURFACES.md)). Numbers come from the one global
> U-sequence and are never reused. The owner's voice governs — these
> drafts are raw material for him to reword, split, or reject.

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

## Amendment — U3 addresses volumes by identity, and the verbs complete

U3 stands as written except for two sentences, and gains three
capabilities its consumer needs before it can replace its own
implementation with this library:

- **Addressing.** The sentence "The library addresses a volume and
  a path within it" becomes: *the library addresses a volume by the
  stable identifier its geometry report gave it, and a path within
  it* — the identity invariant itself is drafted into U4 below,
  where the identifier is born.
- **Opening intent.** "Writing is a separate, explicit mode"
  becomes explicit at the open: *I declare at open whether I need
  writing. A read open never takes write access it does not need; a
  writable open that cannot claim writing fails at the open, before
  the first mutation, never by silently falling back to read-only.*
  (The claim mechanics are the amended P7.)
- **Stat.** Alongside listing, I ask after one path and get its
  entry — or the answer that it does not exist, distinguished from
  failure.
- **Overwrite.** Writing a file that already exists replaces its
  contents, shorter or longer, releasing and reclaiming clusters;
  today's refusal to overwrite is a gap the consumer cannot work
  around.
- **Recursive, idempotent directories.** Creating a directory
  creates missing parents and succeeds when the directory already
  exists.

## Amendment — U4 reports the complete observation

U4's spirit — the disks as they actually are, refusals named,
nothing renumbered — stays. Four things become explicit:

- **Every entry is reported, kind and all.** Each partition row
  carries its kind (primary or logical) beside the pinned type,
  byte offset and length. An entry outside the claim **stays in the
  report**, carrying a structured refusal that states why, instead
  of failing the whole disk or silently vanishing — the volumes
  behind it never renumber. The stricter policy of what to do about
  such a disk belongs to the consumer; the observation is complete
  either way.
- **Blank is an answer.** An all-zero boot sector is a blank disk
  with zero volumes — not an error; the consumer creates blank
  disks and assigns them no letter. A valid unpartitioned
  filesystem is one volume, as today. Non-zero data that is neither
  a supported filesystem nor a partition table is an unreadable
  image, refused by name — kept distinct so corruption or a foreign
  format never becomes an empty answer.
- **Cylinders where they are honest.** Cylinders are reported where
  the disk states them or the format supports an exact derivation,
  and omitted otherwise — never invented.
- **One identity from geometry through file access.** Every
  reported volume carries a stable identifier, and every file verb
  accepts it: for one disk layout, an identifier names exactly the
  same region in every verb that it named in the geometry report.
  The spelling is the library's own and callers treat it as opaque.
  An identifier missing on a later open means the volume is gone,
  even when the new layout happens to hold the same count.

*(Evidence for the identity clause: today the report's volume list
and the file verbs' positional index can disagree — an unreadable
partition's volume is omitted from the report while the index space
still counts its slot — which is exactly the drift U4's "skipping
renumbers every volume after it" warns about.)*
