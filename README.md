# remanence-lib

[![License](https://img.shields.io/badge/license-GPL--3.0--only-blue.svg)](LICENSE)

A self-contained disk image analysis library in Rust. A `Session` owns
two pools — machines, which are configuration, and media, which are
state — and **the medium is the content handle**, the node you hold and
the node everything about a recording answers on.

`session.load_media(source, format)` is the declared reading: you hand
over your own opened file and name what it is — `raw`, `qcow2`, `vdi`,
`h8d`, `zip`, `7z` — and that one format's adapter checks the
declaration against the evidence, refusing by name where it cannot bear
it. **Whoever opens owns the lock**: your open is the claim, the library
asks it exactly one question (may it write?), honours the answer, and
adds no lock of its own. A name is recovered from the handle for
location alone — where a commit journal lands, where a backing chain's
parent is looked for — under an identity check.

Machines and devices are the configuration beside that. A machine takes
a drive as concrete as the one it actually had — a Commodore 1541, a
Heathkit H-17, a hard disk, an archive slot — and `device.insert(id)`
links a pooled medium into it, refusing a medium belonging in another
drive and naming both sides. `device.eject()` **severs only**: the claim
and everything buffered survive in the pool, so a disk outlives the
drive it sat in and a machine can be torn down without touching one.
`session.release_media(id)` is the one verb that destroys state. An
empty drive is configuration in its own right.

**Every pool runs the same three verbs — create, look up, release.** A
lookup answers with absence: `session.machine("pc")`,
`session.device(hdd0)` and `session.medium(id)` hand back an `Option` in
Rust, null in C without touching the error outs, and `None` in Python,
because a question about what a session holds has an honest negative
answer and nothing is manufactured to report it. Creation still refuses
by name — a duplicate machine identity, a slot already taken, the empty
identity — and so do the removals, which are all spelled `release_*`:
`release_machine` cascades (each device ejected, so the media stay
pooled, then the devices, then the machine), `release_device` ejects
first, and `release_media` severs its own link and then ends the claim.
There is no `require_*` form anywhere: a caller who wants a demand
writes it, where they know what the absence means.

`discover_media` answers the other question — *what is this?* — before
any of that: the exact medium, the drives served it, and the drive the
image format declares for the disks it records. It opens the artifact by
name, so there the library's own claim applies in full, and it hands
back a discovery a load consumes so nothing is opened twice; where a
format declares a drive, `add_device_for` composes the acts in one.

A load identifies the layers of the artifact's nesting: the archive
wrapper, image format, physical media geometry, and probable filesystem,
each with comparable confidence and human-readable evidence. Executable,
role-specific adapters recognize and validate formats; ambiguous
strongest matches remain unknown rather than being resolved by catalog
order.

**File access lives on one node, and a partition is what reaches it.** A
medium's content is addressed through the partition that composes it, by
the scheme's own ordinal: `medium.partition_scheme()` names the scheme
the pool was populated under, `medium.partitions()` hands back every
entry that scheme declares, and `medium.partition(1)` is the one you
mean. The pool is established when the medium is loaded and is evidence
from then on, so nothing re-reads a table behind you and nothing is
discovered on demand. A medium recording no scheme bears the **direct
partition** at ordinal 0 instead — the library's own composition of the
whole content, stated as provenance and never offered as something the
medium said.

Two doors open onto the one `StorageSpace` a partition composes.
`volume()` is that space read and written by position within the
partition's own extent; `filesystem()` is the same space reached by the
names it holds. Both hand out the same node, so which question you asked
changes nothing about what comes back, and both answer with an `Option`
because they are lookups rather than attempts: everything behind them
was specified when the pool was established and verified there, so an
absence is a fact about the partition rather than a read that failed.
The namespace opens through that door where the declared partition type
determines one, and through `filesystem_as("hdos")` where nothing does —
the reading is yours and the check is the library's. The file verbs live
on the `StorageSpace` both doors hand out, never on the medium, and
FAT12/FAT16 volumes and the HDOS catalog of a Heathkit `.h8d` are
reached the same way.

```rust
// A partitioned disk: what the scheme declared, and the entry you mean.
println!("{:?}", medium.partition_scheme());     // Some(PartitionScheme::Mbr)
for declared in medium.partitions() {
    println!("{} {} {:?}", declared.ordinal(), declared.placement(),
             declared.type_reading());
}

// Your reading of the type, checked against the byte the table records.
let partition = medium.partition(1).expect("the table declares entry 1");
partition.as_type(remanence::PartitionType::DosPrimary)?;
let mut files = partition
    .filesystem()
    .expect("a DOS data partition determines FAT");
for entry in files.entries("")? {
    println!("{} ({} bytes)", entry.name, entry.size_bytes);
}

// The same partition by position, for what no name reaches.
let mut boot_record = [0u8; 512];
let mut volume = medium
    .partition(1)
    .expect("the table declares entry 1")
    .volume()
    .expect("a data partition composes an extent");
volume.read_at(0, &mut boot_record)?;
```

**Geometry is discovered, and the sector verbs address in it.** What a
drive fixes lives in its type; how one disk was laid out varies disk to
disk, so `medium.geometry()` is *read* off the artifact when the medium
loads and is evidence from then on — nothing declares a geometry onto a
medium that exists. Four sources may speak, and each reading keeps where
it came from: the image format's own declaration (an `.h8d` records 40
cylinders of one side at ten 256-byte sectors) or the block size a raw
load declared, a FAT boot record's recorded sectors-per-track and heads,
the partition table's end tuples where one solves against the extent the
same entry declares, and arithmetic over the content's extent for the
cylinder count. **Sources that disagree settle nothing**: the state is
`Undetermined`, both readings come back, and neither is preferred.
Nothing states one at all is `Unstated`, which is a different fact and
kept as one.

`get_sector` and `put_sector` address in what that established — on the
device types whose `addressing()` says `sector`, which is every floppy
and the CHS hard drive. Cylinders and heads number from zero and
**sectors from one**, because that is the recording's convention rather
than this library's. Everything else refuses by name, its own rule set
saying which: a block-addressed drive or a medium no device recorded has
no such coordinates at all, an unsettled geometry has none to address in
and points at the readings, and a coordinate the geometry does not cover
— or one it covers and the content does not hold — is refused rather
than answered with zeros. A write buffers until `commit()` like every
other write.

```rust
// What the artifact said about its own coordinates, and who said it.
let geometry = medium.geometry();
match geometry.determined() {
    Some(coordinates) => println!("{coordinates}"),   // 131 cylinders of 16 heads…
    None => println!("{}: {:?}", geometry.state(), geometry.conflicts()),
}
for reading in geometry.readings() {
    println!("  [{}] {}: {}", reading.source, reading.at, reading.detail);
}

// One sector in those coordinates: cylinder 0, head 0, sector 1 is the
// first record of the recording, wherever the image format puts it.
let mut sector = [0u8; 512];
medium.get_sector(0, 0, 1, &mut sector)?;
medium.put_sector(0, 0, 1, &sector)?;     // buffered until commit
```

**An archive is a medium like any other.** A `.zip` or `.7z` is loaded
by its declared grammar and may be seated in an archive-family device,
and its content **is** its namespace — the same node a disk's filesystem
is reached through, with no archive journey of its own. Its own vantage
is that namespace: an archive bears the direct partition like every
other medium, extent-less and synthetic, and composes no volume and no
sector beneath it, so the verbs that address a space refuse by name
rather than inventing a phantom volume.
An entry recognized as an artifact of its own is opened from the file
view that names it and loaded into a device of its own — in a machine of
its own where one is being reconstructed, since the host's archive was
never part of the machine whose disk it holds.

Each grammar sits behind its own catalog adapter, and an entry is
produced bounded: one stored uncompressed is read in place from the
claimed archive, and a coded one decodes once into private session
storage — one member of a solid 7z folder without materializing the rest.
Either way the child holds what it reads, so ejecting the archive under a
disk already loaded from it takes nothing away. Archives are read and not
written: a write would have to be encoded back into the grammar's own
form, and no adapter claims that.

A `CaptureSet` opens a KryoFlux capture of a floppy disk — one stream
file per head per drive-step position, archived together — as the one
logical capture it is, rather than as a hundred and sixty-eight
unrelated members. It reads each stream's flux, its asynchronous index
records, its transport control records and its transfer result, keeps
the flux recorded before the first index and after the last, and bounds
the circular observations the indices bracket. The two heads stay two
locations: nothing merges them into an ideal disk, chooses a cleanest
pass, or averages a timing. An incomplete, duplicate, contradictory, or
unrelated member refuses the whole set by name, with the catalog
evidence that refused it. The decoded capture lives in private session
storage and is addressed a bounded section at a time, so a forty-megabyte
capture opens under whatever working set the caller declared.

An opened capture can then be recognized: every enrolled drive profile
is consulted and what claims the capture is ranked, never resolved by
catalog order. A profile is where a family's recording conventions are
declared — the knowledge a capture does not contain — and the probe
reads only interval lengths and the patterns they form, resolving no
bit, assembling no byte, naming no sector and validating no checksum.
What comes back is a bounded confidence and the observations that
produced it: which of the family's zones were recovered and what each
location holds, the derived cell against what the zone claims, the seam
located as an angle, and a named reason for every position not claimed.

A recognized capture then reduces to a **remanence image** — the
physical facts of the disk's surfaces, fit to nothing and carrying no
clock. The reduction is gap-first and works on the strength of all the
evidence rather than the choice of one revolution: every revolution of
every location aligned by gap correspondence, the cell lattice measured
from the intervals themselves, the angles integrated so the circle
closes exactly, coherence decided per transition with indeterminacy
recorded rather than repaired, and adjacent steps carrying the same
recording merged under measured agreement — the fat track measured,
never asserted. It resolves in two stages, a plan that computes
everything and writes nothing and an execution that produces the image,
and the plan carries the complete account of what the image will not
carry, in the capture's own terms and before anything exists to carry
it.

That image is the disk from there on. It renders to **d64, g64 and
p64** — each claimed twice, as a description that writes nothing and a
write that does both, and each stating what its destination did not
carry — and it writes back to the library's own `.remanence` artifact.

An image — or the medium a P64 holds at rest — can then be read the way
a drive reads it. An image carries no clock, so the ladder stands on
the served projection of it, one multiply per point at the family's
reference frame. The family's read channel clocks the medium's
pulses into a circular, track-relative **hardware bitstream**, and its
declared group code resolves that into the family's **encoded
bytestream**. Both rules are the drive profile's: the cell comes from
the density zone the family declares, the counter restarts at every
transition and admits one within half a cell of a boundary, and the
bytes come from the published sixteen-symbol GCR table. Every bit says
whether it was recorded or resolved by a declared rule; a location no
zone covers is refused rather than clocked at a neighbour's rate; and a
pattern the table does not assign keeps its own bits rather than
becoming the nearest value. Neither layer assigns anything above a byte:
no byte is a header, a data field, a sector or a file, and the framing
landmark the codec locates says where bytes begin and nothing about what
follows it.

**The rung above them is where that ends** — and it ends by a layer
stating what it derives rather than by either of those two having
quietly meant more. The **sector layer** reads the recording's own
records out of the bytestream under the grammar the family declares:
which byte opens a header block and which a data block, how long each
is, where the header states its track, its sector and the disk
identity, and which bytes each checksum covers. Pairing is grammatical
rather than measured — the family writes one sync ahead of each block,
so a record's data block is the block the recording carries next — and
every claim comes back with where it sits, the address it states for
itself, both stated checksums beside both computed ones, and how many
of its bytes the codec could not resolve. Reading by the recording's
own track and sector answers where one readable claim, or several
agreeing ones, hold it; an address nothing states, an address no claim
of which reads, and an address readable claims disagree about are each
a refusal naming the rule it broke. Nothing is repaired and no block is
filled in.

**And the disk's own directory is above that.** A recording composes the
direct partition like any other medium, and `filesystem_as("cbmdos")`
over it opens the same namespace node a disk image's partition hands
out — the file verbs live on one node and nowhere else — the space
carrying the BAM header as its label, the directory in the order
it was written, PETSCII names read beside the sixteen bytes as recorded,
and the CBM facts each entry declares: PRG or SEQ or USR or REL, the
locked and never-closed flags, the block count, and the slot it was read
from. A file's size is established by walking its chain rather than
trusting the count, and a chain that reaches a block the recording never
yielded says so on the entry and refuses on the read, so one unrecovered
sector qualifies its own file instead of taking the listing down with
it. `LOAD"$"` — the directory as the drive's ROM synthesizes it — is
deliberately not this: that is a Commodore DOS device, and this is the
filesystem the disk records.

An image can be saved as a P64, and a P64 opened back.
The container's grammar and its own adaptive range coder are the
adapter's claim, stated in the module from the published format
description: the version is validated before anything else is touched,
every chunk is checked against its stored checksum and all of them
against the header's, and a version, reserved flag bit, or chunk
signature past the claim is refused by name. The adapter says what the
container will carry before it writes — a P64 records no policy, no
provenance and no located origin, and each of those is named and
counted first — and refuses a medium its claim cannot encode rather
than approximating one into it. An existing destination is a named
refusal, never an overwrite, and the artifact is built beside its
destination and moved into place whole, so an interruption leaves the
destination absent rather than half a file.

The library is dependency-free at runtime, including its own ZIP
central-directory reader, 7z header reader, RFC 1951 (DEFLATE) and
LZMA/LZMA2 decompressors, and native qcow2 v2/v3 and VDI drivers.

Beyond identification, loading a raw, qcow2 or VDI disk image into a
storage device takes a claim under a declared intent: a read session
denies writes to every other process while admitting other readers; a
writable session admits no observers at all; and an image whose claim
cannot be secured — one held by a running VM, say — is refused outright
at the load. It
inspects the disk as a layered report — the block-active device, what
its leading structure turned out to be, any recognized partition
schema, every region that schema declares, every volume composed, and
every filesystem recognized on one, each fact at the seam that owns it.
The report keeps every one of those facts and is a **view derived from
the partition pool**, so what it states and what the content is reached
through cannot drift apart. Every declared region carries both its raw
type value and a reading of what that value declares, so a type the
release does not read still explains itself. It gives each reported
volume an opaque stable identity, which is the report's own and stays
the report's own; content is addressed by the partition's ordinal
instead, and the file verbs there — list, stat, read, write (overwriting
in place), and create directories with their missing parents — run under
a commit point: nothing touches the image until `commit`, and `rollback`
discards everything. A qcow2 whose content
lives partly in a backing chain — raw or qcow2 members, relative
paths resolved from the image that names them — opens as one composed
disk, every backing member claimed immutable for the session's life.
Writes allocate copy-on-write into the top image only and preserve the
backing relationship; a missing member, a cycle, or a chain past the
claimed depth is refused by name.
A VDI is an ordinary image of the same stack: its version is validated
before anything else is touched, its dynamically allocated, fixed and
differencing image types are claimed by name — the one other type the
format defines, undo, refused rather than attempted — and a block the
block map marks discarded reads as the zeroes the format says it holds,
never confused with an allocated block that happens to hold them. A write
into a block a dynamic image never allocated allocates one, inside the
commit and never during a read.
A VDI differencing image opens as one composed disk too, and the way it
finds its parent is the format's own: a VDI records the parent's
**identity** and no path at all, so the image declaring that identity is
searched for beside the child and in the directory above it, and that
identity is what checks the file the search found. A file standing where
the parent should be whose identity does not match is refused by name
rather than read as a substitute — evidence a backing path alone cannot
give. A missing parent, a cycle, a chain past the claimed depth, or a
parent whose own version or type falls outside the claim is refused at
the open.
Where the stopped machine ran DOS, it also answers which drive letter
named which volume. A DOS machine persisted no such map — its letters
were assigned at boot by a rule over the machine's own configuration, and
nothing on the disks records the result — so the mapping is derived: the
machine facts are the caller's, either asserted (which medium is in which
floppy slot, which disks are attached in what order, where a CD-ROM
driver was declared) or read from a machine's own device set in the order
its devices were added, with families no claimed rule letters passed over
by family; the library applies one named assignment rule over the reports
already inspected, and the answer says which volume each letter names.
The rule is a claim like any other: two MS-DOS variants are claimed by
name, stating the variant settles the map, and stating none leaves a
letter the variants disagree on undetermined with each rule's answer
rather than averaged into one that is neither. `LASTDRIVE`, `SUBST`,
`JOIN`, `ASSIGN`, a resident block-device driver and a network redirector
are outside every claimed rule, and a letter one of them could have
changed is reported undetermined rather than approximated.

