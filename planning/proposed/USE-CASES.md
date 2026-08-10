<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# USE-CASES (proposed)

> **Status:** drafted at the owner's direction. Nothing here binds; a use
> case is pledged by moving it to `planning/pledged/` and reaches root
> [USE-CASES.md](../../USE-CASES.md) only on full delivery. Numbers come
> from the one global U-sequence and are never reused.


## U7 — A C64 emulator delegates the hardware behind the 1541 disk VIA

I am writing a C64 emulator that executes the 1541's 6502 code and emulates
its two 6522 VIAs. My emulator owns the CPU, RAM, ROM, address decoding,
VIA registers and timers, IEC bus, and scheduler. Remanence owns the
drive-side electronics connected to the disk VIA, the mechanism, and the
inserted medium. It does not emulate either 6522.

I load `<floppy-name>.p64` into the session's media pool — a `c1541-disk`,
served form, my own read-only open its claim — seat it in my machine's
1541 drive, and
obtain the drive-hardware presentation from that device (the P15 seat).
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
let mut session = Session::new();
let disk = session.load_media(File::open(floppy_path)?, Format::P64)?;

let c64 = session.add_machine("c64")?;
let unit8 = c64.add_device(cbmfloppy0)?;
unit8.insert(disk.id())?;

let mut hardware = unit8.hardware(C1541DriveOptions {
    weak_pulse_seed: 0x0123_4567_89ab_cdef,
    write_protected: true,
})?;

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

`load_media` recognizes and validates P64 over my own read-only open —
the claim is mine, honoured as afforded, for the medium's pooled
lifetime; `insert` seats it, the family check refusing a
medium the 1541 is not served; `hardware` composes the drive electronics
and mechanism over the seated medium, mechanism state living behind the
presentation and never on the medium (P33). `reset` resets only Remanence-owned drive
electronics and mechanism state. The caller separately resets the CPU and
VIAs and then supplies their resulting drive-side control bundle.

My read-only open protects the host artifact — my lock, checked for
what it affords and honoured exactly. The medium is also
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
- Save states, power sequencing, parallel cables, board modifications, and
  non-1541 variants. Disk swapping and several drives are the device tier's
  ordinary facts now (insert, eject, more devices); this journey drives one
  seated disk, and running-machine swap semantics are a later use case's.
- A universal signal enumeration shared by unrelated drive families. The
  timed-causality operations are common; this signal bundle is specific to
  the 1541 drive-side electronics.

## U8 — I edit a DOS-readable file without flattening a mixed-structure disk

I have a copy-protected P64 image with an intentionally mixed structure.
Its ordinary Commodore directory and initial program remain accessible
through standard DOS/IEC operations, as they were on the real disk. After
that entry point loads, the program may upload custom 1541 code which reads
nonstandard GCR, deliberate errors, weak pulses, protection data, private
data, or other flux evidence which Commodore DOS does not represent. The
standard part is not merely a partial recovery from a damaged disk, and the
nonstandard part is not debris around the files; both are functioning
parts of one artifact.

I open the image for writing, reach the DOS-readable directory through its
P19 file-access presentation, and replace one existing file. I do not
flatten the disk to sectors first and I do not sacrifice the unrepresented
recording merely because I chose a high-level editing operation.

The P64 flux remains the single P23 active media layer. Filesystem, sector,
and encoded-recording structures are derived views of that state, not
independently mutable copies. The high-level edit must therefore project
its proven local write set downward into the active flux while every other
presentation immediately observes the same resulting media.

The file-access result reports both what it can present and the scope of
that interpretation. A valid filesystem namespace does not assert that
every track, sector-shaped region, gap, half-track, or pulse belongs to the
filesystem. Lower-layer inspection remains available for everything
outside the view, with ambiguity, weak-event strength, timing, and
provenance preserved.

Before accepting the edit, Remanence computes its complete filesystem
write set: directory and allocation metadata, existing or newly allocated
data blocks, checksums, and every other logical structure the operation
would change. It then proves that each changed block maps unambiguously to
a bounded encoded-recording extent which the filesystem adapter is allowed
to replace. A nominally free block is not safe merely because allocation
metadata calls it free; it is eligible only when its recording extent is
understood and does not contain or overlap evidence outside the claimed
filesystem representation.

The smallest useful success replaces a file whose new contents fit its
existing allocation. A longer or relocated replacement may allocate more
space only from additional extents that pass the same proof. If the
filesystem is ambiguous, the write footprint crosses an opaque region, a
replacement cannot fit its bounded extent, or preserving track timing
would require shifting unidentified recording, Remanence refuses the
operation before changing session state and names the reason.

For an accepted edit, Remanence regenerates only the claimed recording
extents needed by that write set. Those regenerated pulses carry synthetic
provenance under P13. Outside them, the saved P64 remains event-for-event
equivalent: pulse position and strength, weak regions, missing pulses,
index-relative placement, revolution distinctions, and uninterpreted
format records are preserved to the fidelity the input adapter claimed.
Re-serializing container bookkeeping need not make the file byte-identical,
but it must not normalize, decode and remaster, or otherwise rewrite an
untouched recording region.

The edit uses the ordinary P2 commit point. Until commit, both the P19 view
and lower-layer inspection see one overlaid media state and rollback
restores the original image. Commit durably publishes the edited file and
the preserved opaque recording as one result; under P9, interruption may
not leave a mixture. Reopening the result must show the new file through
P19 and the same unedited flux evidence outside the declared write extents.

This use case is deliberately not a promise to repair an arbitrary damaged
filesystem, expose protection data as pseudo-files, make every P64 writable,
or infer that unrecognized space is disposable. Its point is narrower: a
partial high-level interpretation can support a proven local edit without
claiming or rewriting the whole lower-level artifact.

## U9 — A C64 emulator delegates ordinary 1541 DOS access

I am writing a C64 emulator which executes the C64 processor and KERNAL but
does not execute the 1541's processor, ROM, or custom drive code. I attach
`<floppy-name>.d64` as read-only device 8 and delegate the standard
Commodore DOS device to Remanence. Ordinary software can list the directory
and LOAD a program through the same device, channel, and byte semantics it
uses with a 1541, without my emulator acquiring a second processor or a
private sector-and-filesystem implementation.

My emulator owns the C64-side KERNAL integration. It turns the KERNAL's
standard serial operations into decoded IEC protocol events and copies
received bytes into C64 memory itself. Remanence owns the addressed drive's
DOS protocol state, channels, command/status behavior, filesystem
interpretation, and sector access. It does not execute drive firmware to
produce that behavior.

The smallest data operation at this seam is one IEC data byte together
with its end-or-identify indication. Primary LISTEN, TALK, UNLISTEN, and
UNTALK commands and secondary OPEN, CLOSE, and DATA commands remain
separate ordered protocol events. A whole-file `load` call would sit above
the standard device contract, hide observable channel behavior, and move
C64 memory ownership into the wrong module.

### The exact semantic interface

The Rust presentation has this semantic shape; P5 requires the C and Python
presentations to carry the same state transitions, bytes, end indication,
and refusals in their own idiom.

```rust
let mut drive = C1541Dos::open(
    floppy_path,
    AccessIntent::Read,
    C1541DosOptions {
        device_address: C1541DeviceAddress::Eight,
    },
)?;

drive.reset()?;

drive.primary(IecPrimary::Listen(8))?;
drive.secondary(IecSecondary::Open(0))?;
for (index, value) in filename.iter().copied().enumerate() {
    drive.write_byte(IecByte {
        value,
        eoi: index + 1 == filename.len(),
    })?;
}
drive.primary(IecPrimary::Unlisten)?;

drive.primary(IecPrimary::Talk(8))?;
drive.secondary(IecSecondary::Data(0))?;
loop {
    let byte: IecByte = drive.read_byte()?;
    kernal_accept(byte.value);
    if byte.eoi {
        break;
    }
}
drive.primary(IecPrimary::Untalk)?;
```

`IecPrimary` preserves the device address carried by LISTEN and TALK. One
logical device instance observes the ordered primary events and responds
only to its configured address; commands for another address do not alter
its state. `IecSecondary` distinguishes the OPEN, CLOSE, and DATA classes
and carries the four-bit channel number. The interface does not expose D64
sectors, host files, or a special directory-listing operation.

`write_byte` is valid only while the device is the active listener on an
opened protocol path. `read_byte` is valid only while it is the active
talker and returns exactly one byte. The `eoi` flag carries the protocol's
logical end indication rather than inventing an out-of-band length. Invalid
event ordering is a caller error. A valid command which Commodore DOS
rejects is guest-visible DOS behavior, normally reported through channel
15, rather than a Remanence interface failure.

There is no emulated IEC wire clock at this seam. Calls are ordered but
untimed, and byte readiness is represented by the call result rather than
by CLOCK and DATA transitions. Device presence, EOI, channel state, and DOS
status remain observable because standard software depends on them. A
caller which needs electrical handshakes, cycle timing, or arbitrary line
driving must use a lower family seam.

`C1541Dos::open` recognizes and validates the sector image, creates the
direct whole-medium volume, recognizes the Commodore filesystem, and takes
the P7 claim for the device's lifetime. Failure at any layer is reported
with its evidence; opening for this use never guesses a filesystem merely
to manufacture a DOS device. The D64 sectors remain the authoritative P13
image layer. No GCR track, flux stream, motor, head, or rotational state is
synthesized because this composition does not need one. The addressed
sectors are also the P23 active media layer, naturally presented as CHS to
the DOS implementation. Asking the same media instance for the lower U7
hardware-emulation service would first require an explicit generate-flux
transition; it is not performed by an ordinary DOS/IEC read.

### One complete standard LOAD

To load a named PRG, the caller sends LISTEN for device 8, OPEN for channel
0, and the PETSCII filename as IEC bytes with EOI on the final byte, then
UNLISTEN. It sends TALK for device 8 and DATA for channel 0, then calls
`read_byte` repeatedly until the returned byte carries EOI. Remanence
follows the directory entry and sector chain, validates every structure it
claims, and emits the file bytes in Commodore DOS order. For a PRG the first
two bytes are its little-endian load address; the caller's KERNAL logic
interprets that address and writes all bytes into C64 memory.

A directory load follows the same protocol with the conventional directory
filename. Its generated directory program is DOS-device behavior at this
seam, not a P19 directory listing disguised as a byte stream. Reading the
command/status channel likewise uses ordinary OPEN/DATA/TALK operations.
The smallest useful success is one correct talk byte with its correct EOI
state, repeatable through a complete standard directory or PRG load.

