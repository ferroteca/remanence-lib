<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The storage model and its vocabulary

The object model of remanence's storage world — its nodes, their names,
their cardinalities, and the rule for which of them a caller ever holds —
argued in the owner's design discussion of 2026-08-04. It serves the U2
amendment ([../USE-CASES.md](../USE-CASES.md)) and maps the principle and
surface amendments its final section names. This is proposed, not
implementation approval; nothing here binds until each mapped change lands
through its own gate, and this document is swept when its demands deliver —
a design does not outlive delivery.

## The spine

```
artifact (a file) ──recognized by──▶ format adapter (P12) ──loads──▶ medium
device ──holds 0..1──▶ medium
medium ──carries 0..1──▶ partition scheme ──defines──▶ partitions (nestable)
volume ◀──formed from: whole medium │ one partition │ composed regions
volume ──bears 0..1──▶ content claim   (filesystem = the file-bearing kind)
filesystem ──contains──▶ files and directories
file ──may be recognized as──▶ artifact (image or archive)  ⟲ recursion
```

Every cardinality is strict, and each is where a delivered behavior already
lives. A device may be empty (a drive with no disk; a slot before attach).
A medium may be unpartitioned — a floppy has *zero* partitions, not one
trivial one, because a partition is an entry in a partitioning scheme and
no scheme is present; the whole medium then forms one direct volume (P17's
delivered case). A volume may bear no filesystem — swap, boot code, raw
database extents, unformatted space, and content outside every claim are
ordinary volumes, and P19's honest-absence rule is the 0 in this 0..1, not
a caveat beside it.

**Volume and filesystem are one node seen from two vantages.** A volume is
the *addressable space* — extent and composition, the word used looking
down ("those two partitions comprise one volume"). A filesystem is the
*namespace* — names and files, the word used looking in. When a volume
bears a filesystem they are one model node with two vantage words; there
is no third node between them, and no "container" node anywhere.

Volume composition and filesystem recognition remain two acts with two
owners even though their result is one node: "these regions form one
space" is P17's act, "this space bears FAT" is P18's, and a spanned volume
shows their order matters. The seams stay; only the node unifies.

## Two trees, two roots

The model has a device tree and a media tree, and no type above either.

```
StorageDevice                          Medium
  ├─ FloppyDevice (CbmFloppyDevice…)     ├─ floppy disk (flexible magnetic)
  ├─ ChsHardDiskDevice                   ├─ logical-block medium
  ├─ LbaHardDiskDevice                   ├─ tape
  ├─ TapeDevice, OpticalDevice           ├─ optical disc
  └─ ArchiveDevice (virtual)             └─ archive (virtual)
```

`StorageDevice` roots the device tree: the slot and mechanism — what the
machine holds, named by attachment identity (`hdd0`, `cbmfloppy0`), the
side that P32's device set already speaks. `Medium` roots the media tree:
the content-bearing article — what a device holds, the side P14 already
governs. The earlier draft vocabulary's `StorageContainer` tried to root
both trees at once, which is why it needed a word that vague; it is
deleted with no successor, its job split between the two roots.

"Disk", "tape", "floppy", and "archive" are media-centric terms — they
name kinds of the *second* tree. "Hard disk" is the one compound exception
and D19 already ruled it: a device-family name, its medium being
logical-block media.

## The archive

An archive (zip, 7z, tar…) is a **virtual medium**: independent recorded
state with no physical article behind it. It sits exactly in P14's
sentence — "the independent mutable state between image formats and
drives" — loaded and saved by its format adapter, held by no drive, which
P14 permits because media being independent of drives is its point. Its
device-side presence is a virtual slot (`ArchiveDevice`): an attachment
with no mechanism, delegating to its medium as every device does.

What distinguishes the archive family is its native vantage: **an archive
has no addressing scheme, only a namespace.** A zip's byte extent is its
*encoding* (P13: container bytes are the encoding, not the image layer),
not a model space — there is no meaningful "sector 5 of a zip." The
artifact's bytes stay addressable as evidence (opaque-region accounts,
P2 reads); that is plumbing, not a vantage anyone composes on.

The classifier between an image and an archive is one question: **is
there a meaningful sector or block N?** An `.iso` has one — it is a
disk-family image whose space bears ISO 9660. A `.zip` does not.

