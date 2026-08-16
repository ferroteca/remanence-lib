# USE-CASES

> **Status: in force.** Every use case on this list is met by the code
> as it exists today — this is an implementation claim, not an
> aspiration, and **a divergence between an entry here and the code is
> a bug**. Numbers come from the one global U-sequence and are never
> reused. Proposed and pledged use cases live under
> [planning/](planning/README.md) until full delivery brings them here.

## How the walks read

Several entries below carry a walk in code, and these conventions hold
across every one of them. Each is written to the Rust surface, which the
C ABI and the Python module mirror. Local artifacts arrive as the
caller's own opened files — `File::open` is `std::fs::File`, the
portable file; files from inside media are this library's own views —
and **whoever opens owns the lock**: my open is my safeguard and the
library's claim, checked for what it affords (may it write?), honoured
exactly, never escalated.

## U1 — Identify a disk image I know nothing about

I have a file that claims to be a disk image — maybe raw, maybe a
container format the library claims such as qcow2 or VDI, maybe
sitting inside a `.zip`. I attach it to a session and remanence tells
me, layer by layer, what it is: the archive wrapper, the image format, the
physical media it represents, the probable filesystem — each with a
confidence and the evidence behind it, never a bare verdict. When it
doesn't know, it says "unknown" rather than guessing.

## U2 — Browse a vintage volume and pull files out of it

Once an image is identified, I ask the drive what it resolves to and
list its catalog — HDOS today — with the real names, sizes, dates and
flags, and I copy a chosen file's bytes out to the host, without ever
booting anything or mutating the image. I name no volume, because a
disk that bears one namespace has one supported answer at every seam
between; where the answer is not single the library refuses naming what
it found rather than picking for me.

## U3 — I read and write a stopped machine's files

My QEMU automation layer needs to reach inside a stopped
machine's disk image on the host — qcow2, VDI or raw — and work with the
files in its FAT12/FAT16/FAT16B volumes, whether those volumes sit
behind an MBR or bare on a partitionless image: list a directory's
entries, ask after one path and get its entry — or the answer that
it does not exist, distinguished from failure — copy a file out to
the host, write a file in, create a directory. Writing a file that
already exists replaces its contents, shorter or longer, releasing
and reclaiming clusters; creating a directory creates missing
parents and succeeds when the directory already exists. I attach each
image to a storage device in my session, exactly as U4 does, and reach
a volume by the ordinal its own partition scheme declares — a medium
recording no scheme bearing its whole content as the direct one. The
namespace opens under the type that scheme declares, or under my own
reading where nothing declares one, and what answers is the one type
carrying file verbs; a path within it names the file. It states the
volume identity this disk's inspection report issued, so the volume I
worked through and the volume I reported are the same one. All of
this without booting the guest and without any external helper
process: the library does
the format work itself. Reading never changes the image. Writing is
a separate, explicit mode with a commit point: until I commit,
everything I wrote can be rolled back cleanly.

**The steps are U4's setup, with the intent stated in my own open, and
then the file work on the space that opens.**

```rust
let mut session = Session::new();
let hdd0        = session.add_device(HardDrive::MbrSector)?.attachment();

// My open affords the write, and nothing else does: the library checks
// this handle for what it allows and the medium's mode echoes it.
let media = session.load_media(
    File::options().read(true).write(true).open("system.qcow2")?,
    Format::Qcow2 { device: HardDrive::MbrSector })?.id();
session.device_mut(hdd0).expect("just added").insert(media)?;

let mut drive = session.device_mut(hdd0).expect("still here");
let disk = drive.medium_mut().expect("the disk I just inserted");
assert_eq!(disk.mode(), AccessMode::ReadWrite);

// The volume: the ordinal the MBR itself declared, opened under the type
// it declares. A bare image records no scheme, so its whole content is
// partition 0 and the reading is mine — .filesystem_as("fat"). A space
// is a view over the disk (P23), so it lives for the work and no longer:
{
    let mut fat = disk.partition(1).expect("the MBR declared it")
        .filesystem().expect("its declared type determines the namespace");

    for entry in fat.entries("OUT")? {        // names exactly as stored
        println!("{:12} {:>8} {}",
            entry.name, entry.size_bytes, entry.kind.name());
    }

    match fat.stat("OUT/X.TXT")? {            // absence is an answer, and
        Some(entry) => println!("{} bytes", entry.size_bytes), // a failure
        None        => println!("no such path"),   // is an error — never
    }                                              // one wearing the
                                                   // other's clothes
    let bytes = fat.read_file("OUT/X.TXT")?;  // out to the host
    fat.make_directory("OUT/LOGS")?;          // missing parents made, and
                                              // already-there succeeds
    fat.write_file("OUT/LOGS/RUN.TXT", &bytes)?;  // an existing file has
                                              // its contents replaced,
                                              // shorter or longer,
                                              // clusters released and
                                              // reclaimed
}

disk.commit()?;        // the commit point is the disk's — …or
                       // disk.rollback()?, and until one of them the
                       // image on disk holds none of the above
```

