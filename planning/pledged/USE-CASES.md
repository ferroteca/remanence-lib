<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# USE-CASES (pledged)

> **Status:** pledged at the owner's direction. Every use case here is owed
> by the project and reaches root [USE-CASES.md](../../USE-CASES.md) only
> on full delivery. Numbers come from the one global U-sequence and are never
> reused.

## U2 amendment — browsing is a plain walk down, whatever holds the files

In-force U2 claims the browse-and-extract journey over a vintage volume —
HDOS today. This amendment restates the journey's shape; it claims no new
format, and writing stays U3's journey.

Getting a file out of a floppy image is a walk down the scopes that
actually exist: a session to work in, the drive the disk belonged in,
the image in that drive, its filesystem, my file.

```
session = Session()
drive   = session.add_device(heathkit_h17)
drive.load_media("games.h8d")
fs      = drive.filesystem()
file    = fs.get_file("CHESS.ABS")
```

I never named a machine, and that is the point: I am opening an
artifact, not reconstructing a computer. My drive went into the
session's anonymous machine, which holds devices and composes no
namespace — if I asked it for drive letters it would refuse and tell me
to declare the machine I mean, because devices I never grouped are not a
machine's configuration. When I *am* reconstructing a machine, I say so
and add one (U22).

The drive is mine to state, and I state the one my machine had — the
H-17, the hard-sectored Heathkit drive — not "some floppy". Which device
serves a medium is a fact about my machine, not about the image, and
stating it concretely is what gives `load_media` something to check: an
H8D is a ten-sector hard-sectored 5.25-inch disk, and a drive family
that spins soft-sectored media refuses it by name rather than reading it
wrongly. There is no "generic floppy" to add: the lineage's interior
names classify drives and answer questions, but only a concrete entry
instantiates, because only a concrete drive declares anything. Stating
the drive is also how one exists empty.

I hold the drive and nothing else. The disk in it is not a second thing
to carry: the drive answers for what it holds, and answers by name that
it holds nothing when it is empty. Swapping disks changes what my drive
reports without changing what I hold.

The filesystem is resolved, never guessed: `drive.filesystem()` walks
down to the one filesystem when every layer between has exactly one
supported answer, and that is why I name no volume here. When the image
holds two volumes, the resolver refuses by naming both, and I select by
the identity the report issued — never by a position. When the volume
bears no filesystem, the answer is a named absence, not an empty
listing. When the source falls short of its own declaration, I get the
bounded, evidence-stated degraded reading rather than an all-or-nothing
loss.

An archive is the same journey, not a parallel one: loaded into its own
device, `load_media("games.zip")` gives that device a medium whose
content *is* a namespace — its `filesystem()` always answers — and the
same `get_file` walks it. When one of those entries is itself a disk
image, it goes into a drive of its own and I keep reading — the archive
on my host was never part of any machine the disk belonged to, and
nothing here is composing a machine's namespace to be confused by that.
Reading never mutates anything.

*(Deliberately unchanged: the write journey is U3's; typed sector and
block access is the emulator family's demand (U9–U12); HDOS remains the
claimed catalog today. The model this journey falls out of is
[design/storage-model-and-vocabulary.md](design/storage-model-and-vocabulary.md).)*

## U7 — A C64 emulator delegates the hardware behind the 1541 disk VIA

I am writing a C64 emulator that executes the 1541's 6502 code and emulates
its two 6522 VIAs. My emulator owns the CPU, RAM, ROM, address decoding,
VIA registers and timers, IEC bus, and scheduler. Remanence owns the
drive-side electronics connected to the disk VIA, the mechanism, and the
inserted medium. It does not emulate either 6522.

I load `<floppy-name>.p64` into one read-only 1541 drive-hardware instance.
Whenever my disk-VIA implementation changes a connected output, I send the
complete drive-side control state to Remanence at that transition's real
time. Between changes I advance Remanence to its next observable signal
transition and feed the resulting levels and edges back into my VIA and
6502 models. Stock ROM code and uploaded custom drive code therefore use
ordinary 6522 behavior; neither knows that Remanence supplies the hardware
behind the pins.

The smallest useful read outcome is one correctly timed byte-ready edge
with the corresponding raw eight-bit GCR value and sync level. The caller's
6522 decides how that edge latches port A or raises an interrupt, and the
caller routes the real byte-ready fan-out to the 6502 `SO` input. The drive
program performs GCR decoding and recognizes headers, sectors, checksums,
and files.

