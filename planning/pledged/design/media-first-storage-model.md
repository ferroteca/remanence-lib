<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The media-first storage model

The successor object model of remanence's storage world, argued in the
owner's design discussion of 2026-08-05. It succeeds
[storage-model-and-vocabulary.md](storage-model-and-vocabulary.md),
whose feature cycle (F46, F48–F52) is delivered and whose sweep this
document completes by replacing it as the standing design. Pledged at
the owner's direction, not implementation approval: each piece lands
through its own gate, and the document is swept when its features
deliver.

**What it is designed around.** The medium is the content handle — the
node the user holds — and everything the recording can answer, answers
on it. Machines, devices, and the attachment edge exist for the
problems only they can own: the machine namespace (P35), multi-region
volumes (P17), and the hardware-emulation seam with its multi-drive
hook (P15). Their surface structure is fixed now so their plumbing
lands later without user-facing model changes.

**One sentence per tier:** a session owns two pools (machines, media);
a machine owns devices; a device links at most one medium; a medium
owns its evidence (partitions, spaces, files) and answers for its
recording.

## The model from the root

```
Session::new()                                 OWNED root
   │
   ├── MACHINE POOL — configuration
   │     .add_machine("pc")? · .machine("pc") → Option · .machines()
   │     .release_machine("pc")?      cascade: eject each device (sever; media
   │                                  stay pooled) → release devices → machine
   │
   ├── MEDIA POOL — state, independent of every machine
   │     .load_media(source, format)?  → &mut Medium    declared reading —
   │                                     the source: the caller's own opened
   │                                     std::fs::File(s), or File(s) from
   │                                     another medium's namespace
   │     .new_media(kind)?                     → &mut Medium   authored
   │     .medium(id) → Option · .media()
   │     .release_media(id)?          severs its own link if inserted, then ends
   │                                  the claim and discards uncommitted state —
   │                                  THE one state-destroying verb
   ▼
Machine "pc"                                   identity (a real name, never null)
   │
   ├── DEVICE POOL — configuration
   │     .add_device(hdd0)? · .device(hdd0) → Option · .devices()
   │     .release_device(hdd0)?       ejects first — sever, never destroy
   │
   │   ★ .namespace()?  → MachineSpace        P35's seat (plumbing later)
   │     .compose_dos_letters()               its derived-mapping half, delivered
   ▼
StorageDevice hdd0                             config only: slot · device type
   │     .insert(media_id)? / .eject()?       the ONE edge crossing config→state:
   │     .medium() → Option<&mut Medium>      device-type equality at insert;
   │                                          eject severs — claim and buffered
   │                                          writes SURVIVE in the pool
   │   ★ presentations later: .chs()? · .hardware(contract)?     P15's seat
   ▼
Medium                                         pool-owned, holdable — ALL content
   │     device_type · article · recording facts (geometry…) · mode · assurance
   │     .get_sector(…)? / .put_sector(…)?  · .read_at()? · .commit()? / .rollback()?
   │     .bitstream()? / .bytestream()?       kind-declared rules, no arguments
   │
   ├── PARTITION POOL — populated under the DEVICE SPEC, never probed
   │                                   the spec's scheme (MBR, GPT) checked
   │                                   at load; the schemeless types bear
   │                                   the direct partition
   │     .partition(1) → Option · .partitions()
   │       ▼
   │     Partition — raw type + reading · extent · role · .active()
   │       │   .as_type(PartitionType::DosPrimary)?   a declared reading, checked
   │       │   .volume()     → Option<&mut StorageSpace>   the vantage doors:
   │       │   .filesystem() → Option<&mut StorageSpace>   same node behind both
   │       │   .filesystem_as(id)?            the declared reading where no
   │       │                                  partition type determines one
   │       ▼
   │     StorageSpace — ONE node, TWO vantage traits (D26)
   │           volume: .read_at()? / .write_at()?   within the extent
   │           filesystem: .files()? · .stat()? · .get_file()? · .label()?
   │             ▼
   │           File — .bytes()? · .read_at()? · .write_at()?
   ▼
   (recursion: a File's content loads as a medium of its own — see the
    source shapes of load_media)
```

## The rules

1. **Ownership.** `Session` is the one owned handle. Media are
   pool-owned; every other node is a borrow that cannot outlive its
   parent. Nobody ever holds what the session does not.
2. **Answers.** In-memory lookup → `Option`: absence is an answer, no
   error manufactured. Creation, linking, and everything touching
   evidence → `Result`: the world can refuse, and refusals are named
   (P10). `stat` keeps `Result<Option<…>>` — failure to read and
   honest absence are different answers (U3). *Delivered: the machine,
   device and media pools answer with `Option`, and so do the partition
   pool's lookup and the two vantage doors.*
