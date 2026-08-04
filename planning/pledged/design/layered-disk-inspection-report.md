<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The layered disk inspection report

> **Status:** delivered, together with the removal half that retired the
> model it replaced; both handles have been struck and retired. This
> remains the written statement of the report — implemented in
> `crates/remanence/src/report.rs` and produced by `Disk::inspect` —
> serving U4 and in-force P13, P16–P18, P21, and P23.
>
> Two things it describes are of its moment rather than of the code. The
> geometry surface it says coexists with the report is gone, deleted by
> that removal half; and the report is now reached through the storage
> device holding the medium, not from a disk opened on its own.

## Outcome

One inspection operation answers what a stopped machine's disk image
contains while preserving the boundaries between the facts it discovers:

- the image adapter supplies one addressed device whose active durable
  layer is block;
- a partition-schema adapter may expose addressed regions on that device;
- volume composition may form volumes from the whole device or from those
  regions; and
- a filesystem adapter may recognize a filesystem on each volume.

The result is one navigable report, not four unrelated probing APIs. Its
relationships are explicit and typed, so a caller can select a reported
volume for a later file operation without interpreting offsets, partition
numbers, labels, filesystem names, or array positions.

The present `DiskGeometry` result satisfies U4 for the formats implemented
today, but its public shape collapses these seams. `geometry()` is really a
content-inspection verb; partition rows and FAT-derived volumes share one
flat snapshot; and `VolumeInfo.kind` names the recognized filesystem rather
than the volume — that record lives in `fat.rs` and carries cluster counts
and boot-record geometry, so volume and filesystem are one type today. F38
preserves the observable guarantees while replacing that model rather than
stretching it into a generic layer bag.

## One deep inspection seam

The intended caller flow is semantically:

```rust
let mut disk = Disk::open(path, AccessIntent::Read)?;
let report = disk.inspect()?;

for region in report.regions() {
    // Show every declared partition region, including a refused one.
}
for volume in report.volumes() {
    // Show storage composed from the whole device or one or more regions.
}
for filesystem in report.filesystems() {
    // Show only filesystem facts actually recognized on a volume.
}
```

Callers do not enumerate catalogs or invoke MBR, direct-volume, or FAT
adapters themselves. `inspect()` runs the applicable in-force composition
once and snapshots its results. No separate discovery path is introduced for
the convenience API. Selecting a reported volume for a file verb is F39's
half; until it lands, the identities this report issues serve presentation
and the relationships below.

## The report is a typed graph

The semantic model is:

```text
DiskReport
  device: DeviceInfo
  content: Blank | Schema | DirectVolume | UnknownNonblank
  partition_schema: optional PartitionSchemaInfo
  regions: [RegionInfo]
  volumes: [VolumeInfo]
  filesystems: [FilesystemInfo]

DeviceInfo
  id, image format, byte bounds, authoritative layer, active layer

PartitionSchemaInfo
  kind, recognition evidence, issues

RegionInfo
  identity, declared role, declared type (raw value and its reading),
  addressed bounds, issues

VolumeInfo
  identity, origin = WholeDevice | Regions([region identity]), byte bounds,
  composition evidence, issues

FilesystemInfo
  owning volume identity, kind, label, declared geometry, evidence, issues
```

This is not a universal recursive `Layer { kind, children, properties }`
tree. Each record has the vocabulary of its architectural seam. Relationships
express provenance without pretending that a partition is a volume or that
a filesystem is a property which all volumes possess.

**The schema is singular, and it agrees with `content`.** A device carries at
most one recognized leading schema in this scope, and `content: Schema` names
that one. A list would admit a second the outcome could not name, and a
containment field on the record would model schemas nested inside regions — a
shape F38 can never produce, since MBR's extended chain is expressed as
regions rather than as a schema within one. Nesting enters if and when a
format needs it, with the outcome vocabulary amended in the same change.

The report may expose read-only convenience lookups, such as retrieving a
volume by identity or the filesystem recognized on a volume. Those are
views over the same graph, not duplicate models. Ordering exists for stable
presentation but never supplies identity.

Physical geometry and filesystem-declared geometry remain distinct. A FAT
boot record's sectors per track and head count belong to `FilesystemInfo`
as declared filesystem evidence; they do not manufacture a physical drive
or change the block-active device into CHS.

The report distinguishes total composed volumes from volumes for which a
host-readable filesystem was successfully recognized. U4's present
drive-reporting count remains available as the latter derived count; it does
not force an unrecognized or non-filesystem volume to disappear from the
layered report. In F38's initial format scope that count is the number of
successful FAT recognitions, while the semantic distinction stays ready for
other filesystem adapters.

That distinction is load-bearing rather than decorative, because what
`volumes` holds changes here. Today a partition whose FAT volume fails to
open produces no volume at all and parks its error on the partition row.
Under this model the volume exists, composed, with the failure owned by the
filesystem seam — so a bare count of volumes no longer answers the question
U4 asks, and the derived count is what preserves it.