### The exact semantic interface

Every typed presentation of the common P15 hardware layer uses the same
timed-causality operations:
reset, read current time, find the next outward deadline, advance to a
time, apply one timestamped control change, and inspect current outward
signals without advancing. The 1541 specialization supplies typed control
and signal bundles rather than CPU addresses or 6522 registers:

```rust
let mut hardware = Hardware::<C1541Drive>::open(
    C1541DriveOptions {
        weak_pulse_seed: 0x0123_4567_89ab_cdef,
    },
    vec![MediaAttachment {
        slot: C1541MediaSlot::Drive,
        source: floppy_path.into(),
        access: AccessIntent::Read,
        write_protected: true,
    }],
)?;

let effects = hardware.reset(C1541Tick::ZERO)?;
apply_drive_signals_to_via_and_cpu(effects);

let effects = hardware.interact(
    at,
    C1541DriveStimulus::SetControl(C1541DriveControl {
        stepper_phase,
        motor_on,
        density_zone,
        byte_ready_enabled,
        write_mode,
        port_a_drive: DrivenByte { value, mask },
    }),
)?;
apply_drive_signals_to_via_and_cpu(effects);

while let Some(deadline) = hardware.next_event_tick() {
    if deadline > scheduler_limit {
        break;
    }
    let effects = hardware.advance_to(deadline)?;
    apply_drive_signals_to_via_and_cpu(effects);
}

let signals = hardware.inspect(C1541DriveInspect::Signals)?;
```

The semantic bundles are:

```rust
pub enum C1541DriveStimulus {
    SetControl(C1541DriveControl),
}

pub struct C1541DriveControl {
    pub stepper_phase: C1541StepperPhase,
    pub motor_on: bool,
    pub density_zone: C1541DensityZone,
    pub byte_ready_enabled: bool,
    pub write_mode: bool,
    pub port_a_drive: DrivenByte,
}

pub struct DrivenByte {
    pub value: u8,
    pub mask: u8,
}

pub struct C1541DriveSignals {
    pub port_a_drive: DrivenByte,
    pub write_protected: bool,
    pub sync_active: bool,
    pub byte_ready: LineLevel,
}

pub enum C1541DriveEvent {
    SignalsChanged(C1541DriveSignals),
}

pub enum C1541DriveInspect {
    Signals,
}
```

`C1541Drive` binds the common hardware interface's associated types to
`C1541Tick`, `C1541DriveStimulus`, the unit response,
`C1541DriveEvent`, `C1541DriveInspect`, and `C1541DriveSignals`.
`DrivenByte` describes which port-A pins the caller's VIA currently drives;
it does not expose the VIA data-direction register itself. An effects record
contains a new outward snapshot only when a caller-visible signal changes.
The transition of `byte_ready` is the physical event the caller presents to
the VIA's CA1 input and, through the 1541's real fan-out, to the 6502 `SO`
input. Remanence does not emit a VIA interrupt or mutate a CPU flag.

The C presentation uses the common opaque `remanence_hardware_t` and the
operations `remanence_hardware_open`, `_reset`, `_now`,
`_next_event_tick`, `_advance_to`, `_interact`, and `_inspect`. A 1541
contract descriptor and typed C records preserve the controls, signals, and
events above without opaque payload bytes. The Python presentation carries
the same semantic interface in Python idiom under P5.

`open` recognizes and validates P64, loads its evidence-bearing magnetic
state, inserts it into a compatible 1541 mechanism, and takes the P7 claim
for the instance's lifetime. `reset` resets only Remanence-owned drive
electronics and mechanism state. The caller separately resets the CPU and
VIAs and then supplies their resulting drive-side control bundle.

`AccessIntent::Read` protects the host artifact. The medium is also
physically write-protected in this journey, so `write_protected` is asserted
and modeled write current cannot change it. VIA register writes and output
changes still complete normally on the caller's side.

### Time and causal ordering

One `C1541Tick` is one cycle of the drive's 16 MHz reference clock. All
times are absolute ticks since open. The caller maps its CPU and VIA phases
to that clock consistently.

`next_event_tick` reports the earliest tick at which `port_a_drive`,
`write_protected`, `sync_active`, or `byte_ready` can change without another
caller input. `advance_to` may reach that deadline but may not silently
cross an undelivered transition. `interact` first advances to its
timestamp and is refused if doing so would cross such a deadline. At one
tick, an already caused outward transition is delivered before a new
control change, and any transition caused by that change follows it.

