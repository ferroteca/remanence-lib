<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# ARCHITECTURE (proposed)

> **Status:** proposed architectural changes drafted at the owner's
> direction. Nothing here binds. P15 keeps its permanent handle; if its
> amendment is pledged, it replaces P15's current legacy-mechanism cut in
> [pledged/ARCHITECTURE.md](../pledged/ARCHITECTURE.md). P22 and P23 are
> new proposed principles.

## P15 — Every drive family declares its integration seams (proposed amendment)

Every emulated drive belongs to a family whose catalog declares one or more
named adapters at the integration seams that family can faithfully present.
A composition selects one of those seams for a caller; several adapters may
operate over the same media instance without creating several independent
copies of its recorded state. The default seam is the common drive interface
presented to the system, with the device beneath it opaque. LBA hard drives
and modern optical or large-storage drives stop there: physical geometry,
pickups, servos, integrated controllers, firmware, error correction
internals, and microcode are not inferred or emulated.

Selected legacy families may instead claim one or more family-specific
hardware seams. The most reusable cut is at the controls and signals
between a caller-owned controller or I/O chip and the drive-side electronics
Remanence models. The caller advances both sides on one explicit clock,
applies timestamped control changes, and consumes causally ordered signal
transitions. A family may also claim a higher seam which includes a
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

This is both the composition law and the proposed common public hardware
interface; the pseudocode does not pledge literal Rust names or layout.
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
the timing mechanism combines the durable in-memory media state with the
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
Open and reset use the family timing/mechanism profile's declared initial
state; they never infer head position or rotational phase from image bytes.

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
| media | durable in-memory disk state plus evidence/provenance | media state and representable changes |
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
the family contract as `Hardware<C>`; C carries a contract tag with typed
stimulus, response, event, and inspection records; Python carries the same
contract identity on its `Hardware` object. A stimulus from one contract is
refused by an instance of another. The depth lies in one causal interface
and timing engine; locality lies in keeping 1541 drive-side circuitry, Disk
II sequencing, H17 USRT and hard-sector behavior, bus-drive rules, and
family events in their contract implementations.

Older drive families may also have a CHS interface that presents their
recorded cylinder, head, and sector layout without emulating a particular
controller. A family may provide CHS and programmed-hardware adapters over
the same media instance. CHS never exposes or invents geometry beneath an
LBA drive. The selected drive family declares the available seams; the
composition chooses among them explicitly, and neither an image format nor
a media profile chooses one implicitly.

### Knock-on if pledged

The P15 section of the pledged F19 design currently places data separation,
encoding, and byte assembly above a raw mechanism-transition seam. U7 now
places the 1541 public cut above those drive electronics but below the disk
VIA, so that section must move to the same truth when the amendment is
pledged. F19 itself still adds no drive implementation or emulator
presentation, and its delivery cut does not otherwise change.

## P22 — Magnetic recording can descend to timed flux transitions

For a magnetic-media family that claims a physical recording path,
Remanence can represent recorded state at the granularity of individual
timed flux-transition events. Each event carries its timing and any
detection-strength semantics claimed by the authoritative image or derived
model. This is the lowest modeled magnetic-data layer: analog head voltages,
amplifier waveforms, magnetic field shapes, and transistor-level behavior
remain outside the model.

When a low-level composition claims that physical recording path, the flux
layer is its durable mutable in-memory media state for the session, not a
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
be decoded upward while retaining ambiguity and evidence. Sectors, encoded
tracks, or another higher layer may be synthesized downward to flux only
when the image format, media profile, drive family, and mastering rules
claim a deterministic mapping; the result remains synthetic rather than
evidence of an original recording. This principle never causes an LBA or
otherwise opaque device to acquire invented flux geometry.

The flux floor is an internal modeling capability, not a universal public
interface. P15 still determines the programmed seam visible to a drive
emulator, and P3 and P12 still require each named image format to enter and
leave through the representation seam it actually supports.

### Knock-on if pledged

The pledged F19 image-format design already recognizes timed flux as an
authoritative layer and physical media profiles as the owners of index and
sector-hole topology. When P22 is pledged, that design must make the
separate flux-data and marker/sensor channels explicit. U7 supplies the
concrete P64 journey; P22 does not by itself pledge a standalone P64 image
adapter or a public flux interface.

## P23 — One durable layer is active

Every independently mutable open state instance has exactly one **active
layer**: the durable in-memory representation against which all of its
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
chosen image adapter.

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
state, media profile, drive family, encoding and mastering rules are used
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
legacy floppy family may lower CHS to flux. Block is terminal unless its
own family explicitly defines a non-speculative lower mapping; P15 forbids
inventing one for an opaque LBA device. File container participates only
through a declared container-to-child or filesystem-materialization path.
Encoded-track and bitstream image representations enter flux through their
family derivations rather than becoming extra rungs.

The transition is atomic for the media instance. Once the lower state is
validated, it replaces the old active layer as the single durable mutable
session state. Existing higher presentations are rebound as derived views
of it and their caches are invalidated. They may decode sectors and files
upward, but they cannot continue mutating the former CHS copy. The active
layer does not rise again during that open media lifetime merely because a
lower presentation closes; doing so could discard state the higher layer
cannot express. Returning to a higher active representation requires
closing the composition or an explicit conversion which names the loss.

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

### Knock-on if pledged

The pledged F19 design must distinguish authoritative image layer, active
layer, and derived views in its composition and capability rules.
U9 supplies the CHS-active DOS/IEC journey. U7, U10, U11, and U12 supply
hardware-emulation journeys whose physical read paths require flux-active
media. U8 requires higher file edits over flux-active mixed media to mutate
only representable regions through derived sector/filesystem views while
preserving all unrelated lower-layer state.