This D64 journey is the minimum claim, not a format restriction built into
the DOS interface. Another image representation may feed the same adapter
only when its sectors and filesystem can be derived with the evidence and
ambiguity rules of P3, P4, and P13. In particular, admitting a P64 here
does not flatten or discard recording outside the DOS-visible sector view.

### Deliberately outside this use case

- Executing the 1541 CPU, ROM, DOS firmware, or uploaded machine code.
- Emulating IEC CLOCK, DATA, and ATN levels, handshakes, or cycle timing.
- C64 software which bit-bangs a nonstandard serial protocol or installs a
  fast loader that requires custom drive behavior.
- Reading raw GCR, weak pulses, protection data, or nonstandard tracks.
- SAVE, scratch, format, validate, block-write commands, or any mutation of
  the image. Writable Commodore DOS behavior requires a separate use case.
- Claiming every command and error of every 1541 ROM revision. P3 requires
  the delivered adapter to enumerate its exact DOS compatibility claim.
- Returning host paths or touching C64 memory. P19 file access and C64-side
  memory integration remain separate presentations owned at other seams.

## U10 — An Apple II emulator delegates the Disk II controller below its CPU

I am writing an Apple II emulator which executes the machine's 6502 code.
My emulator owns that CPU, RAM, motherboard ROM, slot ROM bytes, floating
data bus, and machine scheduler. Remanence is the programmed hardware in a
slot-6 Disk II controller and its attached drive: the sixteen soft
switches at `$c0e0–$c0ef`, data latch, logic-state sequencer, read channel,
stepper and motor behavior, mechanism, and inserted media.

I load a read-only `<floppy-name>.woz` version 2.1 image into drive 1. The
image contains at least one track selected through its FLUX map, so that
track's authoritative state is a correctly looped stream of flux-transition
timings rather than a normalized sector or nibble stream. Other tracks may
use WOZ exact-length bitstreams. Because this journey requests the physical
Disk II path, P23 materializes one flux-active media state: FLUX-mapped
tracks enter directly, while exact-length bitstream tracks are synthesized
downward with their timing and synthetic provenance preserved. The Disk II
read channel and controller logic operate only on that active state.

When the emulated CPU reads or writes any address in the slot I/O window,
my address decoder forwards that one timed bus transaction to Remanence.
The Apple program—boot ROM, DOS code, or arbitrary copy-protection code—can
control phases and motor, select either drive, manipulate Q6 and Q7, poll
the data latch, exploit floating-bus and sequencer side effects, and count
CPU cycles exactly as it did on the card. It has no Remanence-specific path
for asking for a sector, nibble, bit, flux transition, or decoded file.

The smallest disk datum visible at this seam is the controller's current
eight-bit data-latch value when the selected soft-switch access drives it
onto the CPU data bus. The caller also observes when the card drives no bus
bits and must then resolve its own floating-bus value. The Apple program
interprets latch values, timing, prologs, checksums, sectors, and files.
Remanence performs none of that interpretation for this composition.

### The exact semantic interface

The Rust presentation fixes the semantic shape below. P5 requires the C
and Python presentations to carry the same operations, ordering, bus-drive
semantics, and refusals in their own idiom.

```rust
let mut hardware = Hardware::<AppleDisk2>::open(
    AppleDisk2Options {
        slot: Apple2Slot::Six,
        controller: Disk2Controller::SixteenSector,
        read_noise_seed: 0x0123_4567_89ab_cdef,
        machine_timing: Apple2Timing::Ntsc,
    },
    vec![MediaAttachment {
        slot: Disk2Drive::One,
        source: floppy_path.into(),
        access: AccessIntent::Read,
        write_protected: true,
    }],
)?;

let effects = hardware.reset(Apple2Tick::ZERO)?;
apply_effects(&effects);

hardware.advance_to(at)?;

let effects = hardware.interact(
    at,
    ProgrammedIoStimulus::Read { address: 0xc0ec_u16 },
)?;
let cpu_value = effects.response.resolve(floating_bus(at));
apply_effects(&effects);

let effects = hardware.interact(
    at,
    ProgrammedIoStimulus::Write {
        address: 0xc0e9,
        value: value_on_cpu_bus,
    },
)?;
apply_effects(&effects);

let monitor_drive = hardware.inspect(
    ProgrammedIoInspect::Io { address: 0xc0ec_u16 },
)?;
```

The semantic result types are:

```rust
pub type AppleDisk2Stimulus = ProgrammedIoStimulus<u16>;
pub type AppleDisk2Inspect = ProgrammedIoInspect<u16>;

pub struct DataBusDrive {
    pub value: u8,
    pub driven_mask: u8,
}
```

`AppleDisk2` binds the common hardware interface to `Apple2Tick`, the
stimulus and inspection types above, `DataBusDrive` as response and
inspection, and an uninhabited event type. The C presentation uses the same
common `remanence_hardware_*` operations as U7 with an Apple Disk II
contract descriptor and typed records. The Python presentation wraps the
same hardware instance and does not replace interactions with a sector,
nibble, or file iterator.

`DataBusDrive::resolve(floating)` selects `value` where `driven_mask` has
ones and the caller's floating-bus byte elsewhere. For this Disk II card,
an even soft-switch read drives all eight bits of the latch and an odd read
drives none. A mask rather than `Option<u8>` keeps the programmed-I/O
concept correct for another card which drives only selected bits.

The `Read` and `Write` stimuli accept only complete sixteen-bit addresses in
the configured slot's I/O window. They never mask an arbitrary address to
four bits. Both interactions perform the addressed soft-switch action;
the write value matters when the current Q6/Q7 sequencer state loads the
data latch. A read returns the bus drive after the controller-compatible
same-access state transition. An address outside the configured window is
the caller's decode error and is refused.

The `Io` inspection is for a stopped debugger or monitor. It reports what
the card would currently drive at that address without advancing time,
toggling a soft switch, clocking or resetting the sequencer, changing the
latch, moving the head, or acknowledging any state. Guest execution never
uses it.

The controller supplies no CPU interrupt or asynchronous motherboard line
in this journey. Its common `HardwareEffects` therefore has no family event
payload. That absence is real hardware shape, not permission to omit time:
rotation, bit arrival, motor delay, sequencer clocks, and stepper motion
progress inside the adapter and affect later bus transactions.

### Time and access ordering

One `Apple2Tick` is one 6502 bus cycle under the selected machine timing
profile. Its timestamp denotes the fixed card-select and data-sample phase
of that cycle, so the caller and adapter do not independently choose a
subcycle convention. Times are absolute cycles since open and must be
monotonic. The adapter maps WOZ's bit timing and 125 ns flux timing into
this clock with an internal phase accumulator; it never rounds every media
event independently to a CPU cycle.

`advance_to(tick)` progresses the controller and mechanism to a CPU cycle
without performing a bus access. A read or write first advances to its
timestamp and then performs exactly one transaction. The Apple adapter has
no autonomous effect which the caller must apply before continuing, so it
does not need U7's externally visible deadline loop. This difference is
captured by an optional common `next_event_tick`: it returns `None` here,
while a family with interrupt or connector-line events returns its next
undelivered deadline.

At a shared tick, any internal transition scheduled before the CPU sample
phase is applied first; the soft-switch action follows; the returned bus
drive is sampled last. Reset, advance, read, and write all return the state
at the transaction's absolute tick. A past timestamp, or a timestamp whose
machine timing profile disagrees with the one fixed at open, is refused.

### What the sixteen soft switches expose

For a controller in slot 6, `interact` recognizes exactly:

| Address | Action |
|---|---|
| `$c0e0/$c0e1` | phase 0 off/on |
| `$c0e2/$c0e3` | phase 1 off/on |
| `$c0e4/$c0e5` | phase 2 off/on |
| `$c0e6/$c0e7` | phase 3 off/on |
| `$c0e8/$c0e9` | motor off/on |
| `$c0ea/$c0eb` | drive 1/drive 2 select |
| `$c0ec/$c0ed` | Q6 low/high |
| `$c0ee/$c0ef` | Q7 low/high |

Q6 and Q7 select the logic-state-sequencer function; they are not
independent high-level commands hidden behind convenience methods. The
adapter preserves address parity and access kind because real software
depends on their coupled effects. Even-address reads drive the latch.
Odd-address reads leave the data bus floating. Reading `$c0ed` also resets
the sequencer and clears the latch in the claimed controller behavior.
Writing while Q6/Q7 select data-load mode loads the value present on the
CPU bus.

The four phase controls drive the stepper magnets with their timing and
combined-phase behavior, including the quarter-track positions represented
by WOZ's track map. Selecting motor off starts the historical one-second
delayed shutoff rather than stopping rotation immediately. Drive selection changes
which attached mechanism receives the enabled phases and motor. The
read-only journey has media in drive 1 and an empty drive 2, but all sixteen
switches retain their hardware behavior.

The controller implements the complete claimed read path: rotational phase,
head position and settling, WOZ quarter-track selection, flux-to-pulse
recovery, MC3470-style filtering and fake-bit behavior, logic-state
sequencing, shift register, data latch, write-protect sensing, and the
documented and claimed undocumented soft-switch interactions. The caller
does not reproduce any of those behind a second private disk model.

The slot ROM bytes remain caller-owned. Remanence reports whether the WOZ
metadata and selected 13- or 16-sector controller variant are compatible,
but it neither supplies copyrighted firmware nor maps `$c600–$c6ff`. This
keeps the ownership rule consistent with U7: executable CPU code stays
above the selected hardware seam, although the two families place that
seam at different physical cuts.

### One complete low-level read

After open and reset, the caller starts its unchanged slot-6 boot ROM or
another Apple program. The program accesses the phase switches to position
the head, selects drive 1, selects motor on, and places Q7 and Q6 in the
read/shift configuration. Every access reaches Remanence at its actual CPU
cycle, including accesses made only for their switch side effects.

As the selected track rotates, flux timings from a FLUX-mapped WOZ track—or
bits from a TMAP-mapped track—pass through the same read-channel and
logic-state-sequencer model. The program repeatedly reads an even
soft-switch address such as `$c0ec`. Each transaction returns the latch
value actually driven in that CPU cycle. The program commonly tests bit 7
to recognize a completed disk nibble, then consumes the byte and continues;
copy-protection code may instead use other addresses, exact cycle spacing,
sequencer resets, quarter tracks, long zero runs, or motor-off delay.

