<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# USE-CASES (pledged)

> **Status:** pledged at the owner's direction. Every use case here is owed
> by the project and reaches root [USE-CASES.md](../../USE-CASES.md) only
> on full delivery. Numbers come from the one global U-sequence and are never
> reused.

## U17 — I use a hard-drive image as a logical block device

I am writing a consumer which needs the guest-visible contents of a hard
drive image at LBA granularity. I have a raw or qcow2 image and want to ask
for its logical block size and bounds, read complete logical blocks, and—when
I opened it for writing—replace complete logical blocks. I do not want to
parse qcow2 allocation tables, follow backing chains, infer partitions, mount
filesystems, emulate a storage controller, or confuse the image file's bytes
with the virtual device encoded inside it.

For qcow2, LBA zero means the first guest-visible logical block, not the qcow2
header. Reads compose allocated, zero, compressed, and backing-file data into
the byte sequence the guest device exposes. Writes allocate into the top
image only and remain inside Remanence's ordinary P2 commit point. For raw,
the same interface addresses the image's fixed guest-visible byte extent
without introducing a format-specific call.

### The exact semantic interface

The smallest useful read is one or more complete logical blocks from one
opened block presentation:

```rust
let mut session = Session::new();

let disk = session.load_media(
    File::open(hard_drive_image)?,
    Format::Qcow2 { device: HardDrive::MbrBlock },
)?;

let info = disk.block_info();
let contents = disk.read_blocks(
    Lba::new(first_lba),
    BlockCount::new(number_of_blocks),
)?;
```

The names are semantic pseudocode, not a pledge of literal Rust layout.
`info` reports at least the logical block size, logical block count, total
guest-visible byte length, access mode, active durable layer, and any
format-established block capabilities or limits relevant to correct use.
`read_blocks` returns owned bytes only after the entire requested range has
been read; a caller never receives a successful partial transfer.

The C and Python presentations preserve the same operation and units. C
receives an owned data handle or caller-owned output according to the
eventual binding design, Python receives `bytes`, and neither presentation
silently changes LBA into a serialized-file offset. All presentations use
checked fixed-width sizes and report overflow or out-of-range access before
performing I/O.

One medium is one block state: there is no device selection to make and no
identity to echo — the medium in my hand is the presentation. A later
multi-image composition selects media by their pool identities; `hdd0`, a
source-array position, and an image path never substitute for one.

### Logical block size is a claimed fact

The block presentation cannot exist without a logical block size because LBA
has no byte meaning without one. When an image format authoritatively records
that size, its adapter supplies it and a conflicting caller assertion is
refused. When the format does not record it—as with a geometry-opaque raw
byte image—the caller supplies it in the load declaration itself:
`Format::Raw { device: HardDrive::MbrBlock, block_bytes: 512 }`,
recorded as declared configuration, never as observed image evidence.

The declared `block_bytes: 512` therefore does not teach Remanence that
every hard drive has 512-byte sectors. It says that this consumer requires a
512-byte logical-block presentation for this load. A presentation which
requires format-declared size instead can refuse an image which lacks it.
Remanence never guesses block size from filename extension, total byte length,
partition-table plausibility, filesystem boot records, host sector size, or
the fact that 512 is common.

The logical block size is the address unit visible at this seam. Qcow2 cluster
size, host filesystem allocation size, overlay chunk size, and a reported
physical-sector hint are different facts and never change LBA arithmetic.
The guest-visible byte length must be an exact multiple of the selected
logical block size or the block presentation is refused.

### Reads and writes share one block-active state

The loaded medium holds one P23 block-active durable state. The partition,
volume, filesystem, and namespace presentations may later be derived from
that same state, but they are not invoked merely to satisfy a
block read. A block write changes the bytes those higher presentations would
subsequently observe; Remanence does not maintain a second mutable filesystem
or partition copy.

A writable journey is semantically:

```rust
let image = File::options().read(true).write(true).open(hard_drive_image)?;
let disk = session.load_media(
    image, Format::Qcow2 { device: HardDrive::MbrBlock })?;

disk.write_blocks(Lba::new(target_lba), replacement_blocks)?;
let observed = disk.read_blocks(
    Lba::new(target_lba),
    BlockCount::new(replacement_block_count),
)?;

disk.commit()?;
```

`replacement_blocks` must contain a positive whole number of logical blocks,
and the entire addressed range must lie within the fixed device bounds.
Validation occurs before the active state changes. Until `commit`, reads
through this presentation and every other view over the same state see the
replacement while the host image remains untouched; `rollback` discards it.
Releasing an uncommitted medium does not imply commit.

Commit encodes the changed block-active state through the original image
adapter under P2, P9, and P13. A raw image receives the corresponding byte
ranges. A qcow2 image allocates changed clusters in the top image, preserves
its backing relationship, and never modifies a backing file. Unsupported
image features or an encoding which cannot represent the change are refused
before a writable presentation is offered or before any host byte is changed.

### This is the block seam, not image-file access or controller emulation

The image-format adapter owns serialization and exposes one geometry-opaque
logical-block media instance. The block presentation exposes only the
claimed logical block size, bounds, reads, writes, access state, commit,
rollback, and externally observable refusals. That small interface hides raw
file I/O, qcow2 tables and compression, backing-chain traversal, sparse
allocation, overlays, journaling, and durable commit.

No cylinders, heads, tracks, zones, platters, rotation, controller registers,
commands, queues, firmware, caches, error-correction internals, or timing are
invented. A consumer emulating ATA, SCSI, IDE, NVMe, virtio-blk, or another
controller may translate its own commands into this block interface; the
controller and its timing remain the consumer's responsibility unless a
separate typed hardware presentation explicitly claims them.

Block is terminal under P13 and P23. Remanence never materializes CHS or flux
beneath this logical-block device, and no flux image is accepted by decoding
it into a speculative LBA disk. Conversely, opening the image at the block
seam does not parse its partition schema, compose a volume, recognize a
filesystem, or expose a P19 file container unless the caller separately asks
for those presentations.

### Completion and refusals

The read journey succeeds when the returned byte length is exactly
`number_of_blocks * logical_block_size` and every byte is the guest-visible
content of the requested LBA range. The report accounts for the source image,
image-format interpretation, authoritative layer, block-active device,
logical block-size evidence or assertion, bounds, and any backing artifacts.

The write journey succeeds when reads observe the complete replacement before
commit, rollback restores the prior view without touching the host image, and
commit durably updates only the source artifacts the image adapter is allowed
to mutate. Read-only writes, zero-length transfers, non-whole-block buffers,
integer overflow, ranges outside the device, conflicting or absent block-size
claims, truncated backing chains, unsupported qcow2 features, and commit
failures are named refusals. Remanence never clamps, grows the virtual device,
zero-fills an invalid range, returns a short successful transfer, or falls
back to serialized image-file access.

### Architectural consequence

The current disk stack already composes raw and qcow2 guest-visible byte
access behind a crate-private device seam and uses that state for partition
and filesystem work. U17 makes the architectural block presentation an
application capability rather than exposing that implementation trait or
the image file's bytes. Its future surface design must replace, not merely
publish, the byte-addressed internal shape with the P15/P23 logical-block
contract above.

That public replacement touches S1, S2, and S3 and must land coherently across
Rust, C, and Python. It depends on the in-force image adapters,
authoritative and active layer model, provenance, and assigned device identity. It is separate
from the delivered layered inspection report, which states the device and its
relationships, while U17 supplies direct block operations over that device.
Neither should grow a second orchestration path to serve the other.

### Deliberately outside this use case

- Reading or writing the serialized bytes of a qcow2 header, table, cluster,
  refcount structure, compression stream, or backing-file pathname.
- Partition discovery or editing, volume composition, filesystem recognition,
  file access, or operating-system namespace reconstruction.
- Resizing, truncating, growing, compacting, converting, rebasing, flattening,
  snapshot management, discard/TRIM, write-zeroes hints, or sparse-map
  inspection.
