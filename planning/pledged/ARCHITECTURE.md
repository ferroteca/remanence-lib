<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# ARCHITECTURE (pledged)

> **Status:** pledged at the owner's direction. Every principle here is
> owed by the project and is armed only when it reaches root
> [ARCHITECTURE.md](../../ARCHITECTURE.md), where a divergence becomes
> a bug. Numbers come from the one global P-sequence and are never
> reused.
>
> A principle that establishes a seam guarantees that the architecture
> can host implementations at that seam without redesigning adjacent
> layers. Examples test the required generality; they do not claim or
> pledge support for any named variation. Actual support remains a named,
> enumerated claim under P3.

## P12 — Image formats are implementations at representation seams

Every supported image format is an adapter at the seam matching the
representation it persistently encodes. The adapter owns its identity,
recognition and evidence, validation and refusals, variants,
interpretation, capabilities, decoding, and encoding where writing is
claimed. Raw sectors, logical-block containers, encoded tracks, flux
recordings, and filesystem-level images illustrate distinct image-format
families; they do not collapse into one universal interface.

There is no universal image-format language. Shared modules own mechanisms
demonstrated by multiple implementations, while the choices and parameters
that give those mechanisms meaning stay with the applicable image-format
module. The image-format catalog is wiring: every entry pairs a descriptor
with behavior, and adding an ordinary image format changes its module,
tests, and one mechanical enrollment. Central orchestration neither
interprets string-named format rules nor branches on an image-format
identifier.

This deepens P1 and makes P3 and P4 local obligations: the module making a
claim is the module that knows what it supports and why it refuses the
rest. P13 governs which representation the image makes authoritative.

## P13 — One image layer is authoritative

Every loaded image has exactly one authoritative image layer, declared by
the image-format adapter that recognizes it. It may be a file tree or
filesystem structure, addressed sectors or logical blocks, encoded tracks,
flux transitions, or another representation claimed by that image-format
family.
Persistent container bytes are its encoding, not automatically its image
layer. Every other representation is derived from the authoritative one:
decoding toward logical meaning, or deterministic synthesis toward a
lower-level mechanism. A decoded view carries its evidence; synthesized
detail is identified as synthetic and is never presented as recovered
evidence.

An authoritative layer does not imply that every other layer exists.
Derivation stops at the seam claimed for the image and integration contract. A
virtual hard-disk image presented through LBA never gains inferred
platters, heads, tracks, flux, or other hardware state. A sector-addressed
legacy image may infer a hardware-level representation when its image format,
media profile, integration contract, and synthesis rules together claim the
mapping. That
representation is synthesized from the sectors; it is not evidence of the
original medium's physical recording.

Block and flux are disjoint representation families. No adapter or
composition converts a geometry-opaque logical-block device into flux, or
flux into a logical-block device. This is a prohibition, not merely the
absence of a current derivation. A flux-active medium may still expose
derived sector, filesystem, and file presentations, and a block-active
device may expose derived volume, filesystem, and file presentations;
neither interpretation changes the durable active layer into the other
family.

The authoritative layer does not change during an open image's lifetime.
A writable composition is offered only when every derivation on the path
can project its changes back to that layer and its image format without
unclaimed loss. Otherwise that composition is read-only or refused before
use. Choosing another authoritative layer is an explicit conversion that
creates a new image and names any loss; it is never a side effect of
loading, attaching, or saving the original.

## P14 — Media is independent recorded state

A media instance is the independent mutable state between image formats
and drives. It names an immutable, family-specific profile containing
passive compatibility facts; the recorded contents belong to the instance.
Magnetic flexible media, optical media, and logical-block media illustrate
families whose state and compatibility facts differ too much for one
schema. A family owns its representation and small interface.

Image-format adapters load and save media state. Hardware and CHS
presentations operate on that state through their own seams. A media profile
contains neither image recognition nor hardware behavior and cannot
implicitly choose how far hardware emulation descends. The media-type
catalog may be declarative because media profiles are passive; it is not a
language for behavior.

## P15 — Hardware emulation is one common timed-causality layer

Remanence implements one common hardware-emulation layer, not a catalog of
modeled drive products. A typed integration contract selects the real seam
presented to a caller and configures the common layer's reusable timing,
mechanism, read-channel, and electronics modules. Several presentations may
operate over the same media instance without creating independently mutable
copies of its recorded state. Product names in examples identify the
contract and physical profile being tested; they do not create catalog
entries or make support claims.

The default integration seam is the common drive interface presented to the
system, with the device beneath it opaque. LBA hard drives and modern optical
or large-storage drives stop there: physical geometry, pickups, servos,
integrated controllers, firmware, error correction internals, and microcode
are not inferred or emulated.

Selected legacy integration contracts may instead claim one or more
hardware seams. The most reusable cut is at the controls and signals
between a caller-owned controller or I/O chip and the drive-side electronics
Remanence models. The caller advances both sides on one explicit clock,
applies timestamped control changes, and consumes causally ordered signal
transitions. A contract may also claim a higher seam which includes a
controller when that is independently useful, but the common hardware
interface does not require Remanence to emulate the processor-facing chip.
It need not expose every internal pin or make flux and bit streams public.

The Commodore 1541 fixes the required generality. Its lowest public drive
seam is the drive-facing side of the disk 6522 VIA, not the 6502 address bus
and not the VIA register file. The caller owns both 6522s, their interrupts,
the IEC bus, and the CPU-visible memory map. Remanence accepts timestamped
motor, stepper, density, byte-ready-enable, read/write-mode, and port-A-drive
state. It returns the read-channel port-A value, sync, byte-ready, and
write-protect signals at their causal times. The caller connects those
signals to its disk VIA and routes the real byte-ready fan-out to the 6502
`SO` input.