Names are the seam's rule, not mine. A read matches the way DOS matched
— without regard to case — and gives me back the name as stored, so
what I show a user is what the directory holds. A write takes the name
I have and stores the DOS one, uppercasing and padding it itself,
because doing that in my code is doing the library's job where it
cannot be checked against the format. When a name cannot be a DOS name
the refusal names which of the namespace's rules it broke — an empty
base, too long a base or extension, a stray separator, an excluded
character, a leading or trailing space, a reserved device name — so I
can branch on the rule and tell the user which one in my own words
without parsing a sentence. Nothing is truncated, transliterated, or
renamed to fit: a refused name is refused.

```rust
let mut fat = disk.partition(1).expect("the MBR declared it")
    .filesystem().expect("its declared type determines the namespace");

fat.write_file("out\\x.txt", &bytes)?;   // stored as OUT\X.TXT: matching
                                         // and uppercasing are the seam's

if let Err(error) = fat.write_file("OUT/report.2026.txt", &bytes) {
    match error.rule().and_then(DosNameRule::from_identity) {
        Some(DosNameRule::Separator)   => println!("one dot, and no more"),
        Some(DosNameRule::BaseTooLong) => println!("eight before the dot"),
        Some(rule) => println!("{rule}"),   // the set is enumerated, so a
                                            // rule I don't branch on still
                                            // has a stable spelling
        None => return Err(error),          // no name rule broke: this is
    }                                       // some other refusal entirely
}
```

## U4 — I retrieve a stopped machine's partition and volume information

My automation layer's drive reporting runs on host-side facts about a
stopped machine's disk images, and this library is where those facts come
from. What the *guest* called those volumes is not among them and I do
not ask for it here. For each disk — qcow2, VDI or
raw — one inspection answers, keeping each fact at the seam that owns
it rather than flattening them into one snapshot.

**The steps have the machine's own shape**: a session standing for the
stopped machine, a drive in it for each image, the medium loaded into
that drive, and one
inspection per drive — because which drive a fact came from is the fact
my drive reporting is *about*.

```rust
let mut session = Session::new();       // the scope the stopped machine's
                                        // disks are reconstructed in

// One drive per image, in the order I attach them. The
// convenience composes three acts over one claim: discover the artifact,
// add a device of the type its format records, load it and insert it.
let hdd0 = session.add_device_for("system.vdi", AccessIntent::Read)?
                  .attachment();                      // → hdd0

// The same three acts said one at a time — the door for a format that
// records several device types, nothing in a qcow2 saying which drive
// wrote it, so the device is mine to declare:
let hdd1 = session.add_device(HardDrive::MbrSector)?.attachment();
let data = session.load_media(                        // my open, my lock
    File::open("data.qcow2")?,
    Format::Qcow2 { device: HardDrive::MbrSector })?.id();
session.device_mut(hdd1).expect("just added")
    .insert(data)?;              // checked both ways: a drive takes only
                                 // the recordings its device type made
```

Then the reporting itself: I walk the drives in attachment order, and
each answers for what is in it.

