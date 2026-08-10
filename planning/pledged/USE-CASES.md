<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# USE-CASES (pledged)

> **Status:** pledged at the owner's direction. Every use case here is owed
> by the project and reaches root [USE-CASES.md](../../USE-CASES.md) only
> on full delivery. Numbers come from the one global U-sequence and are never
> reused.

## U16 — I reconstruct a stopped machine's storage namespace from its disk set

I have a set of raw or qcow2 disk images captured from one stopped machine,
and I know which image occupied `hdd0`, `hdd1`, and so on. I want Remanence to
tell me how the installed operating system saw that storage: Windows drive
letters and volume mount points, or one Unix root with its mounted
filesystems. I provide those attachment placements, but I do not want to
attach the images to the host, boot the guest, or manually restate every
partition and mount.

I load every image read-only, seat each in my declared machine, and
inspect. Remanence inspects every seated medium and reports
the independently established devices, partition schemas, regions, volumes,
filesystems, and candidate operating-system installations. If the entire set
contains exactly one supported installation candidate, I may ask Remanence to
accept that singleton and reconstruct its persisted storage namespace. If it
contains two or more candidates, Remanence returns the complete lower-layer
report and every installation candidate but does not compose a guest
namespace until I select the installation I mean.

The fallback selection is an operating-system installation, not merely a
"boot partition." On an ordinary UEFI Windows system the EFI System
Partition which starts the boot and the NTFS volume containing Windows and
its SYSTEM hive are different regions. A Unix `/boot` may likewise be
separate from the root filesystem whose `/etc/fstab` defines the mounted
namespace. A frontend may let me begin by choosing a reported partition, but
that choice is sufficient only when it resolves to exactly one evidenced
installation.

### The exact semantic interface

The smallest useful operation is opening the machine's composed namespace
after the seated set has been inspected:

```rust
let mut session = Session::new();
let sys  = session.load_media(
    File::open(system_drive)?,
    Format::Qcow2 { device: HardDrive::MbrBlock },
)?.id();
let data = session.load_media(
    File::open(data_drive)?,
    Format::Qcow2 { device: HardDrive::MbrBlock },
)?.id();

let pc = session.add_machine("captured-pc")?;
pc.add_device(hdd0)?.insert(sys)?;
pc.add_device(hdd1)?.insert(data)?;

let report = pc.inspect()?;

let files = pc.namespace(InstallationSelection::Unique)?;
let contents = files.read_file(path_from_caller)?;
```

For Windows, `path_from_caller` may be
`C:\Users\Paul\Documents\example.txt`. For Unix it may be
`/home/paul/example.txt`. The returned object is the machine namespace —
P35's `MachineFilesystem`, presenting the P19 interface over the selected
installation's composed mapping — so ordinary file
operations do not need to know which image, partition, volume, or filesystem
supplies a path.

`InstallationSelection::Unique` is deliberately strict. It succeeds only
when the report contains exactly one supported installation candidate; it
does not mean "best," "most likely," or "first." Passing `Unique` is the
caller's explicit permission to accept that singleton convenience. A caller
which always requires an affirmative human choice can inspect first and use
`ById` even when only one candidate exists.

If unique selection returns ambiguity, the same open set and report remain
usable:

```rust
for installation in report.operating_system_installations() {
    show_candidate_and_evidence(installation);
}

let selected = choose_installation_from_report(&report)?;
let files = pc.namespace(
    InstallationSelection::ById(selected.id()),
)?;
```

The names are semantic pseudocode, not a pledge of literal Rust layout. The C
and Python presentations preserve the same deep operation: load each source,
declare each placement, inspect, then request the unique installation or
pass back one opaque installation identity issued by that report. They do not ask the caller to
reconstruct partitions or namespace mappings from source paths, array order,
partition numbers, drive letters, labels, UUID spellings, or byte ranges.