Remanence owns the 1541 read/write electronics below that seam, motor and
head motion, pulse filtering and recovery, sync detection, byte assembly,
the mechanism, and the inserted media. It does not implement one 6522
register or decide how a VIA edge changes an interrupt flag or latch. The
caller advances only to Remanence's next externally visible drive-signal
deadline and may not run past an undelivered transition.

The containing emulator also owns the 1541's 6502, RAM, ROM, IEC peers, and
scheduling. The program running on that CPU owns GCR decoding, sector and
filesystem recognition, DOS, and serial protocol. A P64 adapter may carry
timed flux positions and strengths as its authoritative image layer; the
same state is the P23 active media layer for this physical composition.
The drive hardware resolves it through the read channel so weak pulses
affect the caller's independently emulated VIA behavior without becoming a
P64-shaped public interface.

The same 1541 family may also present a higher **Commodore DOS device
seam** for a caller that does not execute the drive CPU, ROM, or uploaded
machine code. That adapter owns the standard drive firmware behavior and
exposes the byte- and command-level IEC channel service visible to ordinary
programs: device addressing, channel open and close, LOAD and SAVE data,
and the command/status channel. It is a distinct contract, not a shortcut
inside the programmed-hardware adapter. It does not claim IEC electrical
timing, arbitrary bit-banged protocols, fast loaders that replace the
serial protocol, or custom code uploaded to the drive.

A raw sector image naturally supplies the recorded-sector state needed by
that DOS adapter even though it lacks the evidence required by the
programmed-hardware seam. Conversely, P64 can supply the low hardware seam
and may be decoded upward only to the sectors that its evidence supports.
Using a sector image at the low seam requires an explicit P13-governed
P23 generate-flux transition; the generated flux becomes the active media
layer but cannot reproduce protection, weak regions, or other evidence the
sector image never stored. Image capability therefore constrains which
adapters can make a faithful claim, but the image format does not silently
select a seam.

The P19 file-container interface is a separate **file-access
presentation**, not another emulated-drive seam. A filesystem adapter may
interpret the same media state and expose directories and files for tools
that copy files into or out of an image. It neither emulates Commodore DOS
nor turns file operations into drive commands. When drive and file-access
presentations are writable in one composition, they observe one media
state and use the same P2 commit point rather than maintaining divergent
copies.

The Apple II Disk II supplies a second programmed-hardware family and
proves that the concept is broader than an on-drive CPU. Its low seam is
the Apple CPU address bus at one controller card's sixteen slot soft
switches—`$c0e0–$c0ef` for slot 6. Remanence owns the soft switches, data
latch, logic-state sequencer, read channel, mechanism, and media. The
caller owns the Apple CPU, memory, motherboard and slot ROM bytes, floating
bus, and scheduling. Opening this physical composition materializes one
P23 flux-active media state: WOZ 2.1 flux-mapped tracks enter directly and
exact-length bitstream tracks synthesize downward with provenance. That
state remains behind the seam and becomes timed latch behavior rather than
a public WOZ-shaped stream.

The Heath H17 supplies a third shape. Its low seam is an 8080/Z80 I/O-port
window, normally `0x7c–0x7f`, above an AMI S2350 synchronous USRT and the
board's discrete drive-control and read/write electronics. The caller owns
the CPU, memory, firmware, address decoding, and scheduler. Remanence owns
the four ports, USRT, read channel, selected mechanism, hard-sector sensor,
and media. A CPU program receives and transmits bytes through the USRT; it
does not directly sample the recovered serial bitstream. Raw SCP flux and
hard-sector timing form the P23 active media layer behind the seam and
become timed port status, data bytes, and hole-detect state.

The original MITS 88-DCDD Altair disk system supplies an older fourth
shape. Its claimed seam is the 8080's three programmed I/O ports, normally
`0x08–0x0a`, above the two-board controller and Pertec FD-400 drive
electronics. Remanence owns drive selection, controller status, sector
position counting, byte serialization, read/write readiness, head load and
step behavior, up to sixteen media slots, and the selected mechanism. Its
77-track medium has 32 hard sectors of 137 bytes. Sector-true and data-ready
state evolve with rotation and byte timing even though the claimed polled
journey emits no asynchronous CPU event.

### The hardware emulation layer is governed by timed causality

The common floor is not a storage datum. A byte is too high for the Disk
II, a bit is below the H17's programmed seam, a flux transition is absent
from sector-only sources, and an index or hard-sector mark is a separate
signal rather than recorded data. Forcing any one of them into every family
would either discard behavior or expose an implementation-shaped stream.

The hardware emulation layer's defining cross-family characteristic is
**causal interaction on a monotonic timeline**:

1. the adapter has state at one absolute family-typed time;
2. time may advance without a caller interaction;
3. the caller may apply one timestamped stimulus;
4. the adapter may produce one observation of that interaction and zero or
   more timestamped outward effects; and
5. the next externally visible autonomous effect, if any, is a schedulable
   deadline.

Reset, monotonicity, same-time ordering, deterministic stochastic choices,
and side-effect-free stopped inspection complete that contract. Address
widths, registers, soft switches, pins, bytes, partial bus drive, interrupt
lines, connector lines, flux transitions, marker signals, and sector
interpretation are specializations carried by family types. They are not
members of the lowest common vocabulary.

The **hardware emulation layer** realizes that contract. It is an actual
runtime execution layer, not a durable representation and not merely a
resemblance among family interfaces. It contains the electronics below the
selected family seam, read channel, mechanism behavior, and their ephemeral
continuation state, together with common machinery for clocks, advancement,
causal ordering, outward effects, deterministic replay, and composition. A
higher family seam may place a controller here; a lower seam leaves that
controller with the caller. The active media layer is its durable
dependency; typed stimuli, observations, and signals connect the modules.

The hardware layer exposes one semantic interface parameterized by a
family contract. Opening selects the contract and therefore its clock,
stimuli, responses, events, inspections, and configuration; it does not
change the lifecycle or causal rules:

```rust
trait HardwareContract {
    type Tick: Copy;
    type Configuration;
    type MediaSlot;
    type Stimulus;
    type Response;
    type InspectQuery;
    type Inspection;
    type Event;
    type Error;
}

struct Hardware<C: HardwareContract> { /* private */ }

struct MediaAttachment<Slot> {
    slot: Slot,
    source: ArtifactSource,
    access: AccessIntent,
    write_protected: bool,
}

struct HardwareEffects<Response, Tick, Event> {
    at: Tick,
    response: Response,
    events: Vec<Event>,
}

impl<C: HardwareContract> Hardware<C> {
    fn open(
        configuration: C::Configuration,
        media: Vec<MediaAttachment<C::MediaSlot>>,
    ) -> Result<Self, C::Error>;
    fn reset(
        &mut self,
        at: C::Tick,
    ) -> Result<HardwareEffects<(), C::Tick, C::Event>, C::Error>;
    fn now(&self) -> C::Tick;
    fn next_event_tick(&self) -> Option<C::Tick>;
    fn advance_to(
        &mut self,
        at: C::Tick,
    ) -> Result<HardwareEffects<(), C::Tick, C::Event>, C::Error>;
    fn interact(
        &mut self,
        at: C::Tick,
        stimulus: C::Stimulus,
    ) -> Result<
        HardwareEffects<C::Response, C::Tick, C::Event>,
        C::Error,
    >;
    fn inspect(
        &self,
        query: C::InspectQuery,
    ) -> Result<C::Inspection, C::Error>;
}
```

This is both the composition law and the common public hardware interface;
the pseudocode does not pledge literal Rust names or layout.
`HardwareEffects` returns the operation's absolute completion time, its one
response, and causally ordered same-time outward events. `advance_to` and
`reset` use the unit response. A contract with no outward event uses an
uninhabited event type. The C and Python presentations preserve these six
stateful operations and use contract-specific typed records rather than
opaque payload bytes or caller downcasts.

The interface is general in operation and specific in vocabulary. A 1541
contract accepts drive controls and emits drive-side signals; a Disk II
contract accepts slot-bus transactions and returns data-bus drive; an H17
contract accepts port transactions and returns bus drive plus board effects;
an 88-DCDD contract accepts port transactions and returns its polled status,
sector position, and data. No caller constructs a generic pin map or
interprets an image-shaped event. This keeps the module deep while scheduling,
fixed-point time conversion, event ordering, replay, determinism, and test
harnesses are implemented once.

The four journeys instantiate it as follows:

| Contract | `Stimulus` | `Response` | `Event` | `InspectQuery` / `Inspection` |
|---|---|---|---|---|
| 1541 drive hardware (U7) | complete drive-side control change | unit | changed drive-side signal snapshot | current signals |
| Apple Disk II (U10) | complete slot-bus read or write | data-bus drive | none for the claimed journey | controller state or side-effect-free bus drive |
| Heath H17 (U11) | complete I/O-port read or write | data-bus drive | ordered board effects such as boot-RAM write-enable | controller state or side-effect-free bus drive |
| MITS 88-DCDD (U12) | complete I/O-port read or write | data-bus drive | none for the claimed polled journey | controller state or side-effect-free bus drive |

One `interact` call is therefore the smallest causal caller action in every
case. It is not necessarily the smallest media datum: for U7 it changes a
control bundle, while for U10, U11, and U12 it performs a bus transaction.

The same kernel applies recursively below the public seam. A mechanism and
medium advance in time, consume motor/head/write-control changes, and emit
read-channel and marker-signal transitions. A controller consumes those
signals and emits bus-visible state. A public family adapter composes that
private signal graph and exposes only the interaction at its claimed seam.
Flux is therefore a valid lowest **modeled media phenomenon**, while timed
causality is the defining **cross-family characteristic of hardware
emulation**. They answer
different questions and neither replaces the other.

#### Causality is generated, not materialized

The hardware emulation layer does not expand track-relative flux positions
into a durable timeline of absolute-time pulse occurrences, recovered bits,
bytes, bus transactions, or future effects. When a caller advances time,
the timing mechanism combines the durable media state with the
current mechanism and controller continuation state and generates only the
transitions needed to reach the requested time or next externally visible
deadline. Past internal transitions may be discarded once their
consequences have been incorporated into current state.

Some ephemeral continuation state necessarily survives between calls:
current time and fractional phase, registers and latches, motor and head
state, read-channel/PLL history, deterministic noise-generator state, and
any outward effect already caused but not yet delivered. This state makes
the simulation continuous; it does not make timed causality a durable data
representation. It is initialized by reset and machine configuration, not
read from the disk image, and disappears with the open composition unless
a separate future snapshot presentation explicitly preserves it.

Head position is the canonical mechanism example. Hardware emulation must
model the selected mechanism's radial position, including fractional-track
positions where its stepper and family permit them, together with motion,
settling, selected side, motor speed, and rotational phase. Those values
determine which durable flux is observed and where a modeled write changes
the medium, but they are properties of the drive mechanism, not of the
disk. Ejecting or reopening an image never carries them with the medium.
Open and reset use the integration contract and physical profile's declared
initial state; they never infer head position or rotational phase from image
bytes.

The durable mutable state of the inserted disk belongs to the media model.
For a composition which claims a physical recording path, that session
state descends to the P22 flux floor: circular track-relative transitions
and strength semantics, with marker channels and provenance alongside
them. A sector or encoded-track source is synthesized downward under P13
when the family and mastering rules permit it; the resulting flux is the
durable mutable media state for that composition, while provenance retains
that it was fabricated rather than captured. Controller writes alter this
media state. Timed causality merely projects it through rotation and the
read channel and does not make synthetic flux captured evidence.

#### Hardware emulation is not an image layer