```rust
for attachment in session.attachments() { // attachment order, which is
                                          // configuration I own
    let mut drive = session.device_mut(attachment).expect("just listed");
    let Some(disk) = drive.medium_mut() else { continue };  // an empty
                                 // drive is an answer, not a refusal
    let report = disk.inspect()?;         // once: the whole layered report

    match &report.content {            // stated, never inferred from
        DiskContent::Schema => {}      // which lists came back empty
        DiskContent::DirectVolume => {}
        DiskContent::Blank => {}
        DiskContent::UnknownNonblank { evidence } => println!("{evidence}"),
    }

    for region in &report.regions {    // every declared entry, in place
        println!("{attachment} {} {} {:#04x} {} {}",
            region.declared_number,    // the schema's own number, kept
            region.declared_placement, // "primary" · "logical"
            region.declared_type,      // the byte exactly as recorded
            region.declared_type_reading,   // what that byte declares, so
                                            // I keep no type table of mine
            region.issue.as_ref().map_or("read", |e| e.category().as_str()));
    }                                  // refused, and still numbered here

    for volume in &report.volumes {             // what actually composed
        match report.filesystem_on(volume.id) { // asked by identity, not
            None => println!("{:?} unread", volume.id),  // found by index
            Some(fs) => println!("{:?} {:?} {:?}",
                fs.kind,                // None where refused — the volume
                                        // stands either way
                fs.label.as_ref().and_then(|l| l.name.as_deref()),  // None
                                        // is "unlabeled", resolved by the
                                        // format rather than by my string
                fs.declared_geometry),  // what the boot record states
        }
    }

    let composed = report.composed_volume_count();            // two counts,
    let readable = report.readable_filesystem_volume_count(); // never one

    // Carrying an identity: I hold what the report issued and compare it
    // with what the object answers, and I build none of my own.
    if let Some(volume) = report.volumes.first().map(|v| v.id) {
        let space = disk.partition(1).expect("the schema declared it")
            .filesystem().expect("its declared type determines one");
        assert_eq!(space.volume_id(), Some(volume));  // one volume, one
                                                      // name, and U3's
                                                      // file work begins
                                                      // on that space
    }
}
```

The two halves stay apart throughout: a drive is configuration I state,
a medium is session state, and only the insert crosses. Ejecting severs
that link and leaves the disk in the pool with its claim and its
buffered writes intact; releasing the drive takes the configuration
down and never the medium. And every fact above comes
off the image alone: nothing boots, and reading changes no byte.

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
neither erasing its volume nor renumbering what follows. The label is
one whole answer, decided where the format is known: the label itself,
or the fact that the volume has none, with the format's own spelling of
unlabeled already resolved so I never compare a string to find that out
and an unlabeled drive is never given a placeholder. Whatever the
filesystem read to decide it comes back beside the answer, so I have the
literal bytes of any structure I care about without opening a sector.

I need two counts, not one: how many volumes composed, and how many
carry a filesystem the host read. A disk holding none is a disk I show as
holding none, and an unreadable volume stays in the report rather than
vanishing to keep that number right. Neither count says what a guest
called any of them, and I would not want one that pretended to: which
volume a guest's drive letter named is a fact about that guest, and I
carry the volume identity below and name it in my own terms. A disk that
cannot be read answers with the reason it could not
be read, never the symptom.

For one disk layout, an identity names exactly the same region, volume,
or filesystem in every file verb that it named in this report, and on
every later open of an unchanged layout. It belongs to the library and
I treat it as opaque — I never build one from a partition number, an
offset, a label, or a position in a list — and if it is absent on a
later open, that object is gone rather than renumbered. Carrying one
is holding the value this report issued and comparing it against the
value the object itself answers with, a partition and the space opened
on it each stating the identity of the volume they compose; it is
never a value I built handed to a verb. These identities are scoped to
the device holding the image, so two devices holding like layouts issue
like identities and it is the device I name that tells them apart. All
of it from the image alone, booting nothing.

## U5 — qcow2 images are first-class citizens of identification

Opening a qcow2 in the workbench identifies it like anything else:
the qcow2 container layer with its version and virtual size, the
partitions inside the virtual disk, the volumes inside those — the
same session, the same evidence model, the same adapter-driven
detection that identifies an h8d today.

## U6 — Differencing images are first-class disks

A stopped machine's disk is often an image whose content lives
partly behind it, and the library claims that shape in the two
formats it reads: a **qcow2 backing chain** — raw or qcow2 members,
possibly several levels deep — and a **VDI differencing chain**. I
open the top image and work exactly as U3 describes, as if the chain
were one disk: reads compose through the chain, a block or cluster
the top image never allocated reading through to the image behind it
where the format requires it, one it explicitly holds as zero
masking what is behind it, compressed clusters decompressed wherever
in the chain they sit. Writes allocate copy-on-write into the top
image only. A parent is never modified and the chain is never
flattened: after commit, the delivering hypervisor's own tooling
still reports the same relationship and reads the changed guest
bytes. A missing parent, a cycle, a chain deeper than the claimed
bound, a parent whose own version or type falls outside the claim,
encryption, an external data file — each is a named refusal (P3),
never a partial interpretation.