## Identity has two scopes, and one determinism rule

P21's `DeviceId` is opaque and unique inside the loaded composition. It is
assigned after the image format is resolved and is not derived from a path,
attachment name, image type, or catalog order. A caller opening one disk
normally need not provide or echo it.

Region, volume, and filesystem identities are also opaque library values, and
their stability rule is stronger than the device's: for an unchanged
single-disk layout, a **later open in a later process** issues an identity
naming the same object, and removal never causes a later object to inherit a
departed one's identity.

Those two properties together settle more of the implementation than they
may appear to. Opacity forbids a caller deriving the value; cross-open
stability forbids the library deriving it from anything a fresh open does not
reproduce. So **a public identity is a deterministic function of the layout's
structure** — never a report index, an enumeration counter, a session-scoped
handle, or an allocation address, each of which satisfies opacity and fails
stability the first time a caller stores one. The implementation keeps
whatever private structural key it likes; what the contract fixes is that the
same layout yields the same value, and that two distinct objects never yield
an equal one.

The contract does not expose partition ordinal, LBA, byte offset, filesystem
kind, label, or array index *as* identity, and callers never construct one.
This is a real tightening rather than a restatement: today's identity is the
string `partition:3`, built from the partition number and parsed back on the
way in.

An unreadable region retains its identity and position, so a failure cannot
renumber the objects which follow it.

## Composition behavior

F38 composes only the paths already required by U4:

1. The raw or qcow2 image adapter exposes one addressed, block-active
   device. Qcow2 backing chains remain one composed guest-visible device.
2. If MBR is recognized, its schema adapter reports every declared primary,
   extended, and logical region with pinned type values and addressed
   bounds. Structural container regions are reported but are not thereby
   volumes.
3. Direct volume composition yields a volume from each eligible data region.
   For a partitionless image it may instead yield one whole-device volume.
4. The FAT adapter independently recognizes FAT12 or FAT16 on a volume and
   reports its label and boot-record geometry.

This path never synthesizes flux, CHS, a drive, or controller behavior.
Block is both the addressed-device seam and the only active durable layer
for this feature. The permanent block/flux family boundary therefore needs
no exception for inspection.

F38 adds no format recognition. Its orchestration consumes the adapters,
evidence, authoritative layer, active layer, and provenance established by
the in-force adapter architecture. The current direct one-region composition
is sufficient; a catalog or implementation for complex volume managers
remains absent.

## A declared type is reported twice: as recorded, and as read

Every region carries the type value exactly as its schema records it, and a
reading of what that value declares. Both are present whether or not the
type is inside F38's read claim, because the row a caller most needs
explained is the one this feature refuses to read.

The reading is fit to quote in a refusal a user sees. Type `0x07` reads as
NTFS or exFAT; `0xee` says the disk is GPT rather than MBR, which is the
sentence that turns a confusing empty result into an answer. A kind tag —
`Fat16`, `Extended`, `Unsupported` — does not meet that bar: it tells a
caller which arm to take and nothing it can say out loud, which leaves the
caller maintaining a partition-type table of its own. That table is exactly
what P16 places inside the schema adapter, and a second copy outside the
library is the same duplication wearing a consumer's name.

Today the pinned name is absent exactly where it would help most — it is
`None` when the type falls outside the claim, and the issue carries the
refusal alone. Making the reading unconditional is the substance of this
section.

The reading describes what the value *declares*, never what the region
contains: an unread `0x07` region is not thereby asserted to hold NTFS. The
issue on the row still owns the refusal, and the reading is what makes that
refusal quotable.

## Absence, evidence, and refusal stay distinct

What the device's leading structure turned out to be is a **classified
outcome the report states**, not an inference from which lists came back
empty. `content` carries exactly one of: blank; a recognized partition
schema; a direct unpartitioned volume; nonblank content no adapter claims.
A caller reading `blank` beside a possibly-empty `regions` and a
possibly-empty `volumes` has to reconstruct that judgement from three
fields, each of which is empty for more than one reason — and it is a
judgement this library already makes in order to compose anything at all.
Internally it exists already, as the three-way discovery result; what the
public shape does is throw it away.

**The fourth arm is a behavior change, not a restatement.** Nonblank content
no adapter claims is a refusal today: discovery returns an error and the
whole call fails. Here it is a reported outcome on a successful call,
carrying its evidence. That is the right answer for a report whose purpose is
to say what a disk turned out to be — a disk in no format we know is a fact
about the disk, not a failure of the operation — but a consumer will observe
it, so it is stated rather than folded into "preserves present behavior".
Refusals that remain refusals are untouched: an image that cannot be *read*
still fails, and the line this draws is between an unreadable device and an
unrecognized one.

With the outcome named, the rest of the report says what followed from it:

- an all-zero device is blank;
- a valid MBR with no data volumes is the schema outcome with a recognized
  schema and zero composed volumes;
- a partitionless FAT image is the direct-volume outcome, with no partition
  schema, one whole-device volume, and one recognized filesystem;
