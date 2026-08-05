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
   │     .load_media(source, format, intent)?  → &mut Medium   declared reading
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
StorageDevice hdd0                             config only: slot · family
   │     .insert(media_id)? / .eject()?       the ONE edge crossing config→state:
   │     .medium() → Option<&mut Medium>      family/media-type check at insert;
   │                                          eject severs — claim and buffered
   │                                          writes SURVIVE in the pool
   │   ★ presentations later: .chs()? · .hardware(contract)?     P15's seat
   ▼
Medium                                         pool-owned, holdable — ALL content
   │     media_type · article · recording facts (geometry…) · mode · assurance
   │     .get_sector(…)? / .put_sector(…)?  · .read_at()? · .commit()? / .rollback()?
   │     .bitstream()? / .bytestream()?       kind-declared rules, no arguments
   │
   ├── PARTITION POOL — evidence (the direct partition when scheme: None)
   │     .partition(1) → Option · .partitions()
   │       ▼
   │     Partition — raw type + reading · extent · role · .active()
   │       │   .as_type("dos-primary")?       a declared reading, checked
   │       │   .volume()     → Option<&mut StorageSpace>   the vantage doors:
   │       │   .filesystem() → Option<&mut StorageSpace>   same node behind both
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
   honest absence are different answers (U3).
3. **Lifecycle.** Create / lookup / release at every pool. No
   get-or-create, no auto-creation, no `require_*` forms — a caller
   who wants a demand writes it. Releasing configuration cascades
   through configuration and severs links; state is destroyed only by
   `release_media` and session drop. Destruction order is construction
   order reversed, every step spoken.
4. **Fact classes.** *Evidence is discovered onto media; declarations
   are configured onto machines; authorship creates media whole.*
   Nothing crosses. A medium's geometry is discovered (format
   declaration, BPB, MBR end-tuples — with provenance, `Undetermined`
   on conflict) or authored at creation — never declared onto an
   existing medium.
5. **Creation grammar.** Every creation verb declares a **concrete
   catalog entry by its stable id** and the check is that entry's own:
   `load_media` a format (`zip`, `kryoflux { disk }`, `qcow2 { disk }`),
   `new_media` an authored kind, `as_type` a partition type. A
   classification ("archive", "some floppy") can check nothing and is
   refused as a declaration.
6. **Source shapes.** `load_media` reads one artifact however reached:
   a host path; a collection of host paths; a `File` inside another
   medium's namespace; a collection of `File`s. A format declares
   which source shape it reads (KryoFlux: a collection; zip: one
   artifact), as it declares everything else.