`inspect` is side-effect free and untimed. It exists for wiring,
stopped debugging, and tests; guest execution does not use it to obtain a
new byte or bypass causal advancement.

### One complete P64 read

The unchanged 1541 firmware configures its disk VIA. The emulator resolves
the VIA's output registers, data directions, and control-line modes into one
`C1541DriveControl` snapshot and calls `interact` whenever that snapshot
changes. Starting the motor and changing stepper phase cause the modeled
spindle and head to advance; density selection configures the 1541 read
channel.

As flux passes the head, Remanence performs pulse detection, filtering,
clock recovery, sync detection, and byte assembly. At a byte boundary it
updates `port_a_drive` and transitions `byte_ready` at the modeled tick. The
emulator supplies those signals to its VIA and CPU. Firmware can then wait
with `BVC`, clear overflow with `CLV`, and read its own `$1c01` VIA register.
That register read is entirely caller-owned; the byte it returns originated
from Remanence's current drive-side `port_a_drive` signal.

Repeating this interaction gives arbitrary drive code its raw timed GCR
byte stream. The smallest complete success is not `read sector N` and not a
public flux iterator: it is one drive-side byte-ready transition which an
independent 6522 implementation turns into the correct programmer-visible
read.

### The P64 floor

Under P22, one timed flux-transition pulse with detection strength is the
lowest modeled magnetic-data unit. P64 pulse positions and strengths remain
inside the image, active-media, and read-channel modules. Strong, weak, and
missing pulses become timed recovery, sync, byte-ready, and GCR values at
the hardware seam. Successive passes may differ where stored pulse strength
demands it, while the same image, control history, and weak-pulse seed remain
reproducible.

No public P64-shaped pulse iterator is required. The family hardware
presentation translates durable flux into the signals needed by a controller
or I/O-chip emulator while preserving timing and uncertainty.

### The reference-emulator retrofit

This use case must be testable by adapting a cycle-aware C64 emulator which
already executes the 1541 CPU and emulates its VIAs. VICE's current source
supports this cut: its common VIA implementation and 1541 VIA adapter are
separate from `drive/rotation.c`, whose 1541 paths already perform GCR and
P64 circuit simulation and update read data, sync, and byte-ready state.

The retrofit therefore keeps VICE's CPU, VIA, memory map, IEC bus, and VIA
interrupt behavior. Its P64 attachment creates a Remanence drive-hardware
instance; disk-VIA output changes become `interact` calls; Remanence
deadlines enter VICE's scheduler; and returned signal transitions feed the
existing VIA/CPU inputs. The successful fork boots the unchanged drive ROM,
loads through IEC, runs uploaded custom code, and reproduces seeded weak
pulses without routing VIA register accesses into Remanence.

### Deliberately outside this use case

- Emulating either 6522, the 1541 CPU, RAM, ROM, IEC bus, DOS, or uploaded
  machine code.
- Decoding GCR into sectors or files.
- Mutating the magnetic medium; a writable journey is separate.
- Save states, power sequencing, disk swapping, multiple drives, parallel
  cables, board modifications, and non-1541 variants.
- A universal signal enumeration shared by unrelated drive families. The
  timed-causality operations are common; this signal bundle is specific to
  the 1541 drive-side electronics.

## U16 — I reconstruct a stopped machine's storage namespace from its disk set

I have a set of raw or qcow2 disk images captured from one stopped machine,
and I know which image occupied `hdd0`, `hdd1`, and so on. I want Remanence to
tell me how the installed operating system saw that storage: Windows drive
letters and volume mount points, or one Unix root with its mounted
filesystems. I provide those attachment placements, but I do not want to
attach the images to the host, boot the guest, or manually restate every
partition and mount.

I open the complete set read-only. Remanence inspects every image and reports
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

The smallest useful operation is opening one composed file-container view
after the set has been inspected:

```rust
let machine = ArtifactSet::open(
    [
        AttachedArtifact {
            attachment: Attachment::Hdd(0),
            source: system_drive,
        },
        AttachedArtifact {
            attachment: Attachment::Hdd(1),
            source: data_drive,
        },
    ],
    AccessIntent::Read,
)?;

let report = machine.inspect()?;

let files = machine.open_system_namespace(
    InstallationSelection::Unique,
)?;

let contents = files.read_file(path_from_caller)?;
```