Each medium's claim is my own read-only open, honoured as afforded for
the pooled lifetime, and the machine's device set carries the placements: `hdd0` and `hdd1` are
attachment identities I declared by adding the devices, and no ordering of
loads establishes anything — the attachment order is the order I added the
devices, a fact of my configuration. The attachment is an asserted
placement in the stopped machine, not a device identity and not evidence
extracted from any image. Each medium retains its own P23 active durable
layer; seating several media in one machine never concatenates or otherwise
merges their address spaces.

### Installation selection substitutes for unmodeled boot policy

Remanence does not model the machine's BIOS or UEFI boot policy: firmware
NVRAM boot order, one-time boot overrides, controller enumeration rules,
chain-loading, or a bootloader menu's default and saved selection. It
therefore cannot claim which installed operating system the stopped machine
would actually have booted.

Instead, Remanence reports every supported operating-system installation it
can establish from the supplied storage. When more than one exists, caller
selection substitutes for the missing firmware-and-bootloader decision. That
selection is recorded as an assertion in the result's provenance; it is not
promoted into evidence recovered from the images.

The `hdd0` placement, an MBR active flag, an EFI System Partition, EFI boot
files, or a bootloader configuration may help identify and describe
candidates. None selects one candidate on the caller's behalf. The singleton
convenience is safe only because there is no competing candidate in the
report, and it still makes no claim about historical boot behavior.

### Discovery stops where evidence stops

The successful composition has two phases:

1. Image, partition-schema, volume-composition, and filesystem adapters
   inspect each disk through the ordinary layered seams. Every valid lower
   result remains reportable whether or not an operating system can be
   selected.
2. Operating-system adapters identify installation candidates and the
   persisted configuration by which each candidate maps discovered volumes
   into its namespace. A namespace-composition adapter then maps the selected
   volumes' P19 filesystem views at the evidenced roots and paths.

For Windows, the selected installation's registry and other claimed system
metadata establish persisted drive-letter, volume-GUID, and folder-mount
mappings. EFI boot files or an active partition may be useful evidence for
finding an installation, but do not assign `C:`. For Unix, the selected root
filesystem and its persisted mount configuration establish `/` and the mounts
beneath it; boot-loader configuration may corroborate the selected root, but
`/boot` does not define the namespace merely because it is bootable.

The installation result records the evidence which makes it a candidate,
its root volume or equivalent namespace authority, any separate boot-related
regions, and every storage mapping it claims. The composed namespace keeps
provenance back through each mounted filesystem view, volume,
region, assigned device identity, asserted attachment, and source image.
Unmounted or unreferenced volumes remain in the lower-layer report; namespace
composition does not erase them.

Exactly one candidate means the candidate set contains one entry, not that
one interpretation scored higher than its competitors. If two Windows
installations, two Unix roots, or one of each are reported, unique selection
returns ambiguity with every candidate's evidence. It does not rank them by
boot flag, attachment, EFI entry, recognizable SYSTEM hive, or familiar
directory structure. If no candidate is established, it returns a named
absence or refusal while preserving the disk report.

An explicitly selected installation changes only the final interpretation.
It does not suppress competing installations or promote the caller's choice
into observed fact. The report distinguishes discovered mappings from
caller-selected installation identity.

### Full namespace means all required storage is accounted for

A successful result contains the complete storage namespace which the
selected, supported installation requires from the supplied image set. Each
required local mount must resolve uniquely to a discovered volume and an
opened filesystem view. Optional, inactive, removable,
network, and deliberately unsupported mappings may remain named omissions
when the operating system's persisted configuration makes that status clear.

If a required image is missing, a persisted volume identity matches no
discovered volume, two volumes match equally, a required filesystem cannot be
opened, or the installation's mount configuration is inconsistent, opening
the full namespace returns a refusal at the owning seam. It does not silently
drop the mount and present a partial tree as the requested complete result.
The inspection report still exposes every independently valid disk fact and
the unresolved mapping evidence so the caller can supply a corrected image
set or choose a different installation deliberately.

### This is namespace composition, not a multi-device volume

