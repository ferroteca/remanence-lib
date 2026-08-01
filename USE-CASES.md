# USE-CASES

> **Status: in force.** Every use case on this list is met by the code
> as it exists today — this is an implementation claim, not an
> aspiration, and **a divergence between an entry here and the code is
> a bug**. Numbers come from the one global U-sequence and are never
> reused. Proposed and pledged use cases live under
> [planning/](planning/README.md) until full delivery brings them here.

## U1 — Identify a disk image I know nothing about

I have a file that claims to be a disk image — maybe raw, maybe
sitting inside a `.zip`. I open it and remanence tells me, layer by
layer, what it is: the archive wrapper, the image format, the
physical media it represents, the probable filesystem — each with a
confidence and the evidence behind it, never a bare verdict. When it
doesn't know, it says "unknown" rather than guessing.

## U2 — Browse a vintage volume and pull files out of it

Once an image is identified, I list its catalog — HDOS today —
with the real names, sizes, dates and flags, and I copy a chosen
file's bytes out to the host, without ever booting anything or
mutating the image.

## U3 — Reliquary reads and writes a stopped machine's files

Reliquary, my QEMU automation layer, needs to reach inside a stopped
machine's disk image on the host — qcow2 or raw — and work with the
files in its FAT12/FAT16/FAT16B volumes, whether those volumes sit
behind an MBR or bare on a partitionless image: list a directory's
entries, ask after one path and get its entry — or the answer that
it does not exist, distinguished from failure — copy a file out to
the host, write a file in, create a directory. Writing a file that
already exists replaces its contents, shorter or longer, releasing
and reclaiming clusters; creating a directory creates missing
parents and succeeds when the directory already exists. The library
addresses a volume by the stable identifier its geometry report gave
it, and a path within it — mapping volumes to guest drive letters
stays reliquary's job, standing on U4's volume enumeration. All of
this without booting the guest and without any external helper
process: the library does
the format work itself. Reading never changes the image. Writing is
a separate, explicit mode with a commit point: until I commit,
everything I wrote can be rolled back cleanly.

## U4 — I retrieve a stopped machine's partition and volume information

Reliquary's drive reporting and its guest drive-letter map run on
host-side facts about a stopped machine's disk images, and this
library is where those facts come from. For each disk — qcow2 or
raw — I need: the partition table as it actually is, types pinned
value by value, an unreadable entry refused with the reason rather
than skipped (skipping renumbers every volume behind it); each
volume with its stable identifier, filesystem kind, its label, and
the geometry its boot record states, where it states one; and the
volume count per disk, because letters are assigned one per volume
actually read on the host — a disk holding none takes none, and a
disk that cannot be read answers with the reason it could not be
read, never the symptom. For one disk layout, an identifier names
exactly the same region in every file verb that it named in this
report. Its spelling belongs to the library and callers treat it as
opaque; if it is absent on a later open, that volume is gone rather
than renumbered. All of it from the image alone, booting nothing.

## U5 — qcow2 images are first-class citizens of identification

Opening a qcow2 in the workbench identifies it like anything else:
the qcow2 container layer with its version and virtual size, the
partitions inside the virtual disk, the volumes inside those — the
same session, the same evidence model, the same registry-driven
detection that identifies an h8d today.

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
