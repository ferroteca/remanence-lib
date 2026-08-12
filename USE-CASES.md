# USE-CASES

> **Status: in force.** Every use case on this list is met by the code
> as it exists today — this is an implementation claim, not an
> aspiration, and **a divergence between an entry here and the code is
> a bug**. Numbers come from the one global U-sequence and are never
> reused. Proposed and pledged use cases live under
> [planning/](planning/README.md) until full delivery brings them here.

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
image to a storage device in my session, ask that device which
filesystem it resolves to — or select one by the opaque identity its
inspection report issued for a volume, where several bear one — and
work through the filesystem it answers with, which is the one type
carrying file verbs; a path within it names the file. Where the guest
was DOS, U22's composer maps that same volume identity to the drive
letter I show a user. All of
this without booting the guest and without any external helper
process: the library does
the format work itself. Reading never changes the image. Writing is
a separate, explicit mode with a commit point: until I commit,
everything I wrote can be rolled back cleanly.

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

## U4 — I retrieve a stopped machine's partition and volume information

My automation layer's drive reporting runs on host-side facts about a
stopped machine's disk images, and this library is where those facts come
from (the guest's own drive letters are U22's mapping, over the same
facts). For each disk — qcow2, VDI or
raw — one inspection answers, keeping each fact at the seam that owns
it rather than flattening them into one snapshot.

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
vanishing to keep that number right. Which volume a guest's drive letter
named is not a count and not mine to derive: that is U22's composer, over
a rule this library owns. A disk that cannot be read answers with the reason it could not
be read, never the symptom.

For one disk layout, an identity names exactly the same region, volume,
or filesystem in every file verb that it named in this report, and on
every later open of an unchanged layout. It belongs to the library and
I treat it as opaque — I never build one from a partition number, an
offset, a label, or a position in a list — and if it is absent on a
later open, that object is gone rather than renumbered. These
identities are scoped to the device holding the image, so two devices
holding like layouts issue like identities and it is the device I
name that tells them apart. All of it from the image alone, booting
nothing.

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

## U22 — I present a stopped DOS machine's drives without reimplementing DOS

I automate a stopped DOS machine from the host, and I hold its
configuration: which image sits in which floppy slot, which images are its
hard disks and in what order they are attached, and whether a CD-ROM is
present. I show a user the drives that machine's DOS would have
presented — `A:`, `C:`, `D:` — each with the label DOS would have shown,
and then write `A:\OUT\X.TXT` into one of them.

The facts I own are machine configuration: medium, slot, and attachment
order. Every other fact in that sentence is a rule of the format or of DOS
— whether a volume has a label at all, what a file may be called, and which
letter a volume takes — and each is read from the disk by the same library
that reads the disk. All three are the library's: I ask a volume for its
label and get one answer, I hand over the name I have and get back the rule
any refusal broke, and I assert my machine facts and get back the mapping.

### The label is the filesystem's own reading

I ask a volume for its label and get one answer: the label, or the fact
that it has none. FAT spells "no label was given" as `NO NAME`, so that
string is absence, not a label, and the distinction is made where the
format is known rather than by a string comparison in my code.

The two places FAT records a label — the boot record's field and the root
directory's volume-ID entry — can disagree, and choosing between them is a
policy about a format, not about my application. The answer applies that
policy; both readings stay beside it as evidence, so a caller with a
different need can see the literal bytes without opening a sector.

### The name is the file-access seam's own rule

A read matches a name the way DOS matched it — without regard to case —
and gives me back the name as stored, so what I show the user is what the
directory holds. A write validates and normalizes at the same seam: I hand
over `out\x.txt` and the library stores `X.TXT`, because uppercasing is
part of what writing a DOS name means and doing it in my code is doing the
library's job badly.

When a name cannot be a DOS name, the refusal names the rule it broke —
too long a base, a second dot, a character the format excludes, a reserved
device name — so I can tell the user which rule, in their words, and can
branch on the rule without reading a sentence. A generic "invalid name"
leaves me guessing, and guessing here means reimplementing the rule set to
produce a message.

### The letter is a mapping derived from a declared rule

