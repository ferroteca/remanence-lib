<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# USE-CASES (pledged)

> **Status:** pledged at the owner's direction. Every use case here is owed
> by the project and reaches root [USE-CASES.md](../../USE-CASES.md) only
> on full delivery. Numbers come from the one global U-sequence and are never
> reused.

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

## U23 — I save a KryoFlux capture of a C64 disk as a P64 image

I have a KryoFlux capture of a Commodore 64 floppy: raw stream files, one per
drive-step position, captured from both of the disk's sides and delivered
inside 7z archives — the second being the unrecorded back of a single-sided
disk, which the capture cannot tell me and the drive family can. It is capture evidence, not a disk image. Each stream holds
several recorded revolutions, flux before the first index and after the last,
index and control/OOB records beside the flux, and a transfer result — and
nothing in it says which revolution "the" disk was, or which channel to
believe.

I want a P64 out of it: one file, addressed by 1541 half-track, holding timed
pulses with strength, which a 1541 drive-hardware instance opens and turns
back into byte-ready edges (U7). I am asking for a transformation, not a
reading of the capture, and I want to be told exactly what it will do and
exactly what it cannot carry **before** it writes anything.

### The exact semantic interface

The names below are semantic pseudocode, not a pledge of literal Rust layout.
P5 requires the C and Python presentations to carry the same stages, the same
declared-loss account, and the same refusals in their own idiom.

```rust
let capture = CaptureSet::open(
    capture_set_sources,
    AccessIntent::Read,
)?;

let report = capture.inspect()?;
let side = choose_side_from_report(&report)?;

let plan = capture.plan_mastering(
    MasteringRecipe::C1541 {
        side,
        observation: ObservationPolicy::Selected(selection_rule),
        half_tracks: HalfTrackMap::Declared(drive_steps_per_track),
        pulse_strength: PulseStrengthPolicy::FromDisagreement { seed },
    },
    MasteringTarget::P64(p64_options),
)?;

for loss in plan.declared_loss() {
    report_to_caller(loss);
}

let outcome = plan.write_new_artifact(destination_p64)?;
```

`CaptureSet::open` takes the P7 claim on every member artifact of the set for
the operation's lifetime and reads nothing else. `inspect` reports the set as
F31 recognized it — members and their catalog identities, sides, source
track positions, capture runs, observations, markers, transfer results, and
issues — so the recipe names a side and a policy by an identity Remanence
already reported, never by an index the caller invented. The C1541 profile
declares that the family records one surface (P30), so naming the side
confirms a declared fact rather than choosing between two beliefs about one
surface.

`plan_mastering` computes the whole transformation and writes nothing. It
returns the mastered medium's shape, the provenance every part of it will
carry, and the complete declared-loss account. `write_new_artifact` is the
only step that touches the filesystem; it creates the destination under its
own claim, and an existing destination is a named refusal rather than an
overwrite.

### What the transformation is, and what owns each half of it

Two owners, and neither infers the other's answer (P29):

- The **C1541 mastering profile** owns the physical reduction. Which side
  supplies evidence; which observation of a source position is used and how
  several are reconciled; how the set's source drive-step positions map onto
  1541 half-tracks; how each observation's exact `TimeBase` ticks project into
  the destination's rotation-relative timebase, which for a 1541 is the drive's
  16 MHz reference clock across one 300 RPM rotation; and how disagreement,
  weakness, and absence across observations become pulse strength. Every one
  of those is a named policy input. A reduction no policy names is a refusal,
  not a default.
- The **P64 image-format adapter** owns its grammar and its capability claim
  (P12): what the container can hold, the version it claims (P8), how a
  mastered medium encodes into it, and what it refuses by name.

The source stays exactly as it was. The capture remains the authoritative
layer of the artifacts it came from, the set is never edited or consumed, and
the mastered P64 is a separate artifact with its own authoritative layer —
P13's explicit conversion, requested, never a side effect of opening or saving.

### The declared-loss account

P64 cannot carry a KryoFlux capture. That is not a defect of either format,
and it is not something the caller should discover from a smaller file. Before
the write, the plan enumerates every reduction in the source's own terms: the
unselected side; the observations of each position not selected;
flux recorded before the first index and after the last; marker channels and
control/OOB records that have no P64 expression; retained `ForeignRecord`s,
capture metadata, and transfer results; and any timing resolution the
destination's timebase cannot express. A count is not an account, and loss
reported after the fact does not satisfy this.

The saved image says what it is. Its pulses carry selected-and-projected
provenance, not recovered-evidence provenance, and nothing in it is presented
as an observation of the original recording that was not one.

### Reproducibility

The same capture set, the same recipe, and the same seed produce the same
mastered medium, and — the P64 encoding being deterministic — the same
destination bytes. A policy whose variation the profile cannot state is
refused rather than shipped as approximately repeatable.

### One complete conversion

The conformance journey is the prepared Pinball Construction Set disk-one
capture set: both sides, 84 stream members each, opened through
`SevenZipCatalog` and recognized as one capture set by F31. The caller
inspects it, names a side and a selection policy, reads the declared-loss
account, and writes the P64.

The smallest useful success is one mastered half-track: the selected
observation's transitions appear in the saved file at their projected
positions with their assigned strengths, and reopening the result through the
P64 adapter's own decode presents that half-track unchanged. The smallest
complete journey is the whole capture set converted to one P64 which a U7
drive-hardware instance opens, inserts, and reads.

This use case claims that the declared reduction is performed faithfully,
reproducibly, and with its loss named. It does not claim that any particular
protected title loads in an emulator from the result: whether protection
survives is a property of the capture and the chosen policy, and Remanence
reports what it did rather than promising an outcome it cannot see.

### Refusals

An incomplete, duplicate, or contradictory capture set is refused by F31
before mastering begins. Past that: a source position no declared half-track
map covers; a position whose observations disagree in a way the selection
policy does not resolve; a run with no two trustworthy index boundaries where
the policy requires a circular observation; a timebase the destination cannot
express; a mastered medium the P64 claim cannot encode; and an existing
destination path. Each names the rule it broke and leaves no file behind (P6,
P9).

### Deliberately outside this use case

- Recovering GCR, sectors, a filesystem, or files from the capture. Nothing
  in this journey descends below flux or interprets what the pulses mean.
- Writing back to KryoFlux streams, editing the capture set, or any mutation
  of the sources.
- Repairing a bad capture, filling a gap, averaging timings, or choosing a
  cleanest pass in the absence of a declared policy that says so.
- D64, G64, or any destination but P64; disk two of the set; other capture
  containers; and other drive or machine families.
- A public flux, pulse, or capture-run iterator. The transformation is the
  surface; the evidence stays behind it.
- Emulator integration. Producing the image and consuming it (U7) are
  separate journeys that meet at the file.
