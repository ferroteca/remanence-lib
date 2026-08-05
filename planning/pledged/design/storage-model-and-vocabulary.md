<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The storage model and its vocabulary

The object model of remanence's storage world — its nodes, their names,
their cardinalities, and the rule for which of them a caller ever holds —
argued in the owner's design discussion of 2026-08-04. It serves the U2
amendment ([../USE-CASES.md](../USE-CASES.md)) and the P14, P19 and P32
amendments and P35 in [../ARCHITECTURE.md](../ARCHITECTURE.md), delivered
by F48–F51 in [../FEATURES.md](../FEATURES.md), the machine scope and
the one storage handle having landed already. Pledged, not
implementation approval: this is guidance toward work the project owes,
each piece landing through its own gate, and the document is swept when
its features deliver — a design does not outlive delivery.

**Scope of the pledge.** The model is pledged for the families the
project claims today — flexible magnetic media, logical-block media, and
the archive this document adds — and for volumes formed from a whole
medium or one partition. Where it describes a shape another family would
take, that is illustration and pledges nothing: optical and tape media
(proposed P24, P26) and volumes composed across several regions remain
proposed, and nothing pledged here depends on them.

## The spine

```
artifact (a file) ──recognized by──▶ format adapter (P12) ──loads──▶ medium
device ──holds 0..1──▶ medium
medium ──carries 0..1──▶ partition scheme ──defines──▶ partitions (nestable)
volume ◀──formed from: whole medium │ one partition   (composed regions: P17, proposed)
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
space" is P17's act, "this space bears FAT" is P18's, and their order
matters wherever composition is not trivial. The seams stay; only the
node unifies.

## Two trees, two roots

The model has a device tree and a media tree, and no type above either.

```
StorageDevice                          Medium
  ├─ concrete floppy drives              ├─ floppy disk (flexible magnetic)
  │    (Commodore 1541, Heathkit H-17…)  ├─ logical-block medium
  ├─ concrete hard-disk drives           └─ archive (virtual)
  └─ archive slot (virtual)