- ATA, IDE, SCSI, SAS, NVMe, virtio, USB mass-storage, BIOS, UEFI, controller,
  queue, interrupt, DMA, cache, timing, or command-set emulation.
- Invented CHS, physical-sector behavior, platter geometry, flux, or magnetic
  recording underneath a geometry-opaque logical-block device.
- Partial-block writes, short successful transfers, or implicit block-size
  guessing.

## U23 — I save a KryoFlux capture of a C64 disk as a P64 image

> **Withdrawn from the in-force list, 2026-08-10 (D28), and rewritten
> around the shape it is owed in.** Of the four steps below, the first
> three are built — F59 delivered the collection-sourced `load_media`
> and the disk it masters (D35) — and two gaps remain. **Step 4 is
> unbuilt**: the write verbs are format-specific, hang on the
> `.remanence` image root, and no verb takes a destination format on
> the medium, so a capture loaded as a medium reaches no writer at all
> until this lands. And **the load declares the drive where this entry
> is owed a recognition**: `Format::KryoFlux { device }` takes the
> caller's declaration, where a commercially mastered C64 disk carries
> no 1541's signature and requiring the caller to name the drive would
> refuse good captures. The recognition this entry leans on is built
> and runs inside the declared load — a pinned verdict with its
> evidence, riding the medium as provenance — but no load takes the
> *ranking* yet, and `disk.recognition()` is unbuilt with it. **Step 2
> does not fold into step 1** — step 1's call already answers with the
> archive, and one verb giving two answers chosen by inspecting content
> is the discovery the declared tier exists to keep out (D28).
>
> **Two shortcuts are deliberate, approved at the owner's direction on
> 2026-08-10, and neither is laziness.** The namespace is reached
> without a partition step, the direct-partition ceremony buying nothing
> for a medium that records no scheme. And the drive is recognized
> rather than declared, as above. This entry sits
> above the media-first walks and is not governed by their
> declared-tier preamble.

I have a KryoFlux capture of a Commodore 64 floppy: raw stream files,
one per drive-step position, captured from both of the disk's sides and
delivered inside a 7z — the second side being the unrecorded back of a
single-sided disk, which the capture cannot tell me and the drive
family can. It is capture evidence, not a disk image. Each stream holds
several recorded revolutions, flux before the first index and after the
last, index and control/OOB records beside the flux, and a transfer
result — and nothing in it says which revolution "the" disk was. I want
a P64 out of it: one file, addressed by 1541 half-track, holding timed
pulses with strength. I am asking for a transformation, not a reading
of the capture, and I am told exactly what it will do and exactly what
it cannot carry **before** it writes anything.

```rust
let mut session = Session::new();

// 1. The archive is a medium, and this is what that call answers with.
let arc     = session.load_media(File::open("pcs_disk1.7z")?, Format::SevenZip)?;

// 2. Its content is a namespace; the capture is the collection I gather
//    from it and declare. Materializing a disk out of an archive's
//    contents is a second act because it is one. I name the format and
//    not the drive: what wrote this disk is the library's to recognize.
let members = arc.filesystem()?.files("")?;
let disk    = session.load_media(members, Format::KryoFlux)?;

// 3. What comes back is a disk, reached the way every medium is — not a
//    root peculiar to captures — and it says what the evidence made of
//    it rather than what I guessed.
println!("{}", disk.recognition()?);     // the ranked verdict and its
                                         // evidence: rates, zones, the
                                         // stepping it reads back on

// 4. One verb takes the destination format, and describing it writes
//    nothing: I read what the crossing costs and then decide.
let crossing = disk.describe_as(Format::P64)?;
for loss in crossing.declared_loss() { println!("{loss}"); }

disk.save_as("pcs_disk1.p64", Format::P64)?;
```