For Windows, `path_from_caller` may be
`C:\Users\Paul\Documents\example.txt`. For Unix it may be
`/home/paul/example.txt`. The returned object is the P19 file-container view
of the selected installation's composed namespace, so ordinary file
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
let files = machine.open_system_namespace(
    InstallationSelection::ById(selected.id()),
)?;
```

The names are semantic pseudocode, not a pledge of literal Rust layout. The C
and Python presentations preserve the same deep operation: supply one typed
attachment placement per source, atomically open the read-only artifact set,
inspect it, then request the unique installation or pass back one opaque
installation identity issued by that report. They do not ask the caller to
reconstruct partitions or namespace mappings from source paths, array order,
partition numbers, drive letters, labels, UUID spellings, or byte ranges.

`ArtifactSet::open` takes a P7 claim for every supplied image as one atomic
operation. Every source carries one caller-supplied, composition-unique
attachment identity such as `hdd0` or `hdd1`; array order establishes
nothing. Each image-format adapter exposes its own addressed device, and
Remanence separately assigns that device a composition-scoped P21 identity.
The attachment is an asserted placement in the stopped machine, not the
device identity and not evidence extracted from the image. Each device
retains its own P23 active durable layer; opening the set never concatenates
or otherwise merges their address spaces.

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
   volumes' P19 file-container views at the evidenced roots and paths.

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
provenance back through each mounted file-container view, filesystem, volume,
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
opened filesystem file-container view. Optional, inactive, removable,
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
device or one of its regions. P19 then maps several filesystem file-container
views into the selected operating system's namespace. A path crossing from
`C:` to `D:`, or from `/` into `/home`, crosses a namespace mapping; it does
not make those volumes one address space.

U14 is the distinct case where regions from several devices form one striped
volume before a filesystem can be read. U16 needs a multi-source open and
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
- A generic container tree, global or caller-authored device identities, or a
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
let artifact = Artifact::open(
    hard_drive_image,
    AccessIntent::Read,
)?;

let mut blocks = artifact.open_block_device(
    BlockDeviceSelection::Unique,
    BlockConfiguration {
        logical_block_size: BlockSizeSelection::Asserted(512),
    },
)?;

let info = blocks.info();
let contents = blocks.read_blocks(
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

An ordinary single-image open yields one block device and therefore does not
ask the caller to echo a P21 device identity. `BlockDeviceSelection::Unique`
means exactly one candidate, not the strongest guess. A later multi-image
composition may select a reported device by its opaque identity, but `hdd0`,
a source-array position, and an image path never substitute for that identity.

### Logical block size is a claimed fact

The block presentation cannot exist without a logical block size because LBA
has no byte meaning without one. When an image format authoritatively records
that size, its adapter supplies it and a conflicting caller assertion is
refused. When the format does not record it—as with a geometry-opaque raw
byte image—the caller must supply the size expected by the consuming hardware
or software. The report records that value as asserted configuration rather
than observed image evidence.

`BlockSizeSelection::Asserted(512)` therefore does not teach Remanence that
every hard drive has 512-byte sectors. It says that this consumer requires a
512-byte logical-block presentation for this open. A presentation which
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

Opening the presentation establishes one P23 block-active durable state. The
partition, volume, filesystem, and file-container presentations may later be
derived from that same state, but they are not invoked merely to satisfy a
block read. A block write changes the bytes those higher presentations would
subsequently observe; Remanence does not maintain a second mutable filesystem
or partition copy.

A writable journey is semantically:

```rust
let artifact = Artifact::open(
    hard_drive_image,
    AccessIntent::Write,
)?;

let mut blocks = artifact.open_block_device(
    BlockDeviceSelection::Unique,
    BlockConfiguration {
        logical_block_size: BlockSizeSelection::Asserted(512),
    },
)?;

blocks.write_blocks(Lba::new(target_lba), replacement_blocks)?;
let observed = blocks.read_blocks(
    Lba::new(target_lba),
    BlockCount::new(replacement_block_count),
)?;

blocks.commit()?;
```

`replacement_blocks` must contain a positive whole number of logical blocks,
and the entire addressed range must lie within the fixed device bounds.
Validation occurs before the active state changes. Until `commit`, reads
through this presentation and every other view over the same state see the
replacement while the host image remains untouched; `rollback` discards it.
Dropping an uncommitted writable open does not imply commit.

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