I supply the machine facts — medium, slot, attachment order — and the
library returns the mapping: which volume each drive letter names. It
returns letters it can establish and says plainly which it cannot, rather
than filling the gap with an order that happens to look right. A machine
with one floppy still has two floppy letters, the second being the phantom
drive rather than a second volume; a machine with none has neither.

This is not the Windows case. Windows persists its own mapping, and reading
that mapping is a separate journey; DOS persists nothing, so the mapping is
a *rule* applied to machine facts, and the rule is what has to be named. The
answer therefore states which assignment rule produced it and treats what
the rule cannot settle — a resident driver's letters, a `LASTDRIVE`
ceiling, an assignment a DOS variant makes differently — as undetermined
rather than assumed. I state which DOS the machine ran and the mapping is
settled by that variant's rule; I state none and a letter the claimed
variants disagree on comes back undetermined with each rule's answer in the
reason, never averaged into one that is nobody's.

I address the result by the identity the report gave me, not by the letter:
the letter is what I show a user, and the identity is what I pass back to
the library.

### What I keep

Parsing a guest address is mine: `A:\OUT\X.TXT` splits into a letter and
path segments because the address is my user's input, not the disk's
content. Naming which image occupies which slot is mine, because it is a
fact about the machine I configured and not evidence in any image.
Restating a named refusal in my own words is mine. Everything between those
two ends belongs to the library.

*(Deliberately outside this entry: booting or emulating the guest, DOS
itself, its drivers or its firmware; long file names, VFAT and FAT32, this
journey being the 8.3 namespace; reconstructing a mapping a resident
driver, `SUBST`, `JOIN`, `ASSIGN` or a network redirector would have
changed at runtime, or inferring one from a `CONFIG.SYS` the images may not
even hold; claiming every DOS variant's assignment order at once, the
applied rule being a named claim like any other (P3) and disagreement
between variants being reported rather than averaged; inferring slot or
attachment order from filename, array position, or image content; guessing
a label from a directory name, a filesystem kind, or a file inside the
volume; and repairing a name the caller supplied, which is refused rather
than truncated, transliterated, or renamed to fit.)*

## The media-first walks

**No discovery, complete user specification — the defining attribute of
the walks below.** The caller declares what they have — the format, the
device it records, every interpretation — and every declaration is
checked against evidence, refused by name where the evidence cannot
bear it. Local artifacts arrive as the caller's own opened files —
`File::open` below is `std::fs::File`, the portable file; files from
inside media are this library's own views — and whoever opens owns the
lock: my open is my safeguard and the library's claim, checked for what
it affords (may it write?), honoured exactly, never escalated. These
walks are **permanent**: they remain valid, supported workflows even
when discovery and other conveniences evolve to make the same results
easier to achieve. Conveniences layer above the declared tier; they
never replace it.

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
disk.put_sector(0, 0, 1, &boot)?;               // the authored geometry answers
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

## U33 — The disk outlives its source, and enters a machine of its own

Media are session state, independent of every machine and of each
other. The archive I mastered a disk out of is not the disk's parent —
I can release it, and the disk keeps answering; I can seat the disk in
a reconstructed machine, unseat it, and tear the machine down, and the
disk is untouched throughout.

```rust
// …after U26's chain: `arc` (the archive) and `disk` (1541 disk) in the pool

session.release_media(arc_id)?;          // the source archive leaves the
                                         // session; the mastered disk is
                                         // free-standing and still answers:
let mut b = [0u8; 1];
disk.bytestream()?.location(Location::track(1))?.read_at(0, &mut b)?;

let mut c64 = session.add_machine("c64")?;
let unit8 = c64                          // the drive an emulator will one
    .add_device(FloppyDrive::Commodore1541)?   // day address as unit 8
    .attachment();
session.machine_mut("c64").expect("just added")
    .device_mut(unit8).expect("just added")
    .insert(disk_id)?;

session.machine_mut("c64").expect("still here")
    .device_mut(unit8).expect("still here")
    .eject()?;                           // sever — claim and state survive
session.release_machine("c64")?;         // the cascade: configuration falls
                                         // with its owner; state never does
```