An image that is short of what it declares is neither accepted whole nor
thrown away whole. Every open states what it established — verified, or
degraded with the condition that narrowed it — before anything is read,
so a caller meets a deficiency by being told rather than by an operation
failing halfway. A raw image whose FAT boot record declares more bytes
than the file holds opens degraded and read-only for the session's whole
life: the declaration, what the source actually holds, the first byte
that is missing and the exact extent that reads all come back as
evidence. Directories and files that are wholly present read normally; a
file whose cluster chain runs into the missing tail is refused by name
with its range, never clipped, zero-filled or served in part; and every
write, every commit and every other mutation is denied with the same
stable condition, in Rust, C and Python alike. Where the shortfall leaves
no safe bound to state — a boot record declaring two different sizes —
the medium is refused outright rather than read in part.

The in-force vision — what the library is for (U-numbers) and the rules
it holds itself to (P-numbers) — is in [USE-CASES.md](USE-CASES.md) and
[ARCHITECTURE.md](ARCHITECTURE.md).

## Layout

```
crates/
  remanence/        # the core library (pure Rust, no runtime dependencies)
  remanence-ffi/    # C ABI: staticlib + cdylib, cbindgen header for C and C++
  remanence-py/     # Python module (PyO3), built with maturin
```

## Building

```bash
cargo build            # core + C FFI (generates crates/remanence-ffi/include/remanence.h)
cargo test             # the full test suite
```