**I declare what I have, and no more than I have.** The format is mine
to name — these are KryoFlux streams and I know it — and the member
names must carry their positions, the set must be complete, and the
streams must parse, any failure refusing the whole declaration by name
before any reduction begins. **What wrote the disk is not mine to
name**, and this is not a convenience: a great many C64 disks were
never written by a 1541 at all. A commercially duplicated title comes
off a mastering machine whose signature is its own, and it reads back
on a 1541 without carrying a 1541's own fingerprint. Made to declare
the drive, I would either be refused a disk that is perfectly good or
be believed about something I was guessing at — and the first is worse,
because it turns a real capture into an error message. So the library
recognizes the recording from the evidence and reports a **ranked
verdict with what it saw**, rather than confirming or rejecting a claim
I had no standing to make.

**I do not name a side either.** The family records one surface, and
which of the capture's two heads carries it is measured — the
unrecorded back of a single-sided disk reads as noise, and telling it
from a recording is the library's job, not the fixture's and not mine.

**Two owners, and neither infers the other's answer.** The **drive
profile the recognition named** owns the physical reduction and runs it
under that profile's declared defaults — the rig's lattice, the write
geometry, the tolerances, the frame each observation projects into,
which for a 1541 is the drive's 16 MHz reference clock across one 300
RPM rotation. I do not restate what the family already declares; a
choice no family convention can make refuses by name and I answer it by
growing my declaration (P29, nothing unnamed). Where the recognition
cannot name a profile at all, that is a refusal naming the verdicts it
weighed — not a default profile applied quietly, which would put a
reading in my hands that nothing established. The **P64 image-format
adapter**
owns its grammar and its capability claim: what the container can hold,
the version it claims, how a disk encodes into it, and what it refuses
by name. Each states its own crossing in its own terms, so I read two
accounts in sequence — the reduction's, riding the disk's provenance,
and the container's, answered by `describe_as` — rather than one
assembled by whichever of them ran last.

**P64 cannot carry a KryoFlux capture.** That is not a defect of either
format, and it is not something I should discover from a smaller file.
The reduction enumerates what it drops in the source's own terms — the
head bearing no recording; positions whose evidence names no recording
at all, being noise floor, guard band, or a fringe no recording claims;
marker channels and control/OOB records with no expression past the
angular frame they established; retained foreign records, capture
metadata and transfer results; and flux recorded before a transfer's
first index and after its last, which no bounded revolution covers — and
the container enumerates what it cannot express of what survives: the
declared policy itself, each half-track's provenance, the located
origin, and the disk's own statement that it was derived at all. A
count is not an account, and loss reported after the fact would not do.

**The saved image says what it is.** Its pulses carry
selected-and-projected provenance, not recovered-evidence provenance,
and nothing in it is presented as an observation of the original
recording that was not one. The same capture and the same declaration
produce the same disk and — the P64 encoding being deterministic — the
same destination bytes.

Writing the artifact is the only step that touches the filesystem; it
creates the destination under its own claim, and an existing
destination is a named refusal rather than an overwrite, leaving no
file behind. Past that: an incomplete, duplicate or contradictory
capture set, refused before any reduction begins; a capture the
recognition can name no profile for, refused with the verdicts it
weighed; a source position the named profile's half-track map does not
cover; a timebase the destination cannot express; a disk the P64 claim
cannot encode; and an existing destination path. Each names the rule it
broke.

**A position whose content its neighbour also holds is no longer among
them.** The old entry refused it until I declared which it was, because
flux alone cannot tell a head reading its neighbour from an instrument
that did not move. The gap-first reduction measures it instead — two
adjacent steps carrying the same recording group under measured
agreement in the gap domain, the fat track measured rather than
asserted — so what was a refusal awaiting my declaration is now a fact
the reduction establishes and reports.

The journey runs on the prepared Pinball Construction Set disk-one
capture set: both sides, 84 stream members each, opened through the 7z
catalog and recognized as one capture. I read the account, write the
P64, and reopen the result through the adapter's own decode, which
presents the same half-tracks, at the same angles, with the same
strengths.