Disk-image formats do not serialize or hydrate the hardware emulation
layer. An image adapter reads an artifact into an evidence-bearing media
state: sectors, encoded bits, timed pulses, flux intervals, marker evidence,
or some explicitly fabricated combination according to that format's
information floor. Hardware emulation receives the resulting active media
state as its durable dependency when a family composition is opened.

At runtime, the hardware emulation layer retains its ephemeral family state
and generates and orders the modules' interactions. None of that
continuation state or generated traffic becomes part of a disk image merely
because it affects a read. Conversely, an image parser does not replay bus
transactions or initialize a controller by manufacturing a history of
stimuli.

For a writable composition, controller activity may change the shared
media state through the modeled write channel. P2 commit then asks a
compatible image adapter to persist the resulting **media change**. It does
not persist the controller, mechanism, scheduler, pending effects, or
generated hardware-emulation timeline. A format which cannot represent that
media change is refused or requires an explicit conversion policy; it is
never extended implicitly with runtime state.

The separation is therefore:

| Layer | Owns | Exchanges with persistence |
|---|---|---|
| family presentation | the controls, signals, or programmed interface at the selected real hardware cut | nothing directly |
| hardware emulation | timed-causal execution, below-seam electronics, read-channel/mechanism behavior, and ephemeral continuation state | nothing |
| media | durable disk state plus evidence/provenance | media state and representable changes |
| file container | durable named entries, byte streams, and container metadata | container state and representable changes |
| image adapter | parsing and encoding one named artifact format | bytes of that format |

#### The natural integration seams are file containers, durable media, and hardware emulation

Remanence has three foundational kinds of external integration seam:

1. A **file-container seam** joins archive and package adapters to durable
   collections of named entries, byte streams, and container metadata. An
   entry may in turn be opened as a separate disk instance, but the
   container is not disk media.
2. A **durable-media seam** joins a disk-image adapter, transformation,
   analysis, or higher semantic presentation to the active flux, CHS, or
   block representation. Its interface uses the native vocabulary and
   evidence limits of that durable media layer.
3. A **hardware-emulation seam** joins a machine emulator, controller or I/O
   chip implementation, or hardware test harness to a family presentation.
   Its interface exposes the controls and signals at the selected real
   hardware cut under the timed-causality contract.

The architectural join between durable media and hardware emulation is
direct: one hardware-emulation composition consumes zero or more typed
media slots and, when writable, mutates the active durable-media instance
in the selected occupied slot. Every inserted disk remains an independently
mutable P23 instance with exactly one active layer. The join remains an
internal deep seam; it does not require a universal public byte, bit, pulse,
or generic-event stream. A file-container entry can supply an attachment,
while the container and every disk retain separate active states under P23.

Filesystem and DOS/IEC conveniences do not create a fourth foundational
kind of integration seam; they are semantic presentations over durable
media. Archive-entry access is a presentation over a file container.
Likewise, the family presentation is the public face of hardware emulation,
not another layer between the caller and it. This classification says where
integrations attach; it does not require every container, durable media
layer, or family to expose the same interface.

An emulator save state, deterministic interaction trace, or hardware logic
analyzer capture could legitimately preserve hardware-emulation state or
traffic, but each would be a different authored format and application
surface with its own use case and norm. It is not a disk image and does not
alter the disk-image interface.

### Some hardware seams share a programmed-I/O specialization

U10, U11, and U12 establish a real specialization because three
substantially different family adapters occupy it. Their shared interface is
**timed programmed I/O**, not a byte, bit, nibble, sector, or flux stream. A
caller performs complete decoded CPU-bus transactions against a stateful
adapter and observes only what that programmed hardware places back on the
CPU bus or on explicitly claimed external lines. U7 instead uses the more
general timed-causality contract directly at a drive-side signal seam; it
proves that programmed I/O is one useful specialization, not the hardware
layer's universal interface.

The specialization is a shared family-message vocabulary carried through
the one `Hardware` interface, not a second stateful interface:

```rust
enum ProgrammedIoStimulus<Address> {
    Read { address: Address },
    Write { address: Address, value: u8 },
}

enum ProgrammedIoInspect<Address> {
    Io { address: Address },
}
```

Disk II binds `Address` to `u16`; H17 and 88-DCDD bind it to `u8`. In all
three contracts,
`interact` receives the complete decoded address and returns
`DataBusDrive`. On a write, a zero driven mask reports that the peripheral
does not drive the CPU bus; the response is normally ignored but remains an
electrical statement rather than an invented unit result. `inspect` accepts
the matching inspection query and performs no bus-cycle side effects.

`DataBusDrive` carries an eight-bit value and an eight-bit driven mask, so
the caller resolves undriven bits from its own bus model. A family whose
mapped hardware always drives the whole byte uses `0xff`; no-drive and
partial-drive behavior require no sentinel or invented floating value.

The programmed-I/O adapters deliberately retain different associated types
and effects; the lower 1541 adapter is shown to make the distinction clear:

| Concern | 1541 drive hardware (U7) | Disk II programmed hardware (U10) | H17 programmed hardware (U11) | 88-DCDD programmed hardware (U12) |
|---|---|---|---|---|
| CPU owner | caller's drive 6502 | caller's Apple 6502 | caller's 8080/Z80 | caller's 8080 |
| Caller-owned interface hardware | both 6522 VIAs | none below the CPU seam | none below the CPU seam | none below the CPU seam |
| Interaction | timed drive-control snapshot and signal changes | sixteen slot soft switches | four decoded I/O ports | three decoded I/O ports |
| Read result | GCR port value plus sync, byte-ready, and write-protect signals | full latch byte or floating bus | USRT data/status or controller status | drive status, sector position, or data byte |
| External effects | drive-signal transitions | none in the claimed journey | same-transaction boot-RAM write-enable; no autonomous event | none in the claimed polled journey |
| Public time | 1541 reference-clock ticks | Apple CPU cycles under a timing profile | host CPU T-states under a timing profile | Altair 8080 T-states under a timing profile |
| Preserved media test | P64 timed pulses and strength | WOZ bit timing and flux tracks | raw SCP flux plus hard-sector mark timings | raw SCP flux plus 32-sector marker timing |

