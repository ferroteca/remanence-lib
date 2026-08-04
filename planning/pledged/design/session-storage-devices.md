<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Session storage devices

> **Status:** the device tier it specifies is delivered; the feature that
> carried it has been struck and its handle retired. This remains the
> written statement of the tier — `crates/remanence/src/machine.rs` and
> `crates/remanence/src/storage_device.rs`.
>
> **P32 is not thereby armed.** What is delivered is the tier: a session
> holding family-typed devices, attachment identities, machine-down
> attach and detach. Region enumeration for families beyond block, file
> access moving onto a region, and a device capability presenting
> `Hardware<C>` remain unbuilt, and the sections below describing them
> are still the owed shape rather than a claim about the code.

## No machine, just the session

An earlier shape of this design put a `Machine` object above `Session`, to
give drive-letter-style reasoning (U22) an explicit scope narrower than "the
whole library" and wider than "one disk." That scope already exists: it is
the session. A session is the sole machine. Nothing here groups several
sessions into a machine, and nothing needs to — U22's automation caller
already opens one session per stopped machine it automates.

## Device, not disk

Today a session opens one `Disk`. This design adds a tier above it: the
**storage device**, a durable slot a session holds, distinct from whatever
medium is currently attached to it.

```text
Session
  storage devices (dynamic set)
    StorageDevice  — id, family, slot, attached medium (0 or 1)
      family capability — what the device lets a caller do
```

