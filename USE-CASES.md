# USE-CASES

> **Status: in force.** Every use case on this list is met by the code
> as it exists today — this is an implementation claim, not an
> aspiration, and **a divergence between an entry here and the code is
> a bug**. Numbers come from the one global U-sequence and are never
> reused. Proposed and pledged use cases live under
> [planning/](planning/README.md) until full delivery brings them here.

## U1 — Identify a disk image I know nothing about

I have a file that claims to be a disk image — maybe raw, maybe
sitting inside a `.zip`. I attach it to a session and remanence tells
me, layer by layer, what it is: the archive wrapper, the image format, the
physical media it represents, the probable filesystem — each with a
confidence and the evidence behind it, never a bare verdict. When it
doesn't know, it says "unknown" rather than guessing.

## U2 — Browse a vintage volume and pull files out of it

Once an image is identified, I list its catalog — HDOS today —
with the real names, sizes, dates and flags, and I copy a chosen
file's bytes out to the host, without ever booting anything or
mutating the image.

## U3 — I read and write a stopped machine's files

My QEMU automation layer needs to reach inside a stopped
machine's disk image on the host — qcow2 or raw — and work with the
files in its FAT12/FAT16/FAT16B volumes, whether those volumes sit
behind an MBR or bare on a partitionless image: list a directory's
entries, ask after one path and get its entry — or the answer that
it does not exist, distinguished from failure — copy a file out to
the host, write a file in, create a directory. Writing a file that
already exists replaces its contents, shorter or longer, releasing
and reclaiming clusters; creating a directory creates missing
parents and succeeds when the directory already exists. I attach each
image to a storage device in my session and work through that device;
the library addresses a volume by the opaque identity its inspection
report issued for it, and a path within it — mapping volumes to guest
drive letters stays the caller's job, standing on U4's volume
enumeration. All of
this without booting the guest and without any external helper
process: the library does
the format work itself. Reading never changes the image. Writing is
a separate, explicit mode with a commit point: until I commit,
everything I wrote can be rolled back cleanly.

## U4 — I retrieve a stopped machine's partition and volume information

My automation layer's drive reporting and its guest drive-letter map
run on host-side facts about a stopped machine's disk images, and this
library is where those facts come from. For each disk — qcow2 or
raw — one inspection answers, keeping each fact at the seam that owns
it rather than flattening them into one snapshot.

I need what the disk turned out to be, *stated*: blank, a recognized
partition schema, one unpartitioned volume, or content nothing claims —
not something I reconstruct from which lists came back empty. I need
the partition table as it actually is, types pinned value by value,
each declared region carrying both its raw type value and a reading of
what that value declares, so a type this library will not read still
tells me what it says it is and I keep no partition-type table of my
own. An unreadable entry is refused with the reason rather than
skipped, and it keeps its place: skipping renumbers every volume behind
it. I need each volume that actually composed, and each filesystem
actually recognized on one — its kind, its label, and the geometry its
boot record states where it states one — with a failed filesystem
neither erasing its volume nor renumbering what follows.

I need two counts, not one: how many volumes composed, and how many
carry a filesystem the host read. Letters are assigned one per volume
actually read — a disk holding none takes none — and an unreadable
volume stays in the report rather than vanishing to keep that number
right. A disk that cannot be read answers with the reason it could not
be read, never the symptom.

For one disk layout, an identity names exactly the same region, volume,
or filesystem in every file verb that it named in this report, and on
every later open of an unchanged layout. It belongs to the library and
I treat it as opaque — I never build one from a partition number, an
offset, a label, or a position in a list — and if it is absent on a
later open, that object is gone rather than renumbered. These
identities are scoped to the device holding the image, so two devices
holding like layouts issue like identities and it is the device I
name that tells them apart. All of it from the image alone, booting
nothing.

## U5 — qcow2 images are first-class citizens of identification

Opening a qcow2 in the workbench identifies it like anything else:
the qcow2 container layer with its version and virtual size, the
partitions inside the virtual disk, the volumes inside those — the
same session, the same evidence model, the same adapter-driven
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
the attached medium reaching through the chain — the write half is
where the consumer's stopped-machine workflow lives today and
cannot move here without it.)*

## U23 — I save a KryoFlux capture of a C64 disk as a P64 image