The caller repeats ordinary bus transactions until its Apple code has found
and decoded the structure it wants. That code, not Remanence, decides that
latch values form address prologs, data fields, sectors, or files. The
smallest useful success is therefore one correctly timed, fully driven
latch read from `$c0ec`, repeatable into an arbitrary Apple disk-code nibble
stream—not `return sector N` or `return next nibble`.

### The WOZ and flux floor

WOZ 2.1 is the emulator-facing preservation format for this journey. Its
exact-length bitstream tracks carry their optimal bit timing; its optional
FLUX map identifies tracks whose TRKS entries instead encode correctly
looped flux-transition intervals at 125 ns resolution. P22 and P23 preserve
the FLUX entries and synthesize the exact-length bitstream entries into one
durable flux-active media state before controller service begins. The WOZ
image's per-track authoritative representation and provenance do not
change. A2R is the related raw-capture format, capable of multiple captures
and index timings, but solving capture evidence into an emulation-ready
rotating medium is outside this smallest journey.

Long transition-free regions and cleaned WOZ fake-bit regions are not
flattened to one fixed nibble. The modeled read channel produces the
noise-driven false pulses and varying latch values which the physical
controller would expose. Given the same image, controller history, machine
timing, and `read_noise_seed`, one instance is reproducible; changing the
seed may change only stochastic read-channel outcomes.

No public bitstream, flux iterator, nibble callback, or track-buffer API is
required. The use case is met only when arbitrary Apple code can exercise
the slot soft switches and observe bitstream timing, flux timing, fake
bits, quarter tracks, sequencer side effects, and motor behavior through
ordinary CPU reads and writes.

### The reference-emulator retrofit

This journey must be testable by adapting an existing cycle-aware Apple II
emulator which already executes the machine CPU and slot ROM. The viable
cut is its Disk II card I/O dispatcher:

1. Its WOZ attachment path creates one Remanence Disk II controller with
   the image in drive 1 instead of parsing tracks into private drive state.
2. Its slot dispatcher routes all reads and writes for the complete
   `$c0e0–$c0ef` window to `interact`; CPU, motherboard,
   floating bus, and `$c600–$c6ff` slot ROM remain with the emulator.
3. Its CPU scheduler timestamps each access in absolute CPU cycles and
   resolves `DataBusDrive` against its own floating-bus value. Existing
   shortcuts which return a nibble directly or advance media only when
   `$c0ec` is polled must not remain active on this path.
4. Its reset path calls `reset`, and its stopped monitor calls `inspect`.
5. Its existing display may derive drive activity from reported state, but
   display ownership and sound do not cross the programmed-I/O seam.

Replacing only the WOZ parser, only the rotating-track buffer, or only the
`$c0ec` read handler does not meet U10: each leaves motor, phases, Q6/Q7,
sequencer, latch, floating-bus behavior, or timing owned on both sides. The
retrofit succeeds when unchanged boot code reads a normal WOZ track and an
unchanged protection test reads a FLUX-mapped or fake-bit track without any
sector, nibble, bit, or flux callback added to the emulator.

### Deliberately outside this use case

- Executing the Apple II's 6502, RAM, motherboard ROM, or slot ROM.
- Decoding nibbles into sectors or files, or providing DOS/ProDOS services.
- Mutating the medium. Write-mode switch and latch behavior remain visible,
  but a writable-media journey is separate.
- Supplying slot firmware or choosing 13-sector versus 16-sector firmware
  on the caller's behalf.
- IWM, SWIM, 3.5-inch, double-sided, or later Apple drive families.
- More than one inserted disk, disk swapping, ejection, or save states.
- Drive sound, LEDs, and graphical presentation.
- A standalone public WOZ bitstream, A2R capture, or flux interface.

## U11 — An H89 emulator delegates the H17 controller below its CPU

I am writing a Heath H89 emulator which executes the machine's Z80 code.
My emulator owns that CPU, memory, monitor and disk firmware, I/O decoding,
the general-purpose port, and the machine scheduler. Remanence is one H17
hard-sector controller at ports `0x7c–0x7f` and one attached H-17-1 drive:
the four CPU-visible ports, AMI S2350 synchronous USRT, discrete read/write
and drive-control electronics, motor and stepper behavior, optical
hard-sector sensor, read head, and inserted medium.

I load a read-only `<floppy-name>.scp` into drive 0. It is a raw capture of
a standard single-sided, 40-track, ten-sector Heath disk. It contains timed
flux-transition intervals, not reconstructed sectors or bytes. It was
captured in a hard-sector raw mode which retained every pulse from the
drive's index line as a successive SCP index-time interval: ten sector
marks and the additional closely spaced index mark for each revolution.
Those flux and reconstructed marker channels form the P23 active media
layer for this physical-controller journey.

When the emulated CPU executes an `IN` or `OUT` against one of the four
ports, my I/O decoder forwards that one timed transaction to Remanence.
The unchanged monitor, HDOS code, or arbitrary controller program can
select the drive, start the motor, step the head, observe hole/track-zero/
write-protect state, program the USRT fill and sync characters, enter sync
search, poll receive status, and consume received bytes exactly through
the board's ports. It has no Remanence-specific request for a sector,
decoded FM bit, flux transition, or file.

The smallest disk datum visible at this seam is one received eight-bit
USRT character read from `0x7c`, together with the status transitions and
timing which made it available. The controller does not expose the serial
read-data bitstream to software: its read electronics and S2350 recover,
synchronize, and assemble that stream below the ports. Software may work
byte by byte and may impose any sector interpretation it wants, but it
cannot sample an individual disk bit at this boundary.

### The exact semantic interface

The Rust presentation fixes the semantic shape below. P5 requires the C
and Python presentations to carry the same operations, ordering, bus-drive
semantics, events, and refusals in their own idiom.

```rust
let mut hardware = Hardware::<HeathH17>::open(
    HeathH17Options {
        base_port: 0x7c,
        host_timing: HeathHostTiming::H89_2_048Mhz,
        mechanism: H17Mechanism::H17_1,
        scp_markers: ScpMarkerInterpretation::Heath10SectorRaw,
        read_noise_seed: 0x0123_4567_89ab_cdef,
    },
    vec![MediaAttachment {
        slot: H17Drive::Drive0,
        source: floppy_path.into(),
        access: AccessIntent::Read,
        write_protected: true,
    }],
)?;

let effects = hardware.reset(H89Tick::ZERO)?;
apply_effects(&effects);

hardware.advance_to(at)?;

let effects = hardware.interact(
    at,
    ProgrammedIoStimulus::Read { address: 0x7d_u8 },
)?;
let status = effects.response.resolve(io_floating_bus(at));
apply_effects(&effects);

let effects = hardware.interact(
    at,
    ProgrammedIoStimulus::Write {
        address: 0x7f_u8,
        value: control,
    },
)?;
apply_effects(&effects);

let monitor_drive = hardware.inspect(
    ProgrammedIoInspect::Io { address: 0x7c_u8 },
)?;
```

The semantic result types are:

```rust
pub type HeathH17Stimulus = ProgrammedIoStimulus<u8>;
pub type HeathH17Inspect = ProgrammedIoInspect<u8>;

pub enum HeathH17Event {
    BootRamWriteEnable(bool),
}

pub struct DataBusDrive {
    pub value: u8,
    pub driven_mask: u8,
}
```

`HeathH17` binds the common hardware interface to `H89Tick`, the stimulus
and inspection types above, `DataBusDrive`, and `HeathH17Event`. The C
presentation uses the same common `remanence_hardware_*` operations as U7
and U10 with an H17 contract descriptor and typed records. The Python
presentation wraps the same hardware instance and does not replace port
interactions with a byte or sector iterator.

The `Read` and `Write` stimuli accept only the complete eight-bit port
addresses `0x7c–0x7f` selected at open. They never mask an arbitrary port
to two bits. A read advances to its timestamp, performs all read side
effects, and then returns the data-bus drive sampled by the CPU. A write
advances first and then presents its byte to the selected controller
function. An address outside the window is the caller's decode error and is
refused.

`DataBusDrive` preserves the real board's per-port driven bits rather than
inventing zero for any electrically undriven input. The caller resolves
the remainder against its own I/O-bus model. `inspect` reports the current
drive without advancing time, consuming receiver data, clearing status,
entering sync-search mode, or acknowledging sync detect. Guest execution
never uses it.

The controller has no interrupt or other autonomous CPU line in this
journey, so `next_event_tick()` returns `None`. Port `0x7f` bit 7 controls
whether the caller-owned boot RAM is writable, however, so a write which
changes that output returns a same-transaction `BootRamWriteEnable` event.
The caller applies events in returned order before executing its next CPU
operation. Remanence does not own or mutate the machine's memory map.

### Time and access ordering

One `H89Tick` is one Z80 T-state under the selected H89 timing profile.
Its timestamp denotes the profile's fixed I/O sample point. Times are
absolute since open and monotonic. The adapter maps SCP's 25 ns units,
motor rotation, recovered FM clock, and USRT transfers into that clock
with internal phase accumulators; it does not round every flux interval or
character independently to a T-state.

`advance_to(tick)` progresses the USRT, electronics, rotation, and
mechanism without performing a port access. A read or write first applies
all internal transitions due before the fixed sample point, then performs
exactly one port transaction, then reports the sampled bus and external
effects. Two accesses cannot occupy the same CPU I/O cycle. A past
timestamp or a timing profile inconsistent with the one fixed at open is
refused.

### What the four ports expose

The adapter preserves the different read and write meanings of the same
decoded ports:

| Port | `IN` | `OUT` |
|---|---|---|
| `0x7c` | receive-data register; acknowledges receive-data-available | transmitter holding register |
| `0x7d` | USRT status | fill-character register |
| `0x7e` | sync reset; enters search-for-sync mode | receive sync-character register |
| `0x7f` | drive and sync-detect status | drive, mechanism, write-gate, and boot-RAM control |

The `0x7d` status claim includes receive-data-available, receiver overrun,
receiver parity error, fill-character-transmitted, and transmitter-buffer-
empty. Remanence models their S2350 timing and documented clear conditions;
it does not calculate them when software happens to poll. If another byte
completes before software consumes the receive register, the adapter
performs the real one-character-register overrun behavior rather than
quietly buffering a byte stream.

Writing `0x7d` programs the byte the transmitter supplies whenever its
holding register has no caller byte ready. Writing `0x7e` programs the sync
character; reading `0x7e` resets synchronization and starts the S2350's
search for that character. Sync acquisition, the receive register, and the
sync-detect indication are stateful controller behavior, not a host-side
scan of decoded bytes.