The Python module is excluded from default builds so a Python toolchain is
never required for library work:

```bash
cargo build -p remanence-py    # needs Python >= 3.10
# or, for distributable artifacts (sdist + abi3 wheel into crates/remanence-py/dist/):
uv build crates/remanence-py
```

uv drives the maturin build backend in an isolated environment, so no
tooling beyond uv itself needs installing. Publishing is `uv publish`
from that `dist/`, and is owner-gated. **The Python package is tested
on Windows only, for now**, and its packaging classifiers say so; the
POSIX code paths exist and should stay correct, but they are
unexercised and unclaimed.

An example C consumer is at
[crates/remanence-ffi/examples/identify.c](crates/remanence-ffi/examples/identify.c),
with build instructions in its header comment.

## Using the library

```rust
// A session owns two pools: machines, which are configuration, and
// media, which are state. The medium is the content handle. The source
// is your own open file — whoever opens owns the lock — and the format
// is your declaration, checked by that format's own adapter.
// The declaration carries the device the content was recorded by: an
// h8d records a Heathkit H-17 and so carries the type bare, while a
// format that records several — `Format::Qcow2 { device:
// HardDrive::MbrBlock }` — takes the caller's word for which.
let mut session = remanence::Session::new();
let medium = session.load_media(
    std::fs::File::open("disk.h8d")?,
    remanence::Format::H8d,
)?;
let identification = medium.identify();
for layer in &identification.layers {
    println!("{:?} {} ({}%)", layer.kind, layer.id, layer.confidence);
}
let disk = medium.id();

// Seating it in a drive is a separate act, and the drive is the slot
// rather than the disk: ejecting severs and takes nothing away.
let mut device = session.add_device(remanence::FloppyDrive::HeathH17)?;
println!("{}", device.attachment());      // heathfloppy0
device.insert(disk)?;

// Content is reached through the partition that composes it. This image
// records no scheme, so the direct partition is the whole of it, and
// nothing determines a namespace over it — the reading is declared here
// and the library checks it.
let medium = session.medium_mut(disk).expect("pooled");
let mut filesystem = medium
    .partition(0)
    .expect("the direct partition")
    .filesystem_as("hdos")?;
for entry in filesystem.entries("")? {
    println!("{} ({} bytes)", entry.name, entry.size_bytes);
    // Whatever this filesystem states past name, kind and size, in its
    // own spelling: an HDOS catalog date, its flag letters.
    for fact in &entry.declared {
        println!("    {} = {}", fact.key, fact.value);
    }
}
let bytes = filesystem.get_file("HDOS.SYS")?.bytes()?;

// What the open established about the evidence beneath it, before
// anything is read from it — including whose open the claim is.
let assurance = session.medium(disk).expect("pooled").assurance();
println!("{} {:?} {}", assurance.outcome, assurance.condition, assurance.claim);
for line in &assurance.evidence {
    println!("  {line}");
}

// An archive is a medium: declared by its own grammar, and its content
// is its namespace — the direct partition it bears opens onto it.
let archive = session
    .load_media(std::fs::File::open("captures.7z")?, remanence::Format::SevenZip)?
    .id();
let mut content = session
    .medium_mut(archive)
    .expect("pooled")
    .partition(0)
    .expect("an archive bears its direct partition")
    .filesystem()
    .expect("an archive's content is its namespace");
for entry in content.entries("")? {
    println!("{} ({} bytes)", entry.name, entry.size_bytes);
}

// An entry recognized as an artifact of its own becomes a medium of its
// own, under the claim the archive already holds — and it outlives the
// archive, which is what makes the pool independent of every machine.
let member = session
    .medium_mut(archive)
    .expect("pooled")
    .partition(0)
    .expect("an archive bears its direct partition")
    .filesystem()
    .expect("an archive's content is its namespace")
    .get_file("track00.raw")?
    .discover()?;
// A KryoFlux stream is bytes to every adapter here, so the discovery
// says "a raw image" and asserts no device — the declaration is yours,
// through the `_as` door the plain one points at.
let inner = session
    .load_discovery_as(member, remanence::DeviceType::HardDrive(
        remanence::HardDrive::MbrSector))?
    .id();
session.release_media(archive)?;          // the disk keeps answering

// Asking what an artifact is, before a machine has been configured for
// it. The discovery holds the claim under which that was established;
// a load consumes it, so nothing is opened twice.
let discovery = remanence::discover_media("disk.h8d", remanence::AccessIntent::Read)?;
println!("{} in {:?}", discovery.article(), discovery.accepting_devices());
match discovery.device_type() {
    Some(device) => println!("the format records a {}", device),
    None => println!("declare one of {:?}", discovery.device_types()),
}
let found = session.load_discovery(discovery)?.id();
session
    .add_device(remanence::FloppyDrive::HeathH17)?
    .insert(found)?;

// Or the acts at once, where the format records one device type —
// refused by name where it records several and named none.
let drive = session.add_device_for("disk.h8d", remanence::AccessIntent::Read)?;

// The drive letters a DOS machine would have presented: the machine
// facts are the caller's — here its own device set, in attachment
// order — and the assignment rule is the library's.
let letters = session
    .anonymous_mut()
    .compose_dos_letters(Some(remanence::DosAssignmentRule::MsDos5), &[])?;
for mapping in &letters.mappings {
    println!("{}: {:?}", mapping.letter, mapping.outcome);
}

let capture = remanence::CaptureSet::open("captures.7z")?;
for member in &capture.inspect().members {
    let run = &member.runs[0];
    println!(
        "step {} head {:?}: {} transitions, {} revolutions",
        member.position.numerator,
        member.head,
        run.transitions,
        run.observations.len()
    );
}
```

```python
import remanence
for device in remanence.device_slots():
    print(device.id, device.name, device.device_class, device.article,
          device.scheme)

print(remanence.formats())          # what a declaration may name

session = remanence.Session()
# Your own open, and your declaration of what it is. The descriptor is
# duplicated, so closing the Python file leaves the claim intact.
with open("disk.h8d", "rb") as source:
    medium = session.load_media(source, "h8d")   # one device recorded it,
                                                 # so it needs no `device=`
print(medium.assurance.outcome, medium.assurance.claim, medium.mode)
for layer in medium.identify().layers:
    print(layer.kind, layer.id, layer.confidence)

# What the artifact said about its own coordinates, and who said it.
geometry = medium.geometry
print(geometry.state, geometry.cylinders, geometry.heads,
      geometry.sectors_per_track, geometry.sector_bytes)
for reading in geometry.readings:            # `conflicts` where they disagree
    print(" ", reading.source, reading.at, reading.detail)
first = medium.get_sector(0, 0, 1)           # sectors number from one

# Content is reached through the partition that composes it: this image
# records no scheme, so the direct partition is the whole of it, and the
# namespace is declared rather than determined.
filesystem = medium.partition(0).filesystem_as("hdos")
for entry in filesystem.entries():
    print(entry.name, entry.size_bytes,
          [(fact.key, fact.value) for fact in entry.declared])
data = filesystem.get_file("HDOS.SYS").bytes()

# Seating and unseating are configuration; nothing about them is
# destructive. `release_media` is the one verb that ends state.
device = session.add_device("h17")
print(device.attachment)            # heathfloppy0
device.insert(medium.id)
device.eject()                      # the drive stays; the disk stays too
session.release_media(medium.id)

# What an artifact is, before a machine has been configured for it.
discovery = remanence.discover_media("disk.h8d", writable=False)
print(discovery.article, discovery.accepting_devices, discovery.device_type)
found = session.load_discovery(discovery)   # consumed: one claim, one open
session.add_device("h17").insert(found.id)

# Or the acts at once, where the format records one device type.
drive = session.add_device_for("disk.h8d", writable=False)

# The letters, from the machine's own device set — or from asserted
# facts, where the caller holds them instead.
drives = session.machine().compose_dos_letters()  # no variant stated:
for mapping in drives.mappings:                   # disagreement is reported
    print(mapping.letter, mapping.outcome, mapping.volume, mapping.reason)

with open("captures.7z", "rb") as source:
    archive = session.load_media(source, "7z")
for entry in archive.partition(0).filesystem().entries(""):
    print(entry.name, entry.size_bytes)

with remanence.CaptureSet("captures.7z") as capture:
    for member in capture.inspect().members:
        run = member.runs[0]
        print(member.position.numerator, member.head, run.transitions,
              len(run.observations))

    verdict = capture.recognize().verdicts[0]
    print(verdict.profile_name, verdict.confidence)
    for line in verdict.evidence:
        print(" ", line)

    # The gap-first reduction: every revolution of every location, not
    # the choice of one. The plan computes it whole and writes nothing.
    plan = capture.plan_reconstruction(remanence.ReconstructionPolicy(
        side=0, recordings="measured"))
    report = plan.report()
    print(report.swept_positions, len(report.recorded_positions))
    for loss in report.declared_loss:
        print(loss.code, loss.count, loss.detail)

# What comes back is the family's own image, and it is the disk from
# here on: the renditions and the presentation ladder both hang on it.
image = plan.execute()

# What a 1541's read channel and GCR codec make of what it holds.
bits = image.materialize_c1541_bitstream(remanence.ReadChannelPolicy(
    density="declared", unzoned="refuse", weak_pulse="seeded",
    seed=0x0123456789abcdef))
bytes_ = bits.materialize_c1541_bytestream(remanence.GcrCodecPolicy(
    alignment="landmark", unassigned_symbol="declare-loss"))
for track in bytes_.inspect().locations:
    print(track.half_track_numerator, track.bytes, track.resolved_bytes,
          track.alignments, track.unframed_bits)

# And the sectors the recording states for itself, above those bytes.
sectors = bytes_.recognize_c1541_sectors(remanence.SectorPolicy(
    checksum_failure="declare-loss", unpaired_record="declare-loss"))
for claim in sectors.inspect().claims:
    print(claim.track, claim.sector, claim.readable, claim.rule,
          claim.header_checksum_stated, claim.header_checksum_computed)
print(sectors.read_sector(18, 0)[:3])   # the BAM: 18, 1, and DOS 'A'

# And the directory CBM DOS wrote across those sectors: the recording's
# own direct partition, and the reading declared over it.
space = sectors.partition().filesystem_as("cbmdos")
print(space.kind, space.label().name, space.evidence()[0])
for entry in space.entries():
    print(entry.name, entry.size_bytes,
          {fact.key: fact.value for fact in entry.declared})
print(space.read_file("PCS.4000")[:2])  # a PRG's own load address


# Each rendition states its loss before it writes anything.
for loss in image.describe_p64().declared_loss:
    print(loss.code, loss.count, loss.detail)
image.write_p64("pinball.p64")
image.write_d64("pinball.d64")
image.write_g64("pinball.g64")

with remanence.P64Image("pinball.p64") as image:
    for track in image.inspect().half_tracks:
        print(track.index, track.half_track_numerator, track.pulses,
              track.strong_pulses, track.weak_pulses)
```

## Changes

Release-facing changes are recorded in [CHANGELOG.md](CHANGELOG.md).
Pre-1.0 the project makes no compatibility promise: a surface change lands
across the Rust, C, and Python presentations together and the old shape is
deleted, so read the changelog before upgrading.

## Planning and governance

Maintainer-facing planning lives under [planning/](planning/README.md); the
map there explains how ideas enter and how decisions are recorded.
Repository guidance for agents and contributors is in
[AGENTS.md](AGENTS.md).

## License

GPL-3.0-only. See [LICENSE](LICENSE).

remanence-lib is copyleft. You may run, study, modify, and redistribute it
freely; any work you distribute that incorporates it must also be
GPL-3.0-only. It cannot be taken into a proprietary product.

Paul Galbraith holds copyright in the project and **reserves the right to
relicense it**, on any terms, at any time. No relicensing is planned or in
preparation — the reservation exists so the option is not lost by default,
not because it is about to be used. It takes nothing back from what has
already been released: every version published under the GPL stays under
the GPL, permanently. Contributions are accepted under a copyright
assignment that keeps the reservation intact; see
[CONTRIBUTING.md](CONTRIBUTING.md).

The name **Remanence** is owned by Paul Galbraith and is not licensed
for use by forks or redistributions. See [TRADEMARKS.md](TRADEMARKS.md).