*(This entry claims that the declared reduction is performed
faithfully, reproducibly, and with its loss named. It does not claim
that any particular protected title loads in an emulator from the
result: whether protection survives is a property of the capture and
the family's declared reduction, and the library reports what it did
rather than promising an outcome it cannot see. Nothing here descends
below flux or interprets what the pulses mean — no GCR, no sectors, no
filesystem, no files — the sources are never edited or consumed, and
no public flux, pulse, or capture-run iterator is offered: the
transformation is the surface and the evidence stays behind it.
Consuming the image is a separate journey that meets this one at the
file.)*

## The media-first walks

Ten walks were pledged with the media-first storage model, numbered U25
through U34; the delivered ones live at root
[USE-CASES.md](../../USE-CASES.md), keeping their numbers, and the rest
wait here.

**No discovery, complete user specification — the defining attribute of
every use case below.** The caller declares what they have — the
format, the device it records (the partition scheme riding the device
spec), every interpretation — and every declaration is checked against
evidence, refused by name where the evidence cannot bear it. No
discovery does any specifying here. **Partition information in
particular is specified, never discovered**: the scheme rides my
declared device type, checked against the table at load; a partition's
interpretation is my reading of its entry (`check_type`); and a namespace
nothing determines is my reading too (`filesystem_as`) — checked,
every one, and probed for never. **Local artifacts arrive as the caller's own opened files** —
`File::open` below is `std::fs::File`, the portable file; files from
inside media are this library's own views — and whoever opens owns the
lock: my open is my safeguard and the library's claim, checked for what
it affords (may it write?), honoured exactly, never escalated. A name
recovered from a handle serves location only — the commit journal's
*beside*, a backing parent's *next door* — under an identity check, and
a nameless handle refuses those journeys by name. The simplified
workflows where discovery does the specifying
work belong to the question tier
([../proposed/design/question-tier.md](../proposed/design/question-tier.md)),
proposed and argued separately — and **these walks are permanent**:
they remain valid, supported workflows even when discovery and other
conveniences evolve to make the same results easier to achieve.
Conveniences layer above the declared tier; they never replace it. Together the walks exercise every core
concept the media-first storage model
([design/media-first-storage-model.md](design/media-first-storage-model.md))
pledges: the session's devices and media and their lifecycles, the
declared creation grammar
and its source shapes, the device types, the partition pool and
the vantage doors, the edge, writing and the commit point, and
authorship.

## U27 — I walk a qcow2 hard disk to its DOS root directory

I have a qcow2 of a DOS machine's hard disk, and I know how that
machine addressed it. I declare both facts — the format, and the device
it records — then walk the partition table the way DOS did: the first
entry, which I say is a DOS primary, and the FAT volume on it.

```rust
let mut session = Session::new();

let disk = session.load_media(
    File::open("dos_hd.qcow2")?,
    Format::Qcow2 { device: HardDrive::MbrBlock },
)?;                                      // the spec carries the scheme: the
                                         // table must parse as MBR at load,
                                         // or the declaration refuses by name
assert_eq!(disk.device_type(),
           Some(DeviceType::HardDrive(HardDrive::MbrBlock)));

let part = disk.partition(1).expect("the declared table bears entry 1");
part.check_type(PartitionType::DosPrimary)?;   // declared, checked against
                                               // the raw type byte — 0x06
                                               // bears it, 0x05 refuses
                                               // naming both sides

let fs = part.filesystem().expect("DosPrimary determines FAT; verified at check_type");
for entry in fs.files("")? {
    println!("{:12} {:>9} {}", entry.name, entry.size_bytes,
             entry.fact("attributes"));
}
```

The partition ordinal is the table's own fact — MBR entry 1, its place
preserved — and my `DosPrimary` is a reading the evidence must bear,
never a relabeling of it.

## U28 — I read COMMAND.COM's first bytes off a CHS hard disk image

A VDI this time, of a disk its machine addressed by cylinder, head and
sector. The geometry is not mine to state: the image's own structures
recorded it — the FAT boot sector wrote down sectors-per-track and
heads, the MBR's end tuples agree — so the disk answers sector
questions from evidence, and I go straight to the file I care about.