A device is family-typed: a 1541 drive, an HDD, an optical drive, a tape
drive are different families, matching the families P14 and P15 already
separate at the media and hardware-emulation tiers. The device is the slot;
the medium (P14's independent recorded state) is what currently occupies it.
Ejecting a floppy and inserting another leaves the `cbm-floppy0` device where
it was — only its attached medium changed.

## The device set is dynamic, and attach/detach is machine-down

A session's devices are not fixed at open. A caller attaches and detaches
devices — not just media — over the session's life: add a floppy drive mid
session, remove an HDD entirely.

That reconfiguration is a **machine-down operation**. It is unavailable
while a P15 hardware-emulation composition (`Hardware<C>`) is open over the
device being reconfigured, matching real hardware and every VM: devices are
added, removed, and rewired between runs, never while the emulated processor
is executing against them.

Machine-down status is also what makes id reuse safe (below): nothing is
holding a live reference to a detached device's old occupant, because
nothing is running.

## Identity: an attachment identity, which P21 already carved out

In-force P21 anticipated this precisely, and supplies the vocabulary:

> An attachment identity such as `hdd0` is distinct from device identity. A
> caller supplies placement only when placement changes semantics and cannot
> be inferred. This principle adds neither multi-device opening nor
> multi-device volume composition; those capabilities require their own
> proposal.

Three things follow. **"Device identity" is already taken** — P21 and D6
reserve it for the opaque value the library assigns an addressed virtual
device, and a storage device's `hdd0` is an *attachment identity*, a
different thing at a different tier. Calling the latter a device identity
would put two opposite disciplines behind one phrase. **The condition for
caller-supplied placement is met**: which slot a medium occupies is exactly
what a drive-letter rule reasons over, and no evidence in any image records
it. And **this is the proposal P21 named** — multi-device opening was routed
to its own proposal rather than refused, and P32 is it.

The project holds volume and region identity to a strict opaque-handle
discipline — U4 and U22 are explicit that the caller never reconstructs an
identity from position, order, or a guessed name, because those identities
are evidence read from a disk. An attachment identity is the opposite, and
correctly so: which devices exist and what family they are is machine
configuration the caller supplies, the same class of fact U22 already
carries as "machine facts" (medium, slot, attachment order) rather than
something read as evidence.

So an attachment identity is composed, not opaque: `hdd0`, `floppy0`,
`cbm-floppy0` —
family plus index, the naming a caller already expects from a VM, and from
bare-metal device enumeration, where nobody chooses a BIOS-level name for an
attached device either. The caller chooses the **slot** an attach lands in
(attach explicitly as `hdd1`, leaving `hdd0` free) but never an arbitrary
name. An attach that does not name a slot takes the lowest free index for
that family.

Because attach/detach is machine-down, an index freed by detaching may be
reused by a later same-family attach that does not name a slot — swap in a
new HDD and it can become `hdd0` again. This is not the renumbering U4
refuses for evidence-bearing lists (an unreadable partition keeps its place
so nothing behind it shifts); it is caller-owned configuration with no live
state depending on the old occupant, reconfigured while the machine is down.

## Family compatibility is enforced at attach

A device only accepts a medium of its own family. Attaching a `.vmdk` to
`floppy0`, or 1541 flux to `hdd0`, is refused by name. This is the
device-tier expression of a rule P14 already states at the media tier — a
family owns its media representation — applied where a caller actually
performs the attach.

## Example: heterogeneous devices, one composer

A session with a `.vmdk` attached (auto-provisioned as `hdd0`, since the
caller named no slot) and a C64 flux capture attached (auto-provisioned as
`cbm-floppy0`) illustrates the shape end to end. `hdd0` carries a DOS-visible
boot partition; `cbm-floppy0` carries a 1541 disk image. The U22/F26
drive-letter composer sees the full device set but reasons only over the
families its claimed rule understands: it assigns `C:` (and further letters)
from `hdd0`'s recognized DOS partitions, and produces no letter for
`cbm-floppy0` — not an error, not an omission, simply outside every DOS
assignment rule the composer claims. Nothing about `StorageDevice` needed to
know that in advance; the composer's own family scope is what limits it.

## Storage device is a marker, not a functional interface

The only things every `StorageDevice` shares, regardless of family, are its
id, its family, its slot, and whichever medium is currently attached. There
is no shared read/write or inspection method set across families — the same
position P15 already takes about the hardware-emulation contract ("there is
no universal tick frequency... no universal register operation enum"),
restated one tier higher, at the device rather than at the timed-causality
contract beneath a device.

What a caller can *do* with a device comes from a family-typed capability
obtained from it, not from `StorageDevice` itself:

- A modern block-addressed family (HDD, optical) offers direct block-level
  I/O and region enumeration (below).
- A 1541 offers head control and the read/write electronics directly. Its
  family capability either is, or wraps, the P15 `Hardware<C>` contract for
  the 1541 family — not a second, parallel raw-media interface alongside it.
  A device that wants live, timed-causal hardware behavior and a device that
  wants to read the same medium's flux evidence reach it through the one
  capability, not two.

Identification and inspection (U1's layer-by-layer report; P16 partitions
and volumes; P19 file containers) address the **attached medium** directly,
independent of a device's family-specific operate capability. A 1541's flux
evidence is readable without issuing any head-control operation, exactly as
a stopped HDD's partitions are readable without issuing block I/O — neither
requires opening the device's functional capability at all.

## A device answers for regions, not for files

An earlier turn of this design gave the device a `list_files()` shortcut,
available wherever composition beneath it happened to be unambiguous. That
is withdrawn. Optical is what killed it: a mixed-mode disc has a data track,
audio tracks, and possibly further data tracks, each needing to stand apart
from neighbours it is not compatible with, and no single file interpretation
spans them. A shortcut that works for a floppy and not for a CD is a
shortcut whose safety has to be tested before every use, which is worse than
not having one.

So the root of a device enumerates **regions**, and file access is a
capability of a region. Uniformly — a partitioned HDD, a bare floppy, and a
mixed-mode CD all answer the same first question, and nothing has to decide
whether a shortcut applies.

`region` is the project's existing word, not a new one. In-force U4 asks for
"each declared region carrying both its raw type value and a reading of what
that value declares", and the delivered layered inspection report already
states a leading structure, enumerates declared regions, and issues opaque
region identities. `list_partitions` would be wrong twice: proposed P24 says
explicitly that the optical seam "does not make tracks into partitions", and
the delivered report already generalized past partitions to declared regions.

### Regions are family-declared, and there may be only one

Each family declares what its regions are:

| Family | Regions |
|---|---|
| Partitioned block device | partition-table entries (P16) |
| Unpartitioned floppy | one direct region (P16's direct volume) |
| Mixed-mode CD | tracks and sessions, each with its declared mode |
| DVD / Blu-ray | essentially one — the UDF volume space |

The CD/DVD split is the point rather than an inconvenience. A CD carries
audio *outside* any filesystem, so it decomposes into many regions; a DVD
carries audio as files (`AUDIO_TS`, `VIDEO_TS`) inside one UDF volume, so it
decomposes into one. Proposed P24 already refuses to impose one family's
schema on DVD, Blu-ray and later media, and this is that refusal expressed
in the enumeration. The caller asks the same question either way.

### A region is a positive claim; an opaque extent is the absence of one

These are different things and the design turned on telling them apart:

- A **region** is a positive claim: some seam declares this extent and
  states its kind. A mixed-mode disc's audio tracks are regions. Something
  knows exactly what they are — an audio program with indexes and gaps.
- An **opaque extent** is the absence of a claim: part of one
  interpretation's floor that no reading accounts for. A protected 1541
  disk's tracks 36–40 and half-tracks are opaque. Nothing knows what they
  are.

A region carrying no file interpretation is therefore an ordinary answer,
not a refusal — in-force P19 already holds that Remanence "neither calls
valid non-file data empty nor manufactures pseudo-files to force it through
P19." An opaque extent is governed instead by the pledged P19 scope-of-claim
amendment, which itemizes it without a name and states its hook in the
floor's own addressing.

### Not every sub-artifact is a region

An El Torito boot image is not a region. Its Boot Record Volume Descriptor
sits at sector 17, pointing at a boot catalog, which points at an extent
*inside* the data track — usually also a named file in the ISO, though it
need not be. That is pledged P25's recursion, whose text already names "an
optical boot-catalog extent" among the artifact mappings it governs.

So a disc holding a boot image, a filesystem, audio, and a further
filesystem is really three regions with an artifact mapping inside the
first. Keeping the two mechanisms apart is what stops region enumeration
from becoming a dumping ground for every interesting extent: a region is a
top-level division of the medium declared by its leading structure, and an
artifact mapping is an evidence-bearing edge out of a recognized structure.

### A directory is another file container

Where a region's filesystem is hierarchical, `list_files()` on that region
answers with the root directory, and **each directory is itself a file
container**. Hierarchy is modeled by containers nesting, not by a flat
namespace the caller navigates with path strings.

The **inspection report shows the root directory and stops.** It does not
walk the tree — that keeps it bounded under P27, and the deep contents of a
standard filesystem are not where a disk report earns its keep. What the
report is actually for is the part the interpretation could *not* explain.
Navigation into subdirectories remains an ordinary API capability, needed by
U3's path reads and directory creation; it is just not what the report
renders.

This keeps one interface at every level of a tree: whatever a caller can do
with the root, it can do with any directory in it, without a second
directory-shaped type existing beside the container type. A flat container
— T64, a C64 KERNAL tape derivation, an archive with no directory entries —
is simply one with no directory members, not a different kind of thing.

Path addressing does not go away, and the two are not in competition. U3
asks after a path and expects an entry or a distinguished "does not exist";
U22 keeps guest-address parsing (`A:\OUT\X.TXT` → letter plus segments) on
the caller's side. Nested containers are how the tree is *shaped*; a path
lookup is a convenience over that shape, and P19's existing refusal to
flatten or guess an ambiguous path is unaffected.

## Visualizing a floppy whose backing is not all standard

A protected 1541 or Apple disk is one interpretation with an opaque
remainder, and the scope-of-claim account *is* the visualization: a
track/sector map classified four ways. For a protected 1541 disk, track 18
is claimed structure, named files are data hooks, the BAM states what it
claims free, and tracks 36–40 and any half-tracks are opaque.

**Why an opaque extent is opaque is worth carrying**, and the historical
protection mechanisms give the taxonomy rather than it being invented:

- addressable but outside the interpretation's declared extent — 1541
  tracks 36–40 and half-tracks, PC tracks past 40/80;
- inside the extent but unreadable by the standard channel — Apple's custom
  address/data prologues replacing D5 AA 96 / D5 AA AD, deliberate GCR
  errors, bad CRCs;
- readable but unclaimed — allocated in the map with nothing naming it;
- structurally impossible under the format's own rules — duplicate or
  out-of-range sector IDs, a sector size the format does not admit;
- non-deterministic — weak bits, where opacity meets P22 evidence. This one
  is not merely unexplained but *unstable*, and the account says so rather
  than flattening it to opaque.

### What the historical systems could not do

None of CBM DOS, Apple DOS 3.3, or MS-DOS reported opaque regions, because
none of them could: each *was* the filesystem rather than a view over an
artifact, with no floor-addressing vocabulary for "this extent exists and I
cannot explain it." Tracks beyond the standard count were outside their
universe entirely; unreadable tracks produced an I/O error and no partial
answer; a self-referential Apple catalog chain could make `CATALOG` loop
forever rather than admit anything.

They are a cautionary model, not a template — but one thing transfers
exactly. The user-visible tell on a protected 1541 disk was the
**blocks-free count**: protection allocated blocks in the BAM with nothing
in the directory naming them, so the number did not add up, and experienced
users read the discrepancy as "there is something here the directory is not
telling me." FAT's bad-cluster marker (`0xFFF7`) is the closest any of them
came to naming it, and it still says "do not use this" rather than "I do not
know what this is."

That is precisely why the pledged amendment records free space as "that
metadata's claim, never as a verdict that the extent is empty, disposable,
or safe to reuse." The gap between the allocation map's claim and the truth
is where the whole protection industry lived. The amendment is already
right here, and the history is evidence for it.

## What is still open

**How the P19 scope-of-claim amendment's account distributes over nested
containers.** That pledged amendment obliges a file-bearing view to account
for every addressable unit of its floor in four classes — data hook, claimed
structures, metadata-claimed free space, opaque region — stated in the
floor's own addressing. Nested directory containers are all views over one
shared floor, so it is not settled whether the account belongs to the root
view alone, to each nested container for the portion it claims, or to the
floor-owning provider beneath all of them.

Two arguments now bear on it, and both point the same way. Two of the four
classes — allocation metadata and the space it claims free — are global to
an interpretation and not divisible per directory, which rules the second
out. And region enumeration supplies the positive case: an account's domain
is **what its provider was composed over**. Usually that is one region; a
multisession optical volume whose later session references extents in an
earlier one is composed over several and accounts for all of them. A
directory is never handed an extent at all — it is a navigational position
inside one interpretation — so it owes no separate account.

That is the third reading, and it agrees with the amendment's own wording
("this obligation falls on the provider that presents"). What stops this
design from simply asserting it is that adopting it decides something the
amendment left implicit: that the P19 *interface* and the P19 *obligation*
attach to different things — the interface to every container object, the
obligation to the provider beneath them. That is a clarification of a
pledged amendment rather than a reading of it, so it belongs to whichever of
P32 and the amendment delivers second.

## Deliberately absent

- A `Machine` object above `Session`. The session is the machine scope.
- Opaque device identity. Devices are caller-configured facts, not disk
  evidence, and are named accordingly (above).
- A shared `StorageDevice` method set that every family must implement.
  Functional capability is family-typed and obtained from the device, not
  defined by it.
- Hot-swap semantics. Attach and detach are machine-down operations only.
- A second, parallel raw-media interface for hardware-capable families
  (the 1541) alongside their `Hardware<C>` capability.
- A device-level `list_files()`. The root answers for regions; file access
  belongs to a region.
- One universal region decomposition across optical families. Regions are
  family-declared, and a DVD legitimately has one where a mixed-mode CD has
  many.
- Regions as a home for every interesting extent. A boot image reached
  through a boot catalog is a P25 artifact mapping, not a region.
- A report that walks the whole directory tree. It shows the root and stops.