That classifier is one instance of a question this model asks at every
level: **can a session serve one location by key, from the artifact as
it stands?** Addressability by key is also what separates an image from
a capture at the magnetic rungs — a G64 writes each track's length down
and serves by track, while a NIB's fixed windows overlap a revolution
with the wrap recorded nowhere, so nothing is servable without analysis
(D15). The two axes stay orthogonal: the *rung* says what an artifact
records — flux, bits, bytes — and *addressability* says whether it is
an image or a capture at that rung. Pledged P31 names this test
*servability*; addressability is this model's word for the same axis,
and when P31 arms, its test should take this name.

The media-kind table, by native vantage:

| Media kind | Native vantage | Below it |
|---|---|---|
| logical-block medium | space (LBA) | partitions → volumes → 0..1 filesystem |
| flexible magnetic | space (flux/CHS) | direct volume → 0..1 filesystem |
| optical disc | space (sectors) | tracks/sessions → volumes → filesystem |
| tape | space (sequential) | family-owned structure |
| **archive** (virtual) | **namespace** | files directly |

## Vocabulary rulings

| Term | Ruling |
|---|---|
| **container** | Retired everywhere. Not because it is nonstandard but because it is standard for five different things (archive, image container format, multimedia container, Docker, LUKS) and can never disambiguate. |
| **device** | The slot/mechanism side: `StorageDevice` and its families. What the machine holds. |
| **medium** | The content side: what a device holds. The type the delivered `Disk` actually is (see surface impact). |
| **disk, floppy, tape, disc** | Family vocabulary of the media tree, never generic terms. |
| **archive** | The virtual media kind — namespace-native. Already the surface's own word (`Archive`, the archive catalog). |
| **volume** | The space vantage of the volume/filesystem node. Only this sense: a rar "volume" (`.part2.rar`) is an artifact member here, and a tape-set "volume" is a medium. |
| **filesystem** | The namespace-vantage word generally — a volume-backed filesystem (FAT), an archive's namespace, and the machine-composed namespace are its kinds, distinguished by qualifier, not by the word. As a type, the one node that carries file verbs. |
| **Machine** | The session type renamed: the machine scope, which is what `machine.rs` already calls it. Whether "session" survives in principle prose as the claim-and-cache lifetime word (P7's writable session, P27's session cache) is an open question below. |
| **MachineFilesystem** | The machine-level composed namespace — drive letters, mount trees — one navigable whole over several child filesystems. |

## Vantage capabilities are traits, not a tower

Addressing vantages are capability declarations a family claims (P3),
not a linear inheritance chain — a chain is not even expressible in the
implementation language, which has traits and no inheritance:

```rust
trait StorageDevice { fn attachment(&self) -> AttachmentId; }

trait FluxAddressable:   StorageDevice { /* flux vantage */ }
trait SectorAddressable: StorageDevice {
    type Sector;                       // family-typed result
    fn sector(&self, c: u32, h: u32, s: u32) -> Result<Self::Sector>;
}
trait BlockAddressable:  StorageDevice { fn block(&self, n: u64) -> Result<Block>; }
trait Partitionable:     StorageDevice { /* orthogonal to addressing */ }
```

A family implements the set it claims: `CbmFloppyDevice` is
`SectorAddressable + FluxAddressable` because the 1541 family claims a
physical recording path (P22); a raw-sector floppy family without a
claimed flux path implements `SectorAddressable` alone; `LbaHardDiskDevice`
implements `BlockAddressable` and nothing flux-shaped, which encodes P13's
block/flux disjointness in the type system. Family-typed results
(`CbmFloppySector`) are associated types, statically known, no downcasts.

These traits are internal Rust structure — the crate's organization, not
the public shape. **The surface carries node-kind types** — `Machine`,
`StorageDevice`, `Medium`, `Volume`, `Filesystem`, `File` — with the
family as data on the handle and every capability an enumerated claim
whose refusal is named (P3, P10): the one mechanism that presents
identically in Rust, C, and Python (P5). The delivered surface already
chose this shape — one `Disk` for every image format, `DeviceFamily` an
enum value — and where it minted distinct types (`CaptureSet`,
`MasteredMedium`, `C1541Bitstream`) they are distinct node kinds, not
families of one kind. **Types follow node kinds; families are data;
capabilities are claims.** The C ABI then maps conventionally: one
opaque handle type per node kind with its function prefix
(`remanence_machine_*`, `remanence_device_*`, `remanence_medium_*`,
`remanence_filesystem_*`…), families as enum values, refusals through
the delivered status/category/rule mechanism, and view lifetimes under
the ABI's existing "borrowed, owned by their handle" discipline — the
sqlite3/libgit2 shape, which a family lattice would have made
impossible.