```

Both trees list what is claimed. A family the project has not claimed —
an optical or tape drive, an optical disc or tape — takes its place in
the tree it belongs to when its own principle and feature arrive, and
this model neither pledges nor prepares for it.

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
| **archive** (virtual) | **namespace** | files directly |

The split that matters is space-native against namespace-native, not the
length of this table: a family claimed later declares its own native
vantage and what lies below it, and does so in its own principle.

## Vocabulary rulings

| Term | Ruling |
|---|---|
| **container** | Retired everywhere. Not because it is nonstandard but because it is standard for five different things (archive, image container format, multimedia container, Docker, LUKS) and can never disambiguate. |
| **device** | The slot/mechanism side: `StorageDevice` and its families. What the machine holds. |
| **medium** | The content side: what a device holds. A model node and data on the device handle, not a type of its own (see surface impact). |
| **disk, floppy, tape, disc** | Family vocabulary of the media tree, never generic terms. |
| **archive** | The virtual media kind — namespace-native. Already the surface's own word (`Archive`, the archive catalog). |
| **volume** | The space vantage of the volume/filesystem node. Only this sense: a rar "volume" (`.part2.rar`) is an artifact member here, and a tape-set "volume" is a medium. |
| **filesystem** | The namespace-vantage word generally — a volume-backed filesystem (FAT), an archive's namespace, and the machine-composed namespace are its kinds, distinguished by qualifier, not by the word. As a type, the one node that carries file verbs. |
| **Session** | The outermost scope, keeping the name and the meaning the principles already give it: the P7 claims, the P27 cache budget and private session storage, and the set of machines within it. |
| **Machine** | One device set inside a session, carrying an identity — attachment identities, attachment order, the configuration U22 and P35 reason over. A reconstructed computer is one; the session's anonymous machine is the one whose identity is null, and behaves as any other. Machines in a session do not know about each other. |
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

A family implements the set it claims: a 1541 device is
`SectorAddressable + FluxAddressable` because the family claims a
physical recording path (P22); a raw-sector floppy family without a
claimed flux path implements `SectorAddressable` alone; a device whose
declared addressing nature is LBA implements `BlockAddressable` and
nothing flux-shaped, which encodes P13's block/flux disjointness in the
type system. Family-typed results are associated types, statically
known, no downcasts.

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
(`remanence_session_*`, `remanence_machine_*`, `remanence_device_*`,
`remanence_volume_*`,
`remanence_filesystem_*`…), families as enum values, refusals through
the delivered status/category/rule mechanism, and view lifetimes under
the ABI's existing "borrowed, owned by their handle" discipline — the
sqlite3/libgit2 shape, which a family lattice would have made
impossible.

**A CHS hard drive and an LBA hard drive are separate devices**, and the
pledged P32 amendment already says how: a device declares an **addressing
nature** when it is created — CHS where a declared geometry is observable
and load-bearing, LBA where a flat block number hides it — as machine
configuration the caller supplies, never a fact reaching the medium. That
amendment deliberately declines to confine a *family* to one nature,
because a hard drive answers both depending on the command issued; two
devices differing in nature is the shape, not two families. This model
adds nothing to it and depends on it: the reason is the same one the
amendment gives, that a CHS⇄LBA translation was a BIOS fact rather than a
disk fact, and MBR entries carrying both coordinate kinds disagree on any
large disk.

## Handles and values

**A handle is a claim scope or an independently mutable state instance.
Everything else is a value: a record in a report, or a selector
parameter.** This is P7 (claims have lifetimes), P23 (truth lives in
instances), P21 (identity is unobtrusive), and P19 (composition is
transparent) said as one sentence.

Applied — and **the model's two nodes are exposed as one handle**. A
caller never holds a medium outside a device: discovery returns a
discovery, every load goes into a device, and a child artifact gets its
own device in a machine of its own — the host's archive was never part of
the machine the disk inside it belonged to. So the device is the handle, homing the media state of
whatever currently occupies it, and the medium stays a model node whose
facts are attributed on that handle rather than a second object to hold.
The nodes are undisturbed — D19's three facts keep three owners, and a
medium's `media_type` sits beside its device's `family` without either
becoming the other.

- `Session` — handle: the claim scope, the cache budget, and the set of
  machines. Everything below it lives for as long as it does, which is
  what lets one machine's device be backed by state another machine
  holds — a stored archive entry loaded into a drive elsewhere in the
  same session is source-backed through the claim the session already
  owns (P27), with no lifetime question between them.
- `Machine` — handle: one device set. Attachment identities and
  attachment order are its facts, and P35's namespace composes over its
  devices and no others, which is why an archive slot in one machine can
  never be lettered by another's composer.

  **A machine carries an identity, and the anonymous machine is the one
  whose identity is null.** A session has one of those, and
  `session.add_device(…)` places a device there — deterministically, the
  same machine every time, so this is not autocreation by guess. It
  serves the caller who is opening artifacts rather than reconstructing
  a machine, and it composes a namespace exactly as a named machine
  does: **provenance is the guard, not a refusal** (D23). A derived
  mapping travels with the machine facts and the rule that produced it
  and is never evidence, so a caller who adds two unrelated floppies and
  asks for letters gets a deterministic answer stating what it came
  from — surprising perhaps, never dishonest. An archive device is
  passed over one level down by family, having no partitions or volumes
  for an assignment rule to reach. It is not "machine zero": no
  attachment order it carries is more meaningful than any other's, and
  moving a device from it into a named machine is a reconfiguration, not
  a rename.

- `StorageDevice` — the one storage handle: the slot, its family, and
  the state of the medium in it. **Devices are added; media are loaded —
  as two acts.** The pair is `machine.add_device(family)` then
  `device.load_media(…)`, which makes an *empty* device a first-class
  configuration (the drive U22 letters whether or not a disk is in it),
  and "load" is the verb P14 already uses for what a format adapter does
  to media state. The order of `add_device` calls is the
  attachment-order fact U22's composer consumes. The handle survives
  eject and reload; views taken through it — a filesystem, a file —
  invalidate when the medium beneath them leaves, and an empty device
  refuses every content verb by name.
  **`discover_media(path)` is a first-class library function, on no
  handle at all**: it claims the artifact for the read, identifies it,
  and answers with a report — the exact medium, the concrete device
  families that accept it, and the image format's declared default —
  mutating nothing and needing no machine, since it consults catalogs
  and evidence, never configuration. **The discovery it returns is a
  consumable handle — a claim scope holding expensive work.**
  Discovering a flux capture parses streams and probes drive profiles;
  `load_media` accepts a discovery as it accepts a path and consumes
  it, the parsed state moving into the device so nothing is done twice —
  P29's plan-and-execute shape one seam over. The claim taken at
  discovery holds until consumption or drop, so no window exists between
  the question and the load in which the artifact could change (P7
  continuity). Over discovery sits one machine-level convenience,
  `machine.add_device(path)`: it adds a fresh device of the
  **format-declared default family** and loads the medium into it,
  returning that device. The default lives on the format because it is
  ecosystem knowledge the media type cannot honestly hold — a ten-sector
  hard-sectored 5.25-inch disk is the article of both a Heathkit H-17
  and a North Star MDS, but an H8D records a Heathkit disk — while the
  supported-device list is derived by asking the families, which declare
  the media they accept (D19's direction, unchanged). A format declaring
  no default (a raw image says nothing about its machine) refuses by
  name toward the two explicit acts. A declaration nobody makes is a
  refusal, not a guess (P3).
- Volumes — values: the identity the inspection report issued, passed as
  a selector where several exist, never held, never numbered. A volume
  has no format-defined ordinal anywhere, so none is accepted.
- Partitions — values addressed by their **format-defined coordinate**:
  MBR entry 1 is a fact of the on-disk table (U4 preserves its place), so
  `partition(1)` names a format fact rather than inventing a position.
  One node kind takes an ordinal because its format defines one; the
  other refuses it because nothing does.
- `Filesystem` — the namespace node, and **the one type that carries
  file verbs**: `get_file` lives here and nowhere else. A device does
  not carry file access, because a device holding a partitionable
  medium bearing `get_file` would be a category error in the type, not
  a refusal waiting to happen. Three providers reach the type: a volume
  that bears one (`volume.filesystem()`, 0..1 — swap is a named
  absence), a device holding an archive medium, whose content *is* one
  (`device.filesystem()`, always), and the machine's composed namespace
  (`machine.filesystem()` → `MachineFilesystem`). Where the medium is
  space-native, `device.filesystem()` is a **resolver**: it walks
  volume → filesystem where every seam has exactly one supported answer
  and refuses naming the candidates otherwise — in-force P19's
  transparency clause as a method. It creates nothing and never
  guesses. **A device may be asked what it resolves to; it may not be
  told to act as something it isn't** — the line between a query whose
  answer set already includes *refuse* and *absent*, and a content verb
  that presumes. A `Filesystem` is a view over its provider's state,
  never an instance: mutations project into the active layer (P23), and
  over an archive the named-entry state is what it presents. Its kind —
  FAT, zip, HDOS, machine-composed — is data, not a type.
- Files — values reached by path; a file view is borrowed from its
  filesystem, never an instance, and offers bounded/streamed forms with
  whole-value conveniences beside them (P27).

**One handle, two nodes, and the facts stay attributed.** Slot-side
facts — the attachment identity, the family, occupancy, and the media
lifecycle verbs `load_media` and `eject`, since "insert the disk" cannot
hang off the disk — answer for the device. Content-side facts — the
media type and its passive profile, the active layer, assurance,
identification, volumes, dirty state, commit and rollback — answer for
the medium in it, and refuse by name when the slot is empty. Keeping the
nodes distinct in the data is what preserves D19's pair (the medium
states an index hole, the drive states no sensor for it) and U24's
flippy (the same medium, reloaded under a different side policy) with
one object in hand. The device outlives every medium loaded into it, may
exist empty (U22 letters an empty drive), and is where mechanism state
sits when hardware emulation arrives (P15: motor, stepping, ready,
disk-change, the write-protect *sensor* against the medium's *notch*).
Pledged P32 already routes the P15 contract through the device's family
capability rather than a raw media interface, and P33 already excludes
mechanism state from the active layer, so hardware emulation is a device
capability operating over the loaded medium's state. When the emulator
family (U9–U12) lands, the guest talks to the device — unit 8.

## Transparency, and the simple case

**`machine → device → content` is the access path** — the one route,
and the device is where it stops being a chain: the device homes the
medium's state, so no medium handle stands between the drive and what it
holds. The acceptance test for the model is four plain moves:

```
session = Session()
drive   = session.add_device(heathkit_h17)   # the anonymous machine
drive.load_media("myfloppyimage.h8d")
fs      = drive.filesystem()
file    = fs.get_file("myfile.txt")
```

The device is the caller's to state, because which device serves a
medium is machine configuration, not image content — and it is stated
concretely, the drive the machine actually had. Lineage interior names
("floppy drive") classify entries and answer queries; only a concrete
entry instantiates, since only a concrete drive declares the facts
`load_media` checks a medium against. The one-step convenience composes
the first two moves without changing the path, returning the same device
handle.

The filesystem is resolved, never named: `drive.filesystem()` walks
volume → filesystem because every seam there has exactly one supported
answer — in-force P19's transparency clause as a method. Degradation is
explicit: two volumes and the resolver refuses naming both, selection
then running by report identity through `drive.volume(id).filesystem()`;
a volume with no filesystem is a named absence; a damaged source
degrades bounded and read-only (P28). **Transparent when there is one
supported result; explicit when there are several; never guessing.** An
archive is the same journey through its own added device, its
`filesystem()` answering always because its content is one — which today
it is not (see surface impact).

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

The set is pledged beside this document: the P14, P19, and P32
amendments and P35 in [../ARCHITECTURE.md](../ARCHITECTURE.md),
delivered by F48–F51 in [../FEATURES.md](../FEATURES.md) — the
`Filesystem` node, the uniform archive open, the two-act access path,
and discovery with declared defaults. The renames are delivered: the
session gained machines beneath it and `Disk` merged into
`StorageDevice`. This document serves those entries and the U2
amendment, and is swept when they deliver.

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

- `Session` **keeps its name and meaning** — the claim and cache scope —
  and gains `Machine` beneath it as the device set. `Disk` **merges into
  `StorageDevice`** rather than being renamed: the device homes the media
  state, so the two delivered types become one handle and the medium
  survives as model node and as data. D2's "disk stack" prose naming
  follows.
- The access path `machine → device → content`: `add_device` then
  `load_media`, each returning its verb's noun. Today's `attach(path)`
  is the one-step shape; the model keeps it as one convenience over
  `discover_media` — `add_device(path)`, adding a fresh device of the
  format-declared default family and returning it — with the canonical
  two-step beneath it. The media-first machine-level spelling is dropped
  rather than kept as a synonym: with one handle both spellings would
  return the same device. `discover_media` itself is new library-level surface,
  the format adapters' default-device declaration is a new catalog fact,
  and a device that exists empty is new surface.
- Uniform open: archives enter through the same add-device-and-load
  journey; the
  separate `archive[/entry]` path syntax and the `Archive` type's
  standalone journey fold into the model (the archive catalog itself is
  untouched — it becomes the family's adapter).
- File verbs move to the `Filesystem` node: `get_file` lives there and
  nowhere else — the device exposes none — with `device.filesystem()`
  resolving-or-refusing and `device.volume(id).filesystem()` selecting
  where several candidates exist; `list_hdos_files`'s selector-free
  signature stops being an inconsistency and becomes the resolver's
  transparent form.

**Records.** The vocabulary rulings become a D-entry when landed (D2-style
retirements: container everywhere, bare "disk" as a generic). No new
numbers are issued by this document: a design is identified by its path,
and the U2 amendment keeps U2's number.

## Open questions carried

- **Further conveniences are deliberately not pledged.** The explicit
  walk is what the model owes past the anonymous machine, which is a
  structural rule rather than a shortcut. There is obvious room beyond
  it — a filesystem reached straight from a session, a device added
  from a path without naming a family — and each is its own later
  proposal, weighed as the machine-level one-step was: admissible where
  it declares, refused where it would guess.
- Do archive media occupy visible slots (`arc0`) in the attachment
  namespace, or does the virtual slot stay entirely behind the report?
  F49 settles it.

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
