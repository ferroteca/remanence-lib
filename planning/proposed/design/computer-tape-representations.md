<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# C64 tape recovery and tape-family seams

Design for [F23](../FEATURES.md#f23--c64-tape-file-recovery-and-tape-family-seams),
serving U21 and proposed P26 alongside pledged P12–P15, P19, P21, and P23.
This is proposed, not implementation approval. Public names remain delivery
surface design.

## Begin with the real journey

The first supported journey is not “read a tape.” It is:

1. open a C64 TAP capture without discarding its pulse evidence;
2. inspect its declared and observed timing facts;
3. derive standard KERNAL header and data blocks;
4. reconcile the two recorded copies without silently choosing between
   conflicts;
5. enumerate recovered logical files; and
6. read one file's exact recorded payload.

This establishes two distinct seams:

```text
C64 TAP bytes -> C64 pulse state -> KERNAL interpretation -> P19 file container
                                 \-> evidence and issues
```

The pulse state is the durable active media representation. The KERNAL
interpretation and P19 container are derived. A failed or unsupported decoder
does not make the pulse capture invalid.

## TAP and T64 are different sources

C64 TAP records timing observations closely enough to preserve custom loaders.
Its adapter reports at least:

```text
C64TapeReport
  TAP version
  machine/platform declaration
  video-system declaration
  time base
  ordered pulse intervals with source extents
  truncation, unsupported encodings, ambiguity, and other issues
  zero or more derived interpretations
```

Version-specific zero encodings and long-pulse forms are normalized only after
their source form and extent are retained. Conversion to durations must be
checked and deterministic. No decoder may round timings and then present the
rounded values as captured facts.

T64 instead stores a directory and logical payloads. It is therefore a P19
file-container adapter with C64 metadata, not a substitute TAP decoder and not
a tape-active representation. Converting between TAP and T64 would be a
generation or interpretation operation, never a lossless relabeling.

## Standard KERNAL interpretation

The initial decoder recognizes only the standard KERNAL tape encoding. It
reports structural observations even when they do not produce a readable file:

```text
C64KernalFileSetInfo
  container identity
  ordered block observations
  logical file candidates
  decoder evidence and issues

C64TapeFileInfo
  opaque entry identity
  raw 16-byte name and display form
  header kind
  start address
  end address
  payload length
  checksum state
  copy relationship
  source pulse extents
  entry issues
```

Header kinds distinguish relocatable program, non-relocatable program,
sequential data, and other standard structural meanings. End-of-tape is a
structural observation, never a `C64TapeFileInfo`.

Names are attributes, not keys. Repeated names and byte-identical programs
remain separate entries unless the decoder has evidence that they are the two
copies of one standard-format recording.

### Redundant copies

The decoder applies one deterministic policy:

- two valid copies with identical decoded header and payload produce one file
  candidate with both source extents retained;
- one valid and one invalid or absent copy may produce one candidate carrying a
  degraded-redundancy issue;
- two valid but different copies produce a conflict and no chosen payload; and
- observations that cannot be paired remain independently inspectable.

This is error-correcting redundancy within one recording, not SCP-style
snapshots of repeated whole-disc reads. The active state retains observations;
the derived file view does not multiply identical payloads.

### Payload meaning

A read returns exactly the bytes encoded as the file's data block. The load
address is header metadata. A PRG export convention that prepends a little-
endian load address would be a separately named transformation and is absent
from U21.

## Rust semantic interface

The concrete entry point owns the media-family behavior:

```rust
pub struct C64Tape { /* active pulse state and source claim */ }

impl C64Tape {
    pub fn open(
        source: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<Self>;

    pub fn inspect(&self) -> Result<C64TapeReport<'_>>;

    pub fn open_files(
        &self,
        container: FileContainerId,
    ) -> Result<FileContainer<'_>>;
}
```

Inspection values expose facts without opening another independent copy of the
source:

```rust
pub struct C64TapeReport<'a> { /* borrows C64Tape */ }

impl C64TapeReport<'_> {
    pub fn tap_version(&self) -> TapVersion;
    pub fn platform(&self) -> Observed<C64Platform>;
    pub fn video_system(&self) -> Observed<VideoSystem>;
    pub fn pulses(&self) -> &[C64PulseInfo];
    pub fn kernal_files(&self) -> Option<&C64KernalFileSetInfo>;
    pub fn issues(&self) -> &[Issue];
}

pub struct C64KernalFileSetInfo { /* inspection value */ }

impl C64KernalFileSetInfo {
    pub fn container(&self) -> FileContainerInfo;
    pub fn files(&self) -> &[C64TapeFileInfo];
    pub fn observations(&self) -> &[C64KernalObservation];
    pub fn issues(&self) -> &[Issue];
}

pub struct C64TapeFileInfo { /* inspection value */ }

impl C64TapeFileInfo {
    pub fn entry_id(&self) -> FileEntryId;
    pub fn raw_name(&self) -> &[u8; 16];
    pub fn display_name(&self) -> &str;
    pub fn header_kind(&self) -> C64TapeHeaderKind;
    pub fn load_range(&self) -> Option<Range<u16>>;
    pub fn payload_len(&self) -> u64;
    pub fn copies(&self) -> &[C64TapeCopyInfo];
    pub fn issues(&self) -> &[Issue];
}
```

The common P19 interface owns rooted enumeration and file access:

```rust
pub struct FileContainer<'a> { /* borrows the active session */ }

impl FileContainer<'_> {
    pub fn info(&self) -> FileContainerInfo;
    pub fn entries(&self, parent: Option<FileEntryId>)
        -> Result<Vec<FileEntryInfo>>;
    pub fn stat(&self, entry: FileEntryId) -> Result<FileEntryInfo>;
    pub fn read(&mut self, entry: FileEntryId) -> Result<Vec<u8>>;
}
```

The precise P19 types must be settled jointly with F20. F23 must not add a
second generic enumeration API merely because the first adapter is tape.

The `C64Tape` name is intentionally concrete. A hypothetical `Tape::open`
that promises one schema for TAP pulses and Aaru records would be a shallow
module: callers would immediately branch on representation and downcast.

## C and Python reflections

S2 uses opaque `remanence_c64_tape`, report, KERNAL file-set, and common
file-container handles. Accessors return borrowed values owned by the parent
handle; entry and container identities are opaque stable values. Opening a
file container retains the source claim and cannot outlive its tape handle
under the ABI's documented ownership rules.

S3 mirrors the semantic objects as `C64Tape`, `C64TapeReport`,
`C64KernalFileSetInfo`, `C64TapeFileInfo`, and the common
`FileContainer`. Python conveniences may iterate entries or return `bytes`,
but cannot change identity, evidence, or conflict policy.

## Generalization after the concrete API

The comparison reveals a useful common architecture, not a useful universal
record type.

### Signal-shaped tape

C64 TAP and similar captures preserve a time base plus ordered transitions,
pulse intervals, samples, or gaps. Family decoders own thresholds, framing,
checksums, block meanings, and redundancy. Their durable active state is the
signal evidence.

### Recorded-object-shaped tape

Aaru and drive-oriented captures may preserve partitions, variable- or
fixed-length records, marks, end observations, device responses, and damage.
Their durable active state is the ordered recorded objects. A mark is not a
zero-length record; a fixed record is not a disk block.

### What is common

Both shapes share:

- a P12 adapter that reports only source-supported fidelity;
- ordered, positioned evidence with provenance and issues;
- one family-owned active representation per independently mutable instance;
- derived interpreters that cannot erase their source evidence;
- opaque identities that do not depend on names or array indexes; and
- P19 once an interpretation truly yields a rooted file container.

That is the generalized seam. Signal intervals and recorded objects remain
family-owned rather than forced into one `TapeObject` enum.

## Transitions remain explicit

A future `generate-tape` transition may encode logical files as C64 pulses or
record-oriented objects. It creates a new representation with declared policy;
it does not edit captured observations in place. A future write journey must
define conflict, placement, append, erase, and commit semantics for its chosen
family.

Likewise, a P15 tape-drive presentation is not assumed by U21. Datasette motor
control, sensing, playback timing, and electrical behavior differ materially
from SCSI-style rewind, space, locate, and status. Each needs a concrete use
case before a family presentation is designed.

## Acceptance shape

- TAP v0 and v1 timing forms retain source extents and normalize without
  invented precision.
- A standard KERNAL program is enumerated and read through P19.
- Duplicate names remain selectable through distinct opaque identities.
- Agreeing redundant copies yield one payload with two evidenced extents.
- A degraded single-copy recovery is reported; conflicting valid copies are
  not silently selected.
- Custom-loader pulses remain inspectable when no file container exists.
- T64 is not reported as pulse evidence or a tape-active source.
- The public API contains no universal `TapeObject` downcast seam.
- C, Python, and Rust expose the same identities, evidence, and refusal policy.

## Deliberately absent

- Physical acquisition and recovery procedures.
- Datasette or generic tape-drive emulation.
- Arbitrary custom-loader decoding.
- TAP writing, pulse synthesis, or mutation of captured evidence.
- PRG or T64 export.
- An Aaru support claim before its own adapter is implemented and tested.