**CHS and LBA hard drives are separate partitionable device families.**
One device type exposing both vantages would need a CHS⇄LBA translation
inside it, and translation was a BIOS fact, not a disk fact — the same
drive under two translations yields two incompatible layouts, and MBR
entries carry both coordinate kinds, disagreeing on any large disk. Which
addressing the machine used is caller-asserted machine configuration at
attach, the same fact class U22 already makes the caller own. Evidence
advises the choice; the attach names it. (A rigid CHS-addressed medium is
a third media family when that device family arrives; P14 claims flexible
magnetic and logical-block today.)

## Handles and values

**A handle is a claim scope or an independently mutable state instance.
Everything else is a value: a record in a report, or a selector
parameter.** This is P7 (claims have lifetimes), P23 (truth lives in
instances), P21 (identity is unobtrusive), and P19 (composition is
transparent) said as one sentence.

Applied:

- `Machine` — handle: the claim scope and device set.
- Devices — handles: slots with lifetimes; the working handle for machine
  work. **Devices are added; media are loaded — as two acts.** The pair
  is `machine.add_device(…)` then `device.load_media(…)` — machine
  configuration first, media placement second, which makes an *empty*
  device a first-class configuration (the drive U22 letters whether or
  not a disk is in it), and "load" is the verb P14 already uses for what
  a format adapter does to media state. **The return follows the verb's
  noun**: `add_device` returns the device handle, `load_media` returns
  the media handle. The order of `add_device` calls is the
  attachment-order fact U22's composer consumes.
  **`discover_media(path)` is a first-class library function, on no
  handle at all**: it claims the artifact for the read, identifies it,
  and answers with a report — the exact medium, the concrete device
  families that accept it, and the image format's declared default —
  mutating nothing and needing no machine, since it consults catalogs
  and evidence, never configuration. **The discovery it returns is a
  consumable handle — a claim scope holding expensive work.**
  Discovering a flux capture parses streams and probes drive profiles;
  `load_media` accepts a discovery as it accepts a path and consumes
  it, the parsed state moving into the loaded medium so nothing is done
  twice — P29's plan-and-execute shape one seam over. The claim taken
  at discovery holds until consumption or drop, so no window exists
  between the question and the load in which the artifact could change
  (P7 continuity). Over discovery sit the machine's two dual one-step
  conveniences:
  `machine.add_device(path)` returning the device, the wording that
  fits fixed storage, and `machine.load_media(path)` returning the
  medium, the wording that fits removable media. Each adds a fresh
  device of the **format-declared default family** and loads into it —
  stated, never a silent reuse — and a format declaring no default (a
  raw image says nothing about its machine) refuses by name toward the
  explicit acts: `add_device(family)`, then `load_media(path)` on the
  device. The default lives on the format because it is ecosystem
  knowledge the media type cannot honestly hold — a ten-sector
  hard-sectored 5.25-inch disk is the article of both a Heathkit H-17
  and a North Star MDS, but an H8D records a Heathkit disk — while the
  supported-device list is derived by asking the families, which
  declare the media they accept (D19's direction, unchanged). A
  declaration nobody makes is a refusal, not a guess (P3).
- The medium — handle: the mutable state instance. Device content-verbs
  **delegate to the occupied medium**; an empty slot is a named refusal;
  the device handle survives eject and load. On the flux side the same
  rule generates the opposite answer: each explicit P33 transition
  produces a new state instance, so capture set, mastered medium, and
  bitstream are separate handles.
- Volumes — values: the identity the inspection report issued, passed as
  a selector where several exist, never held, never numbered. A volume
  has no format-defined ordinal anywhere, so none is accepted.
- Partitions — values addressed by their **format-defined coordinate**:
  MBR entry 1 is a fact of the on-disk table (U4 preserves its place), so
  `partition(1)` names a format fact rather than inventing a position.
  One node kind takes an ordinal because its format defines one; the
  other refuses it because nothing does.
- `Filesystem` — the namespace node, and **the one type that carries
  file verbs**: `get_file` lives here and nowhere else — `Medium` in
  general has no file access, because a partitionable medium bearing
  `get_file` would be a category error in the type, not a refusal
  waiting to happen. Three providers reach the type: a volume that bears
  one (`volume.filesystem()`, 0..1 — swap is a named absence), an
  archive medium whose content *is* one (`medium.filesystem()`, always),
  and the machine's composed namespace (`machine.filesystem()` →
  `MachineFilesystem`). On a space-native medium, `medium.filesystem()`
  is a **resolver**: it walks volume → filesystem where every seam has
  exactly one supported answer and refuses naming the candidates
  otherwise — in-force P19's transparency clause as a method. It creates
  nothing and never guesses, which is what distinguishes it from the
  rejected machine-level conveniences. A `Filesystem` is a view over its
  provider's state, never an instance: mutations project into the
  medium's active layer (P23), and over an archive the medium's
  named-entry state is what it presents. Its kind — FAT, zip, HDOS,
  machine-composed — is data, not a type.