Every volume in this journey is composed independently through P17 from one
device or one of its regions. P19 then maps several filesystem views into the
selected operating system's namespace. A path crossing from
`C:` to `D:`, or from `/` into `/home`, crosses a namespace mapping; it does
not make those volumes one address space.

U14 is the distinct case where regions from several devices form one striped
volume before a filesystem can be read. U16 needs several media seated in one machine and
machine-level namespace composition, but it neither requires nor implies
multi-device volume assembly, manual stripe recipes, or cross-source write
transactions. U13 remains the focused one-image Windows case and demonstrates
the same unique-installation route without an artifact set.

### Completion and refusals

The journey succeeds when Remanence returns the requested file through the
selected installation's complete composed namespace and the report accounts
for every source image, caller-supplied attachment, assigned device,
partition schema, region, volume, filesystem, installation candidate,
selection, and mount mapping involved.

An unsupported image or filesystem feature, invalid partition schema,
ambiguous installation, unreadable registry hive or Unix mount configuration,
missing or duplicate attachment identity, missing required source image,
unresolved persisted volume identity, path which escapes the composed
namespace, or missing requested file is reported with the strongest known
cause and its evidence. Remanence never guesses an attachment from array or
filename order, an installation from attachment order, or a namespace mapping
from partition order, size, label, type, directory names, or coincidental
readability.

### Architectural consequence

U16 is a concrete read-only use for the multiple-source topology which D5
deferred. It claims only the minimum needed here: atomic read-only opening
of a caller-supplied image set with one required typed attachment placement
per image, composition-scoped device and result identities, reported
operating-system installation candidates, explicit caller selection whenever
the candidate set is not a singleton, and P19 namespace composition over
independently opened filesystem views. It does not reopen D5's deferred
multi-device volumes or cross-source writes.

D5's multi-source-open deferral was adjudicated before this pledge: pledged
P32 admits a session device set carrying `hdd0`-style attachment identities,
which is the assignment D5 declined and D5's entry now records as no longer
declined. The placements above are that vocabulary, and D5's deferrals of
volumes spanning devices and of cross-source transactions stand untouched.
P19 already admits a system-wide namespace adapter but deliberately leaves
its implementation to a later feature, so the surface design this needs
lands with the feature that cuts the work. Until it is delivered, the
current single-disk surfaces continue to bind.

### Deliberately outside this use case

- Booting or emulating the guest, firmware, controllers, or drive hardware.
- Reconstructing or simulating BIOS/UEFI NVRAM boot policy, one-time boot
  overrides, controller boot order, chain-loading, or bootloader-menu choice.
- Writing any image, filesystem, registry, mount configuration, or file;
  coordinating commit or rollback across source images.
- Constructing a striped, mirrored, parity, spanned, dynamic-disk, LVM-like,
  or other volume from regions on several devices; U14 owns that distinct
  journey.
- Guessing which installation the caller means when more than one remains
  evidenced.
- Inferring an omitted `hddN` placement from array order, filename, image
  contents, or the selected installation.
- Treating an EFI System Partition, active MBR partition, Unix `/boot`, or
  first disk in the supplied array as the installation selector.
- Inventing Windows drive letters or Unix mount points from partition order,
  filesystem labels, directory names, or host conventions.
- Live host attachment, concurrent mutation, network shares, removable media,
  runtime-only mounts, or configuration not persisted in the supplied
  artifacts.
