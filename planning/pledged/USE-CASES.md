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

## U22 — I present a stopped DOS machine's drives without reimplementing DOS

I automate a stopped DOS machine from the host, and I hold its
configuration: which image sits in which floppy slot, which images are its
hard disks and in what order they are attached, and whether a CD-ROM is
present. I want to show a user the drives that machine's DOS would have
presented — `A:`, `C:`, `D:` — each with the label DOS would have shown,
and then write `A:\OUT\X.TXT` into one of them.

The facts I own are machine configuration: medium, slot, and attachment
order. Every other fact in that sentence is a rule of the format or of DOS
— whether a volume has a label at all, what a file may be called, and which
letter a volume takes — and each is read from the disk by the same library
that reads the disk. Two of the three have since become the library's: I
ask a volume for its label and get one answer, and I hand over the name I
have and get back the rule any refusal broke. The letter is the one I still
re-derive beside it, assigning it from my own copy of the assignment
order — the last of three rules this library already has to know to do its
own job.

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
than filling the gap with an order that happens to look right.

This is not the Windows case. Windows persists its own mapping, and reading
that mapping is U13's and U16's journey rather than this one; DOS persists
nothing, so the mapping is a *rule* applied to machine facts, and the rule
is what has to be named. The
answer therefore states which assignment rule produced it and treats what
the rule cannot settle — a resident driver's letters, a `LASTDRIVE`
ceiling, an assignment a DOS variant makes differently — as undetermined
rather than assumed.

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

### Deliberately outside this use case

- Booting or emulating the guest, DOS itself, its drivers, or its firmware.
- Long file names, VFAT, and FAT32; this journey is the 8.3 namespace.
- Reconstructing a mapping a resident driver, `SUBST`, `JOIN`, `ASSIGN`, or
  a network redirector would have changed at runtime, or inferring one from
  a `CONFIG.SYS` the images may not even hold.
- Claiming every DOS variant's assignment order at once; the applied rule
  is a named claim like any other (P3), and disagreement between variants
  is reported, not averaged.
- Inferring slot or attachment order from filename, array position, or
  image content.
- Guessing a label from a directory name, a filesystem kind, or a file
  inside the volume.
- Repairing a name the caller supplied: a name outside the rules is
  refused, never truncated, transliterated, or renamed to fit.