Reading `0x7f` exposes hole detect, track zero, media write protect, and
sync-character detect. Writing it controls write gate, three independent
drive-select outputs, common motor, step direction, active-high step
command, and boot-RAM write enable. This journey inserts only drive 0 and
uses a single-sided H-17-1 mechanism, but all three select outputs and all
port bits retain their claimed hardware behavior. Selecting multiple
drives is not normalized to one convenient unit: the adapter either models
the electrically simultaneous selection claimed for the board or refuses
that unsupported electrical composition explicitly.

The read-only access intent prevents the write gate from altering media.
It does not hide transmit, fill, or write-gate state, because controller
programs use transmitter status when detecting and operating the board.
An attempted transition which would commit magnetic change is refused at
the operation which requires it.

### One complete low-level read

After open and reset, the caller executes unchanged monitor or disk code.
That code selects drive 0 and turns on the motor by writing `0x7f`, waits
as its own firmware requires, and steps outward until `IN 0x7f` reports
track zero. It then steps to the desired track using timed active-high step
commands. Remanence advances motor speed, rotational phase, head movement,
and settling continuously between those port accesses.

To find a record, the code writes its idle fill byte to `0x7d` and the
Heath sync byte—normally `0xfd`—to `0x7e`. It reads `0x7e` to put the S2350
into sync-search mode. Flux transitions passing the head traverse the
modeled read amplifier, pulse shaping, FM clock/data separation, and S2350
serial receiver. When the programmed sync character is recognized, the
controller changes its sync and receive status at the correct time.

The code polls `0x7f` and `0x7d`. Once receive-data-available is asserted,
one `IN 0x7c` returns the current received character and acknowledges that
status. Further timed polls and data reads yield the header or data field
one character at a time; the guest code checks volume, track, sector, and
checksum bytes and decides whether to keep searching. It may also observe
and count hole-detect pulses, but Remanence never turns those pulses into a
requested sector on its behalf.

The smallest useful success is one correctly timed `IN 0x7c` which returns
the byte assembled by the S2350 after a real sync search, repeatable into
an arbitrary controller-visible byte stream. Returning a sector, handing
the caller recovered FM bits, or letting it pull a flux iterator would all
cross the H17's actual programmed boundary.

### The SCP flux and hard-sector-marker floor

SCP is used here as the raw flux container: each nonzero sample is the
timed interval to the next captured flux transition. Those timings remain
authoritative evidence behind the medium and read-channel models; they are
not predecoded to H17Disk, H8D, sectors, or ideal FM bytes when opened.
Multiple captured passes remain evidence from which a deterministic
emulation pass is selected or solved under an explicit policy and seed.

The H17's optical hole signal is a different physical stream from magnetic
flux. The base SCP format gives each stored segment an index-time value but
does not name hard-sector marks. `Heath10SectorRaw` therefore accepts only
the explicit raw-capture convention in which every index-line pulse was
retained as a successive timing boundary. It recognizes the closely
spaced extra mark which identifies the revolution boundary and preserves
the other ten positions as sector marks. An ordinary index-cued or splice
capture which discarded those intermediate marks is not silently accepted
under this policy.

SCP preserves the mark positions in that convention, not the optical
signal's asserted pulse widths. The H-17-1 mechanism profile therefore
fabricates those widths and records that provenance in the open result.
This is adequate for the controller read journey, but it is not represented
as captured evidence. A future capture format with an independent marker
signal can replace the fabricated widths without changing the four-port
API. If the short index pair cannot be identified consistently, open is
ambiguous and refused rather than inventing rotational alignment.

Weak, missing, extra, and irregular flux transitions remain in the magnetic
stream and affect recovered bits and USRT bytes through the read channel.
Given the same capture-selection policy, seed, image, timing profile, and
controller history, the journey is reproducible. No public flux, FM-bit,
or byte-pull interface is required.

### The reference-emulator retrofit

This journey must be testable by adapting an existing cycle-aware H8/H89
emulator which already executes the CPU and monitor/disk firmware. The
viable cut is its H17 I/O-device dispatcher:

1. Its disk-attachment path creates one Remanence H17 adapter with the raw
   SCP in drive 0 instead of decoding an H8D/H17Disk or private raw track.
2. Its I/O decoder routes every `IN` and `OUT` for `0x7c–0x7f` to
   `interact`; CPU, memory, ROM, and general-purpose port
   remain with the emulator.
3. Its scheduler timestamps each transaction in absolute T-states and
   advances the adapter even while guest code is not polling disk ports.
4. It applies `BootRamWriteEnable` to its own memory-map logic at the
   returned transaction time. Reset calls `reset`; a stopped monitor uses
   `inspect`.
5. Its old controller, byte scheduler, hard-sector pulse generator, and
   track buffer are disabled on this path. Display and sound may observe
   state but do not become part of the controller seam.

Replacing only the SCP parser, only the track byte source, or only `IN
0x7c` does not meet U11: each leaves some combination of rotation, hole
detect, sync search, fill transmission, overrun, stepping, or port side
effects owned on both sides. The retrofit succeeds when unchanged firmware
reads a normal Heath disk and a controller-level diagnostic observes the
same timed port behavior from deliberately irregular raw flux without any
sector, bit, or flux callback added to the emulator.

### Deliberately outside this use case

- Executing the H8/H89 CPU, RAM, monitor ROM, or disk firmware.
- Interpreting received bytes as HDOS/CP/M sectors, directories, or files.
- Writable media, formatting, or persistence; write-path port behavior is
  visible only as required for controller fidelity.
- H8 host timing, alternate base ports, or machine-specific memory maps;
  this smallest journey fixes the H89 timing profile and `0x7c` base.
- Double-sided or 80-track extensions, side selection, and later H37/H47
  soft-sector controller families.
- More than one inserted disk, disk swapping, ejection, or save states.
- Treating H8D or H17Disk as flux evidence; they remain useful higher-level
  representations with different information floors.
- Claiming that synthesized hard-sector pulse widths were captured by SCP.
- Drive sound, LEDs, and graphical presentation.
- A standalone public SCP, recovered-FM, or controller-byte stream API.

## U12 — An Altair emulator delegates the original MITS 88-DCDD controller

I am writing an Altair 8800 emulator which executes the machine's 8080 code.
My emulator owns the CPU, memory, boot ROM, S-100 I/O decoding, and
scheduler. Remanence owns the original two-board MITS 88-DCDD controller,
its selected Pertec FD-400 mechanism, and the inserted media. The programmed
seam is the three standard I/O ports `0x08–0x0a`.

I attach `<floppy-name>.scp` as a read-only, physically write-protected
32-hard-sector disk in drive 0. The original medium has 77 tracks, 32
physical sectors per track, and 137 bytes per sector. My emulator performs
ordinary timestamped `IN` and `OUT` cycles. Remanence advances rotation,
hard-sector position, head state, the read channel, and the controller's
parallel-byte latch between those cycles.

This controller has no DMA and this journey claims no CPU interrupt. Guest
software must poll sector position and data-ready state and read every byte
at the controller's cadence. The smallest useful read is one timed data-port
transaction after the status port reports a new byte; the smallest complete
journey reads one 137-byte physical sector.

### The exact semantic interface

The Altair adapter uses the same `Hardware<C>` interface and the same
`ProgrammedIoStimulus<u8>` specialization as U11:

```rust
let mut hardware = Hardware::<Mits88Dcdd>::open(
    Mits88DcddOptions {
        base_port: 0x08,
        host_timing: AltairTiming::Altair8800_2Mhz,
        mechanism: AltairMechanism::PertecFd400,
        scp_markers: ScpMarkerInterpretation::Mits32SectorRaw,
        read_noise_seed: 0x0123_4567_89ab_cdef,
    },
    vec![MediaAttachment {
        slot: Mits88DcddDrive::Drive0,
        source: floppy_path.into(),
        access: AccessIntent::Read,
        write_protected: true,
    }],
)?;

hardware.reset(AltairTick::ZERO)?;

hardware.interact(
    at,
    ProgrammedIoStimulus::Write {
        address: 0x08_u8,
        value: 0x00, // select and enable drive 0
    },
)?;

hardware.interact(
    at,
    ProgrammedIoStimulus::Write {
        address: 0x09_u8,
        value: 0x04, // load head
    },
)?;

let effects = hardware.interact(
    at,
    ProgrammedIoStimulus::Read { address: 0x09_u8 },
)?;
let sector_position = effects.response.resolve(io_floating_bus(at));

let effects = hardware.interact(
    at,
    ProgrammedIoStimulus::Read { address: 0x08_u8 },
)?;
let status = effects.response.resolve(io_floating_bus(at));

let effects = hardware.interact(
    at,
    ProgrammedIoStimulus::Read { address: 0x0a_u8 },
)?;
let data = effects.response.resolve(io_floating_bus(at));

let monitor_status = hardware.inspect(
    ProgrammedIoInspect::Io { address: 0x08_u8 },
)?;
```

`Mits88Dcdd` binds `MediaSlot` to `Mits88DcddDrive`, `Tick` to
`AltairTick`, `Stimulus` to `ProgrammedIoStimulus<u8>`, `Response` and
`Inspection` to `DataBusDrive`, `InspectQuery` to
`ProgrammedIoInspect<u8>`, and `Event` to an uninhabited type. The common C
and Python hardware interfaces use an 88-DCDD contract descriptor and the
same typed programmed-I/O records as the other port-mapped contracts.

The media list may occupy any subset of the controller's sixteen slots.
Every occupied slot owns a separate P7 claim and P23 active-media instance;
empty slots are real controller state, not open errors. Drive selection is
ephemeral hardware state and never changes which image owns which durable
state.

### What the three ports expose

The contract recognizes exactly the configured three-port window. At the
standard base:

| Port | `IN` | `OUT` |
|---|---|---|
| `0x08` | drive/controller status | select or disable one of sixteen drives |
| `0x09` | sector number and active-low sector-true | step, head load/unload, interrupt controls, head current, or begin write sequence |
| `0x0a` | next latched read byte | next write byte |

On status input, the controller reports active-low write-byte-ready, head
movement allowed, head loaded, interrupt-enabled, track-zero, and new-read-
byte state in their documented bit positions. The selected board revision
fixes otherwise unused-bit levels. Sector-position input reports sector
number 0–31 in bits 1–5 and active-low sector-true in bit 0. Deselecting the
controller or reading sector position without a loaded head returns the
declared no-drive value rather than inventing sector zero.

A port-`0x09` write is one complete controller command byte. Several asserted
bits take effect in the same I/O transaction and in documented ordering;
the interface does not decompose them into generic step or motor methods.
Port-`0x0a` reads consume the current data latch and participate in the real
ready/overrun behavior. `inspect` performs none of those read side effects.