There is no universal tick frequency: each family declares a typed clock
and maps its internal media timing without per-event rounding. There is no
universal event enumeration: an uninhabited event type is correct for a
card with no autonomous CPU-visible output. There is no universal register
operation enum: full addresses and read-versus-write are part of the
hardware behavior, including mirrors, parity, and undocumented side
effects.

The public hardware handle is common, but it is never untyped. Rust carries
the integration contract as `Hardware<C>`; C carries a contract tag with typed
stimulus, response, event, and inspection records; Python carries the same
contract identity on its `Hardware` object. A stimulus from one contract is
refused by an instance of another. The depth lies in one causal interface
and timing engine; locality lies in typed presentations which compose 1541
drive-side circuitry, Disk II sequencing, H17 USRT and hard-sector behavior,
bus-drive rules, and their events from that common layer.

Older storage contexts may also have a CHS interface that presents their
recorded cylinder, head, and sector layout without emulating a particular
controller. CHS and programmed-hardware presentations may operate over the
same media instance. CHS never exposes or invents geometry beneath an LBA
drive. The selected integration contract declares the available hardware
seam; the composition chooses it explicitly, and neither an image format nor
a media profile chooses one implicitly.

### Knock-on requirements

The P15 section of the pledged F19 design places the 1541 public cut above
the drive electronics but below the disk VIA, matching U7. It specifies the
common hardware layer without introducing a drive catalog. F19 itself still
adds no hardware implementation or emulator presentation, and its delivery
cut does not otherwise change.

## P16 — Partition layouts are an independent seam

A partition-layout adapter consumes one addressed device or region and
exposes its child regions with the layout metadata and identities it
claims. Layouts may nest, so a child region can be offered to the same seam
again. MBR, GPT, and BSD disklabels or slices illustrate variation at this
seam; naming them does not promise their implementation.

The adapter owns recognition, evidence, validation, refusals, and the
meaning of its regions. It does not open filesystems or decide whether a
region is a volume. An image container such as VHDX or qcow2 ends at its
addressed virtual device and does not absorb partition-layout semantics.

The partition-schema catalog enumerates layout adapters such as MBR, GPT,
and BSD disklabel. It does not enumerate individual MBR partition-type
bytes, GPT partition-type GUIDs, or comparable entry classifications. Those
values belong to, and are interpreted by, the adapter for their containing
schema.

## P17 — Volume composition is an independent seam

A volume-composition adapter consumes addressed storage regions and exposes
logical volumes through one volume interface. A whole unpartitioned medium,
a direct one-partition volume, and a volume assembled across regions
illustrate different compositions.
A partition and a volume may overlap one-to-one, but they are never
synonyms and neither implies the other.

The adapter owns membership, mapping, identity, validation, and refusals.
A filesystem receives a volume and does not know whether it came from one
region or nested layouts. A raw volume is not itself a file container;
after P18 gives it filesystem semantics, that view may be a whole P19 file
container or one mounted part of a larger one. A future composition needing
several devices must argue and design that capability when it is proposed;
this seam neither forbids nor prepares for it.

An addressed medium with no partition layout can form one direct volume.
This is the ordinary legacy-floppy case, not a missing partition scheme or
a special public path. The direct composition preserves the separate P17
interface while requiring no partition choice from the caller.

## P18 — Filesystems are an independent seam

A filesystem adapter consumes one volume and exposes a P19 file-container
view of the namespace, metadata, and data operations it claims. FAT, HDOS,
CP/M, NTFS, ext, and other filesystems illustrate variation at this seam;
their mention is not a support claim. The adapter owns recognition and
evidence, structural validation, version and feature ceilings, refusals,
and all filesystem semantics it implements.

A filesystem does not parse an image container, discover the partition
layout around its volume, or know how that volume was composed. The
filesystem catalog and interface remain independent of those adjacent seams.

## P19 — File containers are the common file-access seam

A file container exposes a rooted namespace of named files and containers,
with their metadata and data operations, independently of what backs that
view. A serialized container such as ZIP, tar, or 7z; one filesystem on one
volume; and a Windows or Unix namespace composed from many mounted
filesystems illustrate different providers at the same seam. The examples
are not support promises.

This is the high-level convergence point for file access. A caller opens an
artifact to reach the files it contains; supported composition may pass
through serialized containers, image formats, partition layouts, volume
composition, filesystems, or namespace mappings, but every file-bearing
result presents the P19 interface. The result retains the layers,
identities, and evidence that produced it. Multiple roots or ambiguous
paths are exposed or refused explicitly rather than flattened or guessed.

When every applicable seam has one supported result, composition is
transparent. A simple legacy floppy image with one direct volume and one
recognized filesystem opens as that filesystem's file container without
asking the caller to select or configure the intervening layers. Drive and
mechanism emulation are not constructed merely to reach files; P14 and P15
enter only when the requested operation needs them.

P19 is the usual high-level destination, not a universal content model. A
partition or volume may validly contain boot data, swap, database or object
storage, volume-manager metadata, or another claimed structure with no file
namespace. That result remains visible at its applicable seam and may gain
a separate adapter when its semantics warrant one. Remanence neither calls
valid non-file data empty nor manufactures pseudo-files to force it through
P19. Opening specifically for file access returns a named absence or
refusal when no file-bearing interpretation is claimed.

Serialized-file-container adapters consume byte streams. P18 filesystem
adapters consume volumes. Namespace-composition adapters consume file
containers plus explicit drive, mount, folder, or volume mappings and
expose another file container. Composition preserves the identity and
provenance of its sources rather than flattening or copying them. A file
container may therefore be backed by a whole volume, by part of a storage
graph, by no volume at all, or by several mounted filesystem containers.