**How the parent is named is the formats' own business, and they
differ.** A qcow2 names its backing file by a relative path resolved
from the image that names it, and nothing in the file says the file
found there is the right one. A VDI names its parent by the parent's
**identity** and records no path at all, so the library searches for
the image declaring that identity — where the child sits, then the
directory above it, which is where this format's tooling leaves a
base image — and the identity is what checks the file the search
found. A file standing where the parent should be whose identity
does not match is a named refusal, never a substitute read in its
place; that check is evidence the other format does not offer, and I
get the benefit of it where it exists rather than a resolution
levelled down to what both can do.

*(Identification (U5) is deliberately untouched: a differencing
image identifies as the container it is, qcow2 or VDI. This entry is
about the attached medium reaching through the chain — the write
half is where the consumer's stopped-machine workflow lives today
and cannot move here without it.)*

## The media-first walks

**No discovery, complete user specification — the defining attribute of
the walks below**, and what sets them apart from the walks in U3 and U4,
which reach for discovery where the caller has no declaration to make.
The caller declares what they have — the format, the device it records,
every interpretation — and every declaration is checked against
evidence, refused by name where the evidence cannot bear it. These walks
are **permanent**: they remain valid, supported workflows even when
discovery and other conveniences evolve to make the same results easier
to achieve. Conveniences layer above the declared tier; they never
replace it.

## U25 — I master a 1541 disk from the captures on my filesystem and read its first byte

I have a directory of KryoFlux stream files — 168 of them, two heads by
eighty-four step positions, straight off the instrument — and I know
what they are: a capture of a Commodore 1541 disk. Nothing here is
inside any image or archive; these are loose files on my own
filesystem, opened by me — the locks are mine. I name what I have, and
I get back the disk itself.

```rust
let mut session = Session::new();

let members: Vec<File> = capture_paths          // …00.0.raw, …00.1.raw, all 168
    .iter().map(File::open)
    .collect::<io::Result<_>>()?;               // my opens — my locks, read-only

let disk = session.load_media(
    members, Format::KryoFlux { device: FloppyDrive::Commodore1541 })?;
assert_eq!(disk.device_type(),
           Some(DeviceType::Floppy(FloppyDrive::Commodore1541)));

let mut first = [0u8; 1];
disk.bytestream()?
    .location(Location::track(1))?
    .read_at(0, &mut first)?;
```

My declaration is checked, not trusted: the member names must carry
their positions, the set must be complete, the streams must parse, and
the capture must actually bear the c1541 claim — any failure refuses
the whole declaration by name. The reduction runs under the profile's
declared defaults; a choice no family convention can make refuses by
name and I answer it by growing my declaration. What I get back is a
1541 disk with the whole story as provenance — evidence, policy, and
the declared account of what the reduction could not carry — and the
byte I read is the first *framed* byte, because nothing before sync is
a byte at all.

## U26 — I open a captured C64 disk from an archive and list its CBM DOS directory

The same capture, but archived, the way captures actually circulate.
The archive is a medium; its entries are a namespace; the capture is a
collection of files I gather from that namespace and declare — the same
journey as U25 with one more link in front. Then I want what any C64
user wants first: the directory.

```rust
let mut session = Session::new();

let arc     = session.load_media(File::open("pcs_disk1.7z")?, Format::SevenZip)?;
let members = arc
    .partition(0).expect("an archive bears its direct partition")
    .filesystem().expect("an archive's content is its namespace")
    .files("")?;
let disk    = session.load_media(
    members, Format::KryoFlux { device: FloppyDrive::Commodore1541 })?;

let mut cbm = disk
    .partition(0).expect("flux media record no scheme: the direct partition")
    .filesystem_as("cbmdos")?;   // MY reading, checked against the recorded
                                 // structures: a protected or blank disk
                                 // refuses it by name — and the streams
                                 // beneath still answer

println!("{:?}", cbm.label()?);          // the BAM header: the disk's own name
for entry in cbm.entries("")? {
    println!("{:16} {:>4} {}",
        entry.name,                              // PETSCII: raw beside its reading
        entry.fact("size-blocks").unwrap_or(""), // CBM records size in blocks
        entry.fact("type").unwrap_or(""));       // PRG · SEQ · USR · REL
}
```

The listing is the recorded directory in directory order — the order is
evidence — with the disk name and ID as the BAM recorded them. This is
the file-access presentation reading recorded structures; it is not CBM
DOS running, and `LOAD"$"` — the directory as the drive's ROM
synthesizes it — is the future Commodore DOS device seam's journey, not
this one.