### Time, polling, and the design stress test

One `AltairTick` is one 8080 T-state under the selected machine timing
profile. The original controller presents one parallel data byte roughly
every 32 microseconds. Rotation, sector-hole passage, sector-true width,
byte assembly, readiness, latch replacement, and head movement all advance
from elapsed time, never from how often software reads a port.

The claimed polled controller has no autonomous CPU line, so
`next_event_tick()` returns `None`. This does not suspend the hardware.
`advance_to` and every later `interact` advance all internal state to the
requested tick. A sector-true window can pass between polls, and delayed
data reads can exercise the controller's real latch or overrun behavior.
Only a future contract which claims an interrupt output would return that
line transition as a schedulable event.

This is the main pressure test for the common interface. It proves that a
deadline means “an outward effect the caller must apply,” not “every internal
state change that could affect a later read.” No read-count-driven rotation,
special polling callback, or second clock API is needed.

### One complete raw-flux sector read

After reset, guest firmware selects drive 0 through port `0x08`, loads the
head through port `0x09`, and waits until head and movement status permit a
read. It polls port `0x09` until the requested sector number is present with
sector-true asserted. That value comes from the modeled 32-hole marker
channel and rotational phase, not from a sector counter advanced by the
poll itself.

The firmware then polls port `0x08` for each active-low new-byte indication
and reads the corresponding byte from port `0x0a`. Remanence recovers and
assembles those bytes from the selected track's active flux state. After 137
successful byte transfers, guest software interprets the record according
to the Altair disk format; Remanence does not substitute a CHS-sector read
inside this hardware journey.

The SCP flux and its capture evidence remain below the controller. The
32-sector marker topology is a separate P22 sensor channel declared by the
media/mechanism profile. If the capture does not preserve each hole's exact
timing, any regular marker timing supplied by the profile is fabricated and
retains that provenance; it is never described as captured evidence.

### Reference-emulator comparison

Open SIMH's current AltairZ80 controller exposes the same three ports and
documents the Pertec FD-400, sixteen-drive selection, 77 tracks, 32 hard
sectors, and 137-byte records. Its sector-position shortcut advances and
alternates sector-true in response to reads. That is useful functional
emulation but is deliberately insufficient for this journey: a Remanence
retrofit routes the three port cycles to `interact` and derives sector and
byte state from elapsed time and the active flux/marker state.

The retrofit succeeds when unchanged Altair boot or disk code reads the raw
capture, a deliberately slow polling test can miss a sector-true window or
lose data according to modeled hardware behavior, selecting an empty drive
returns the declared no-drive state, and no sector, byte-stream, hard-sector,
or flux callback is added beside the common hardware interface.

### Deliberately outside this use case

- Executing the Altair CPU, memory, boot ROM, operating system, or disk
  software.
- DMA or a claimed interrupt-driven transfer path.
- Altair Minidisk, later third-party controllers, soft-sector substitutions,
  or non-Pertec mechanism profiles.
- Writable media, formatting, or persistence; write-controller behavior is
  modeled only as required for faithful read-side state.
- Hot insertion, ejection, save states, sound, and front-panel presentation.
- Treating a sector-only Altair image as captured flux or captured
  hard-sector timing.
- A standalone public SCP, marker, recovered-bit, controller-byte, or CHS
  interface inside this hardware journey.

## U13 — I read a file from an offline Windows 11 VHDX

I have `<machine-name>.vhdx`, a self-contained virtual hard disk containing
a complete, unencrypted Windows 11 installation. It uses the ordinary UEFI
layout: a GPT with an EFI System Partition, a Microsoft Reserved Partition,
the main NTFS Windows partition, and a Windows Recovery partition. I want to
read `C:\Users\Paul\Documents\example.txt` without attaching the VHDX to
Windows, booting the guest, choosing a partition number, or pretending that
the largest NTFS partition is necessarily `C:`.

I open the artifact read-only and ask for the uniquely evidenced offline
Windows namespace. Remanence opens the VHDX as logical-block media, applies
the GPT partition schema, forms the applicable volumes, recognizes their
filesystems, recognizes the Windows installation, reconstructs its persisted
drive-letter mapping, and returns the file through the P19 file-container
interface.

### The exact semantic interface

The smallest useful public operation is one ordinary file read after opening
the composed namespace:

```rust
let artifact = Artifact::open(
    windows_vhdx,
    AccessIntent::Read,
)?;

let windows = artifact.open_file_container(
    FileContainerTarget::WindowsInstallation {
        selection: Selection::Unique,
    },
)?;

let requested_path = WindowsPath::parse(path_from_caller)?;
let contents = windows.read_file(requested_path)?;
```

The names are semantic pseudocode, not a pledge of literal Rust layout.
The C and Python presentations preserve the same two-stage operation: open a
uniquely identified Windows file-container view, then read a path from it.
They do not expose a generic untyped layer bag or make the caller repeat
library-assigned device identities.

`Artifact::open` takes the P7 claim for the VHDX and every required backing
artifact for the open lifetime. This journey uses a standalone VHDX with no
parent. Read intent protects the host artifact and every durable layer from
mutation.

### The composed interpretation

The successful composition is:

1. The VHDX image-format adapter validates the VHDX structures and exposes
   one geometry-opaque logical-block device. Block is the P23 active durable
   layer; VHDX allocation, metadata, and logging are its artifact encoding.
2. The GPT partition-schema adapter validates the primary and backup GPT,
   reconciles their evidence, and exposes addressed regions with their GPT
   identities, type GUIDs, attributes, names, and bounds.
3. Volume composition offers direct volumes only where the region semantics
   permit one. The EFI System Partition's direct volume may expose a FAT
   file-container view; the Windows and recovery direct volumes may expose
   distinct NTFS file-container views. The Microsoft Reserved Partition
   remains a valid region with no invented volume, filesystem, or
   file-container result.
4. The NTFS adapter exposes each recognized NTFS volume through its own P19
   file-container view. Those views remain distinct; neither GPT order,
   partition size, filesystem label, nor the Basic Data type GUID assigns a
   Windows drive letter.
5. A Windows-installation namespace adapter identifies candidate installations
   from claimed filesystem evidence, reads the selected installation's
   offline SYSTEM registry hive, and interprets the Mount Manager's persistent
   `MountedDevices` mappings. It correlates the evidenced `\DosDevices\C:`
   identity with one discovered volume and maps that volume's file-container
   view at `C:`.
6. The composed Windows namespace is another P19 file-container view. It
   resolves the requested Windows path under the mapped `C:` root and asks the
   underlying NTFS view for the file bytes.

Every layer and interpretation remains reportable. The result identifies the
VHDX artifact, block device, GPT schema and partition, direct volume, NTFS
filesystem, Windows installation, `C:` mapping evidence, and final file. The
logical path does not erase the EFI, MSR, recovery, or unallocated regions.

### `C:` is namespace evidence, not storage geometry

GPT establishes partition regions and their schema-owned identifiers. NTFS
establishes a filesystem namespace on a volume. Neither establishes a drive
letter. `C:` belongs to the Windows namespace composed above those
file-container views.

If the SYSTEM hive is absent, unreadable, or inconsistent; its persisted
identifier cannot be correlated with a discovered volume; or several Windows
installations remain equally supported, `Selection::Unique` returns a named
ambiguity or refusal with the competing evidence. Remanence does not fall
back to the first Basic Data partition, the largest NTFS volume, a volume
label, or the presence of a `Windows` directory alone.

The caller may separately request a specific GPT region, volume, filesystem,
or file-container view by an identity already reported by Remanence. That is
explicit lower-level selection, not an alternate meaning of `C:` and not a
requirement for this transparent unique journey.

### What “container” means here

This journey does not form a hierarchy of generic containers:

- the VHDX is an image artifact encoding a logical-block device, not a file
  container;
- GPT is a partition schema which exposes addressed regions, not a partition
  container;
- a partition is a region and a volume is addressed logical storage; neither
  is a file container merely because another adapter may interpret it;
- NTFS supplies a file-container view over one volume; and
- the Windows namespace adapter composes one or more file-container views
  into another file-container view whose roots include drive letters.

ZIP reaches the same P19 result by a different route: its serialized named
entries decode directly into file-container state without disk media, GPT,
volumes, or a filesystem. Conversely, selecting a VHDX file from a ZIP
creates a separate image instance whose inner composition follows this
journey; it does not turn the outer ZIP into disk media.

### Completion and refusals

The journey succeeds when the exact bytes of
`C:\Users\Paul\Documents\example.txt` are returned through the composed
file-container interface and the layer report accounts for every selection
and mapping without a caller-supplied partition number or device identity.

Recognition remains evidence-bearing throughout. A known but unsupported
VHDX feature, invalid GPT, unsupported NTFS feature required by the requested
file, corrupt registry hive, missing file, or ambiguous Windows installation
is reported at the owning seam. A later weaker interpretation never hides a
stronger known refusal.

### Deliberately outside this use case

- Booting or emulating Windows, Hyper-V, UEFI, a storage controller, or disk
  hardware.
- Writing the VHDX, filesystem, registry, namespace mapping, or file.
- VHDX differencing chains, checkpoints, live host attachment, or concurrent
  mutation by another process.
- BitLocker, EFS, guest credentials, ACL-policy emulation, reparse points that
  escape the composed namespace, and network shares.
- Guessing `C:` from partition order, size, GPT type, NTFS label, or directory
  names.
- Treating the ESP, MSR, Windows, and recovery regions as children of a
  generic partition-container interface.

## U14 — I read a file from a striped volume assembled from several images

I have `<member-a>.vhdx` and `<member-b>.vhdx`, two virtual disks captured
from the same machine. Each image contains several partition regions, and one
region from each image contributes to the RAID-0 volume I want. The images do
not carry enough trustworthy metadata for Remanence to choose those regions
or recover the stripe layout automatically. I know which image occupied
`hdd0` and `hdd1`, which reported regions are the members, their order, and
the stripe-unit size. I want to compose the read-only volume, recognize its
NTFS filesystem, and read
`\Shared\example.dat` without materializing a reconstructed disk image.

I open the artifacts together and inspect their combined layered report. I or
my GUI select one reported partition region from each artifact and provide
the indispensable layout facts which the artifacts do not establish.
Remanence validates that typed recipe, presents one logical volume over the
selected regions, recognizes NTFS, opens its P19 file-container view, and
returns the file.

### The exact semantic interface

