<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# ARCHITECTURE (pledged)

> **Status:** pledged at the owner's direction. P14, P15, P25, P31, the P23
> and P10 amendments, and both P19 amendments remain
> owed by the project and are armed only when they reach root
> [ARCHITECTURE.md](../../ARCHITECTURE.md), where a divergence becomes
> a bug. Numbers come from the one global P-sequence and are never
> reused.
>
> P22, P29 and P30 have left this file for that one, the flux family
> they govern being delivered. What still cites them here cites an
> in-force principle.
>
> A principle that establishes a seam guarantees that the architecture
> can host implementations at that seam without redesigning adjacent
> layers. Examples test the required generality; they do not claim or
> pledge support for any named variation. Actual support remains a named,
> enumerated claim under P3.

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

**The bitstream a drive works on is materialized when it is used, not
stored.** The floor is whatever the composition materialized — timed flux
for a P64 or a raw capture, hardware bitstream where the source is
bitstream-authoritative and nothing deeper is needed, as G64 is. Where flux
is the floor, the read channel projects it into clocked bit state as
rotation advances and that state is ephemeral: hardware emulation holds no
durable layer of its own, and no second history of recovered bits is kept
beside the flux that produced them. The drive consumes a bitstream either
way and does not care which floor supplied it, which is what lets one
hardware model serve both a flux capture and a bitstream image.

This is which of the P23 amendment's two permitted paths the project takes,
not a new claim: that amendment lets a hardware profile materialize a
hardware-bitstream active layer, and the ordinary drive composition
declines to, leaving the floor where the image put it. A profile that does
perform that ascending transition is doing something explicit and separate,
governed there.

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

The P15 section of the in-force image-format architecture places the 1541 public cut above
the drive electronics but below the disk VIA, matching U7. It specifies the
common hardware layer without introducing a drive catalog. The delivered adapter layer still
adds no hardware implementation or emulator presentation, and its delivery
cut does not otherwise change.

## P23 amendment — the durable magnetic layers, named from flux medium up

P23's durable active-layer vocabulary is corrected at its bottom and
extended above it.

**The correction at the bottom is in force.** Renaming the `flux` row to
**flux medium**, the clause that gives flux capture no row, capture becoming
medium by mastering rather than by lowering, and generate-flux being
generate-medium are all claims the code already honors, and they moved to
root [ARCHITECTURE.md](../../ARCHITECTURE.md) as the delivered half of this
amendment. They were armed on their own because they depend on no unbuilt
work — the delivered flux stack is where they were true — and because in-force
P22 named two flux models while in-force P23's vocabulary still named one,
which is a divergence between two armed principles rather than a gap awaiting
a feature.

What remains below is the extension above the medium, which stays pledged
until the layers it names exist.

### Hardware bitstream and encoded bytestream sit above it

The vocabulary gains **hardware bitstream** and
**encoded bytestream** between flux medium and CHS. Hardware bitstream is circular,
track-relative, clocked bit state, including the timing and provenance
required by its declared drive family. G64 illustrates an image whose
authoritative and initial active layer are hardware bitstream.

Encoded bytestream is the circular, track-relative byte sequence a declared
family codec materializes from hardware bitstream. For a 1541 it is GCR-decoded
bytes, before the library identifies synchronization, headers, data fields,
sectors, or files. No source format is presumed to begin at this layer.

The magnetic-disk path above encoded bytestream is CHS, then filesystem.
A family-owned synchronization and sector interpretation materializes CHS
only where its claimed rules support it; a byte sequence is not assumed to
contain sectors. P18 then recognizes and presents a filesystem above CHS.
CHS is durable active media state; filesystem remains the higher derived
seam, not a peer mutable media copy.

The magnetic ladder therefore reads: flux capture → flux medium → hardware
bitstream → encoded bytestream → CHS → filesystem. Block stays terminal and
disjoint from all of it, and P23's prohibition on crossing between the block
and flux families is untouched in both directions.

Flux medium, hardware bitstream, and encoded bytestream are distinct durable
layers, not caches and not mutable peer copies. A source whose authoritative
layer is a flux medium begins medium-active; a hardware profile may
explicitly materialize a hardware-bitstream active layer from it; a declared
codec may then materialize an encoded-bytestream active layer. Each
transition is atomic, preserves source state and codec/profile as provenance,
and makes the destination the sole mutable session truth. Descending or
returning to a lower layer is a separate explicit mastering transition. In
either direction, P13 governs write availability: any unrepresentable
projection is refused or requires explicit conversion.