- an unknown nonblank payload is its own outcome, never blank and never an
  empty known schema;
- a malformed declared partition remains a region with its issue rather
  than disappearing; and
- a composed volume whose filesystem is unknown or refused remains a
  volume, while the filesystem evidence or refusal stays at the filesystem
  seam.

That final separation is intentional. Filesystem recognition does not
create the underlying volume, and its failure cannot erase it. The report
may summarize a downstream problem beside a region for presentation, but
the structured issue remains owned by the operation which knows the cause.

A recognized-invalid result is never discarded to let a weaker adapter
manufacture a convenient answer. Ambiguity reports the tied candidates and
their evidence under P4. Optional facts are absent, not filled with sentinel
strings or zero values.

## What this owes P19, and what it does not

`FilesystemInfo` is a report record, not a file-access seam. Recognizing FAT
here does not present it through the delivered file-container contract, and
F38 carries none of what that contract defines: no coverage account, no
floor, no hooks, no item identities.

That is a scope statement rather than an omission. The file-container design
holds the delivered filesystem listings at their present shape until a
feature presents them through the seam, and no feature has. When one does,
the filesystem record is where the presentation hangs, and the account is
answered by the provider owning the floor — never synthesized here from a
report.

## Public-surface landing

F38 lands additively, across all three presentations at once:

- **S1 — Rust:** introduce the immutable inspection report, its typed
  records, and its identities; add the inspection operation beside
  `Disk::geometry()`.
- **S2 — C:** expose an owned report handle with indexed, bounds-checked
  access to its records and relationships. Strings remain borrowed from
  their owning handle, and the generated header is committed.
- **S3 — Python:** expose the same immutable report graph and identity
  semantics with Python value objects.

The three presentations expose the same distinctions, relationships,
optional facts, and issues. A binding may use handles where Rust uses
borrows, but it may not flatten filesystem facts into a volume or replace
an opaque identity with an array index.

**Both models coexist between F38 and F39, and that is this cut's price.**
What the pre-1.0 rule forbids is one presentation retaining the old model
while another has moved, and this cut never does that: each feature moves
Rust, C, and Python in one change. The old surface is not a compatibility
view and is not documented as a choice — it is the thing F39 deletes.

## Delivery cut

1. Introduce the core report records, identity rules, and typed
   relationships.
2. Route raw/qcow2, MBR, direct-volume, and FAT results from the built-in
   adapters into the report without duplicating recognition.
3. Land the C and Python presentations, the generated header, and the
   examples that read the report.

This order is implementation scaffolding, not a roadmap or permission to
ship a partial surface: the three presentations land together.

## Acceptance

F38 is delivered only when:

- one inspection call reports the block-active device, recognized partition
  schema, all declared regions, composed volumes, and recognized
  filesystems as distinct typed records;
- every relationship can be traversed without parsing a string or using an
  array position as identity;
- an unchanged single-disk layout issues equal region, volume, and
  filesystem identities across separate opens in separate processes, while
  device identity remains composition-scoped;
- unreadable partition entries remain visible, later entries do not
  renumber, and recognized refusals retain their owning evidence;
- blank, empty-partitioned, partitionless-volume, unknown, and invalid
  images are distinguishable, and the report *states* which one rather
  than leaving it to be reconstructed from empty lists;
- every declared region carries its raw type value and a reading of that
  value which a consumer can quote in a refusal, including for a type this
  feature does not read;
- total composed-volume count and host-readable filesystem-volume count are
  separately available, with the latter preserving U4's present reporting
  use;
- raw and qcow2 disks, MBR primary/extended/logical regions, bare volumes,
  and FAT12/FAT16 filesystems retain their current tested behavior, with the
  unrecognized-nonblank outcome changed deliberately and its test moved;
- Rust, C, and Python expose equivalent semantics; and
- the core remains runtime-dependency-free.

U4 is not armed by F38. A use case moves on full delivery, and the surface it
describes is not whole until F39 removes the model this one replaces.

## Deliberately absent

- GPT, NTFS, and new image, partition, volume, or filesystem formats.
- Presenting FAT through the P19 file-container seam, and everything that
  contract defines: coverage accounts, floors, hooks, and item identities.
- Nested partition schemas, and the outcome vocabulary that would name one.
- Multi-device opening, caller-authored topology, manual volume recipes, and
  system-wide namespace composition; U16 owns the latter journey.
- Striped, mirrored, parity, dynamic-disk, LVM-like, or other complex volume
  assembly.
- Guest drive-letter discovery, registry-hive interpretation, mount-point
  reconstruction, or system-wide namespace composition.
- Partition creation, deletion, resizing, or type editing.
- A generic container tree or caller-visible adapter/catalog API.
- Flux, CHS, media, drive, or hardware-emulation materialization.
- Direct logical-block reads and writes; U17 owns that separate presentation.

Those capabilities remain architecturally admitted where the pledged
principles say so, but each needs its own use case, feature, and surface
vetting. D5's deferred multi-device topology is not reopened here.