```rust
let mut session = Session::new();

let disk = session.load_media(
    File::open("dos_hd.vdi")?,
    Format::Vdi { device: HardDrive::MbrSector },
)?;
assert_eq!(disk.device_type(),
           Some(DeviceType::HardDrive(HardDrive::MbrSector)));
// geometry: evidence read UNDER my declarations (BPB, MBR end-tuples) —
// verification fills values, it never picks readings — so
// disk.read_sector(c, h, s) answers, and disagreement between the
// sources comes back Undetermined rather than settled

let part = disk.partition(1).expect("the declared table bears entry 1");
part.check_type(PartitionType::DosPrimary)?;

let mut head = [0u8; 8];
part.filesystem().expect("DosPrimary determines FAT; verified at check_type")
    .get_file("COMMAND.COM")?            // FAT 8.3 matching, without regard
    .read_at(0, &mut head)?;             // to case
```

## U29 — I read a boot partition's boot block, consulting no filesystem

Sometimes the bytes I want are exactly the ones no namespace names. I
find the partition the MBR marks active and read the first sixteen
bytes of its own space — the boot block — through the volume door,
with no filesystem consulted and no offsets computed by hand.

```rust
let mut session = Session::new();

let disk = session.load_media(
    File::open("dos_hd.qcow2")?,
    Format::Qcow2 { device: HardDrive::MbrBlock },
)?;

let boot = disk.partitions().into_iter()
    .find(|p| p.active())                // the MBR's own boot flag — evidence
    .expect("a bootable image marks one partition active");

let mut block = [0u8; 16];
boot.volume().expect("a DOS partition composes its addressable space")
    .read_at(0, &mut block)?;            // byte 0 OF THE PARTITION — addressed
                                         // within the space's own extent
```

The volume door and the filesystem door open onto the same one space; I
chose the vantage that answers by position, and a partition bearing no
filesystem at all would have answered this walk identically — which is
the point.

## U31 — I write a file onto a DOS disk, and nothing moves until I commit

```rust
let mut session = Session::new();

let image = File::options().read(true).write(true).open("dos_hd.qcow2")?;
                                         // my open, my lock — and the library
                                         // checks exactly one thing: whether
                                         // it is allowed to write
let disk = session.load_media(
    image, Format::Qcow2 { device: HardDrive::MbrBlock })?;
let part = disk.partition(1).expect("the declared table bears entry 1");
part.check_type(PartitionType::DosPrimary)?;

let fs = part.filesystem().expect("DosPrimary determines FAT; verified at check_type");
fs.make_directory("OUT")?;
fs.write_file("OUT/REPORT.TXT", report)?;

disk.commit()?;                          // THE commit point: until this line
                                         // the image file was untouched, and
                                         // rollback() would have cost nothing
```

The write authority is my own open's — the library checked what my
handle affords and honoured it — and the commit is durable: the journal
lands beside the file, its name recovered from my handle for location
and nothing else; interrupted anywhere, the next open reconciles to
wholly the old image or wholly the new one.

## U35 — I make a blank DOS floppy, put files on it, and save it as a raw image

Nothing is read here. I want a standard DOS floppy that never existed —
a 1.2 MB 5.25-inch or a 1.44 MB 3.5-inch — formatted FAT12 the way DOS
would format it, with my files on it, written out as the `.img` file an
emulator or a Gotek expects. I am the author of every fact on it.

```rust
let mut session = Session::new();

let disk = session.new_media(NewMedia::Flexible35Hd)?;
                                         // the article: a 3.5-inch HD
                                         // cookie with nothing on it —
                                         // no coordinates, no content
assert_eq!(disk.device_type(), None);    // and no device assumed

let fs = disk.partition(0)               // the direct partition
    .expect("an authored blank bears one")
    .record_as(Recording::Dos144)?;      // THE authored-to-recorded arc:
                                         // 80 cylinders × 2 heads × 18
                                         // sectors of 512 bytes, a BPB,
                                         // two FATs, a 224-entry root —
                                         // exactly what FORMAT lays down
fs.write_file("AUTOEXEC.BAT", script)?;
fs.make_directory("DATA")?;
fs.write_file("DATA/NOTES.TXT", notes)?;
disk.commit()?;                          // the commit point (P2), no
                                         // journal beneath it (D36)

let report = disk.describe_raw()?;       // computes everything, writes
                                         // nothing; states what the raw
                                         // artifact will not carry (P29)
disk.write_raw("blank144.img")?;         // 1,474,560 bytes; an existing
                                         // file is a refusal, never an
                                         // overwrite
```