The common file-container view is not a disk representation and declares
no image layer, media, geometry, partition layout, or volume semantics.
Raw partitions and volumes do not satisfy it. Selecting a file yields a
byte stream; only independent P12 recognition can make that file an image
and declare its authoritative layer under P13.

## P21 — Device identity is assigned, scoped, and unobtrusive

Every addressed virtual device receives an opaque identity when Remanence
composes it. The library assigns that identity; an ordinary single-image
open never asks the caller to provide or choose one. The identity is unique
within its containing open or composition. It implies no globally stable or
user-meaningful identity unless a later interface explicitly claims one.

Device identity qualifies provenance and otherwise-local identifiers only
where more than one device makes that distinction necessary. An interface
already scoped to one disk may continue to accept `partition:1` or another
disk-local identifier without exposing the device identity. A presentation
may report the assigned identity, but never makes callers echo a value that
does not affect the requested operation.

An attachment identity such as `hdd0` is distinct from device identity. A
caller supplies placement only when placement changes semantics and cannot
be inferred. This principle adds neither multi-device opening nor
multi-device volume composition; those capabilities require their own
proposal.

## P22 — Magnetic recording can descend to timed flux transitions

For a magnetic-media family that claims a physical recording path,
Remanence can represent recorded state at the granularity of individual
timed flux-transition events. Each event carries its timing and any
detection-strength semantics claimed by the authoritative image or derived
model. This is the lowest modeled magnetic-data layer: analog head voltages,
amplifier waveforms, magnetic field shapes, and transistor-level behavior
remain outside the model.

When a low-level composition claims that physical recording path, the flux
layer is its durable mutable media state for the session, not a
transient stream generated separately for every read. Track-relative flux
and marker state survives controller interactions and receives modeled
writes. The timing mechanism projects that circular state into ephemeral
absolute-time read-channel events as rotation advances; it does not store a
second event history. A higher-level image synthesized down to this floor
retains synthetic provenance, and commit back to that source is possible
only when its format can represent the resulting media change.

P64 is the concrete lower-bound test for this capability. A P64 path
preserves stored pulse position and strength into the flux layer, and a
read-channel simulation consumes that state with its weak-event semantics
intact. Flattening the image to one deterministic bitcell or byte stream
does not provide P64 fidelity and does not satisfy this principle for that
format.

Flux is one channel, not the whole medium. Index and hard-sector holes and
other mechanical or sensor observations are separate timed state or event
channels; they are not folded into the flux-transition stream. A capture
adapter may preserve several revolutions and their marker timing, while a
normalized media model may define one circular revolution. Every adapter
states which timing, markers, revolutions, and weak-event semantics it
preserves, normalizes, synthesizes, or cannot represent.

P13 governs movement away from the authoritative layer. Captured flux may
be decoded into derived sector, filesystem, and file presentations while
retaining ambiguity and evidence; that interpretation never creates a
block-active device. CHS sectors, encoded tracks, or another representation
inside a declared legacy magnetic family may be synthesized downward to
flux only when the image format, media profile, hardware profile, and
mastering rules claim a deterministic mapping; the result remains synthetic
rather than evidence of an original recording. Logical blocks never enter
that path, and flux never converts into logical blocks.

The flux floor is an internal modeling capability, not a universal public
interface. P15 still determines the programmed seam visible to a drive
emulator, and P3 and P12 still require each named image format to enter and
leave through the representation seam it actually supports.

### Knock-on requirements

The pledged F19 image-format design already recognizes timed flux as an
authoritative layer and physical media profiles as the owners of index and
sector-hole topology. It makes the separate flux-data and marker/sensor
channels explicit. U7 supplies the concrete P64 journey; P22 does not by
itself pledge a standalone P64 image adapter or a public flux interface.

## P23 — One durable layer is active

Every independently mutable open state instance has exactly one **active
layer**: the durable representation against which all of its
current presentations read and, when permitted, write. The active layer is
runtime artifact or media state, not an image-format choice, not a derived
cache, and not the hardware emulation layer. Several presentations
over one instance share it; they never maintain independently mutable file,
sector, track, and flux copies of the same state.

The durable active-layer vocabulary is exactly:

| Active layer | Durable session state | Claim |
|---|---|---|
| **file container** | a rooted namespace of named entries and nested containers, entry bytes, and claimed metadata | container structure, not disk allocation or recording |
| **flux** | circular track-relative flux transitions and strength semantics, with marker/sensor channels and provenance | a modeled magnetic recording surface |
| **CHS** | records addressed by cylinder, head, and sector under a declared geometry | geometry and records, but not their physical encoding |
| **block** | geometry-opaque logical blocks addressed by number | no cylinder, head, track, recording, or mechanism claim |

These are four family-owned representations, not variants of one universal
schema. Flux includes its parallel marker channels; they are not another
active layer. CHS and block both carry record bytes, but CHS's declared
geometry is observable and load-bearing while block deliberately hides it.
File container is semantic named-entry state and makes no disk claim.

Here **durable** means that the representation survives runtime
interactions as the state instance's continuing mutable truth and is the
source offered to P2 commit. It does not mean that the representation is
already serialized, crash-durable, or necessarily encodable by the source
image format. Persistence is a later capability check against P13 and the
chosen image adapter. Nor does durable fix residence: under P27 the state
may be resident in memory or spilled to private session storage, a resource
policy that never changes what the layer means.

Block and flux are mutually non-convertible active-layer families. No active
layer transition crosses between them in either direction. Derived
filesystem or file access over either family is a presentation over the
existing active state, not an intermediate block-or-flux conversion.

Encoded tracks, bitcells, nibbles, and filesystem structures can be
authoritative image layers or derived representations, but they are not
additional durable active layers. A disk composition materializes them
into the applicable flux, CHS, or block state before service begins. A
serialized archive or filesystem-level artifact may instead materialize a
file-container active layer without creating disk media at all.