- Encryption, credentials, ACL-policy emulation, or recovery of secrets.
- A generic namespace tree, global or caller-authored device identities, or a
  universal operating-system model. Caller-authored attachment placement is
  required and remains a separate concept under P21.

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
> around the shape it is owed in.** The journey below is media-first and
> is not what runs today: today it runs through a surface built for
> captures alone — a `CaptureSet` root outside the device model, a plan
> and an image reached from it, and a `write_p64` verb belonging to that
> image. Of the four steps below, the first
> is built and the rest are not: `load_media` takes one path and no
> collection (the collection-sourced load is F59's), there is no disk
> kind a capture loads into, and every write verb is format-specific and
> hangs on the root that produced it. **Step 2 does not fold into step
> 1** — step 1's call already answers with the archive, and one verb
> giving two answers chosen by inspecting content is the discovery the
> declared tier exists to keep out (D28).
>
> **Two shortcuts are deliberate, approved at the owner's direction on
> 2026-08-10, and neither is laziness.** The namespace is reached
> without a partition step, the direct-partition ceremony buying nothing
> for a medium that records no scheme. And **the drive is recognized
> rather than declared** — see the body: a commercially mastered C64
> disk carries no 1541's signature, so requiring the caller to name the
> drive would refuse good captures. The recognition this leans on is
> already built (`CaptureSet::recognize`, a ranked verdict with its
> evidence); what is missing is the load taking it. This entry sits
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

## The media-first walks — U25 through U34

**No discovery, complete user specification — the defining attribute of
every use case below.** The caller declares what they have — the
format, the device it records (the partition scheme riding the device
spec), every interpretation — and every declaration is checked against
evidence, refused by name where the evidence cannot bear it. No
discovery does any specifying here. **Partition information in
particular is specified, never discovered**: the scheme rides my
declared device type, checked against the table at load; a partition's
interpretation is my reading of its entry (`as_type`); and a namespace
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
Conveniences layer above the declared tier; they never replace it. Together the ten exercise every core
concept the media-first storage model
([design/media-first-storage-model.md](design/media-first-storage-model.md))
pledges: the pools and their lifecycles, the declared creation grammar
and its source shapes, the device types, the partition pool and
the vantage doors, the edge, writing and the commit point, authorship,
and the machine tier's own half.

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

## U26 — I open a captured C64 disk from a zip and list its CBM DOS directory

The same capture, but zipped, the way archives actually circulate. The
zip is a medium; its entries are a namespace; the capture is a
collection of files I gather from that namespace and declare — the same
journey as U25 with one more link in front. Then I want what any C64
user wants first: the directory.

```rust
let mut session = Session::new();

let arc     = session.load_media(File::open("pcs_disk1.zip")?, Format::Zip)?;
let members = arc
    .partition(0).expect("an archive bears its direct partition")
    .filesystem().expect("an archive's content is its namespace")
    .files("")?;
let disk    = session.load_media(
    members, Format::KryoFlux { device: FloppyDrive::Commodore1541 })?;

let cbm = disk
    .partition(0).expect("flux media record no scheme: the direct partition")
    .filesystem_as("cbmdos")?;   // MY reading, checked against the recorded
                                 // structures: a protected or blank disk
                                 // refuses it by name — and the sectors and
                                 // streams beneath still answer

println!("{}", cbm.label()?);            // 0 "PINBALL     " PC 2A — the BAM header
for entry in cbm.files("")? {
    println!("{:16} {:>4} {}",
        entry.name,                      // PETSCII: raw beside its reading
        entry.fact("blocks"),            // CBM records size in blocks
        entry.fact("type"));             // PRG · SEQ · USR · REL, flags beside
}
```

The listing is the recorded directory in directory order — the order is
evidence — with the disk name and ID as the BAM recorded them. This is
the file-access presentation reading recorded structures; it is not CBM
DOS running, and `LOAD"$"` — the directory as the drive's ROM
synthesizes it — is the future Commodore DOS device seam's journey, not
this one.

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
part.as_type(PartitionType::DosPrimary)?;   // declared, checked against
                                            // the raw type byte — 0x06
                                            // bears it, 0x05 refuses
                                            // naming both sides

let fs = part.filesystem().expect("DosPrimary determines FAT; verified at as_type");
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
// disk.get_sector(c, h, s) answers, and disagreement between the
// sources comes back Undetermined rather than settled

let part = disk.partition(1).expect("the declared table bears entry 1");
part.as_type(PartitionType::DosPrimary)?;

let mut head = [0u8; 8];
part.filesystem().expect("DosPrimary determines FAT; verified at as_type")
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

## U30 — I reconstruct a DOS machine and letter its drives

Two hard disk images, and I know their order; a third drive that was
present and empty. I declare the machine, its devices in attachment
order, and link the media — and the letters come from *my machine's own
configuration*, not from an assertion beside it.

```rust
let mut session = Session::new();
let c = session.load_media(
    File::open("boot_hd.qcow2")?,
    Format::Qcow2 { device: HardDrive::MbrBlock },
)?.id();
let d = session.load_media(
    File::open("data_hd.qcow2")?,
    Format::Qcow2 { device: HardDrive::MbrBlock },
)?.id();

let pc = session.add_machine("pc")?;
pc.add_device(hdd0)?.insert(c)?;
pc.add_device(hdd1)?.insert(d)?;
pc.add_device(hdd2)?;                    // present and empty — configuration
                                         // in its own right, holding no volume

let map = pc.compose_dos_letters(Some(DosAssignmentRule::MsDos5), &[])?;
for m in &map.mappings {
    println!("{}: {:?}", m.letter, m.outcome);   // C:, D: — by attachment order
}
```

The mapping's provenance names my machine, the devices lettered in the
order I added them, and the empty drive that contributed no volume —
derived from configuration I declared, carried as provenance, never as
evidence.

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
part.as_type(PartitionType::DosPrimary)?;

let fs = part.filesystem().expect("DosPrimary determines FAT; verified at as_type");
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

## U32 — I author a blank CHS disk and lay down its boot sector

Nothing is discovered here: there is no artifact yet. I am the author,
and my facts are the medium's original facts.

```rust
let mut session = Session::new();

let disk = session.new_media(NewMedia::ChsDisk {
    cylinders: 1024, heads: 16, sectors: 63, sector_bytes: 512,
})?;
// authored provenance — geometry mine, marked mine — and no device
// assumed: authorship is its own fact class, and only the future
// authored-to-recorded arc binds a device type
assert_eq!(disk.device_type(), None);

let mut boot = [0u8; 512];
boot[510] = 0x55; boot[511] = 0xaa;
disk.put_sector(0, 0, 1, &boot)?;               // the authored geometry answers
disk.commit()?;
```

The disk is session-backed until an explicit encode gives it an
artifact. The arc from authored to recorded stays reserved: a future
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
// …after U26's chain: `arc` (zip archive) and `disk` (1541 disk) in the pool

session.release_media(arc_id)?;          // the source archive leaves the
                                         // session; the mastered disk is
                                         // free-standing and still answers:
let mut b = [0u8; 1];
disk.bytestream()?.location(Location::track(1))?.read_at(0, &mut b)?;

let c64 = session.add_machine("c64")?;
c64.add_device(cbmfloppy0)?.insert(disk_id)?;   // the drive an emulator will
                                                // one day address as unit 8

c64.device(cbmfloppy0).expect("just added").eject()?;   // sever — claim and
                                                        // state survive pooled
session.release_machine("c64")?;         // the cascade: configuration falls
                                         // with its owner; state never does
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
    .get_file("HDOS_1-0_Issue_#50-00-00_890-1.h8d")?;

let disk = session.load_media(file, Format::H8d)?;   // a File of OURS —
                                                      // it rides the archive's claim
assert_eq!(disk.device_type(),
           Some(DeviceType::Floppy(FloppyDrive::HeathH17)));

let hdos = disk
    .partition(0).expect("flexible media record no scheme: the direct partition")
    .filesystem_as("hdos")?;             // my reading — an h8d could bear
                                         // CP/M, so the choice is mine and
                                         // the check is the library's
for entry in hdos.files("")? {           // a flat catalog: one root of leaves
    println!("{:12} {:>4} {}", entry.name,
             entry.fact("size-sectors"), entry.fact("flags"));
}
```

Nothing was guessed at any step: I named the entry rather than being
served "the only file", I declared each format, and I declared the
filesystem — the reading mine, the check the library's, at every rung.