The existing P23 clauses for active-layer replacement, cache invalidation,
bounded backing, and the ban on independently mutable peer copies apply to
these layers. P22 continues to govern both flux models and the medium's
marker channels.

## P25 — Artifact mappings make nesting recursive

Any recognized structure may expose an evidence-bearing **artifact mapping**
from part of its state to a possible child artifact. The mapping is the one
general recursion mechanism whether the child bytes come from a P19
file-container entry, a filesystem file, an optical boot-catalog extent, a
partition or volume region whose format defines an embedded image, or another
typed range declared by a recognized standard. Nesting is not a special
property of ZIP, ISO, partitions, or any one format family.

An artifact mapping is an edge in the inspection and composition graph, not a
durable layer, partition, volume, filesystem, file container, or claim that
the child has been recognized. It names the parent identity, source extent or
byte projection, applicable standard semantics, evidence, access limits, and
the path by which a representable child change would return to the parent.
Opening that source invokes P12 image adapters normally. Successful
recognition can materialize a child; opening it as an independent state
instance gives it its own P13 authoritative layer and P23 active layer.

Thus a ZIP may remain file-container-active while its ISO entry is an
optical-active child; an El Torito boot entry in that disc may open a
CHS- or block-active boot-disk child; and a P64 stored as a file in the ISO
filesystem may instead open a flux-active child. These layers coexist because
they belong to different state instances. They are never multiple active
copies of one instance, and no parent-to-child mapping converts block into
flux or optical into block.

Discovery is explicit and lazy. Inspection reports mappings and their
relationships without recursively opening every candidate, selecting a
preferred boot entry, or guessing which embedded image the caller wants.
The caller selects a reported mapping and requests recognition of its child.
Unsupported, ambiguous, cyclic, excessively deep, or resource-hostile paths
are bounded and refused with their evidence preserved rather than silently
skipped or flattened.

Mappings may alias or overlap: an El Torito image can also be a named ISO file,
and hybrid structures can assign several meanings to the same bytes. Reports
preserve that identity and overlap. Two paths to the same mutable child must
share one child state, or conflicting writable composition is refused before
mutation; independent mutable copies over aliased bytes are forbidden.

Nested commit proceeds from child to parent. Every child result is first
validated against its image adapter and mapping, then encoded into the parent
state, continuing outward until the root source is representable. P2 commits
the validated composition atomically and P7 holds the necessary claims for
the whole graph. Failure at any seam writes nothing and names the exact child,
mapping, and representation which could not be encoded.

### What arming it will require

The delivered library does not honor this principle, so it stays here until
it does. Today nesting is special to ZIP and 7z and is decided by file
extension; entry resolution is single-level, so an artifact inside an
artifact inside an artifact cannot be reached at all; nothing reports
mappings, discovery is neither explicit nor lazy because there is nothing to
discover; alias and overlap have no representation; and there is no
child-to-parent nested commit. Those are the delivery gaps, not a list of
defects — a principle below the root list is unbuilt work, and it becomes a
bug only when the project asserts the code complies.

## P19 amendment — a file-bearing interpretation states the scope of its claim

In-force P19 refuses in both directions at the edge of an interpretation: it
neither calls valid non-file data empty nor manufactures pseudo-files to
force it through the seam. This amendment adds the positive obligation those
refusals imply: what a file-bearing view says about the parts of its backing
it does not interpret.

Every file-bearing view is a view *of* something: the lowest durable layer
the session has materialized, which is the source of truth and which the
view never is. That floor may be an archive's own named-entry state, CHS
records, logical blocks, or timed flux. Presenting the P19 interface creates
no layer above that system — the system holds its own structure and exposes
a view of it — so this obligation falls on the provider that presents,
whatever it presents. Every addressable unit of the floor falls in exactly
one class:

- the **data hook** of an item the namespace names;
- the **structures the interpretation claims for itself** — directory
  records, allocation metadata, boot and reserved areas, an archive's local
  headers and central directory;