P19's file-container interface does not by itself make file container the
active layer. Over a filesystem on flux, CHS, or block media it is a derived
presentation whose mutations project into that media's active state. Over
a serialized container such as ZIP, the named-entry state itself is active.

Nested artifacts have one active layer per independently mutable instance,
not one layer for the whole object graph. Opening `archive.zip/disk.d64`
can leave the outer ZIP active as a file container while the selected entry
is recognized as a child disk image with its own CHS-active media instance.
If that child later becomes flux-active, commit first encodes the child's
representable result into its entry bytes and then commits the outer
container. Neither instance acquires two active layers.

P13's authoritative image layer and the active layer answer different
questions. The authoritative layer states what the loaded artifact actually
records and what its original format can persist. The active layer states
which representation currently carries the session's mutable truth. They may
coincide—a P64 physical-drive composition can be authoritative and active
at flux—or differ—a raw sector image can remain authoritative at sectors
while a synthesized flux layer becomes active for low-level drive service.
Changing the active layer does not promote synthetic state into recovered
evidence and does not change the authoritative image layer.

For a disk, the initial active layer is the least physically expressive
durable media layer which faithfully serves every presentation requested
when the composition is formed. A Commodore DOS/IEC device over a standard
sector image can use addressed CHS sectors as its active layer; no track,
flux, head, or rotation state is generated. File and sector views above it
derive from and mutate that one state according to their own seams. An LBA
device uses block and cannot be lowered merely because another family knows
CHS or flux.

If a caller later requests a service below the active layer, Remanence must
materialize a new active layer before offering that service. For a
programmed-hardware floppy seam below CHS, this is an explicit
**generate-flux** transition. The applicable image metadata, authoritative
state, media profile, hardware profile, encoding and mastering rules are used
to produce the most honest flux and marker state the evidence permits:

- every known timing, ordering, defect, weak-event semantic, and marker is
  preserved at its known fidelity;
- only detail required by the lower model and absent from the source is
  synthesized, with its provenance retained;
- ambiguity remains ambiguity unless an explicit deterministic policy is
  part of the composition; and
- a missing or contradictory rule refuses the lower service rather than
  manufacturing unjustified precision.

There is no universal linear ladder across all four layers. A declared
legacy floppy family may lower CHS to flux. Block is terminal and never
lowers to flux; flux never rises into block. File container participates
only through a declared container-to-child or filesystem-materialization
path. Encoded-track and bitstream image representations enter flux through
their family derivations rather than becoming extra rungs.

The transition is atomic for the media instance. Once the lower state is
validated, it replaces the old active layer as the single durable mutable
session state. Existing higher presentations are rebound as derived views
of it and their caches are invalidated. They may decode sectors and files
upward, but they cannot continue mutating the former CHS copy. The active
layer does not rise again during that open media lifetime merely because a
lower presentation closes; doing so could discard state the higher layer
cannot express. Returning to a higher active representation requires
closing the composition or a family-permitted explicit conversion which
names the loss. No such conversion exists between block and flux.

Generate-flux materializes circular, track-relative media state; it does
not materialize runtime pulse occurrences. P15's hardware emulation layer
combines that active state with ephemeral mechanism state—head position,
motor speed, rotational phase, settling and read-channel history—to
generate causal observations as time advances. Mechanism state never
becomes part of the active media layer.

Writes always land in the active layer. Commit remains governed by P2 and
P13: the original image may be updated only when every change can project
back to its authoritative layer and encoding without unclaimed loss. A
sector-authoritative image whose active flux acquired an unrepresentable
low-level change is not silently flattened; that writable composition is
refused in advance, or the user explicitly converts to a new image whose
format can make the lower state authoritative.

### Knock-on requirements

The pledged F19 design must distinguish authoritative image layer, active
layer, and derived views in its composition and capability rules.
U9 supplies the CHS-active DOS/IEC journey. U7, U10, U11, and U12 supply
hardware-emulation journeys whose physical read paths require flux-active
media. U8 requires higher file edits over flux-active mixed media to mutate
only representable regions through derived sector/filesystem views while
preserving all unrelated lower-layer state.

## P27 — Sessions stream; memory holds a bounded working set

Remanence is sized by the operation, never by the artifact. A source may be a
floppy image of a few hundred kilobytes or a VHDX, Aaru optical capture, or RF
recording of a hundred-plus gigabytes; the same open, inspect, read, and write
journeys serve both, so no layer — encoding, authoritative, active, derived
view, or uncommitted overlay — is ever loaded whole as a design assumption. An
operation may visit bytes in proportion to its task; it may hold only a
bounded working set. An implementation may hold a whole layer only when its
format bounds that layer's size beneath the working set; every other path
streams. There is no in-memory escape hatch for a format that resists
streaming — its route is materialization to private session storage, below.

Every independently mutable state instance has one backing for its active
layer, decided by what P23 makes active:

- The active layer is **source-backed** when its image adapter serves bounded
  random access directly from the source encoding: a raw image by identity,
  qcow2 or VHDX through their allocation structures, an indexed optical or
  flux capture through its own tables, a per-block-compressed encoding whose
  decode cost per access is bounded. Reads stream from the source on demand
  through the session cache; nothing else is materialized.
- The active layer is **session-backed** when it cannot: state the source does
  not encode at bounded random-access cost — a materialized generate-flux or
  generate-optical layer, downward synthesis, a decoded representation whose
  encoding permits only sequential access, such as a DEFLATE-compressed entry
  the operation must address randomly. Session-backed state is produced by a
  streamed transform into private session storage and is then served exactly
  as source-backed state is: on demand, through the same cache.

Nesting composes backings. A child artifact's source reads are range requests
against its parent's presentation, streaming under the same discipline at
every level of the graph; only an encoding that defeats bounded random access
forces a child to become session-backed, and recursion never materializes a
copy per level.