The smallest useful operation remains one ordinary file read after composing
and opening the filesystem view:

```rust
let artifacts = ArtifactSet::open(
    [
        AttachedArtifact {
            attachment: Attachment::Hdd(0),
            source: first_vhdx,
        },
        AttachedArtifact {
            attachment: Attachment::Hdd(1),
            source: second_vhdx,
        },
    ],
    AccessIntent::Read,
)?;

let report = artifacts.inspect()?;
let first_member = choose_partition_from_report(&report)?;
let second_member = choose_partition_from_report(&report)?;

let volume = artifacts.open_volume(
    VolumeTarget::Constructed(
        VolumeRecipe::Striped {
            members: [first_member, second_member],
            stripe_unit: stripe_unit_from_caller,
        },
    ),
)?;

let files = volume.open_file_container(
    FileContainerTarget::Filesystem {
        selection: Selection::Unique,
    },
)?;

let requested_path = FileContainerPath::parse(path_from_caller)?;
let contents = files.read_file(requested_path)?;
```

The names are semantic pseudocode, not a pledge of literal Rust layout.
The C and Python presentations preserve the same stages: associate every
source with its typed attachment placement, atomically open the artifact set,
inspect it, retain typed references to the chosen regions, construct a
read-only volume from a typed recipe, and read from its filesystem
file-container view.

`ArtifactSet::open` takes one P7 claim for every supplied artifact as one
atomic open operation. It either acquires the complete read-only set or
returns no usable composition. Every source has one caller-supplied,
composition-unique attachment identity such as `hdd0` or `hdd1`; the array
order is not attachment order, member order, stripe order, boot order, or
precedence. Remanence separately assigns each resulting device its opaque,
composition-scoped P21 identity. The attachment says where the caller asserts
the image belonged in the stopped machine; it does not become device identity
or evidence recovered from the image.

The member array inside `VolumeRecipe::Striped` is different: its order is
an explicit volume-topology assertion and is not implied by `hdd0`/`hdd1`.
Each member is a typed region reference from this artifact set's report, so
the caller does not restate source identities, partition numbers, offsets, or
lengths. `stripe_unit_from_caller` is also an assertion. The resulting report
distinguishes attachment placement, volume-recipe assertions, and facts
discovered and validated from the images.

### The composed interpretation

The successful composition is:

1. Each VHDX image-format adapter independently validates its attached
   artifact and exposes one geometry-opaque logical-block device. The report
   retains the caller's `hdd0`/`hdd1` placement separately from the assigned
   device identity. Each device retains its own block active durable layer
   under P23; opening the set does not merge the two byte address spaces by
   attachment or file order.
2. Each partition-schema adapter exposes the regions described by that
   device's GPT or MBR. A region remains owned by its source device; the
   report's typed region reference identifies that association without
   treating a partition number as cross-device identity.
3. The caller selects the two intended regions from the report. Selection
   answers which addressed storage participates; it does not itself say how
   those bytes form a volume.
4. The caller supplies the missing RAID-0 facts through
   `VolumeRecipe::Striped`: member order and stripe-unit size. Region bounds
   supply each member's offset and available length. The volume-composition
   adapter validates the recipe, derives the usable logical length, and
   refuses values which cannot describe a safe complete mapping.
5. The composed volume presents one logical-block address space. For each
   requested range it maps successive stripe units across the members in the
   asserted order, splits reads at stripe boundaries, reads the required
   ranges from the VHDX-backed block devices, and reassembles the bytes in
   logical order. The mapping is a live volume view; it is not a third
   durable layer and creates no reconstructed image in memory or on disk.
6. The NTFS adapter consumes that single logical volume and exposes one P19
   file-container view. Neither NTFS nor the file-container view needs to know
   which member supplied a byte, while provenance can trace every returned
   range through the asserted stripe map to the contributing artifact and
   region offsets.

This is volume composition, not partition composition. The selected
partitions remain regions on their respective block devices. The constructed
striped volume is the addressed logical storage which NTFS consumes.

### Discovered topology and asserted topology remain distinct

Remanence first tries to recognize volume metadata capable of establishing a
topology. When trustworthy metadata uniquely identifies the members, their
order, the stripe unit, and applicable bounds, the caller may instead request
an evidence-derived volume and select among the recognized candidates. The
adapter owns those format-specific facts.

This journey exercises the other necessary path: required topology evidence
is absent, unsupported, damaged, or external to the image set, but the user
knows the layout. A constructed volume is therefore an explicit
interpretation under caller-supplied assumptions. Its report records:

- which facts were read from each image and partition schema;
- which region references the caller selected;
- which layout values the caller asserted;
- which bounds, arithmetic, and downstream signatures Remanence validated;
  and
- which conclusions remain conditional on the assertions.

Successful NTFS recognition is corroborating evidence, not retroactive proof
that the recipe is historically correct. Another member order or stripe unit
may occasionally produce plausible bytes; Remanence does not search recipes
until one happens to parse, silently upgrade a caller assertion into detected
fact, or call the first readable result unique.

Selection and construction are therefore separate interface concepts.
Selecting regions limits the addressed inputs. Constructing the volume states
the relationship between those inputs. This separation applies equally when
an automated adapter discovers the relationship: it produces the same volume
model and provenance, but with evidence-derived rather than caller-asserted
topology.

### Completion and refusals

The journey succeeds when the exact bytes of
`\Shared\example.dat` are returned through the composed filesystem
file-container view. The layer report accounts for both VHDX artifacts, both
caller-supplied attachment placements, both block devices and partition
schemas, the two explicitly selected regions, the asserted member order and
stripe unit, the validated stripe mapping, the composed volume, NTFS, and the
final file.

A region reference from another or expired artifact set, duplicate member,
missing or duplicate attachment identity, missing artifact, zero or
unsupported stripe unit, unusable member length, arithmetic overflow,
out-of-range member read, incomplete final stripe under the recipe's declared
length policy, invalid NTFS, unsupported NTFS feature required by the read,
invalid rooted path, or missing file is reported at the owning seam.
Remanence does not substitute zeroes, infer attachment or member order from
the artifact-open call, concatenate the regions, or scan layout values until
the filesystem appears readable.

A failure report retains evidence discovered independently from each usable
artifact and identifies the rejected selection, recipe, or downstream
interpretation. It never presents a partial RAID-0 volume or file-container
view as successful.

### Architectural consequence while proposed

This is the concrete multi-device-volume use that D5 required before the
architecture acquired multiple-source topology. U14 proposes the minimum
new surface: one atomic `ArtifactSet` open with one required typed attachment
identity per source; typed region references which remain unambiguous across
that set; and one volume-composition interface which accepts either a
recognized topology or an explicit, typed caller-supplied recipe. Both routes
produce the same volume abstraction and preserve the provenance of every
discovered and asserted topology fact.

The required `hdd0`/`hdd1` attachment placements are caller-authored topology,
not global device identities and not RAID member order. U14 does not propose
a generic topology graph, heuristic recipe search, or cross-source write
transactions. Manual construction is not a back door to raw byte ranges: its
members are regions Remanence already discovered and reported.

While U14 remains proposed, D5's deferral continues to bind the pledged and
in-force architecture. Pledging U14 requires its dedicated surface design
and the corresponding narrowly scoped amendment to P17; this draft does not
smuggle either into U7.

### Deliberately outside this use case

- Writing either VHDX, repairing a member, or coordinating a commit across
  artifacts.
- Creating or persisting a disk-format-specific volume database; the
  constructed volume exists only for this read-only open.
- Automatically guessing member regions, member order, stripe-unit size, or
  logical length from coincidentally readable output.
- LDM, Storage Spaces, MD RAID, LVM, ZFS pools, mirrored volumes, RAID-5, and
  spanned volumes. Their adapters may later exercise the same multi-device
  seam, but this journey promises only an explicit two-member RAID-0 recipe.
- VHDX differencing chains, checkpoints, live host attachment, or concurrent
  mutation by another process.
- BitLocker, EFS, Windows drive letters, Windows installation recognition,
  guest credentials, ACL-policy emulation, and network shares.
- Inferring topology from artifact order, filenames, equal sizes, matching
  partition numbers, filesystem signatures, or coincidentally readable data.
- Persisting the composed volume as a synthesized raw, VHDX, or other image.

## U15 — I read a file from a selected partition in a Windows data VHDX

I have `<data-disk>.vhdx`, a virtual disk removed from a Windows machine.
It contains an NTFS partition and the file
`\Users\george\mydocument.txt`, but it does not contain the Windows
installation whose registry recorded how the machine mounted this volume.
From this artifact alone Remanence cannot determine whether Windows called
the volume `D:`, `E:`, mounted it beneath a directory, or left it
unmounted. I still want to inspect the disk, select the partition I mean, and
read the file relative to that partition's filesystem root.

I open the VHDX read-only and inspect its layered report. Remanence opens it
as logical-block media, applies the partition schema, and reports the
partition regions and the evidence associated with each. I or my GUI choose
one reported region. Remanence then forms the applicable direct volume,
recognizes NTFS, opens its P19 file-container view, and reads the rooted path
without inventing a Windows drive letter or requiring a SYSTEM hive.

### The exact semantic interface

The smallest useful operation is one ordinary file read after an explicit
selection from the artifact's own report:

```rust
let artifact = Artifact::open(
    data_vhdx,
    AccessIntent::Read,
)?;

let report = artifact.inspect()?;
let selected_region = choose_partition_from_report(&report)?;

let files = artifact.open_file_container(
    FileContainerTarget::Filesystem {
        storage: StorageSelection::Region(selected_region),
        selection: Selection::Unique,
    },
)?;

let requested_path = FileContainerPath::parse(path_from_caller)?;
let contents = files.read_file(requested_path)?;
```

Here `path_from_caller` is
`\Users\george\mydocument.txt`. The names are semantic pseudocode, not a
pledge of literal Rust layout. The C and Python presentations preserve the
same sequence: inspect, retain one reported region reference, open the
uniquely recognized filesystem file-container view on that region, and read
a path rooted in that view.

`choose_partition_from_report` is deliberately caller or frontend policy,
not a Remanence heuristic. A GUI may show the GPT name and unique-partition
GUID, bounds, size, type, filesystem evidence, and warnings; a program may
match whatever evidence is meaningful to its own task. Its result is the
typed region reference issued by this open artifact, not a caller-constructed
partition number, byte range, or device identity.

### The composed interpretation

The successful composition is:

1. The VHDX image-format adapter validates the VHDX structures and exposes
   one geometry-opaque logical-block device. Block is the P23 active durable
   layer; no host mount or Windows virtual-disk attachment occurs.