## U32 — I author a blank CHS disk and lay down its boot sector

Nothing is discovered here: there is no artifact yet. I am the author,
and my facts are the medium's original facts.

```rust
let mut session = Session::new();

let disk = session.new_media(NewMedia::ChsDisk {
    geometry: RecordingGeometry {
        cylinders: 1024, heads: 16, sectors_per_track: 63, sector_bytes: 512,
    },
})?;
// authored provenance — geometry mine, marked mine — and no device
// assumed: authorship is its own fact class, and only the future
// authored-to-recorded arc binds a device type
assert_eq!(disk.device_type(), None);
assert_eq!(disk.article(), "authored");
assert_eq!(disk.geometry().readings()[0].source, GeometrySource::Authorship);

let mut boot = [0u8; 512];
boot[510] = 0x55; boot[511] = 0xaa;
disk.write_sector(0, 0, 1, &boot)?;               // the authored geometry answers
disk.commit()?;
```

The kinds are enumerated as every creation grammar here is: the **blank
article kinds** — `NewMedia::Flexible525Soft`,
`NewMedia::Flexible525HardTen` — each name one article of the catalog
and make that manufactured substrate with nothing recorded on it, so
they state no coordinates and bear no content; `ChsDisk` is the kind
whose facts *are* coordinates, and a geometry with a zero anywhere in it
is refused when it is stated, which is the one moment authorship offers
to check it.

The disk is session-backed until an explicit encode gives it an
artifact: the commit point is the ordinary one and there is no recovery
journal, because no file changes for an interruption to leave
half-written. The arc from authored to recorded stays reserved: a future
partition editor consumes my geometry into MBR end tuples and BPBs,
after which any later discovery recovers it as evidence — the artifact
testifying for itself.

## U33 — The disk outlives its source, and enters a drive of its own

Media are session state, independent of every device and of each
other. The archive I mastered a disk out of is not the disk's parent —
I can release it, and the disk keeps answering; I can seat the disk in
a drive, unseat it, and release the drive, and the
disk is untouched throughout.

```rust
// …after U26's chain: `arc` (the archive) and `disk` (1541 disk) in the pool

session.release_media(arc_id)?;          // the source archive leaves the
                                         // session; the mastered disk is
                                         // free-standing and still answers:
let mut b = [0u8; 1];
disk.bytestream()?.location(Location::track(1))?.read_at(0, &mut b)?;

let unit8 = session                      // the drive an emulator will one
    .add_device(FloppyDrive::Commodore1541)?   // day address as unit 8
    .attachment();
session.device_mut(unit8).expect("just added")
    .insert(disk_id)?;

session.device_mut(unit8).expect("still here")
    .eject()?;                           // sever — claim and state survive
session.release_device(unit8)?;          // configuration falls; state
                                         // never does
```

## U34 — I load the one image inside an archive, by naming it

An archive holding a disk image is two media, and I take them one
declared step at a time: the archive by its format, then the image by
its own — a `File` from the first medium's namespace being an ordinary
source for the second.

```rust
let mut session = Session::new();

let arc  = session.load_media(File::open("HDOS_1-0.zip")?, Format::Zip)?;
let file = arc
    .partition(0).expect("an archive bears its direct partition")
    .filesystem().expect("an archive's content is its namespace")
    .get_file("HDOS_1-0_Issue_#50-00-00_890-1.h8d")?
    .source()?;                          // a File of OURS, taken as a
                                         // free-standing source — it
                                         // rides the archive's claim

let disk = session.load_media(file, Format::H8d)?;
assert_eq!(disk.device_type(),
           Some(DeviceType::Floppy(FloppyDrive::HeathH17)));

let mut hdos = disk
    .partition(0).expect("flexible media record no scheme: the direct partition")
    .filesystem_as("hdos")?;             // my reading — an h8d could bear
                                         // CP/M, so the choice is mine and
                                         // the check is the library's
for entry in hdos.entries("")? {         // a flat catalog: one root of leaves
    println!("{:12} {:>4} {}", entry.name,
             entry.fact("size-sectors").unwrap_or(""),
             entry.fact("flags").unwrap_or(""));
}
```

Nothing was guessed at any step: I named the entry rather than being
served "the only file", I declared each format, and I declared the
filesystem — the reading mine, the check the library's, at every rung.