Above the backing, caching is per modeled layer. Every durable layer a state
instance currently models — its active layer, and any derived layer a
requested presentation has materialized above it — has its own cache,
streaming under the same rules, and a session's declared bound is one budget
its layer caches share. One layer is one cache: presentations at that layer
share it and never maintain independently mutable copies (P23).

The **active layer's cache** carries the session's mutable truth, and its
policy follows from two residency classes:

- **Clean state is always evictable.** A clean extent can be dropped and
  re-read from its backing at will: from the source while the active layer is
  source-backed — P7 makes that sound, since the claim guarantees the source
  cannot change beneath the session — and from session storage when it is
  session-backed. On a small source the cache simply fills until the whole
  image is resident and disk I/O stops; on a huge one it converges on the
  operation's locality and full residency never happens. Both are one policy
  observed at different sizes, not two designs.
- **Dirty state is never dropped.** The session tracks alteration at extent
  granularity, because eviction is only lawful when the two classes are
  distinguishable. Uncommitted changes live in the P2 overlay: held in memory
  within the session's bound, spilled to private session storage beyond it,
  and either way nothing reaches the source before commit. Eviction moves
  dirty state; only rollback discards it; commit projects it.

A **derived layer's cache** is an accelerator, never a truth. Its extents are
generated on demand by the family's derivation over the layer below — CHS
sectors decoded from active flux when sector access arrives — and it holds
only clean state: a derived write completes downward first, after which
regeneration reproduces it, so a derived cache evicts freely and never
spills. The layer caches are tied through the derivation mapping, in both
directions:

- **A derived write projects down in the same act.** A write at a derived
  layer alters its cache only together with its projection through the
  derivation into the layer below, reaching the active layer's cache — P23's
  rule that writes land in the active layer, kept under caching. A write
  whose projection is refused alters nothing at any layer.
- **A lower write invalidates upward.** A write landing in a lower layer —
  projected from above, or made directly through a lower presentation —
  invalidates the overlapping extents of every derived cache above it, so a
  stale decode is never served; the next read regenerates from the changed
  state.

A C64 floppy carries the shape. A P64 source pins the active layer at flux:
flux streams from the image and altered flux spills to session storage.
Sector-level access derives a CHS cache above it; a sector write alters that
cache and its flux projection together, and a hardware-level flux write
invalidates the sectors it underlies. A sector-format source is instead
CHS-active with one cache — until a hardware request performs P23's
generate-flux transition: the materialized flux becomes the active layer,
session-backed, and the CHS cache is rebound as derived above it, both
layers caching from then on.

Layer caches may work ahead of demand, concurrently. The library may use
threads to predict, prefetch, and offload: reading source extents ahead of a
detected access pattern, deriving upper-layer extents before they are asked
for, running a materializing conversion's pipeline in parallel, and moving
the cache's own extents out ahead of memory pressure — spilling altered
ones, dropping clean ones — with the standard library's threads alone, since
the core takes no runtime dependency. Four rules keep the concurrency
observationally invisible. Prefetch produces only clean state: cache extents
identical to what the caller's own miss would have loaded or derived, never
a mutation — dirty state, downward projection, commit, and every refusal
remain synchronous acts of the caller's operation. Offload never gaps the
truth: an altered extent leaves memory only once its spill write has
completed, and every act that consumes the altered set — write-through,
commit, rollback — first joins the offloads in flight. The work spends the
declared budget: predictive depth is part of the declared read-ahead, its
extents compete inside the session's one bound, and a demand miss always
outranks a prediction. And speculation is silent: a failed speculative read
caches nothing and reports nothing — the caller's own access, if it comes,
re-attempts and owns the diagnostic (P6) — so results, evidence, and
refusals are identical with any number of threads, including none. The P7
claim is what makes concurrent source reads sound: nothing can change the
file beneath the readers.

The bound and its read-ahead are declared session configuration with a stated
default, not discovered behavior.

Commit and materialization stream like everything else. Encoding a result
through an image adapter never assembles the whole output in memory, and a
generate transition never expands the new active layer wholesale; each is a
bounded pipeline from backing to destination. Identification keeps the same
discipline: a probe reads the bounded evidence its claim names, and a claim
that must visit every byte streams the visit. This is the durable-state
sibling of P15's rule that causality is generated, not materialized: neither
runtime timelines nor resident copies are manufactured to make access
convenient.

Private session storage takes the shape P9 gave the journal: no user-owned
file, no cleanup verb, no contract about its location or form, exclusively
held for the session's life. Unlike the journal it is never load-bearing after
interruption — spilled overlay and materialized state are exactly what a
rollback discards, so P9 reconciliation continues to depend on the journal
alone and an interrupted session's spill is simply discarded.

The public presentations carry the same bound. An operation whose result is
proportional to source content offers a bounded or streamed form in Rust, C,
and Python alike (P5), and a whole-value convenience — a file read returning
owned bytes — is a wrapper over that form, never the only route.

This principle constrains resources, not semantics. It adds no entry to P23's
active-layer vocabulary and moves no seam: adapters, presentations, evidence,
and refusals behave identically at every source size, and peak memory bounded
independently of source size is the testable claim that arms it.

### Knock-on requirements

In-force P2 already carries the residency-neutral wording — altered data
stays in the session's cache, in memory or spilled to private session
storage — so arming adds no further P2 amendment. D2's overlay ruling is
untouched — the commit point remains an overlay, never internal snapshots,
and nothing touches the host file before commit; this principle generalizes
only the overlay's residence.

The pledged F19 design's adapter interfaces and shared mechanisms are
stream-shaped from the first implementation — retrofitting streaming beneath
delivered adapters would reopen every seam this principle exists to
protect — and its design document carries the requirement.

Everything still proposed is drafted and judged under this principle from day
one. A whole-value read in proposed pseudocode is a convenience over the
streamed form, and a proposal whose service cannot be rendered within a
bounded working set is not ready to pledge.
