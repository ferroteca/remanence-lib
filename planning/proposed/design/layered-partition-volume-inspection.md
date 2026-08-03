<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Layered partition and volume inspection

Design for
[F20](../FEATURES.md#f20--layered-partition-and-volume-inspection),
serving U3 and U4 and pledged P13, P16–P19, P21, and P23. This is a
proposed destination and delivery cut, not approval to implement it. Type
and method names below describe semantics; pledging F20 would settle the
public spelling during delivery.

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
than the volume. F20 preserves the observable guarantees while replacing
that model rather than stretching it into a generic layer bag.

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

let selected = choose_volume(&report)?;
let entries = disk.entries(selected.id(), root_path)?;
```

Callers do not enumerate catalogs or invoke MBR, direct-volume, or FAT
adapters themselves. `inspect()` runs the applicable in-force composition once
and snapshots its results. File operations consume an identity issued by
that report and resolve it inside the same open disk. No separate discovery
path is introduced for the convenience API.

## The report is a typed graph

The semantic model is:

```text
DiskReport
  device: DeviceInfo
  content: Blank | Schema | DirectVolume | UnknownNonblank
  partition_schemas: [PartitionSchemaInfo]
  regions: [RegionInfo]
  volumes: [VolumeInfo]
  filesystems: [FilesystemInfo]

DeviceInfo
  id, image format, byte bounds, authoritative layer, active layer

PartitionSchemaInfo
  kind, containing device or region, recognition evidence, issues

RegionInfo
  identity, owning schema, declared role, declared type (raw value and its
  reading), addressed bounds, issues

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

The report may expose read-only convenience lookups, such as retrieving a
volume by identity or the filesystem recognized on a volume. Those are
views over the same graph, not duplicate models. Ordering exists for stable
presentation but never supplies identity.

Physical geometry and filesystem-declared geometry remain distinct. A FAT
boot record's sectors per track and head count belong to `FilesystemInfo`
as declared filesystem evidence; they do not manufacture a physical drive
or change the block-active device into CHS.

The report distinguishes total composed volumes from volumes for which a
host-readable filesystem file-container view was successfully opened. U4's
present drive-reporting count remains available as the latter derived count;
it does not force an unrecognized or non-filesystem volume to disappear from
the layered report. In F20's initial format scope that count is the number of
successful FAT file-container views, while the semantic distinction remains
ready for other filesystem adapters.

## Identity has two scopes

P21's `DeviceId` is opaque and unique inside the loaded composition. It is
assigned after the image format is resolved and is not derived from a path,
attachment name, image type, or catalog order. A caller opening one disk
normally need not provide or echo it.

Region and volume identities are also opaque library values. Within one
open disk they are the only selectors accepted by later region- or
volume-scoped operations. Their public stability rule preserves U4: for an
unchanged single-disk layout, a later report issues an identity which names
the same region or volume; removal does not cause a later object to inherit
that identity. Stability is a semantic guarantee, not permission to parse
or construct the value.

The implementation may keep a private structural key from which a stable
public identity is assigned. The contract does not expose partition ordinal,
LBA, byte offset, filesystem kind, label, or array index as identity. An
unreadable region retains its identity and position, so a failure cannot
renumber objects which follow it.

## Composition behavior in F20

F20 composes only the paths already required by U4:

1. The raw or qcow2 image adapter exposes one addressed, block-active
   device. Qcow2 backing chains remain one composed guest-visible device.
2. If MBR is recognized, its schema adapter reports every declared primary,
   extended, and logical region with pinned type values and addressed
   bounds. Structural container regions are reported but are not thereby
   volumes.
3. Direct volume composition yields a volume from each eligible data region.
   For a partitionless image it may instead yield one whole-device volume.
4. The FAT adapter independently recognizes FAT12 or FAT16 on a volume and
   reports its label and boot-record geometry. Recognition produces the P19
   file-container view used by U3's file verbs.

This path never synthesizes flux, CHS, a drive, or controller behavior.
Block is both the addressed-device seam and the only active durable layer
for this feature. The permanent block/flux family boundary therefore needs
no exception for inspection.

F20 adds no format recognition. Its orchestration consumes the adapters,
evidence, authoritative layer, active layer, and provenance established by
the in-force adapter architecture. The current direct one-region composition
is sufficient; a catalog or
implementation for complex volume managers remains absent.

## A declared type is reported twice: as recorded, and as read

Every region carries the type value exactly as its schema records it, and a
reading of what that value declares. Both are present whether or not the
type is inside F20's read claim, because the row a caller most needs
explained is the one this feature refuses to read.

The reading is fit to quote in a refusal a user sees. Type `0x07` reads as
NTFS or exFAT; `0xee` says the disk is GPT rather than MBR, which is the
sentence that turns a confusing empty result into an answer. A kind tag —
`Fat16`, `Extended`, `Unsupported` — does not meet that bar: it tells a
caller which arm to take and nothing it can say out loud, which leaves the
caller maintaining a partition-type table of its own. That table is exactly
what P16 places inside the schema adapter, and a second copy outside the
library is the same duplication wearing a consumer's name.

The reading describes what the value *declares*, never what the region
contains: an unread `0x07` region is not thereby asserted to hold NTFS. The
issue on the row still owns the refusal, and the reading is what makes that
refusal quotable.

## Absence, evidence, and refusal stay distinct

What the device's leading structure turned out to be is a **classified
outcome the report states**, not an inference from which lists came back
empty. `content` carries exactly one of: blank; a recognized partition
schema; a direct unpartitioned volume; nonblank content no adapter claims.
A caller reading `blank` beside a possibly-empty `partitions` and a
possibly-empty `volumes` has to reconstruct that judgement from three
fields, each of which is empty for more than one reason — and it is a
judgement this library already made in order to compose anything at all.

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
When F20 is delivered, U4's wording and all descriptive surfaces move
coherently to this layered expression of the same no-skipping and
known-cause guarantees.

A recognized-invalid result is never discarded to let a weaker adapter
manufacture a convenient answer. Ambiguity reports the tied candidates and
their evidence under P4. Optional facts are absent, not filled with sentinel
strings or zero values.

## Public-surface landing

F20 is one pre-1.0 surface replacement:

- **S1 — Rust:** introduce the immutable inspection report and its typed
  records and identities; replace `Disk::geometry()` with the inspection
  operation; make volume-scoped file verbs accept the opaque volume
  identity; remove `DiskGeometry` and the old flattened records.
- **S2 — C:** expose an owned report handle with indexed, bounds-checked
  access to its records and relationships. Strings remain borrowed from
  their owning handle. Opaque volume identities round-trip from report
  accessors into file verbs; old geometry symbols are removed and the
  generated header is committed.
- **S3 — Python:** expose the same immutable report graph and identity
  semantics with Python value objects. `Disk.inspect()` and volume-scoped
  file operations mirror S1; old geometry classes and methods are removed.

The three presentations expose the same distinctions, relationships,
optional facts, and issues. A binding may use handles where Rust uses
borrows, but it may not flatten filesystem facts into a volume or replace
an opaque identity with an array index. Because bindings track the core,
there is no intermediate release in which one presentation retains the old
model.

## Delivery cut

Once pledged, F20 lands as one coherent replacement in this internal order:

1. Introduce the core report records, identity rules, and typed
   relationships.
2. Route raw/qcow2, MBR, direct-volume, and FAT results from the built-in
   adapters
   into the report without duplicating recognition.
3. Move existing Rust file operations to opaque volume selection and delete
   the flattened geometry model.
4. Move the C and Python presentations, generated header, examples, and
   usage documentation to the same model.
5. Amend U4's descriptive wording to name the layered result while
   preserving its stopped-machine, stability, no-skipping, and known-cause
   guarantees.

This order is implementation scaffolding, not a roadmap or permission to
ship partial surfaces. The public replacement is delivered as a whole.

## Acceptance

F20 is delivered only when:

- one inspection call reports the block-active device, recognized partition
  schema, all declared regions, composed volumes, and recognized
  filesystems as distinct typed records;
- every relationship can be traversed without parsing a string or using an
  array position as identity;
- an unchanged single-disk layout preserves U4's region and volume identity
  semantics across opens, while device identity remains composition-scoped;
- unreadable partition entries remain visible, later entries do not
  renumber, and recognized refusals retain their owning evidence;
- blank, empty-partitioned, partitionless-volume, unknown, and invalid
  images remain distinguishable, and the report *states* which one rather
  than leaving it to be reconstructed from empty lists;
- every declared region carries its raw type value and a reading of that
  value which a consumer can quote in a refusal, including for a type this
  feature does not read;
- total composed-volume count and host-readable filesystem-volume count are
  separately available, with the latter preserving U4's present reporting
  use;
- raw and qcow2 disks, MBR primary/extended/logical regions, bare volumes,
  and FAT12/FAT16 filesystems retain their current tested behavior;
- U3 file listing, reading, writing, directory creation, commit, and rollback
  select the same volume identity supplied by inspection;
- Rust, C, and Python expose equivalent semantics and the former geometry
  API no longer exists;
- generated representations, examples, tests, README, architecture, and U4
  agree with the delivered surface; and
- the core remains runtime-dependency-free.

## Deliberately absent

- GPT, NTFS, and new image, partition, volume, or filesystem formats.
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
- Compatibility aliases for the pre-1.0 geometry surface.

Those capabilities remain architecturally admitted where the pledged
principles say so, but each needs its own use case, feature, and surface
vetting. D5's deferred multi-device topology is not reopened by F20.