- space the allocation metadata **claims free** — recorded as that
  metadata's claim, never as a verdict that the extent is empty, disposable,
  or safe to reuse; or
- an **opaque region** — an extent the interpretation does not claim.

A valid namespace does not assert that every extent of its floor belongs to
it. Being a view rather than the truth is exactly what permits that: a truth
layer must account for everything because it *is* everything, while a view
may present what it can explain and name the rest. The opaque remainder is
itemized without a name: it stands beside the namespace with its hook stated
in the floor's own addressing, readable under P2 as evidence, its meaning
left to whatever other reading claims it. It is never listed as a namespace
entry — the pseudo-file rule stands — never reported as free space, and
never silently dropped from the view. Hooks, including an opaque region's,
are stated in the floor's addressing vocabulary because that is the only
vocabulary in which the account can be checked for totality.

The account is a report the view can produce, not work the open performs:
when it is computed is a resource question under P27, and computing it
mutates nothing (P2). Its classes are claims carrying P4 evidence, and an
extent the interpretation cannot classify is opaque rather than guessed into
a class.

The obligation does not vary with the kind of floor. A serialized container
presents a view of its own named-entry state, and bytes of the archive its
grammar does not account for — a self-extractor stub, padding between
members, data appended past the end of the directory — are opaque regions in
the same sense a protection track is opaque to a Commodore directory. Both
say the same thing: this interpretation does not explain this part of the
artifact.

Nothing is written through a view. The provider owning the floor performs a
write against the floor and the view is regenerated from the result, so no
view holds mutable state that could diverge from the truth it presents. A
view is likewise regenerated rather than migrated when the floor moves — a
sector image whose composition later descends to flux is presented by a new
view in flux addressing. Several views may coexist over one floor, since
none of them is mutable and none is the truth.

## P31 — Capture is a modality, not a layer

An artifact that records a magnetic layer holds it in one of two modalities,
and which one is a property of the artifact rather than of the layer. This
principle lifts a distinction the project had already made once, at one rung,
and states it where it belongs.

**Capture form** is a layer as an instrument recorded it, in that instrument's
own conventions and without the frame that would make it servable. **Served
form** is the same layer as a drive would meet it, complete enough to answer
by location. A capture-form artifact is materialized by a declared reduction
before anything reads it as a layer; a served-form artifact backs its layer
directly.

The distinction was found at flux, where a KryoFlux stream set holds several
observations of one location and no opinion about which the disk was, and a
P64 holds one circular stream per location in a declared frame. It recurs
above flux without being put there: NIB and NBZ hold a hardware bitstream in
capture form — a fixed window longer than a revolution, with the wrap nowhere
recorded — and G64 holds the same layer in served form, with each track's
length and position written down. Two rungs, four formats, and nothing chosen
to make the pattern come out.

### The test is servability, not writability

**No artifact in this family is a writable backing**, in either modality. A
session's writes land in its active layer, and an artifact appears only by an
explicit encode under P13 which builds a new file rather than mutating one in
place. Writability therefore sorts nothing, and a reading that used it would
put G64 and P64 on opposite sides of a line neither is on.

What sorts is whether the artifact can truthfully back its layer for reading:
whether a session can serve one location by key from the file as it stands,
under P27's source-backed residence. A P64 and a G64 can. A stream set and a
NIB cannot — the first has made no selection, the second has established no
circle — so each materializes into private session storage first, and what it
gains there is derived, not read.

### What a capture-form artifact owes

The reduction is a P29 act wherever it happens: declared policy inputs, a plan
that computes before anything is written, a declared-loss account in the
source's own terms, derived provenance on the result, and the same inputs
producing the same output. P29 was stated for mastering a capture to a medium
and binds every such reduction, which is what makes this principle a statement
about vocabulary rather than a new mechanism.

Two obligations follow for any adapter reading a capture-form artifact. The
frame the artifact lacks is **derived and declared as derived**, never
presented as recorded — a track length recovered by analysis says so, and a
location whose frame cannot be established is refused rather than served at
whatever length the file happened to hold. And evidence the artifact could not
record is **absent rather than asserted**: a single reading of an indeterminate
region says the source could not tell, and never that the region is stable.

### An artifact enters at the rung whose defining characteristics it shares