I have a KryoFlux capture of a Commodore 64 floppy: raw stream files,
one per drive-step position, captured from both of the disk's sides
and delivered inside 7z archives — the second being the unrecorded
back of a single-sided disk, which the capture cannot tell me and the
drive family can. It is capture evidence, not a disk image. Each
stream holds several recorded revolutions, flux before the first index
and after the last, index and control/OOB records beside the flux, and
a transfer result — and nothing in it says which revolution "the" disk
was, or which channel to believe. I want a P64 out of it: one file,
addressed by 1541 half-track, holding timed pulses with strength. I am
asking for a transformation, not a reading of the capture, and I am
told exactly what it will do and exactly what it cannot carry
**before** it writes anything.

Opening the set takes the P7 claim on every member artifact for the
operation's lifetime and reads nothing else. Inspecting it reports the
set as the capture-set adapter recognized it — members and their
catalog identities, sides, source track positions, capture runs,
observations, markers, transfer results, and issues — so I name a side
and a policy by an identity the library already reported, never by an
index I invented. The C1541 profile declares that the family records
one surface, so naming the side confirms a declared fact rather than
choosing between two beliefs about one surface. Planning computes the
whole transformation and writes nothing: it reports the mastered
medium's shape, the provenance every part of it will carry, and the
complete declared-loss account. Writing the artifact is the only step
that touches the filesystem; it creates the destination under its own
claim, and an existing destination is a named refusal rather than an
overwrite.

Two owners, and neither infers the other's answer. The **C1541
mastering profile** owns the physical reduction: which side supplies
evidence; which observation of a source position is used and how
several are reconciled; how the set's source drive-step positions map
onto 1541 half-tracks; how each observation's exact timebase projects
into the destination's rotation-relative timebase, which for a 1541 is
the drive's 16 MHz reference clock across one 300 RPM rotation; and
how disagreement, weakness, and absence across observations become
pulse strength. Every one of those is a named policy input, and a
reduction no policy names is a refusal, not a default. The **P64
image-format adapter** owns its grammar and its capability claim: what
the container can hold, the version it claims, how a mastered medium
encodes into it, and what it refuses by name. Each states its own
crossing in its own terms, so I read two accounts in sequence rather
than one assembled by whichever of them ran last.

P64 cannot carry a KryoFlux capture. That is not a defect of either
format, and it is not something I should discover from a smaller file.
Before the write, the reduction enumerates what it drops in the
source's own terms — the unselected side; the observations of each
position not selected; flux recorded before the first index and after
the last; marker channels and control/OOB records with no P64
expression; retained foreign records, capture metadata, and transfer
results; and any timing resolution the destination's timebase cannot
express — and the container enumerates what it cannot express of what
survives: the declared policy itself, each half-track's provenance,
the located origin, the seam, and the medium's own statement that it
was derived at all. A count is not an account, and loss reported after
the fact would not do.

The saved image says what it is. Its pulses carry
selected-and-projected provenance, not recovered-evidence provenance,
and nothing in it is presented as an observation of the original
recording that was not one. The same capture set, the same policy and
the same seed produce the same mastered medium and — the P64 encoding
being deterministic — the same destination bytes.

The journey runs on the prepared Pinball Construction Set disk-one
capture set: both sides, 84 stream members each, opened through the 7z
catalog and recognized as one capture set. I inspect it, name a side
and a selection policy, read the declared-loss account, and write the
P64; reopening the result through the adapter's own decode presents
the same half-tracks, at the same angles, with the same strengths. An
incomplete, duplicate, or contradictory capture set is refused before
mastering begins. Past that: a source position no declared half-track
map covers; a position that holds no observation the selection policy
names; a position whose content its neighbour also holds, until I
declare which it is; a timebase the destination cannot express; a
mastered medium the P64 claim cannot encode; and an existing
destination path. Each names the rule it broke and leaves no file
behind.

*(This entry claims that the declared reduction is performed
faithfully, reproducibly, and with its loss named. It does not claim
that any particular protected title loads in an emulator from the
result: whether protection survives is a property of the capture and
the chosen policy, and the library reports what it did rather than
promising an outcome it cannot see. Nothing here descends below flux
or interprets what the pulses mean — no GCR, no sectors, no
filesystem, no files — the sources are never edited or consumed, and
no public flux, pulse, or capture-run iterator is offered: the
transformation is the surface and the evidence stays behind it.
Consuming the image is a separate journey that meets this one at the
file.)*