7. **The media catalog has two levels.** **Media types** are concrete —
   `c1541-disk`, `h17-disk`, `chs-hd-disk`, `lba-hd-disk`,
   `zip-archive`, `blank-5.25-soft` — and compose **articles**, the
   passive physical substrate (P14 as delivered:
   `flexible-5.25-soft`, `logical-block-512`, `virtual`). D19's three
   facts keep three homes: the article's facts in the article, the
   recording in the type, the drive's behavior in the profile. The
   recognizing format names the concrete type (P14's rule, sharpened).
8. **Kind-declared rules need no arguments.** Being a `c1541-disk`
   *means* reading through the c1541 channel and codec:
   `disk.bytestream()?` takes no policy because the media type carries
   it (P30 declarations reached through the type). Deviation surfaces
   are deferred. Likewise the reduction that creates a mastered medium
   runs under the profile's declared defaults; a choice no family
   convention can make refuses by name, and the answer goes into the
   `load_media` declaration.
9. **Vantage doors.** A partition composes at most one `StorageSpace`
   (an identity rule: both doors hand out the same node).
   `.volume()` answers iff the addressable vantage exists;
   `.filesystem()` iff the namespace vantage does. The doors are pure
   lookups because composition (P17) and recognition (P18) ran at
   partition-pool population. No phantom vantage is ever invented
   (D26); a medium with no scheme bears the **direct partition** — the
   library's own composition act, carried as provenance, never as
   evidence.
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

## Coded use cases

### 1 — A 1541 disk from capture files on the user's own filesystem

The collection is host paths — nothing here is inside any image or
archive. The user names the format and the disk it records; the member
grammar, completeness, stream grammar, and the c1541 claim are all
checked, and the reduction runs under the profile's declared defaults.

```rust
let mut session = Session::new();

let disk = session.load_media(
    &["captures/pcs00.0.raw", "captures/pcs00.1.raw" /* … all 168 … */],
    Format::KryoFlux { disk: "c1541" },
    AccessIntent::Read,
)?;
assert_eq!(disk.media_type(), "c1541-disk");

let mut first = [0u8; 1];
disk.bytestream()?                       // kind-declared channel + codec
    .location(Location::track(1))?       // the family's first location
    .read_at(0, &mut first)?;            // byte 0: the first FRAMED byte —
                                         // nothing before sync is a byte
```

### 2 — The same disk from a zip, then the CBM DOS directory

```rust
let mut session = Session::new();

let arc     = session.load_media("pcs_disk1.zip", Format::Zip, AccessIntent::Read)?;
let members = arc
    .partition(0).expect("an archive bears its direct partition")
    .filesystem().expect("an archive's content is its namespace")
    .files("")?;
let disk    = session.load_media(members, Format::KryoFlux { disk: "c1541" })?;

// flux media record no partition scheme, so: the direct partition —
// and the filesystem door does real work: a protected or blank disk
// honestly bears no namespace, and everything beneath stays readable.
let Some(cbm) = disk
    .partition(0).expect("an unpartitioned disk bears its direct partition")
    .filesystem()
else {
    return Ok(());   // absence is the answer; sectors and streams still answer
};

println!("{}", cbm.label()?);            // 0 "PINBALL     " PC 2A — the BAM
for entry in cbm.files("")? {            // header, and directory order is
    println!(                            // evidence (U4)
        "{:16} {:>4} {}",
        entry.name,                      // PETSCII: raw + reading, untransliterated
        entry.fact("blocks"),            // CBM records size in blocks
        entry.fact("type"),              // PRG · SEQ · USR · REL, flags beside
    );
}
```

This is the file-access presentation (P18/P19), not CBM DOS running:
`LOAD"$"` — the directory as the drive's ROM synthesizes it — belongs
to the future Commodore DOS device seam (P15).

### 3 — A qcow2 as an LBA hard disk: first partition, declared primary DOS, root listing

```rust
let mut session = Session::new();

let disk = session.load_media("dos_hd.qcow2", Format::Qcow2 { disk: "lba-hd" },
                              AccessIntent::Read)?;
assert_eq!(disk.media_type(), "lba-hd-disk");    // MBR discovered as evidence

let part = disk.partition(1).expect("the image's MBR declares entry 1");
part.as_type("dos-primary")?;            // a declared reading, checked against
                                         // the raw type byte — 0x06 bears it,
                                         // 0x05 refuses naming both sides

let fs = part.filesystem().expect("a declared DOS primary bears FAT");
for entry in fs.files("")? {
    println!("{:12} {:>9} {}", entry.name, entry.size_bytes, entry.fact("attributes"));
}
```

### 4 — A VDI as a CHS hard disk: the first 8 bytes of `\COMMAND.COM`

```rust
let mut session = Session::new();

let disk = session.load_media("dos_hd.vdi", Format::Vdi { disk: "chs-hd" },
                              AccessIntent::Read)?;
assert_eq!(disk.media_type(), "chs-hd-disk");    // geometry: discovered evidence
                                                 // (BPB, MBR end-tuples), so
                                                 // disk.get_sector(c,h,s) answers

let part = disk.partition(1).expect("the image's MBR declares entry 1");
part.as_type("dos-primary")?;

let mut head = [0u8; 8];
part.filesystem().expect("a declared DOS primary bears FAT")
    .get_file("COMMAND.COM")?            // FAT 8.3 matching, without regard
    .read_at(0, &mut head)?;             // to case — the delivered name rules
```

### 5 — A qcow2 as an LBA/MBR hard disk: 16 bytes of the boot partition's boot block

The volume door's use case: no filesystem is consulted at all.

```rust
let mut session = Session::new();

let disk = session.load_media("dos_hd.qcow2", Format::Qcow2 { disk: "lba-hd" },
                              AccessIntent::Read)?;

let boot = disk.partitions().into_iter()
    .find(|p| p.active())                // the MBR's own boot flag — evidence
    .expect("a bootable image marks one partition active");

let mut block = [0u8; 16];
boot.volume().expect("a DOS partition composes its addressable space")
    .read_at(0, &mut block)?;            // byte 0 OF THE PARTITION — the boot
                                         // block, addressed within the space's
                                         // own extent, no offsets by hand
```

## The question tier is demoted, not deferred

The armed discovery surface — `discover_media` and its cache sibling,
the consumable `Discovery`, `load_discovery`, `add_device_for`, the
image-format `default_device` declaration — is **removed from S1–S3 by
this design's delivery and returns to `proposed/`**, where it is argued
as one coherent thing:
[../../proposed/design/question-tier.md](../../proposed/design/question-tier.md)
(the unified ask, ranked verdicts, policy templates, and the gated
derivation chains). Nothing pledged here depends on it.

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

## Ledger — what this design reverses or supersedes

Recorded here so the pledge carries its own diff; D-entries land with
the features that make each real.

- **D23's "one storage handle" is reversed** (annotate, never rewrite):
  the medium becomes the held node; the device dissolves to
  configuration and presentations. D23's actual worry — lifetime
  questions from media held outside the session — is answered
  structurally by the pool.
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
- **F51's armed surface is demoted**: the ask-first journey leaves
  S1–S3 whole — `discover_media`, `Discovery`, `load_discovery`,
  `add_device_for`, `default_device` — and returns to `proposed/` as
  the question tier. A demotion, not a drawer entry: the mechanism
  goes back through the gate it came in by.
- **The delivered "zero partitions, not one trivial one" ruling is
  reversed** by the direct partition — the evidence answer
  (`partition_scheme: None`) is unchanged; the navigation answer gains
  the declared synthetic member. In-force P19's transparency clause is
  amended accordingly: uniformity of the walk replaces
  resolve-without-selecting.
- **The media-type vocabulary sharpens**: `media_type` answers the
  concrete type (`c1541-disk`), `article` the substrate. P14 gains the
  two-level catalog; the P32 nature amendment is untouched, exercised
  at the P15 seat.

## Open questions carried

- "Machine" is the wrong name for the machine node; kept until a
  better one arrives.
- The spelling of the declared partition reading (`as_type`) and of
  `Location` addressing.
- Whether `release_media` of a linked medium severs (uniform with the
  cascade) or refuses; the cascade argument says sever.
- The C and Python spellings of the vantage doors and the pool
  lookups (`Option` maps to null/None cleanly; the doors should not
  multiply handle types).