A capture form is defined by needing materialization, not by the rung its bytes
resemble, and the two can differ. What settles the entry rung is which layer's
defining characteristics the artifact actually has; content the entry rung
requires but the artifact never held is then **synthesized under declared policy
and recorded as synthesized**.

**The flux layer's defining characteristic is that a rotational recording's
start and stop are not crisp.** A disk has no natural beginning, an origin is
given rather than found, and the only physical trace of where a write began is a
splice. The delivered model says exactly this already: a flux medium carries an
origin *statement* recording which rule located its circle and on what evidence,
the C1541 profile defaults to the longest gap because that drive never observes
an index, and a located splice is a stated fact rather than a boundary. One rung
up the circle is crisp — a hardware bitstream has a definite cell count per
revolution, and a G64 writes each track's length down.

NIB and NBZ share the flux characteristic and not the bitstream one: a fixed
window longer than a revolution, overlapping itself, with the wrap nowhere
recorded. **That is why they enter at flux** (D16). Their bits carry no
transition timings, so a pulse materialized from them has a position computed
from a bit index and a declared cell width — synthetic, and the flux medium's
own model already refuses to call it anything else, since every pulse names what
put it there. In-force P22 governs the result unchanged: synthetic provenance is
retained, and protection, weak regions and timing evidence the source never
stored cannot be reproduced from it.

**Synthesis places nothing at an exact nominal position.** No drive writes at
the tick, so manufactured transitions carry **jitter**, drawn seeded and
recorded like every other draw in this family, with the amount and the seed
travelling as provenance. Pulses at perfect multiples of a declared cell would
be a recording no mechanism has ever produced, and would let a channel or a
recognizer be exercised against a regularity nothing downstream will meet.

**The amount is derived, not separately declared: half the family's admissible
reading deviation.** A profile already states what deviation from `k × cell` its
reader accepts, and synthesizing at half of it says the writing drive was
comfortably inside its own family's tolerance. The reason to fix the factor
rather than declare a second number is that it makes a property checkable: every
synthesized transition stays well within the band that classifies it, so
recovering the bitstream from a synthesized medium returns exactly the bits that
were synthesized. A round trip that can lose a bit would make the whole
placement unsafe. A caller may declare its own amount as a policy input, and a
looser one is answerable for that guarantee.

Two constraints keep the factor meaning what it says. **Jitter is drawn on the
interval, not on the absolute position** — two independently jittered positions
put twice the deviation into the interval between them, which lands exactly on
the band edge and misclassifies. And **the circle closes exactly**: jitter
redistributes inside a revolution and never changes its total, so the frame's
wrap is the one the reduction declared rather than the sum of a random walk.

Jitter and the reading band are per-transition and uncorrelated. **Spindle speed
variation is a third thing and is folded into neither** — it moves every
transition on a revolution together rather than each independently, so a family
that models it declares it separately or not at all.

The synthesized timing is the **compromise accepted for that placement**, not
the argument for it. What the placement buys is one path instead of two:
everything above the flux medium is the ordinary ladder — read channel,
bitstream, codec — so the class reaches a drive by the route every other flux
source already takes. What it costs is stated rather than hidden: the read
channel recovers bits from timings that were themselves computed from bits.

**This is not licence to place an artifact wherever is convenient.** The test is
the characteristic, applied to the artifact and stated with the reduction it
implies; what P13 continues to forbid is the claim that the artifact *recorded*
what was synthesized for it.

**It adds no active-layer row.** The durable vocabulary is unchanged: a
capture-form artifact carries no session's mutable truth at any rung, for the
reason already stated of flux capture — a write has no coherent destination in
an artifact that records several disagreeing observations, or one window whose
wrap nobody established. What P23 says of flux capture holds of every capture
form.

### Knock-on requirements

In-force P22's two-model clause is scoped to the flux family, which is where
the models were found; this principle generalizes the shape without amending
that clause, and P22's own statement of it stands as written. The pledged P23
amendment's upper half gains one sentence when it arms: capture-form artifacts
take no row at any rung, as flux capture takes none at its own.

## P10 amendment — a refusal may also name the rule it broke

In-force P10 gives every refusal a stable category from one enumerated set,
so an embedder maps behavior without parsing text. That set is deliberately
cross-cutting and small: it answers *how should the caller behave*, and it
answers it for the whole library at once.