2. The applicable GPT or MBR partition-schema adapter exposes addressed
   regions. The report identifies which regions are schema partitions and
   carries their persisted identifiers and attributes as evidence.
3. The caller selects one of those report results by its typed region
   reference. The reference is scoped to the open artifact and names the
   exact device, schema interpretation, start, and length Remanence reported.
   Passing it back does not make the caller restate or validate those facts.
4. Volume composition determines whether the selected region directly forms
   one logical volume. This journey requires that direct mapping; it does not
   guess at a multi-region or multi-device volume from matching signatures.
5. The NTFS adapter consumes the selected volume and exposes one P19
   file-container view rooted at the NTFS root directory.
6. The file-container view resolves the caller's rooted path beneath that
   root and returns the file bytes.

The composition ends at the filesystem file-container view. No
Windows-installation namespace adapter runs, and no drive-letter root is
placed above it. In this view `\Users\george\mydocument.txt` means the
named path from the selected NTFS root; it makes no assertion that the full
path on the source machine was
`C:\Users\george\mydocument.txt`,
`D:\Users\george\mydocument.txt`, or any other mount path.

### Unknown drive letter is an honest result

A partition schema identifies regions. A volume adapter identifies addressed
logical storage. NTFS identifies its own filesystem and namespace. None of
those layers assigns a Windows drive letter.

The persistent mapping between a Windows mount name and a volume belongs to
the Windows installation which made that assignment. In U13 the selected
installation's SYSTEM hive supplies that evidence. This data-only artifact
does not contain that authority, so its layered report states that no Windows
namespace mapping was established. An NTFS volume label, GPT name or type,
partition order, filename, or familiar directory tree is not promoted into a
drive-letter claim.

That absence does not weaken the lower result. The selected region, direct
volume, NTFS filesystem, and file-container view can each be conclusive even
when the higher Windows namespace is unavailable. The caller explicitly
asked for a path in the filesystem root, so the file read needs no substitute
drive letter.

If the caller later supplies the corresponding Windows system artifact, it
may open the artifacts together and request a Windows-installation namespace.
That is a new composition with new evidence, not a mutation or relabeling of
this already-open file-container view.

### Completion and refusals

The journey succeeds when the exact bytes of
`\Users\george\mydocument.txt` are returned from the explicitly selected
partition's NTFS file-container view. The report accounts for the VHDX
artifact, block device, partition schema, selected region, direct volume,
NTFS filesystem, and file, and explicitly makes no drive-letter claim.

A region reference from another artifact or expired open, a selected region
which cannot form the requested direct volume, an invalid partition extent,
a known but unsupported VHDX or NTFS feature required by the read, ambiguous
filesystem recognition, an invalid rooted path, or a missing file is reported
at the owning seam. Remanence does not silently select a different partition,
scan other partitions for a matching pathname, or reinterpret a failed NTFS
selection as evidence for a Windows drive letter.

Selecting by durable GPT identity may be offered as caller policy only when
that identity resolves uniquely within this artifact's report. Duplicate,
missing, or corrupt persisted identifiers require the caller to choose among
the separately reported region references; Remanence does not repair the
ambiguity by falling back to partition order.

### Deliberately outside this use case

- Determining the source machine's drive letter, mount folder, volume GUID
  path, boot role, user-profile location, or Windows installation identity.
- Searching every partition for a pathname and choosing the first match.
- Opening a Windows namespace, interpreting a registry hive, or correlating
  this volume with separately supplied system artifacts.
- Multi-device or multi-region volume composition, including LDM, Storage
  Spaces, spanning, striping, mirroring, and RAID.
- Writing the VHDX, NTFS volume, directory, or file.
- VHDX differencing chains, checkpoints, live host attachment, or concurrent
  mutation by another process.
- BitLocker, EFS, guest credentials, ACL-policy emulation, reparse points that
  escape the selected filesystem view, and network shares.
- Treating the VHDX, partition schema, selected region, or volume as a generic
  file container.

## U18 — I use one mixed-mode CD through optical and block presentations

I open one mixed-mode compact-disc image and use the same recorded disc in
two honest ways. An emulator exercises the disc through the common optical
drive seam: it reads the table of contents and subchannels, reads raw or
decoded data sectors, and plays audio tracks with causal timing. A filesystem
tool selects an eligible data track from that same disc, views its user-data
sectors as logical blocks, and reads files from the filesystem recorded
there. The audio tracks, pregaps, indexes, subchannels, and other optical
state remain present even though they have no block presentation.

My source may be a compound BIN/CUE or CCD/IMG/SUB image, or a single-file
optical image such as CHD or Aaru Image Format. The recognizing image adapter
claims every source artifact and declares the optical information it actually
records. Persistent files and descriptor syntax are encodings of the image,
not file-container entries and not additional mutable copies of the disc.

The durable optical state preserves, at the fidelity supplied by the source:

- ordered sessions, tracks, indexes, pregaps, postgaps, lead-in and lead-out
  facts visible at the applicable optical-drive seam;
- each track's declared mode and the disc-relative address of its frames;
- the main-channel frame payload, including raw 2,352-byte CD frames when
  present;
- the P–W subchannels, including raw 96-byte subchannel frames when present;
  and
- per-field evidence distinguishing captured, declared, decoded, synthesized,
  patched, ambiguous, invalid, and absent information.

Those facts form one P24 optical-active media instance. Main channel and
subchannels are parallel recorded channels of that instance, not separate
active layers. A sparse sidecar such as SBI is an evidence-bearing overlay on
selected Q-channel frames; applying it does not relabel patched values as
captured evidence. Read retries, C2 reports, drive offset, and dump logs remain
capture provenance rather than deterministic recorded bytes.

The emulator opens a typed optical hardware presentation over that state. Its
contract may expose the applicable command, track, sector, audio, and
subchannel behavior—for a CD-family presentation, operations corresponding to
TOC inspection, raw and cooked reads, subchannel queries, seeking, and audio
playback. Remanence advances command completion and audio observations through
P15 timed causality. Pickup position, playback cursor, seek continuation, and
pending command effects are ephemeral hardware state; they never become image
or media layers. Remanence does not infer pits, lands, EFM channel bits, analog
RF, a laser pickup, focus or tracking servos, firmware, or microcode beneath
the common drive-visible floor.

Separately, I select a reported data track whose mode defines a logical
user-data sector view. Remanence exposes only that eligible extent as a derived
block presentation with its declared logical block size—for example, 2,048
bytes for an ordinary Mode 1 CD-ROM data track. The block address starts in
the selected presentation; it is not silently equated with the whole disc's
absolute frame address. Mode decoding and EDC/ECC treatment belong to the
optical-family derivation and remain visible in its evidence and refusals.

A track is not a partition merely because it bounds a block presentation.
The selected block extent may form a volume, and a filesystem adapter may
recognize ISO 9660 or another applicable filesystem on that volume. Audio
tracks and optical-only regions form no fake blocks, partitions, volumes, or
files. Failure to recognize the data track's filesystem does not erase the
track or its optical evidence.

Both presentations observe one active state. A writable higher presentation
may alter a file or data sector only when its changes project through the data
track encoding into the optical frames without flattening or disturbing audio
tracks, subchannels, gaps, indexes, or unsupported structures. An optical
write changes that same state, so a later block or filesystem read observes
the representable result. P2 commit succeeds only when the source image
adapter can encode every change honestly; otherwise writable composition is
refused in advance or I explicitly convert to a richer optical image.

An ISO file is the useful converse case. When I request only ordinary data
access, it may remain block-authoritative and block-active and support volume,
filesystem, and file presentations without inventing a CD. If I explicitly
attach it through an optical-media composition, Remanence performs one atomic
**generate-optical** transition under a selected optical profile and mastering
policy. It constructs the most honest data track, raw frames, layout, and
subchannels those inputs justify, marks every manufactured fact synthetic,
and makes optical the active layer before optical hardware service begins.
This creates a new synthetic optical recording; it does not recover audio
tracks, protection data, original mastering choices, damage, or subchannels
which the ISO never contained. A generic hard-drive block device never enters
this path merely because its bytes happen to resemble an optical filesystem.

The journey succeeds when the emulator can boot or read the data track and
play the audio tracks through its optical command seam, while a higher tool
can independently read the eligible data-track filesystem without flattening
the rest of the disc. Reports preserve the identity and provenance joining
the optical disc, tracks, derived block extent, volume, filesystem, source
artifacts, and any overlay. Ambiguity or damage at one seam remains there and
does not manufacture a simpler whole-disc block device.

### Deliberately outside this use case

- Treating the whole mixed-mode disc as one geometry-opaque block device.
- Presenting audio tracks, gaps, lead-in, lead-out, or subchannels as blocks,
  partitions, volumes, or files.
- Claiming that BIN/CUE contains raw subchannels which only an accompanying
  source or explicit overlay supplies.
- Claiming that CCD/IMG/SUB, CHD, Aaru Image Format, or another encoding has
  identical fidelity without inspecting its actual recorded channels and
  provenance.
- Reconstructing original audio, protection, damage, subchannels, mastering,
  sessions, or physical manufacture from an ISO.
- Pits, lands, EFM channel-bit capture, or analog RF within this CD journey;
  U19 owns the distinct LaserDisc signal case. Laser, focus, tracking,
  spindle-servo, firmware, and microcode emulation remain absent.
- Assuming that the CD-family frame schema is a universal representation for
  DVD, Blu-ray, magneto-optical, or every later optical family.

## U19 — I preserve and use one LaserDisc through signal, player, and data presentations

I open one LaserDisc-family image whose best durable evidence may be either a
raw RF capture or an already-decoded audio/video program. Remanence preserves
the source at its honest P24 optical seam and lets an emulator exercise the
same title through a timed player boundary. If the title is LV-ROM, a data
tool can also read the digital data mapped into the applicable program
channels without flattening or discarding the analog video program.

For a raw `.lds`, `.ldf`, or equivalent capture, the optical signal
representation is authoritative and active. It preserves the sampled RF,
sample clock, capture-chain provenance, discontinuities, conflicting or weak
observations, and every distinction needed to decode the recording again.
Video frames, analog or digital audio, vertical-blanking information, frame
and chapter addresses, and LV-ROM data are evidence-bearing derived views.
They do not become a second mutable program copy and are not promoted to
literal pits, lands, or original surface state.

For a decoded LaserDisc CHD or equivalent image, the family-owned recorded
program representation is authoritative and active. Remanence preserves the
video, supplied audio channels, timing, addressing metadata, and digital-data
mappings the source actually contains. It reports absent RF and capture
evidence as absent; it never fabricates them to make the decoded source look
like a raw capture.