3. **Lifecycle.** Create / lookup / release at every pool. No
   get-or-create, no auto-creation, no `require_*` forms — a caller
   who wants a demand writes it. Releasing configuration cascades
   through configuration and severs links; state is destroyed only by
   `release_media` and session drop. Destruction order is construction
   order reversed, every step spoken. *Delivered for the three pools
   that exist: the `require_*` forms are gone, and `release_machine` /
   `release_device` / `release_media` are the removal vocabulary.*
4. **Fact classes.** *Evidence is discovered onto media; declarations
   are configured onto machines; authorship creates media whole.*
   Nothing crosses. A medium's geometry is discovered (format
   declaration, BPB, MBR end-tuples — with provenance, `Undetermined`
   on conflict) or authored at creation — never declared onto an
   existing medium.
5. **Creation grammar.** Every creation verb declares a **concrete
   catalog entry by its enumerated identifier** and the check is that
   entry's own: `load_media` a format (`Format::Zip`,
   `KryoFlux { device: FloppyDrive::… }`, `Qcow2 { device: HardDrive::… }`),
   `new_media` an authored kind, `as_type` a partition type
   (`PartitionType::DosPrimary`). A classification ("archive", "some
   floppy") can check nothing and is refused as a declaration.
6. **Source shapes, and whose lock.** `load_media` reads one artifact
   however reached: an opened `std::fs::File` — the portable file, the
   caller's own open — or a collection of them; a `File` inside another
   medium's namespace, or a collection of those. A format declares
   which source shape it reads (KryoFlux: a collection; zip: one
   artifact), as it declares everything else. **Whoever opens owns the
   lock.** A local artifact's claim is the caller's open, checked for
   exactly one thing — may the library write through it? — and honoured
   exactly: never escalated through a recovered name, never
   supplemented with locks of the library's own, the claim's class
   recorded on the medium. A name recovered from a handle serves
   location only — the commit journal's beside, a backing parent's
   next door — under an identity check that the name still denotes the
   handle's file; a nameless handle (memory-only, deleted-but-open)
   refuses those location-dependent journeys by name and serves
   everything else.
7. **The catalog speaks in device types, and it is a hierarchy.** A
   medium carries **one device type** — the device its content is
   assumed recorded by — an enumerated identity in two levels: the
   **class** (`Floppy`, `HardDrive`; `Optical` and `Tape` reserved
   for the coming families), then the **concrete type** within it —
   `Commodore1541` is a `FloppyDrive`, and is a `DeviceType`. The
   granularity rule cuts the catalog: a device type is the coarsest
   name that fixes the whole addressing surface and recording
   discipline without per-media parameters — what the device fixes
   lives in the type, what varies disk to disk lives on the medium.
   The floppy class: `FloppyDrive::Commodore1541`, the flux product
   class (encoding, speed zones, timings, tracks);
   `FloppyDrive::HeathH17` and `FloppyDrive::HeathH37`, the Heathkit
   product classes (hard- and soft-sectored); `FloppyDrive::Sector`,
   the generic schemeless sector floppy, geometry per-media. The
   hard-drive class — the machine vantages, the partition scheme part
   of the spec: `HardDrive::MbrSector`, `HardDrive::MbrBlock`,
   `HardDrive::Gpt` (GPT implying block addressing by its own
   definition) — so every partition pool populates kind-determined,
   the table checked at load, and no separate scheme declaration
   exists. Archives were recorded by no device: `device_type()`
   answers `Option`, absence being the honest answer. A type the
   library does not know fails to compile; the catalog strings
   (`c1541`, `mbr-block-hd`) survive as display forms in provenance,
   refusals, and the S2/S3 spellings. Types compose **articles**, the
   passive physical substrate (P14 as delivered:
   `flexible-5.25-soft`, `flexible-5.25-hard-10`, `logical-block-512`,
   `virtual`); D19's three facts keep three homes: the article's
   facts in the article, the recording in the device type, the
   drive's behavior in the profile. A format that admits one device
   type carries it bare (`Format::H8d` → `FloppyDrive::HeathH17`,
   `Format::P64` → `FloppyDrive::Commodore1541`); one that records
   many declares it, the field typed by the class its adapter records
   (`KryoFlux { device: FloppyDrive }`, `Qcow2 { device: HardDrive }`)
   — a flux capture of a hard drive fails to compile — and a pairing
   no adapter declares within the class is a named refusal (P14's
   rule, sharpened). `PartitionType` follows the same enumerated
   rule, declared and checked per partition.
8. **Kind-declared rules need no arguments.** Being a `Commodore1541`
   medium *means* reading through the c1541 channel and codec:
   `disk.bytestream()?` takes no policy because the device type carries
   it (P30 declarations reached through the type). The disciplines are
   **flat attributes of the device-type profile — the traits live on
   the medium**: the actions (`read_blocks`, `put_sector`,
   `partition`) take shape as trait surfaces on `Medium`, each
   answering only where the profile's attribute holds —
   `Commodore1541`'s profile bears flux, so its medium answers the
   flux questions — one type bearing several question surfaces
   without the hierarchy encoding them: the D26 vantage-trait
   pattern, generalized. Deviation surfaces
   are deferred. Likewise the reduction that creates a mastered medium
   runs under the profile's declared defaults; a choice no family
   convention can make refuses by name, and the answer goes into the
   `load_media` declaration.
9. **Vantage doors — specified, never probed.** A partition composes
   at most one `StorageSpace` (an identity rule: both doors hand out
   the same node). `.volume()` answers iff the addressable vantage
   exists; `.filesystem()` iff the namespace vantage does. The doors
   are pure lookups because everything behind them was **specified and
   verified**: the scheme is the device spec's own, checked against the
   table at load — the schemeless types (flux, floppies, archives)
   bearing the direct partition with no step; the namespace vantage
   opens under the declared partition type where it determines one
   (`DosPrimary` determines FAT) and under `filesystem_as` where
   nothing does.
   Verification reads evidence to *check* a reading and to *fill
   values* under it; it never picks a reading — probing belongs to the
   question tier. No phantom vantage is ever invented (D26); the
   **direct partition** is the library's own composition act, carried
   as provenance, never as evidence. *Delivered, with the scheme named
   by the medium's kind until device specs arrive to name it (D32).*
10. **The edge.** `insert` / `eject` are the one crossing between
    configuration and state: insert checks the device family against
    the media type and refuses naming both sides; eject severs only —
    claim, geometry, and buffered writes survive in the pool. An empty
    device is first-class configuration (U22 letters it; P15 will
    answer not-ready through it).

## Reserved seats (structure now, plumbing later)

- **P15** — the device hands out *presentations*: family-typed
  capability objects (`.chs()?`, `.hardware(contract)?`, the Commodore
  DOS device seam), never a common method set on `StorageDevice`.
  Mechanism state (P33) will live behind these — never on the medium.
  The P32 addressing-nature amendment is exercised here, when a
  machine view exists to need it.
- **P17** — `partition.volume()` is the arity-1 composition act;
  multi-region volumes arrive as an additive compose-from-several at
  the medium. Nothing moves.
- **P35** — `machine.namespace()` is the machine-composed namespace's
  seat; `compose_dos_letters()` is its derived-mapping half, delivered.

## The use cases

The model is pledged against ten first-class use cases — **U25 through
U34** in [../USE-CASES.md](../USE-CASES.md) — every one carrying the
tier's defining attribute: **no discovery, complete user
specification**, declarations throughout, partition information
included — specified, never probed. U25–U29
are the walks this design was argued over (the loose-capture mastering,
the zip to the CBM DOS directory, the LBA hard disk to a FAT root
listing, COMMAND.COM off a CHS disk, the boot block through the volume
door); U30–U34 close the concept coverage (the reconstructed machine
and its letters, the write and the commit point, authored media, pool
independence and machine teardown, the single-`File` source). The
simplified workflows where discovery does the specifying work belong to
the question tier, proposed; when they arrive they layer above these
walks, which remain valid forever — the declared tier is permanent
surface, not scaffolding.

## The discovery surface stands beside this model (D30)

This design once removed the armed discovery surface — `discover_media`
and its cache sibling, the consumable `Discovery`, `load_discovery`,
`add_device_for`, the image-format `default_device` declaration — from
S1–S3 on the reading that a declared `load_media` already did its work.
**D30 reversed that**: the two verbs answer different questions, and a
caller who does not yet know what an artifact is has no format to
declare. The surface stays, under the constraint that distinguishes it —
**discovery holds the claim and builds no cache** — which F67 makes
real, the delivered verb materializing a whole medium today.

Nothing pledged here depends on discovery either way, and nothing here
reinstates what the question tier still *proposes*: ranked verdicts,
policy templates and gated derivation chains were never delivered and
stay at
[../../proposed/design/question-tier.md](../../proposed/design/question-tier.md)
to be argued as one thing.

## The deferred drawer

Conveniences, restorable without moving the model. The test:
*admissible where it declares, refused where it would guess.*

- The anonymous machine; get-or-create; the `require_*` forms; the
  one-act attach.
- Policy deviation surfaces (nonstandard channel handling, declared
  geometry overrides); plan preview (the loss account before
  creation); capture-inspection reporting (runs, observations, marker
  evidence over a declared collection).
- A `move` act (slot-to-slot transfer without eject/re-insert
  ceremony), if reconstruction journeys make eject+insert sting.
- A media-pool re-find registry beyond `media()`/`medium(id)`.
- Loading by name: a path-taking `load_media` where the library opens
  the local artifact itself — carrying P7's mandatory denial, since
  there the library opens. The pledged form takes only the caller's
  own opened file.

## Ledger — what this design reverses or supersedes

Recorded here so the pledge carries its own diff; D-entries land with
the features that make each real.

- **D23's "one storage handle" is reversed** (annotate, never rewrite):
  the medium becomes the held node; the device dissolves to
  configuration and presentations. D23's actual worry — lifetime
  questions from media held outside the session — is answered
  structurally by the pool. *Delivered: the annotation is on D23.*
- **D24 is superseded**: the file-view load lands as a `load_media`
  source shape, and the question-tier verbs it anticipated are
  drawer'd.
- **D26 is kept and extended**: one node, two vantages — now reached
  through two vantage doors on the partition, same object behind both.
- **D27's `capture_set` spelling is deleted** before ever shipping
  publicly; the capture is a declared collection reading. The
  `archive[/entry]` path syntax stays dead.
- **F50's shapes move**: `load_media` leaves the device for the
  session (creation), `insert`/`eject` become the edge verbs, the
  family check moves to `insert`. The two-acts discipline survives as
  create-then-link.
- **F51's armed surface was to be demoted, and is not** (D30): the
  ask-first journey stays on S1–S3 — `discover_media`, `Discovery`,
  `load_discovery`, `add_device_for`, `default_device` — because
  discovery and loading answer different questions. What it owes is the
  constraint, not the exit: discovery holds the claim and builds no
  cache (F67).
- **The delivered "zero partitions, not one trivial one" ruling is
  reversed** by the direct partition — the evidence answer
  (`partition_scheme: None`) is unchanged; the navigation answer gains
  the declared synthetic member. In-force P19's transparency clause is
  amended accordingly: uniformity of the walk replaces
  resolve-without-selecting. *Delivered: the amendment is in force,
  folded into P19's own text at the root.*
- **The media-type vocabulary is superseded by the device type**:
  `media_type()` and its two-level strings give way to `device_type()`
  answering `Option<DeviceType>` — one device type per medium, a
  two-level identity (the class, then the concrete type; `Optical`
  and `Tape` reserved), `None` the honest answer for archives and
  authored blanks — beside `article()`. The partition scheme moves
  into the hard-drive specs, so `as_scheme` never ships: every
  partition pool populates under its device spec, checked at load.
  P14 gains the device-type catalog and its granularity rule; the P32
  nature amendment is untouched, exercised at the P15 seat.
- **In-force P7 is amended**: "denying write permission to every other
  process is mandatory in all scenarios" becomes *mandatory where the
  library opens; caller-owned where the caller opened*. Local artifacts
  arrive as the caller's own opened files, the caller's lock is their
  safeguard and the library's claim, the library checks what it is
  afforded and honours it exactly, and the claim's class travels on the
  medium's assurance. *Delivered: the amendment is in force, folded into
  P7's own text at the root.*

## Open questions carried

- "Machine" is the wrong name for the machine node; kept until a
  better one arrives.
- The spelling of `Location` addressing. *The declared partition
  reading is settled: `as_type` takes an enumerated `PartitionType` and
  answers `Result<()>`, the check being the whole of it (D32).*
- Whether `release_media` of a linked medium severs (uniform with the
  cascade) or refuses; the cascade argument says sever. *Settled: it
  severs.*
- The C and Python spellings of the vantage doors and the pool
  lookups (`Option` maps to null/None cleanly; the doors should not
  multiply handle types). *Settled. The pool lookups answer null in C
  without touching the error outs — which is why those entry points
  take none — and `None` in Python. The doors take the error outs and
  distinguish the two answers by which of them they touch: null with
  the outs untouched is the absent vantage, null with the outs set is a
  refused composition, and the `is_addressable` / `bears_namespace`
  predicates sit beside them. One handle type is added, the partition
  itself; the space is the one that was already there.*