One question it cannot answer is *which rule did this input break*. Where a
format, namespace, or grammar defines a bounded set of rules an input must
satisfy — a DOS 8.3 name has seven, and FAT is one filesystem of many — the
category is the same for every one of them, and the only difference between
them is the sentence. A caller that must act on the distinction, or state
it to a user in its own words, is then reduced to parsing the message no
release promises to keep, or to reimplementing the rule set to decide what
it would have said. Widening the category set instead would dissolve it:
the categories would grow one per format rule, and the small cross-cutting
mapping P10 exists to provide would be gone.

The amendment adds one field beside the category, not a second mapping:

Where a refusal is one of an enumerated set of rules defined by a format,
namespace, or grammar, the error also carries a **rule identity** — a
stable machine-readable value naming which rule was broken, from the set
owned by the seam that defines those rules. The category still says how to
behave and remains the interface an embedder maps onto; the rule identity
says which rule, and never substitutes for the category. A refusal
belonging to no such rule set carries none, and that absence is ordinary
rather than an omission. Each rule set is part of the surface that owns it
— adding a rule identity is a surface change, and rewording the diagnostic
that states it is not — and every presentation carries the same identities
(P5).

The rule identity is not a second diagnostic. It names the rule, and P6's
human diagnostic still says what was expected, what was found, and where.

U22's DOS 8.3 refusals are the first demand for this; nothing else the
library refuses today has a rule set behind it.

### What arming it will require

The field is surface on all three presentations, so nothing arms this by
adding it to the Rust error alone. The C and Python errors carry the same
identities, the generated header is committed with them, and the first rule
set is enumerated in the seam that owns it rather than in the error type —
which is the whole point of the field being a seam's value rather than a
second global set. F25 is the feature that carries it.

## P19 amendment — namespace composition may derive a mapping, not only consume one

Pledged P19 admits a namespace-composition adapter which "consume[s] file
containers plus explicit drive, mount, folder, or volume mappings and
expose[s] another file container". Both routes to that mapping assume it
already exists somewhere: recovered as evidence where an operating system
persisted it (U13, U16), or asserted outright by the caller.

A DOS machine persists no such mapping. Its drive letters were assigned at
boot by a rule over the machine's own configuration — which media occupied
which slots, in which order the disks were attached — and nothing on the
disks records the result. There is no evidence to read and nothing for the
caller to assert but the answer it came for. Under P19 as pledged, the only
remaining home for that rule is the caller, which is the one place it
cannot be checked against the volumes the library composed.

The amendment admits a third form at the same seam:

A **namespace-mapping composer** consumes composed volumes with their
identities, plus the machine facts its caller asserts, applies one named
assignment rule, and returns the mapping it establishes. Producing a
mapping and composing a file container over it are separate acts: the
mapping answers on its own, and a composer that can establish only part of
one still answers with that part.

Three constraints keep the derivation from becoming a guess:

- **The rule is an enumerated claim (P3).** The composer names the
  assignment rule it applied. Where variants of one system assign
  differently, it claims the variants it implements and refuses the rest by
  name; it does not average them or pick the most common.
- **Evidence outranks a rule.** Where a system persists its own mapping,
  that mapping governs and no rule may stand in for it. This form exists
  for systems which persist nothing, and it never becomes a fallback for a
  persisted mapping that could not be read — U13's and U16's refusal to
  invent `C:` is untouched.
- **A derived mapping is not evidence.** The asserted machine facts and the
  applied rule travel with the result as provenance, under the same
  discipline that keeps a caller-selected installation out of the evidence
  (U16). Whatever the rule cannot settle is reported undetermined, at the
  granularity of the mapping it failed to establish, and is never filled
  from position, size, order, label, or which volume happened to read
  cleanly.

The composer takes reports the caller already holds and returns a mapping;
it opens nothing. D5's deferral of multi-device topology, multi-device
volumes, and cross-source transactions is therefore untouched, and this
form requires none of the atomic multi-artifact open U16 proposes.

### Two amendments, one principle

P19 now carries two pledged amendments, and they touch different halves of
it. The scope-of-claim amendment above governs what a file-bearing view owes
about its floor; this one governs where a namespace mapping may come from.
Neither depends on the other, they arm separately, and a citation naming
only "the P19 amendment" is ambiguous from here on — name the half.