- Files — values reached by path; a file view is borrowed from its
  filesystem, never an instance, and offers bounded/streamed forms with
  whole-value conveniences beside them (P27).

**What hangs off the device alone is deliberately little**: the
attachment identity, the family, occupancy, and the media lifecycle —
`load_media` and `eject` are drive verbs, since "insert the disk" cannot
hang off the disk. Everything content-shaped belongs to the medium, and
the device may forward it by delegation. The device is not carried by
member count: it is carried by lifetime (the slot outlives every disk in
it), by existence while empty (U22 letters an empty drive), and by being
where machine-facing facts sit — slot kind, presence, attachment order —
and where mechanism state will sit when hardware emulation arrives
(P15: motor, stepping, ready, disk-change, the write-protect *sensor*
against the medium's *notch*). When the emulator family (U9–U12) lands,
the guest talks to the device — unit 8 — and none of that conversation
is medium API.

## Transparency, and the simple case

**`machine → device → media` is the access path** — the one route to
content. Either one-step convenience composes the first two moves
without changing the path — each returns its verb's noun, and the
medium is still reached through the device the call added. The
acceptance test for the model is five plain moves:

```
machine = Machine()
drive   = machine.add_device(heathkit_h17)
medium  = drive.load_media("myfloppyimage.h8d")
fs      = medium.filesystem()
file    = fs.get_file("myfile.txt")
```

The device is the caller's to state, because which device serves a medium
is machine configuration, not image content — and it is stated
concretely, the drive the machine actually had. Lineage interior names
("floppy drive") classify entries and answer queries; only a concrete
entry instantiates, since only a concrete drive declares the facts
`load_media` checks a medium against. The filesystem is resolved,
never named: `medium.filesystem()` walks volume → filesystem because
every seam there has exactly one supported answer — in-force P19's
transparency clause as a method. Degradation is explicit: two volumes and
the resolver refuses naming both, selection then running by report
identity through `medium.volume(id).filesystem()`; a volume with no
filesystem is a named absence; a damaged source degrades bounded and
read-only (P28). **Transparent when there is one supported result;
explicit when there are several; never guessing.** An archive is the
same journey through its own added device, its `filesystem()` answering
always because its content is one — which today it is not (see surface
impact).

## The device/medium split, priced

The split earns its keep in the removable families and costs nothing
elsewhere, and its price was audited rather than assumed:

- **U23 (in force)** — D19's three-fact separation *is* the split
  delivered: what one head saw, what the disk is, what the drive does.
  "The 1541 medium states an index hole and the 1541 drive states no
  sensor" is unsayable with one node.
- **U22 (in force)** — letters follow drives, not disks: `A:` names the
  slot, disks swap under it, an empty drive still letters.
- **U24 (proposed)** — the flippy: *same medium, flipped in the drive* is
  the entire content of the observation, and it has no representation
  without two nodes.
- **U9/U10 (proposed)** — a guest addresses unit 8 while the disk in
  unit 8 changes mid-session; the stable-device/mutable-medium shape is
  the guest's own model.

For fixed and virtual storage the split does no user-facing work, and the
handle rule makes it free there: one call yields the medium, the slot
lives in the report.

## What this maps onto

The set below is drafted: the P14, P19, and P32 amendments and P35 in
[../ARCHITECTURE.md](../ARCHITECTURE.md), delivered by F46–F49 in
[../FEATURES.md](../FEATURES.md) — the renames, the access path, the
`Filesystem` node, and the uniform archive open. This document serves
those drafts and the U2 amendment, and is swept when they deliver.

**Principles.**

- **P19** slims to the namespace convergence it always claimed: one
  file-access interface however reached, honest absence, transparency.
  Its "serialized-container provider form" dissolves — an archive is a
  medium, its adapter a P12 adapter at the namespace seam. The
  namespace-mapping composer's three constraints move out of P19 into the
  machine-namespace statement below; the previously proposed P19/P34
  split is superseded by this relocation.
- **A machine-namespace statement** (amendment or new principle) owns
  `MachineFilesystem`: one composed namespace over several child
  filesystems; the mapping consumed where a system persists it (pledged
  U16's case), derived under the composer's three constraints where
  nothing persists it (in-force U22's case).
- **P14** widens to admit the virtual media family: an archive is a
  medium whose profile carries no physical fact, its state loaded and
  saved by its format adapter like any other.
- **P16, P17, P18** are confirmed unchanged: the volume/filesystem node
  unification does not merge the seams — composition and recognition
  remain two acts with two owners.
- **P32 (pledged)** gains the CHS/LBA device-family split and the
  archive's virtual slot.

**Surfaces (S1–S3, each a gated change).**

- `Session` → `Machine`; `Disk` → `Medium` (the type is the media-tree
  node; D2's "disk stack" prose naming follows the type it was named
  for).
- The access path `machine → device → media`: `add_device` then
  `load_media`, each returning its verb's noun. Today's `attach(path)`
  is the one-step shape; the model keeps it as the two dual conveniences
  over `discover_media` — `add_device(path)` returning the device,
  `load_media(path)` returning the medium, each adding a fresh device of
  the format-declared default family — with the canonical two-step
  beneath them. `discover_media` itself is new library-level surface,
  the format adapters' default-device declaration is a new catalog fact,
  and a device that exists empty is new surface.
- Uniform open: archives enter through the same add-device-and-load
  journey; the
  separate `archive[/entry]` path syntax and the `Archive` type's
  standalone journey fold into the model (the archive catalog itself is
  untouched — it becomes the family's adapter).
- File verbs move to the `Filesystem` node: `get_file` lives there and
  nowhere else — `Medium` exposes none — with `medium.filesystem()`
  resolving-or-refusing and `medium.volume(id).filesystem()` selecting
  where several candidates exist; `list_hdos_files`'s selector-free
  signature stops being an inconsistency and becomes the resolver's
  transparent form.

**Records.** The vocabulary rulings become a D-entry when landed (D2-style
retirements: container everywhere, bare "disk" as a generic). No new
numbers are issued by this document: a design is identified by its path,
and the U2 amendment keeps U2's number.

## Open questions carried

- Does "session" survive in principle prose as the claim-and-cache
  lifetime word, or does `Machine` absorb both meanings deliberately?
- Do archive media occupy visible slots (`arc0`) in the attachment
  namespace, or does the virtual slot stay entirely behind the report?

## Deliberately not proposed

No implementation of any of the above — this document and the U2
amendment are the deliverables of the discussion that produced them.
Typed public sector/block vantage access (`getSector(c,h,s)`) waits for
the emulator-delegation family (U9–U12), whose demand it is; when it
arrives it hangs off the space vantage a space-native medium offers,
never off `Medium` in general — the symmetric placement to file verbs
living only on `Filesystem`. No new format, family, or catalog claim is
made anywhere in this document.

**The one-step conveniences were rejected once and reinstated by
declaration.** The original objection stands against the *undeclared*
form: autocreation with no stated answer to the second call, and a
family chosen by guess. The reinstated forms (see "Handles and values")
answer both in the contract — `discover_media`, a format-declared
default family, a fresh device per call — and refuse where no
declaration exists. What stays rejected is any silent reuse of an
existing slot, and a default declared anywhere but the image format.