The emulator opens a typed LaserDisc player presentation over either source.
The presentation supplies a family-appropriate command and output seam—such
as a named F-code, serial, or SCSI-capable player profile—while P15 owns the
common timed-causality lifecycle. Seeking, play, still, reverse or variable
play where supported, frame or chapter observation, audio/video output, and
pending command completion advance against one causal clock. Pickup position,
CAV or CLV rotational phase, continuation state, and controller progress are
ephemeral hardware state and are never written into the image.

For an LV-ROM title, I select a reported digital-data mapping whose bytes
replace or occupy declared program channels over declared extents. Remanence
opens a bounded block presentation with an explicit block size, address map,
and derivation evidence. The mapping is neither a partition nor proof that the
whole disc is a block device. Analog video and every unmapped audio, video,
blanking, and control region remain present and have no invented blocks.
Volumes, filesystems, and files may compose above only that eligible view.

The journey succeeds when a raw capture remains independently re-decodable,
when either source can drive the same player-facing emulator lifecycle at the
fidelity it actually supports, and when LV-ROM data can be read while the
video program remains observable. Reports join source artifacts, active
representation, decoded observations, player profile, program channels, data
mapping, derived blocks, and any recognized volume or filesystem without
claiming stronger evidence than the source supplies.

### Deliberately outside this use case

- Reconstructing original RF, capture defects, or physical mastering from a
  decoded audio/video image.
- Treating sampled RF as literal pits, lands, surface geometry, or pickup
  behavior.
- Making the whole LaserDisc or all program channels into one block device.
- Treating an LV-ROM channel mapping as a partition merely because it bounds
  digital data.
- Persisting pickup position, CAV/CLV phase, seek continuation, playback
  direction, or pending player commands.
- Laser, focus, tracking, spindle-servo, pickup electronics, firmware, or
  microcode emulation.
- Mastering or writing LaserDisc, or synthesizing plausible RF from decoded
  program content.
- One command protocol or player profile presented as universal to every
  LaserDisc-family machine.

## U20 — I open images recursively embedded in other artifacts

I open a ZIP archive containing a compact-disc image. Remanence identifies the
ZIP as a file container, reports the selected entry as a possible child
artifact, and opens that entry through the applicable optical image adapter.
The ZIP remains the parent file-container state while the disc has its own
optical-active state and all source claims and provenance remain joined.

Inspection of the disc reports every recognized way its content can supply a
further child artifact. An El Torito boot catalog may map a bounded floppy,
hard-disk, EFI-system-partition, or load-image extent without requiring that
extent to be a named ISO file. Separately, the disc's filesystem may contain a
named `disk.p64` file. A file entry and a boot-catalog extent are different
mapping mechanisms but use the same child-opening operation.

I select one reported mapping rather than asking Remanence to guess which
nested object matters. Selecting the boot floppy opens an independent child
disk at the active layer its evidence supports. Selecting `disk.p64` opens a
flux-active child through the P64 adapter. Either child may expose its own
partitions, volumes, filesystems, files, or further artifact mappings. There
is no format-family depth baked into the model; explicit resource limits bound
traversal, and there is no format-specific orchestration branch.

Each report preserves the complete path from root artifact to selected child:
the ZIP entry, optical disc and data track, filesystem file or boot-catalog
entry, exact byte mapping, image recognition evidence, authoritative layer,
active layer, and higher presentations. A mapping reports a candidate source;
it does not claim that its bytes form a recognized image until a P12 adapter
does so.

If two paths name the same bytes—for example, an El Torito boot image which
also appears as a file—Remanence reports the alias rather than manufacturing
two independent mutable children. A writable open shares one child state or
is refused before mutation. On commit, a representable child result projects
outward through every mapping and parent adapter atomically; failure at any
seam writes none of the root artifacts.

The journey succeeds when I can navigate ZIP → optical image → boot image or
filesystem file → nested disk using stable reported identities and the same
selection operation at every step, while each independently mutable instance
retains exactly one active layer and its honest provenance.

### Deliberately outside this use case

- Treating every byte range, file, partition, or volume as a disk image
  without recognition evidence.
- Automatically descending every candidate, choosing the deepest object, or
  selecting a boot entry or operating system on the caller's behalf.
- Treating an artifact mapping itself as a container, partition, volume,
  filesystem, durable layer, or recognized child image.
- Maintaining independently mutable children over aliased or overlapping
  parent bytes.
- Flattening the graph into one synthetic filesystem or one universal block
  address space.
- Bypassing an image adapter because a filename extension or parent standard
  suggests what the child probably contains.
- Partial outward commits when a deeper or enclosing representation cannot
  encode the requested result.

## U21 — I recover a program from a C64 cassette capture

I have a Commodore 64 `.tap` capture and want the files recorded by the
standard KERNAL tape routines. Remanence identifies the TAP version, platform,
video system, time base, and exact ordered pulse observations. Source offsets,
unsupported encodings, truncation, and timing ambiguity remain visible.

I ask for the standard KERNAL interpretation. It derives headers and data from
the pulses and reports each candidate's opaque identity, 16-byte tape name,
header kind, load range, checksum state, copy relationship, issues, and exact
pulse extents. Duplicate names remain separate entries; names and ordering are
not identity.

The standard format records two copies. If both valid copies agree, Remanence
returns their one shared payload and retains both as evidence. If only one copy
is valid, it may recover that payload with an explicit degraded-redundancy
issue. If valid copies disagree, it refuses to choose silently. It never
stores repeated observations as independent snapshots of the cassette.

The successful interpretation supplies a flat P19 file container. I select an
entry by opaque identity and read its exact recorded data bytes. A program's
load address remains metadata from the header; Remanence does not prepend a
synthetic two-byte PRG address to the extracted payload.

The Rust journey has this semantic shape:

```rust
let mut tape = C64Tape::open(tap_path, AccessIntent::Read)?;
let report = tape.inspect()?;
let kernal = report.kernal_files().ok_or(Error::NoFileContainer)?;
let selected = choose_file(kernal.files())?;
let mut files = tape.open_files(kernal.container().id())?;
let contents = files.read(selected.entry_id())?;
```

`C64TapeReport` owns the pulse and decoder evidence. `C64KernalFileSetInfo`
and `C64TapeFileInfo` are inspection values; `C64TapeHeaderKind` expresses the
standard header meaning. `open_files` returns the common P19 `FileContainer`
interface rather than a C64-only extraction API. The C ABI uses opaque tape,
report, file-set, and file-container handles with accessor functions; Python
mirrors the same ownership and value vocabulary.

A custom loader remains a valid, inspectable pulse stream even when no KERNAL
file view can be derived. A `.t64` source is a logical file container with C64
metadata, not a pulse-faithful tape capture; it may reuse P19 but does not
acquire a tape active layer merely because its name says tape.

The journey succeeds when I can inspect the original pulses and their issues,
choose a standard-format file without ambiguity, and recover its exact payload
through the same file-container interface used by other rooted containers.

### Deliberately outside this use case

- Physical acquisition, Datasette mechanics, electrical emulation, or motor
  control; the journey begins with a capture.
- Decoding arbitrary fast loaders or copy-protection schemes.
- Writing TAP, synthesizing pulses, or producing T64 or PRG files.
- Treating T64 as evidence of pulse timings or cassette transport structure.
- Returning an end-of-tape header or other structural observation as a file.
- Inventing a payload, copy agreement, boundary, timing fact, or load address.

## U24 — I recover a flippy's second side from one two-head capture

I have a KryoFlux capture of a C64 disk whose second side was written by
flipping it over in a single-sided 1541. I made the capture in one insertion,
in an ordinary two-head PC drive, so the tool wrote both heads at every step
position and I archived the lot. I never flipped the disk.

Head 0 read the surface facing it. Head 1 read the disk's other surface --
which is where the second side lives, recorded while the disk sat the other
way up. The drive spins one direction regardless, so that surface passed head
1 against the direction it was written in: head 1's observation is the second
side, in reverse. Nothing in the capture says so. The stream declares no
track, no side, and no orientation, and the file name carries a head digit
and nothing more.

I ask Remanence what the capture holds. It reports every step position and
head as separate observations with their own evidence, and says which heads
carry a recorded surface at all — a blank head reads as noise that does not
reproduce between revolutions, and that is an observation about the capture,
not a defect in it. It never presents a noise head as a side.

I ask for the disk. The drive profile declares how many surfaces the family
records and how a captured head maps onto one (P30), and for a flipped second
side that mapping includes the reversal: the surface is reached by projecting
the opposite head's observation in reverse rotational order. Remanence
produces both sides as distinct addressable surfaces, each carrying
provenance that names the head it came from and states that the reversal was
applied. A profile that declares no such mapping refuses the request by name
(P3); it does not reverse a stream speculatively to see whether something
decodes, and it does not decide from content that a head "looks like" a side.

The reversal is a declared projection under P29, not a repair and not a
recovery heuristic. The raw observations are unchanged and remain readable as
captured; the reversed surface is derived, and says so. Reversing an
observation that the profile's mapping does not cover is refused rather than
attempted, and a head whose evidence is absent stays absent — no side is
synthesized to fill a mapping that expected one.

The journey succeeds when one insertion yields both sides, each traceable to
the head that recorded it, with the reversal visible as a declared step
rather than assumed.

### What this is worth

A flippy captured this way is ordinarily captured twice, the operator
flipping the disk between passes. That second pass is a second mechanical
handling of aging media, and for a disk that will not survive many more, it
is the expensive part of the journey. The evidence for both sides is already
in the first capture; what is missing is the declaration that says how to
read it.

### Deliberately outside this use case

- Deciding from content whether a surface is recorded; the profile declares
  its surfaces, and recognition reports structure, not meaning (P30).
- Assuming the two heads of a drive read the same radius at the same step
  position; head alignment is a property of the instrument, not a given.
- Establishing that they do from transition counts. The family's write clock
  is per-zone, so a count is a function of the zone a position falls in as
  much as of the radius, and the two cannot be separated by counting.
- Repairing, averaging, or reconciling the two heads' observations against
  each other — they are different surfaces, and a reversal does not make
  them passes over one.
- Reversing a capture whose profile declares no reversal mapping, or
  inferring the mapping from the data.
- Decoding either side to bits, sectors, or files; this journey ends at the
  surfaces.
- Physical acquisition, drive mechanics, or head selection; the journey
  begins with a capture that already holds both heads.
