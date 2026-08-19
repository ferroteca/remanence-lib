# remanence-lib

[![License](https://img.shields.io/badge/license-GPL--3.0--only-blue.svg)](LICENSE)

A library for reading and analysing disk images — floppy and hard disk
images from vintage and modern systems alike.

Hand it an image file and it will tell you what the file actually is, what
disk geometry it records, which drive it came from, and what filesystem is
on it. Then it will let you list and read the files inside. It is written
in Rust, and it is usable from Rust, C, C++ and Python.

**It has no runtime dependencies.** The ZIP and 7z readers, the DEFLATE and
LZMA decompressors, and the QCOW2 and VDI drivers are all part of the
library. It never shells out to an external tool.

## What it can read

| Kind | Formats |
| --- | --- |
| Disk images | raw, QCOW2 (v2 and v3, including backing chains), VDI (dynamic, fixed and differencing), Heathkit H8D |
| Archives | ZIP, 7z |
| Flux captures | KryoFlux stream sets, P64, and the library's own `.remanence` image |
| Filesystems | FAT12, FAT16, HDOS, CBM DOS |

A flux image can also be written out as D64, G64 or P64.

## The main ideas

There are only a few things to understand before the rest follows.

**You open the file, not the library.** You pass in a file you opened
yourself, and you tell the library what format it is. Whether the file can
be written to is decided by how *you* opened it — the library asks your
handle that one question, respects the answer, and takes no lock of its
own.

**Your declaration is checked, not trusted.** Saying "this is an H8D" is a
claim. The adapter for that one format checks the claim against what is
actually in the file and refuses, by name, if it does not hold up. It does
not go looking through every format it knows to find one that fits.

**A session holds two things: disks and drives.** Disks are state;
drives are configuration. The disk is the handle you keep — everything
about a recording is answered by asking the disk, whether or not a drive
holds it.

**Nothing is guessed and nothing is repaired.** When two sources of
information disagree, you are told they disagree and shown both, rather
than being handed a winner. When data is missing, you get a refusal that
names what is missing, not zeroes standing in for it.

**Putting a disk in a drive is not destructive.** Ejecting a disk from a
drive leaves the disk and everything buffered on it untouched. There is
exactly one call that discards a disk, and it is called `release_media`.

## Identifying an image

Loading an image identifies the layers it is made of — the archive wrapper,
the image format, the physical disk geometry, and the likely filesystem —
each with a confidence score and a plain-language reason.

When two candidates match equally well, the answer is "unknown". Nothing is
settled by whichever format happened to be checked first.

You can also ask what a file is without loading it. `discover_media` opens
it, answers the question, and hands back a result you can pass straight to
a load, so the file is only ever opened once.

## Geometry

How a disk was laid out — cylinders, heads, sectors per track, bytes per
sector — is read off the image when it loads, and treated as evidence from
then on. You cannot declare a geometry onto a disk that already exists.

Up to four sources may have something to say, and each answer remembers
where it came from:

- what the image format itself records (an H8D records 40 cylinders, one
  side, ten 256-byte sectors);
- the sectors-per-track and heads written into a FAT boot record;
- the partition table's end-of-partition figures, where they can be solved
  against the size the same entry declares;
- arithmetic over the size of the content, for the cylinder count.

If those sources disagree, the geometry is *undetermined*: you get every
reading back and none is preferred. If nothing states a geometry at all,
that is *unstated*, which is a different fact and is kept as one.

`read_sector` and `write_sector` address the disk in whatever that
established. Cylinders and heads count from zero and **sectors count from
one**, because that is how the recordings themselves are numbered. Asking
for a coordinate the geometry does not cover, or one it covers but the
content does not hold, is a refusal rather than a block of zeroes. Writes
are buffered until you call `commit`.

## Partitions and files

Files are always reached the same way, whatever the disk: through a
partition.

A partitioned disk gives you the entries its table declares, addressed by
the table's own numbering. A disk with no partition table gets a single
partition at index 0 covering the whole thing — that is the library's own
description of the disk, and it is labelled as such rather than presented
as something the disk said.

Each partition opens two doors onto the same content:

- **by position** — read and write at byte offsets within the partition;
- **by name** — the filesystem on it, with the usual list, read, write and
  create-directory operations.

Where the partition type says which filesystem to expect, you get it
directly. Where nothing does, you name it yourself with
`filesystem_as("hdos")` and the library checks you were right. Either way
the file operations live on the same object, so a FAT volume on a hard disk
image and the HDOS catalog on a Heathkit floppy are reached identically.

Nothing touches the image until you call `commit`, and `rollback` throws
away everything since the last one.

## Archives

A ZIP or 7z file is treated as a disk in its own right. Its contents are
its directory, reached exactly like any other filesystem.

A file inside an archive that turns out to be a disk image becomes a disk
of its own — and it outlives the archive it came from, so ejecting the
archive takes nothing away from a disk already loaded out of it. Entries
are read economically: an uncompressed entry is read in place, and a
compressed one is decoded once into the session's own storage, so a single
member of a solid 7z can be read without unpacking the rest.

Archives are read-only. Writing would mean re-encoding into the archive
format, and no adapter here claims to do that.

## Flux captures

A flux capture records the magnetic pulses on a disk surface, before
anything has decided what they mean. That is the lowest level this library
works at, and it climbs up from there in steps you can stop at.

**Loading a KryoFlux capture.** A capture is a set of stream files — one
per head, per drive-step position — so it is loaded as a declared
collection rather than a single file, and you name the drive family it was
captured from. The whole set is checked together before anything is
decoded: a missing, duplicate, contradictory or unrelated member refuses
the whole set by name. The two heads stay two separate surfaces; nothing is
merged into one idealised disk.

The drive family's profile is then checked against the capture — a profile
is what the library knows about a family's recording conventions, which the
capture itself does not contain. If the capture does not match the family
you claimed, it is refused, and you are shown the numbers behind that
verdict. Which head actually carries the recording is measured the same
way; the unrecorded back of a single-sided disk reads as noise.

The decode itself aligns every revolution of every position, measures the
bit spacing from the intervals rather than assuming it, and records
uncertainty where it finds it instead of smoothing it away. What comes back
is an ordinary disk, carrying the full story of how it was produced: what
the set contained, how the profile check went, what settings were used, and
an itemised account of everything the decode could not carry over.

**Reading a flux disk.** The disk's type carries the rules, so there is no
policy to pass in. A Commodore 1541 disk is read through the 1541's channel
and code table because that is what being a 1541 disk means. You can stop
at three levels:

- the **bitstream** — the pulses clocked into bits, with each bit saying
  whether it was recorded or resolved by a rule;
- the **bytestream** — those bits decoded into the family's bytes. The
  first byte of a track is the first *framed* byte: nothing before the sync
  mark is a byte at all. A bit pattern the code table does not define keeps
  its own bits rather than being rounded to the nearest legal value;
- the **sectors** — the headers and data blocks the recording states for
  itself, each with the address it claims, its stated and computed
  checksums side by side, and a count of bytes the decode could not
  resolve. Nothing is repaired and no block is filled in.

Neither the bitstream nor the bytestream assigns any meaning above a byte.
No byte is "a header" or "part of a file" at that level.

**And above that, the disk's own directory.** `filesystem_as("cbmdos")`
opens a CBM DOS directory the same way any other filesystem is opened: the
BAM header as the volume label, entries in the order they were written,
PETSCII names as recorded, and each entry's own facts — PRG, SEQ, USR or
REL, the locked and never-closed flags, the block count, and which slot it
came from. A file's real size is established by walking its block chain
rather than trusting the stated count, and a file whose chain reaches a
block the capture never recovered says so on the entry and refuses on the
read — so one bad sector spoils its own file instead of the whole listing.

This is the directory the disk records, which is not quite the same thing
as `LOAD"$"` — that is synthesised by the drive's own ROM.

**Saving and rendering.** The library's own `.remanence` image holds the
physical facts of a disk's surfaces, fitted to nothing. It renders to D64,
G64 and P64, and each rendering states what its destination could not
carry. P64 files load back in as ordinary disks. When writing, the library
checks in advance what the target format can hold, names and counts what
would be lost, and refuses rather than approximating. An existing file at
the destination is a refusal, never an overwrite, and the new file is built
alongside and moved into place whole, so an interruption leaves no
half-written file.

## Disk images with backing chains

Loading a raw, QCOW2 or VDI image takes a lock under a stated intent: a
read session keeps other writers out while allowing other readers, a
writable session allows nobody else in, and an image whose lock cannot be
taken — one held by a running VM, say — is refused at the load rather than
part-way through.

You get a layered report of what the image turned out to be: the device,
its leading structure, any partition scheme, every region that scheme
declares, every volume, and every filesystem found. Every region carries
both its raw type byte and a reading of what that byte means, so a type
this release does not recognise still explains itself. The report is a view
of the same partition data you read files through, so the two cannot drift
apart.

A **QCOW2 image with a backing chain** opens as one composed disk, with
every member of the chain held immutable for the session. Writes go into
the top image only, copy-on-write, and preserve the backing relationship. A
missing member, a loop, or a chain longer than claimed is refused by name.

A **VDI differencing image** does the same, but finds its parent the way
the format itself does: a VDI records its parent's *identity* and no path
at all. So the library looks beside the child and in the directory above
it, and checks any candidate it finds against that identity. A file sitting
where the parent should be whose identity does not match is refused, not
read as a substitute. Dynamic, fixed and differencing images are supported;
the format's fourth type, undo, is refused rather than attempted. A block
the map marks discarded reads as the zeroes the format says it holds, and
is never confused with an allocated block that happens to contain zeroes.

## Damaged and incomplete images

An image that is short of what it claims is neither accepted whole nor
thrown out whole.

Every load reports what it established before you read anything: verified,
or degraded along with the reason. A raw image whose FAT boot record claims
more bytes than the file actually contains opens degraded and read-only for
the rest of the session, and you are told what was declared, what the file
holds, the first missing byte, and exactly how far it does read.

Directories and files that are entirely present read normally. A file whose
data runs into the missing tail is refused by name, with its range — never
clipped, zero-filled or served in part. Every write and every commit is
refused with the same stable reason code, identically in Rust, C and
Python. Where the damage leaves no safe boundary to state at all — a boot
record declaring two different sizes — the image is refused outright.

## Creating blank disks

You can also make a disk from nothing. There is no file involved: nothing
is read, probed or opened, and the facts you state at creation become the
disk's own, recorded as having come from you.

You can create any of the catalogued blank media — a manufactured disk with
nothing recorded on it — or state a geometry directly and get a disk with
those coordinates. A blank disk assumes no drive, so it goes into none
until you say otherwise, and it lives in the session until you explicitly
write it out. `commit` works on it as normal, with no recovery journal
underneath, because there is no file for an interruption to leave
half-written.

## Layout

```
crates/
  remanence/        # the core library (pure Rust, no runtime dependencies)
  remanence-ffi/    # C ABI: staticlib + cdylib, cbindgen header, C++ wrapper
  remanence-py/     # Python module (PyO3), built with maturin
```

Each of those carries its own README, aimed at the people who will install
it from a package registry.

## Building

```bash
cargo build            # core + C FFI (generates crates/remanence-ffi/c/include/remanence.h)
cargo test             # the full test suite
```

Building the Python module needs Python 3.10 or newer:

```bash
cargo build -p remanence-py
# or, for distributable artifacts (sdist + abi3 wheel into crates/remanence-py/dist/):
uv build crates/remanence-py
```

uv drives the maturin build backend in an isolated environment, so nothing
beyond uv itself needs installing. Publishing is `uv publish` from that
`dist/`, and is owner-gated. **The Python package is tested on Windows
only, for now**, and its packaging classifiers say so; the POSIX code paths
exist and should stay correct, but they are untested and unclaimed.

Testing the Python bindings and the C/C++ surface is
[Task](https://taskfile.dev), not `cargo test` — neither is reached by
`cargo build`/`cargo test` in any form:

```bash
task test-py    # builds, stages, and runs pytest and mypy against it
task test-ffi   # builds via CMake and runs the C/C++ suite with CTest
```

An example C consumer is at
[crates/remanence-ffi/c/examples/identify.c](crates/remanence-ffi/c/examples/identify.c),
with build instructions in its header comment.

**C++ consumers have a friendlier header** —
[crates/remanence-ffi/c/include/remanence.hpp](crates/remanence-ffi/c/include/remanence.hpp),
header-only and C++17 — built on top of the C ABI rather than alongside it.
It gives you objects that clean up after themselves, views onto the things
the session owns, and failures as a single exception type carrying the same
stable category code. It covers every exported function, the flux ladder
included. The C ABI remains the standard interface and is still fully
available; this adds convenience, not reach. Its example is
[c/examples/identify.cpp](crates/remanence-ffi/c/examples/identify.cpp), beside
the C one.

## Using the library

```rust
// A session holds disks (state) and drives (configuration). The disk
// is the handle you keep. You open the file yourself, and you say what
// format it is — that one format's adapter checks you were right.
// Some formats record exactly one kind of drive, so naming the format
// is enough. Where a format could be any of several, you say which:
// `Format::Qcow2 { device: HardDrive::MbrBlock }`.
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

// Putting it in a drive is a separate step, and the drive is the slot
// rather than the disk: ejecting takes nothing away.
let mut device = session.add_device(remanence::FloppyDrive::HeathH17)?;
println!("{}", device.attachment());      // heathfloppy0
device.insert(disk)?;

// Files live behind a partition. This image has no partition table, so
// there is one partition covering the whole disk. Nothing says which
// filesystem is on it, so you name it and the library checks.
let medium = session.medium_mut(disk).expect("pooled");
let mut filesystem = medium
    .partition(0)
    .expect("the whole disk")
    .filesystem_as("hdos")?;
for entry in filesystem.entries("")? {
    println!("{} ({} bytes)", entry.name, entry.size_bytes);
    // Anything else this filesystem records about the file — an HDOS
    // catalog date, its flag letters — in its own terms.
    for fact in &entry.declared {
        println!("    {} = {}", fact.key, fact.value);
    }
}
let bytes = filesystem.get_file("HDOS.SYS")?.bytes()?;

// What the load established about the image, before anything is read
// from it — including whose file handle the access rests on.
let assurance = session.medium(disk).expect("pooled").assurance();
println!("{} {:?} {}", assurance.outcome, assurance.condition, assurance.claim);
for line in &assurance.evidence {
    println!("  {line}");
}

// A partitioned disk: what the table declares, and the entry you mean.
println!("{:?}", medium.partition_scheme());     // Some(PartitionScheme::Mbr)
for declared in medium.partitions() {
    println!("{} {} {:?}", declared.ordinal(), declared.placement(),
             declared.type_reading());
}

// Your reading of the partition type, checked against the byte in the
// table. A DOS data partition says which filesystem to expect, so you
// do not have to.
let partition = medium.partition(1).expect("the table declares entry 1");
partition.check_type(remanence::PartitionType::DosPrimary)?;
let mut files = partition
    .filesystem()
    .expect("a DOS data partition determines FAT");
for entry in files.entries("")? {
    println!("{} ({} bytes)", entry.name, entry.size_bytes);
}

// The same partition read by position, for what no filename reaches.
let mut boot_record = [0u8; 512];
let mut volume = medium
    .partition(1)
    .expect("the table declares entry 1")
    .volume()
    .expect("a data partition composes an extent");
volume.read_at(0, &mut boot_record)?;

// The geometry the image records, and who said what. If sources
// disagree you get every reading and no winner.
let geometry = medium.geometry();
match geometry.determined() {
    Some(coordinates) => println!("{coordinates}"),   // 131 cylinders of 16 heads…
    None => println!("{}: {:?}", geometry.state(), geometry.conflicts()),
}
for reading in geometry.readings() {
    println!("  [{}] {}: {}", reading.source, reading.at, reading.detail);
}

// One sector in those coordinates. Cylinder 0, head 0, sector 1 is the
// first record on the disk, wherever the image format keeps it.
let mut sector = [0u8; 512];
medium.read_sector(0, 0, 1, &mut sector)?;
medium.write_sector(0, 0, 1, &sector)?;     // buffered until commit

// An archive is a disk in its own right, and its contents are its
// directory.
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

// A file inside it that is a disk image becomes a disk of its own, and
// outlives the archive it came from. A KryoFlux stream is just bytes to
// every adapter here, so the discovery calls it a raw image and names
// no drive — so you name one.
let member = session
    .medium_mut(archive)
    .expect("pooled")
    .partition(0)
    .expect("an archive bears its direct partition")
    .filesystem()
    .expect("an archive's content is its namespace")
    .get_file("track00.raw")?
    .discover()?;
let inner = session
    .load_discovery_as(member, remanence::DeviceType::HardDrive(
        remanence::HardDrive::MbrSector))?
    .id();
session.release_media(archive)?;          // the disk keeps answering

// Asking what a file is before setting anything up. The discovery holds
// the lock and creates nothing; the load takes it over, so the file is
// opened only once.
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

// Or both steps at once, where the format records one kind of drive —
// refused by name where it records several and you named none.
let drive = session.add_device_for("disk.h8d", remanence::AccessIntent::Read)?;

// A KryoFlux capture is a set of files, not one file: gathered from an
// archive here, or a Vec of files you opened yourself.
let capture = session
    .load_media(std::fs::File::open("captures.7z")?, remanence::Format::SevenZip)?
    .id();
let members = session
    .medium_mut(capture)
    .expect("pooled")
    .partition(0).expect("an archive bears its direct partition")
    .filesystem().expect("an archive's content is its namespace")
    .files("")?;
let c64_disk = session.load_media(
    members,
    remanence::Format::KryoFlux { device: remanence::FloppyDrive::Commodore1541 },
)?;
for line in &c64_disk.assurance().evidence {
    println!("{line}");                   // how the profile check went, what
}                                         // ran, and what could not be carried

// The disk's type carries the channel and the code table, so there is
// no policy to pass in.
let mut first = [0u8; 1];
c64_disk.bytestream()?
    .location(remanence::Location::track(1))?
    .read_at(0, &mut first)?;

// And the directory CBM DOS wrote, opened like any other filesystem.
let c64_disk_id = c64_disk.id();
let c64_disk = session.medium_mut(c64_disk_id).expect("pooled");
let mut cbm = c64_disk
    .partition(0).expect("flux media record no scheme")
    .filesystem_as("cbmdos")?;
for entry in cbm.entries("")? {
    println!("{:16} {:>4} {}",
        entry.name,
        entry.fact("size-blocks").unwrap_or(""),
        entry.fact("type").unwrap_or(""));
}

// Creating a disk from nothing: no file, nothing read, and the facts
// you state become the disk's own. No drive is assumed, so it goes into
// none.
let blank = session.new_media(remanence::NewMedia::ChsDisk {
    geometry: remanence::RecordingGeometry {
        cylinders: 1024, heads: 16, sectors_per_track: 63, sector_bytes: 512,
    },
})?;
assert_eq!(blank.device_type(), None);
assert_eq!(blank.article(), "authored");   // nobody manufactured it
for line in &blank.assurance().evidence {
    println!("{line}");                    // your facts, recorded as yours
}
let mut boot = [0u8; 512];
boot[510] = 0x55; boot[511] = 0xaa;
blank.write_sector(0, 0, 1, &boot)?;       // the geometry you stated answers
blank.commit()?;                           // lives in the session; no file yet

// Or a catalogued blank: a disk in its sleeve, with nothing on it.
let unrecorded = session.new_media(remanence::NewMedia::Flexible525HardTen)?;
assert_eq!(unrecorded.article(), "flexible-5.25-hard-10");
```

```python
import remanence
for device in remanence.device_slots():
    print(device.id, device.name, device.device_class, device.article,
          device.scheme)

print(remanence.formats())          # the formats you can name

session = remanence.Session()
# You open the file and say what it is. The library keeps its own copy
# of the descriptor, so closing your Python file afterwards is safe.
with open("disk.h8d", "rb") as source:
    medium = session.load_media(source, "h8d")   # one drive records this
                                                 # format, so no `device=`
print(medium.assurance.outcome, medium.assurance.claim, medium.mode)
for layer in medium.identify().layers:
    print(layer.kind, layer.id, layer.confidence)

# The geometry the image records, and who said what.
geometry = medium.geometry
print(geometry.state, geometry.cylinders, geometry.heads,
      geometry.sectors_per_track, geometry.sector_bytes)
for reading in geometry.readings:            # `conflicts` where they disagree
    print(" ", reading.source, reading.at, reading.detail)
first = medium.read_sector(0, 0, 1)           # sectors are numbered from one

# Files live behind a partition. This image has no partition table, so
# there is one partition covering the whole disk, and you name the
# filesystem you expect.
filesystem = medium.partition(0).filesystem_as("hdos")
for entry in filesystem.entries():
    print(entry.name, entry.size_bytes,
          [(fact.key, fact.value) for fact in entry.declared])
data = filesystem.get_file("HDOS.SYS").bytes()

# Putting a disk in a drive and taking it out again changes nothing.
# release_media is what actually discards it.
device = session.add_device("h17")
print(device.attachment)            # heathfloppy0
device.insert(medium.id)
device.eject()                      # the drive stays; so does the disk
session.release_media(medium.id)

# Asking what a file is before setting anything up. It creates nothing,
# so it takes no cache limit — that is the load's business.
discovery = remanence.discover_media("disk.h8d", writable=False)
print(discovery.article, discovery.accepting_devices, discovery.device_type)
found = session.load_discovery(discovery)   # taken over: one lock, one open
session.add_device("h17").insert(found.id)

# Or both at once, where the format records one kind of drive.
drive = session.add_device_for("disk.h8d", writable=False)

with open("captures.7z", "rb") as source:
    archive = session.load_media(source, "7z")
for entry in archive.partition(0).filesystem().entries(""):
    print(entry.name, entry.size_bytes)

# A KryoFlux capture is a set of files, not one file. The set, the
# stream format and the drive profile's claim are all checked together
# before anything is decoded — and what comes back is a 1541 disk
# carrying the whole story of how it got here.
members = archive.partition(0).filesystem().files("")
disk = session.load_media(members, "kryoflux")   # one drive records it,
for line in disk.assurance.evidence:             # so no `device=` needed
    print(line)              # the verdict, the settings, what was lost

# The disk's type carries the channel and the code table.
bits = disk.bitstream()
bytes_ = disk.bytestream()
for track in bytes_.inspect().locations:
    print(track.half_track_numerator, track.bytes, track.resolved_bytes,
          track.alignments, track.unframed_bits)
first = bytes_.location(1).read_at(0, 1)   # the first *framed* byte of
                                           # track 1: nothing before the
                                           # sync mark is a byte at all

# And the sectors the recording states for itself, above those bytes.
sectors = bytes_.recognize_sectors()
for claim in sectors.inspect().claims:
    print(claim.track, claim.sector, claim.readable, claim.rule,
          claim.header_checksum_stated, claim.header_checksum_computed)
print(sectors.read_sector(18, 0)[:3])   # the BAM: 18, 1, and DOS 'A'

# The directory CBM DOS wrote across those sectors, opened like any
# other filesystem.
space = disk.partition(0).filesystem_as("cbmdos")
print(space.kind, space.label().name, space.evidence()[0])
for entry in space.entries():
    print(entry.name, entry.size_bytes,
          {fact.key: fact.value for fact in entry.declared})
print(space.read_file("PCS.4000")[:2])  # a PRG's own load address

# A P64 already holds a flux disk, so the load takes it straight in —
# same call, same kind of disk.
with open("pinball.p64", "rb") as source:
    p64_disk = session.load_media(source, "p64")
print(p64_disk.device_type, p64_disk.article)

# Creating a disk from nothing: no file, nothing read, and the facts you
# state become the disk's own.
print(remanence.new_media_kinds())     # what you can create
blank = session.new_media("chs-disk", cylinders=1024, heads=16,
                          sectors_per_track=63, sector_bytes=512)
print(blank.device_type, blank.article, blank.authored_as)  # None authored chs-disk
print(blank.geometry.readings[0].source)                    # authorship
blank.write_sector(0, 0, 1, bytes(510) + b"\x55\xaa")
blank.commit()                         # lives in the session until you
                                       # write it out
```

```cpp
#include <remanence.hpp>

#include <iostream>

// One catch block for the whole program: every failure arrives as
// remanence::Error, carrying a stable category code you can branch on
// and, where a named rule was broken, the name of that rule.
try {
    // What a file is, before any drive is set up for it. The lock it
    // takes goes into the load, which takes it over.
    remanence::Session session;
    remanence::Discovery found = remanence::discover_media("disk.h8d");
    std::cout << found.article().value_or("?") << ' ' << found.size() << '\n';

    remanence::Medium medium = session.load_discovery(std::move(found));

    // A Layer borrows the identification it came from, so it has to be
    // given a name: the one-liner over a temporary does not compile.
    remanence::Identification what = medium.identify();
    for (const remanence::Layer& layer : what.layers()) {
        std::cout << layer.id().value_or("?") << ' ' << unsigned{layer.confidence()} << "%\n";
    }

    // The geometry the image records, and who said what.
    remanence::Geometry geometry = medium.geometry();
    if (const std::optional<remanence::Coordinates> settled = geometry.coordinates()) {
        std::cout << settled->cylinders << '/' << settled->heads << '/'
                  << settled->sectors_per_track << '\n';
    }

    // Files live behind a partition. Objects clean themselves up in
    // reverse order of creation; nothing here has a `_free` to remember
    // or a status code to check.
    remanence::Filesystem filesystem = medium.partition(0)->filesystem_as("hdos");
    remanence::EntryList listing = filesystem.entries();
    for (const remanence::Entry& entry : listing.entries()) {
        std::cout << entry.name() << ' ' << entry.size_bytes() << '\n';
    }
    remanence::FileData data = filesystem.read_file("HDOS.SYS");

    // Putting a disk in a drive is not destructive; release_media is
    // what actually discards it.
    remanence::StorageDevice drive = session.add_device("h17");
    drive.insert(medium.id());
    drive.eject();
    session.release_media(medium.id());

    // The flux levels are in the same header. Each one is built from
    // the one below it, and reports what it could not carry over.
    remanence::FluxImage image = remanence::FluxImage::open("capture.remanence");
    remanence::Bitstream bits = image.materialize_bitstream();
    remanence::C1541Sectors sectors = bits.materialize_bytestream().recognize_sectors();
    std::vector<std::uint8_t> bam = sectors.read(18, 0);
    for (const remanence::DeclaredLoss& loss : image.describe_d64().declared_losses()) {
        std::cout << loss.code << ' ' << loss.amount << ": " << loss.detail << '\n';
    }
} catch (const remanence::Error& refusal) {
    std::cerr << static_cast<int>(refusal.category()) << ": " << refusal.what() << '\n';
}
```

## Changes

Release-facing changes are recorded in [CHANGELOG.md](CHANGELOG.md).
Before 1.0 the project makes no compatibility promise: a change to the API
lands across the Rust, C and Python interfaces together and the old form is
removed, so read the changelog before upgrading.

## Design and goals

What the library is for, and the rules it holds itself to, are set out in
[USE-CASES.md](USE-CASES.md) and [ARCHITECTURE.md](ARCHITECTURE.md).

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