The 1.2 MB journey is the same with `NewMedia::Flexible525Hd` and
`Recording::Dos12`: 80 × 2 × 15 × 512, 1,228,800 bytes.

### What the recording states

The recording kinds are an enumerated claim (P3), like every creation
grammar here: each names one published DOS floppy layout and lays down
precisely that — the geometry, the media descriptor byte, the BPB fields,
the FAT count and size, the root directory size — and nothing chosen on
my behalf. The two this journey needs:

| Kind | Article | C × H × S × bytes | Media byte | Bytes |
|---|---|---:|---:|---:|
| `Dos12` | `flexible-5.25-hd` | 80 × 2 × 15 × 512 | `0xF9` | 1,228,800 |
| `Dos144` | `flexible-3.5-hd` | 80 × 2 × 18 × 512 | `0xF0` | 1,474,560 |

A recording kind declares which article it records onto, and recording
it onto another is refused by name: `Dos144` does not fit the 5.25-inch
article's track density, and the check is the catalog's, not a guess
from the author's intent. It is the same shape as a format declaring the
device types it admits.

After the arc the medium is no longer merely authored. Its geometry gains
a reading whose source is the recording I chose; it binds the device type
the layout is recorded for, so it goes into a PC floppy drive like any
loaded image would — which means the drive catalog gains the PC
families (a 5.25-inch 1.2 MB drive and a 3.5-inch 1.44 MB drive), since
today it names Commodore and Heath drives alone; and `partition(0).filesystem()` opens FAT12 over it
by evidence — the boot record I recorded testifying for itself — through
the very same door U31 writes a file through on a loaded disk. The file
verbs are not new; the arc reaches a door already built.

### What the raw encode is

`write_raw` is the sector medium's rendition, paired with `describe_raw`
the way the C64 renditions pair `write_d64` with `describe_d64`: the
content in the recording's own sector order, cylinder-major, head-minor,
sectors from one, and nothing else. The report states what a raw artifact
cannot carry — the article and its facts (P14), the authored provenance,
the recording kind — because a raw image records bytes and no ecosystem.
The file is built alongside and moved into place whole (P9), an existing
file at the destination is refused, and a blank article that was never
recorded onto has no content to encode and says so.

Loading the result back with `Format::Raw { device: FloppyDrive::…,
block_bytes: 512 }` identifies a FAT12 volume whose geometry is read off
the BPB as evidence, agreeing with the recording I chose — which is the
test that the arc recorded what it claimed.

### Deliberately outside this use case

- Choosing a layout by inspecting the article — 720 KB on an HD cookie,
  a 360 KB layout on a 1.2 MB disk. Those are legitimate DOS layouts and
  are further recording kinds when someone names them, not defaults.
- A free-form BPB: every field mine to set. A recording kind is a
  published layout, whole; a partly stated one is refused as a
  classification is (P3).
- Formatting a *loaded* disk. The arc records onto an authored blank; a
  disk with an artifact behind it already testifies for itself, and
  overwriting that testimony is a different journey with its own
  refusals.
- Writing a boot loader. The boot record carries the BPB and the
  signature; the code bytes are zero, and putting a system on the disk
  is `SYS`'s job, not `FORMAT`'s.
- Any encode beyond raw. ImageDisk write is F69 and the flux masterings
  are F74; a raw rendition is the one every emulator reads and the one
  a sector medium states exactly.
- Long file names, FAT16, or any disk that needs a partition table. This
  floppy bears the direct partition and an 8.3 namespace, like U3's.
