# ARCHITECTURE

The whole-system view, the application surface inventory, and the
architectural principles. This document describes the project **as it
exists today**; vision that has not arrived yet lives under
[planning/](planning/README.md).

## The system

One core, two bindings:

- **`crates/remanence`** — the analysis library, pure Rust, zero runtime
  dependencies. Everything the project knows lives here: executable image-format,
  serialized-container, partition-layout, and filesystem adapters with
  built-in role-specific catalogs; the identification model, the HDOS directory lister and
  file extractor, the self-contained ZIP/DEFLATE reader that lets
  an attached medium reach inside archives, and the disk stack —
  the declared-intent deny-write claim, the native qcow2 v2/v3 driver
  with read composition and top-image copy-on-write through backing
  chains, the native VDI driver reading and writing the dynamically
  allocated and fixed image types through the block map the format
  keeps, MBR partition discovery, FAT12/FAT16 volume read/write, the
  assurance gate that meets a source short of its own declaration with a
  bounded read-only reading rather than an all-or-nothing loss, and the
  commit-point session cache that keeps every write bufferable and
  revocable until committed — reads stream through a bounded working
  set, and altered extents hold in memory or spill to private session
  storage, never the image. Above that stack sits the one namespace
  composer that derives rather than consumes a mapping: the DOS
  drive-letter composer, which takes the machine facts a caller asserts
  and the reports it already holds, applies one named assignment rule,
  and answers with the volume each letter names. The magnetic family sits beside that stack
  and never crosses into it: the flux-capture model and its KryoFlux
  capture-set adapter, the flux-medium model above it, the drive-profile
  seam that recognizes a capture as a family's, the C1541 mastering
  profile that reduces one to a medium under a declared policy, and the
  P64 adapter that reads and writes that medium as a container.
- **`crates/remanence-ffi`** — a C ABI over the core: opaque handles,
  accessor functions, borrowed strings owned by their handle. The header
  `include/remanence.h` is generated from the Rust signatures by cbindgen
  at build time; the Rust `extern "C"` items are the definition and the
  header is a first-class representation of them, not a rival.
- **`crates/remanence-py`** — a Python module over the core (PyO3), a
  deliberate mirror of the Rust public surface in Python idiom.

The bindings contain no analysis logic; a behavior lives in the core or it
does not exist.

## The application surfaces

The surfaces through which the world drives or reads this project,
enumerated here in one place so downstream rules answer "does this touch an
application surface?" by lookup, not judgement. Numbers are permanent and
never reused.

- **S1 — The Rust crate API.** The public surface of `crates/remanence`:
  `Session`, `StorageDevice`, `AttachmentId` and `DeviceFamily`;
  `Disk` — reached through a device, never opened directly —
  `Identification` and the container/layout types,
  `Assurance` and the outcome, condition and byte-range types beside it,
  `Archive` and `ArchiveEntry`,
  `list_hdos_files` and `HdosFile`, `Error`/`ErrorCategory`/`Result`,
  `DosMachine` and the drive-letter mapping it composes, and
  the remaining public disk and filesystem records. Defined by the crate's `pub` items; `cargo
  doc` output is a representation of it.
- **S2 — The C ABI.** Every `remanence_*` symbol exported by
  `crates/remanence-ffi`, with the generated `include/remanence.h` as its
  consumer-facing representation. Covers naming, ownership rules (who
  frees what), null/out-of-range behavior, and enum values — an ABI
  change is a surface change even when no Rust type changed.
- **S3 — The Python module.** The `remanence` module registered by
  `crates/remanence-py`: its classes, properties, functions, exception
  type and category attribute, and module constants.

**Norms today are the code.** No prose specification has been written for
any surface yet; the defining code (and for S2, the generated header) is
the authority, which relocates vetting onto review of changes to it. Prose
norms are future work the owner may pledge; when one lands, it becomes the
single norm for its surface and this section names it.

## The architectural principles

> **Status: in force.** Every principle on this list is honored by the
> code as it exists today, and **a divergence between a principle here
> and the code is a bug** — not unbuilt work, a defect to fix. Numbers
> come from the one global P-sequence and are never reused.

### P1 — Self-contained format implementations

Every format the library claims, it implements itself — from published
format documentation, in the library, with no external tool, helper
process, or runtime dependency behind any claim. A ZIP is read by our
reader, a DEFLATE stream by our decompressor, a qcow2 by our driver —
never by shelling out. This is what makes the library embeddable from C
and Python without an environment around it.

### P2 — Reading is harmless

Opening, identifying, listing, and extracting never mutate an image —
not a byte. Write access is a separate, explicit request, and every
write path offers a commit point that can be rolled back until it is
committed: altered data stays in the session's cache — in memory or
spilled to private session storage, never the image — and nothing
reaches the file before the commit. An archivist's tool that damages
what it examines has failed at the door.

### P3 — Claims are enumerated and refusals fail closed

What the library recognizes is a named, enumerated claim — formats,
versions, feature subsets — and anything outside the claim is a named
refusal, never a guess, a silent skip, or an untested approximation. A
partition type we cannot read is refused rather than skipped, because
skipping renumbers every volume after it; a qcow2 feature bit we do not
honor names itself in the error.

### P4 — Identification carries its evidence

No verdict without the observations that produced it. Every
identification names its evidence in human-readable terms, and
confidence is bounded and comparable. "h8d, confidence 100" is not an
answer; "matched expected size of 102400 bytes; matched file extension
'.h8d'" is.

### P5 — One semantic surface, three presentations

Every core capability is reachable from Rust, from C, and from Python,
with the same semantics, and a change to the surface lands on all three
presentations in the same change — never deferred. No capability is
binding-private.

### P6 — Unexpected means stop: fail immediately, write nothing, say why

When the library meets a situation it does not expect — a structure
that contradicts itself, a value no claim covers, a state an operation
cannot account for — it **fails immediately**: it writes nothing, and
it gives a clear indication of the reason. No partial update, no
best-effort continuation, no repair attempted on the caller's behalf,
and no error that names a symptom when the cause is known. Two
consequences make the rule operative: surprises are sought before
mutation begins (a mutating operation validates everything it can up
front), and the reason is a diagnostic — what was expected, what was
found, where. P2's commit point is the backstop, not the excuse:
roll-back exists for the interruptions the world inflicts, never as
license to start writing before the checks are done.

### P7 — The file must never change under our feet

The library cannot support a file changing underneath it while it
works — not while writing, not while merely reading. **Denying write
permission to every other process is mandatory in all scenarios**, from
the moment a file is opened, and a file for which that denial cannot be
obtained is not opened at all: fail fast, with the reason named. A disk
image held open for writing by a running VM is the designed refusal.
On the disk stack the caller declares the session's mode at open —
read, or write — and the mode report echoes the declaration. A
writable open that cannot secure its own write access fails at the
open, never by silent fallback, and **a writable session admits no
observers**: its claim excludes every other read or write for the
session's whole life. A read open takes no stronger access than it
needs and keeps admitting other readers, every remanence write action
refused by name. An identification session, which only reads, still
takes the strongest access the file grants — read/write preferred,
read-only otherwise — with writes denied to others either way. The
claim covers every file of a backing chain, consistently: the top
image is claimed per the declared intent, and every backing file is
claimed immutable through this access — writes denied to others, the
library's own access read-only. Contention anywhere in the chain is
an immediate, named failure, never a hidden wait. The
claim is held from open until the session or disk is completely done:
no claim-on-modify, no release-on-save. On Windows the mapping is
native and kernel-enforced (share modes: a writable disk session
shares nothing; every other open shares reads only); on POSIX the
advisory lock is the claim — shared for a disk read open, exclusive
otherwise — binding cooperating processes and asserted as protocol
against the rest.

### P8 — Versioned formats are supported by explicit version, or refused

Where a container format or filesystem declares its version — a version
field, a feature bitmap, anything the format provides for saying "this
is newer than you know" — the library validates it against the versions
it explicitly claims, **before touching anything else**, and a version
or feature bit beyond the claim fails immediately, naming what it found
and what it supports. Read and write alike. Support for a new version
is a deliberate release: understand what changed, implement it, widen
the stated claim, publish. Where the version is not stamped but
versions are known to exist, the library determines the version by
every available means, declares its ceiling all the same, and fails
fast above it — an undeterminable version on a format known to have
them is itself a named refusal. Where a format genuinely carries no
versioning, the claim is structural and P3 governs: FAT width is
decided by cluster count because the format says so, and FAT32 is
refused by name, never guessed at.

### P9 — Interruption never invents a third state

P2 makes commit the only moment the image changes; this principle
armors that moment. An interruption at any point during commit — a
killed process, lost power — leaves state the next open reconciles
**before exposing the disk**, and after reconciliation the image is
wholly the old state or wholly the committed new state, never a partial
third state.

The durable undo journal beneath the overlay is private transient
state: no user-owned file, no cleanup verb, no contract about its shape
or location. A fault-injection harness terminates a separate process
after each durability boundary in commit and proves reconciliation for
raw, standalone qcow2, VDI, and backing-chain images; in-process rollback
tests are not evidence for this principle.

### P10 — Every refusal is machine-addressable

A refusal's human diagnostic (P6) is not its interface. Every error
carries, beside its message, a stable machine-readable category
from one enumerated set — the same category in Rust, in C, and in
Python (P5) — so an embedder maps behavior without parsing text no
release promises to keep. The category set is itself part of the
surface: adding a category is a surface change; rewording a message
never is.

The initial set, covering every refusal the library makes today:
`locked`, `invalid-image`, `unsupported`, `read-only`, `not-found`,
`not-directory`, `is-directory`, `no-space`, `unavailable`, `io`.

`unavailable` and `io` are deliberately distinct, and P28 is why: the
first says the artifact does not hold what was asked for, and the second
says the host failed to deliver bytes that do exist. A caller behaves
differently about each, and collapsing them would let a host failure be
re-described as imperfect media evidence.

That set is deliberately cross-cutting and small: it answers *how should
the caller behave*, and it answers it for the whole library at once. One
question it cannot answer is *which rule did this input break*. Where a
format, namespace, or grammar defines a bounded set of rules an input must
satisfy — a DOS 8.3 name has seven, and FAT is one filesystem of many — the
category is the same for every one of them, and the only difference between
them is the sentence. Widening the category set to close that gap would
dissolve it: the categories would grow one per format rule, and the small
cross-cutting mapping this principle exists to provide would be gone. So
the error carries one field beside the category, not a second mapping:

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
(P5). Because the sets belong to their seams rather than to the library,
the identity is a value the seam spells rather than a second library-wide
enumeration; a Rust caller reads it back through the seam's own type, and
the C and Python presentations carry the same spellings.

The rule identity is not a second diagnostic. It names the rule, and P6's
human diagnostic still says what was expected, what was found, and where.

The DOS 8.3 namespace owns the first such set — `empty-base`,
`base-too-long`, `extension-too-long`, `separator`, `excluded-character`,
`reserved-device-name`, `surrounding-space` — and nothing else the library
refuses today has a rule set behind it.

### P11 — Portable Rust comes first

Remanence is written as portable Rust, not as a Windows implementation
with incidental reach elsewhere. Core behavior avoids host-specific
assumptions unless the operating system forces them, and any necessary
platform-specific behavior is isolated behind a small internal boundary.
Public semantics stay the same across platforms; where they cannot, the
difference is a named refusal rather than a silent divergence.

Windows is the directly tested and wheeled platform today. Linux, macOS,
and BSD-family systems are expected to remain buildable from source as a
soft portability obligation, and may become directly tested and wheeled
platforms when repeatable CI or trusted native builders are added. A
support claim names the host tuple it covers rather than letting an
operating-system name imply every architecture that operating system can
run.

### P12 — Image formats are implementations at representation seams

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

### P13 — One image layer is authoritative

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

### P16 — Partition layouts are an independent seam

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

### P17 — Volume composition is an independent seam

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

### P18 — Filesystems are an independent seam

A filesystem adapter consumes one volume and exposes a P19 file-container
view of the namespace, metadata, and data operations it claims. FAT, HDOS,
CP/M, NTFS, ext, and other filesystems illustrate variation at this seam;
their mention is not a support claim. The adapter owns recognition and
evidence, structural validation, version and feature ceilings, refusals,
and all filesystem semantics it implements.

A filesystem does not parse an image container, discover the partition
layout around its volume, or know how that volume was composed. The
filesystem catalog and interface remain independent of those adjacent seams.

### P19 — File containers are the common file-access seam

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

A **namespace-mapping composer** is the third form at this seam, and it
*derives* the mapping the two above consume. It takes composed volumes with
their identities, plus the machine facts its caller asserts, applies one
named assignment rule, and returns the mapping it establishes. Producing a
mapping and composing a file container over it are separate acts: the
mapping answers on its own, and a composer that can establish only part of
one still answers with that part. The form exists because a system may
persist no mapping at all — a DOS machine's drive letters were assigned at
boot by a rule over its own configuration, and nothing on the disks records
the result — which leaves the caller as the only remaining home for a rule
the library already has to know.

Three constraints keep the derivation from becoming a guess:

- **The rule is an enumerated claim (P3).** The composer names the
  assignment rule it applied. Where variants of one system assign
  differently, it claims the variants it implements and refuses the rest by
  name; it does not average them or pick the most common. Where the caller
  states no variant, a mapping the claimed variants disagree on is reported
  undetermined rather than settled by the more common rule.
- **Evidence outranks a rule.** Where a system persists its own mapping,
  that mapping governs and no rule may stand in for it. This form is for
  systems which persist nothing, and it never becomes a fallback for a
  persisted mapping that could not be read.
- **A derived mapping is not evidence.** The asserted machine facts and the
  applied rule travel with the result as provenance, under the discipline
  that keeps a caller-selected fact out of the evidence a seam carries (P4).
  Whatever the rule cannot settle is reported undetermined, at the
  granularity of the mapping it failed to establish, and is never filled
  from position, size, order, label, or which volume happened to read
  cleanly.

The composer takes reports the caller already holds and opens nothing. It
reads the machine facts — medium, slot, attachment order — from the caller,
because a session's device set holds only the block family and cannot
express a floppy slot, a CD-ROM drive, or DOS attachment order; when those
families are claimed, a composer may take the same facts from a session's
devices, and nothing else about it changes.

The common file-container view is not a disk representation and declares
no image layer, media, geometry, partition layout, or volume semantics.
Raw partitions and volumes do not satisfy it. Selecting a file yields a
byte stream; only independent P12 recognition can make that file an image
and declare its authoritative layer under P13.

### P21 — Device identity is assigned, scoped, and unobtrusive

Every addressed virtual device receives an opaque identity when Remanence
composes it. The library assigns that identity; an ordinary single-image
open never asks the caller to provide or choose one. The identity is unique
within its containing open or composition. It implies no globally stable or
user-meaningful identity unless a later interface explicitly claims one.

Device identity qualifies provenance and otherwise-local identifiers only
where more than one device makes that distinction necessary. An interface
already scoped to one disk may continue to accept a disk-local identity —
the volume identity a disk's inspection report issues, say — without
exposing the device identity. A presentation
may report the assigned identity, but never makes callers echo a value that
does not affect the requested operation.

An attachment identity such as `hdd0` is distinct from device identity. A
caller supplies placement only when placement changes semantics and cannot
be inferred. This principle adds neither multi-device opening nor
multi-device volume composition; those capabilities require their own
proposal.

### P22 — Magnetic recording can descend to timed flux transitions

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
preserves stored pulse position and strength into the flux medium, and a
read-channel simulation consumes that state with its weak-event semantics
intact. Flattening the image to one deterministic bitcell or byte stream
does not provide P64 fidelity and does not satisfy this principle for that
format.

Flux is one channel, not the whole medium. Index and hard-sector holes and
other mechanical or sensor observations are separate timed state or event
channels; they are not folded into the flux-transition stream. Every adapter
states which timing, markers, revolutions, and weak-event semantics it
preserves, normalizes, synthesizes, or cannot represent.

#### The family holds two models, capture and medium

A capture adapter may preserve several revolutions and their marker timing,
while a normalized media model defines one circular revolution. Those are
two models, and each has its own name so that one word stops doing two jobs.

**Flux capture** is timed transition evidence as an instrument recorded it:
several capture runs and observations of one source location, the
instrument's own timebase, the source's own location identity, parallel
marker channels, and whatever else the capture container expressed. It
asserts nothing about which revolution the disk *was*, and this principle's
refusal to average, deduplicate, or select inside it stands unchanged.

**Flux medium** is one circular pulse stream per location the family
addresses, expressed in a declared rotational frame against a declared
reference clock, with each pulse carrying the family's strength semantics,
beside the medium-level facts that are not per-pulse. It asserts exactly
what a drive would read.

**The boundary is one sentence: disagreement across observations is a
capture fact, and strength is a medium fact.** A capture records that three
passes differed. A medium records that a pulse is weak. Turning the first
into the second is a reduction governed by P29 and performed by neither
model on its own initiative.

What the medium adds is precisely what the flux does not contain — the
rotational frame, the family's addressing, the reference clock, the strength
vocabulary, and which surface is the disk. Every one of those is declared by
a P30 drive profile. That is why this is a second model and not a tidier
first one: the medium is where declared family knowledge and recorded
evidence combine, and a representation holding only one of the two cannot
stand in for it.

What the medium must **not** hold keeps it below the layer above: no
bitcell, no recovered clock, no synchronization, no symbol, no byte. Those
are hardware bitstream and above, and a medium that reached them would erase
the distinction between what a medium is and what a drive makes of it.

P64 is a flux medium. SCP, A2R and KryoFlux streams are flux captures. G64
is a hardware bitstream. Naming them locates them; it changes no support
claim, each of which remains enumerated under P3 and delivered by its own
adapter.

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

#### Knock-on requirements

In-force P12 and P13 recognize timed flux as an authoritative layer and physical media profiles as the owners of index and
sector-hole topology. It makes the separate flux-data and marker/sensor
channels explicit. U23 supplies the concrete P64 journey; P22 does not by
itself claim a standalone P64 image adapter or a public flux interface.

### P23 — One durable layer is active

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
| **flux medium** | circular track-relative flux transitions and strength semantics, with marker/sensor channels and provenance | a modeled magnetic recording surface |
| **hardware bitstream** | circular track-relative clocked bit state, with the timing and provenance its declared drive family requires | what a family's read channel resolved, not what it means |
| **encoded bytestream** | the circular track-relative byte sequence a declared family codec materializes from that bit state | the recording's own bytes, before any of them is a header, a sector, or a file |
| **CHS** | records addressed by cylinder, head, and sector under a declared geometry | geometry and records, but not their physical encoding |
| **block** | geometry-opaque logical blocks addressed by number | no cylinder, head, track, recording, or mechanism claim |

These are six family-owned representations, not variants of one universal
schema. The flux medium includes its parallel marker channels; they are not
another active layer. CHS and block both carry record bytes, but CHS's declared
geometry is observable and load-bearing while block deliberately hides it.
File container is semantic named-entry state and makes no disk claim.

#### Hardware bitstream and encoded bytestream sit above the medium

Hardware bitstream is pre-synchronization and pre-decoding: a bit cell is
not a symbol, a byte, a sector or a file. Encoded bytestream is the byte
sequence one declared family codec resolves out of it — for a 1541,
GCR-decoded bytes — before the library identifies synchronization, headers,
data fields, sectors, or files. A codec locates the family's declared
framing landmark because byte framing has to begin where the family says it
does, and having located one it claims nothing about what follows it. No
source format is presumed to begin at either layer. An image whose
authoritative and initial active layer were hardware bitstream — G64 is the
example — would enter there; the library claims no such adapter today.

The magnetic-disk path above encoded bytestream is CHS, then filesystem. A
family-owned synchronization and sector interpretation materializes CHS only
where its claimed rules support it; a byte sequence is not assumed to
contain sectors. P18 then recognizes and presents a filesystem above CHS.
CHS is durable active media state; filesystem remains the higher derived
seam, not a peer mutable media copy.

The magnetic ladder therefore reads: flux capture → flux medium → hardware
bitstream → encoded bytestream → CHS → filesystem. Block stays terminal and
disjoint from all of it, and the prohibition below on crossing between the
block and flux families is untouched in both directions.

Flux medium, hardware bitstream, and encoded bytestream are distinct durable
layers, not caches and not mutable peer copies. A source whose authoritative
layer is a flux medium begins medium-active; a hardware profile may
explicitly materialize a hardware-bitstream active layer from it; a declared
codec may then materialize an encoded-bytestream active layer. Each
transition is atomic, preserves source state and codec/profile as
provenance, and makes the destination the sole mutable session truth.
Descending or returning to a lower layer is a separate explicit mastering
transition. In either direction, P13 governs write availability: any
unrepresentable projection is refused or requires explicit conversion.

**Neither layer is writable in this release.** Both transitions materialize
state a presentation reads, and no verb mutates either, so the sole-mutable-
truth clause binds without yet having a mutation to bind: a medium and the
bitstream above it may both be held, because neither is an independently
mutable instance. What is already a property of the code is the rest — the
transitions are whole or they refuse, each carries the profile, the codec
and the source's own policy as provenance, and there is no way back down.

The clauses below for active-layer replacement, cache invalidation, bounded
backing, and the ban on independently mutable peer copies apply to these
layers. P22 continues to govern both flux models and the medium's marker
channels.

**Flux capture takes no row.** It is an authoritative image layer under P13,
which is a statement about what an artifact records, and it is read by
inspection and by mastering. It never carries a session's mutable truth: the
rule above is scoped to every independently mutable open state instance, and a
capture set opened to be inspected and mastered is not one. A writable
capture-editing session is claimed by nothing here.

The reason is not bookkeeping. **A capture has no coherent answer to where a
write lands.** A drive writing to a capture would have to choose which of
several disagreeing observations to overwrite, and no answer to that is better
than another. A drive writes to a medium.

**A capture becomes a medium by mastering, not by lowering.** That is a P29 act
with declared policy inputs, whether its destination is a new artifact or an
active layer inside the session so that a drive can be served over a capture.
Only the destination differs; the inputs, the plan, and the declared-loss
account are the same. It is also the mechanism P15 assumes when it says a
drive's floor may be timed flux for a P64 or a raw capture: a raw capture
becomes a floor by being mastered in session under declared policy, never by a
normalization nobody named.

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
at the flux medium—or differ—a raw sector image can remain authoritative at
sectors while a synthesized flux medium becomes active for low-level drive
service, and a capture set is authoritative at flux capture while whatever
serves a drive over it is a mastered medium.
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

**Generate-flux is generate-medium.** It synthesizes a flux medium and never a
capture, because fabricating instrument evidence from sectors would be a false
claim about provenance in the one clause most concerned with honest provenance.
Every requirement above is unchanged by that naming.

There is no universal linear ladder across all four layers. A declared
legacy floppy family may lower CHS to a flux medium. Block is terminal and
never lowers to flux; flux never rises into block. File container participates
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

#### The layer caches are tied

In-force P27 gives every modeled durable layer its own cache under one
declared session budget. Across this principle's layers the tie is exact: a
derived layer's cache is a clean-only accelerator regenerated from the layer
below — a derived write completes downward into the active layer's cache in
the same act or alters nothing, and a write landing in a lower layer
invalidates the overlapping derived extents above it, so a stale decode is
never served. A P64 source pins its flux medium active, with sector access
deriving a CHS cache above it; a sector-format source is CHS-active with one
cache until generate-flux rebinds it as derived over a session-backed
medium, both layers caching from then on. Threads may derive upper-layer
extents ahead of demand under P27's speculation rules.

### P27 — Sessions stream; memory holds a bounded working set

Remanence is sized by the operation, never by the artifact. A source may
be a floppy image of a few hundred kilobytes or a virtual disk of a
hundred-plus gigabytes; the same open, identify, read, and write journeys
serve both, so no representation — a source's encoding, the session's
durable state, a derived view, or the uncommitted write set — is ever
loaded whole as a design assumption. An operation may visit bytes in
proportion to its task; it may hold only a bounded working set. A whole
layer may be held only when its format bounds it beneath the working set;
every other path streams, and a format that resists streaming is
materialized to private session storage, never to memory.

Every session's durable state has one backing. It is **source-backed**
when bounded random access is served directly from the source encoding —
a raw image by identity, qcow2 through its allocation structures — and
reads stream from the source on demand through the session cache. It is
**session-backed** when it cannot be — a decoded representation whose
encoding permits only sequential access, such as a DEFLATE-compressed
archive entry the session must address randomly — and is then produced
once by a streamed transform into private session storage and served
from there through the same cache.

Caching is per modeled durable layer, under one declared session budget.
The active state's cache carries the session's mutable truth in two
residency classes: **clean state is always evictable** — droppable and
re-read from its backing at will, sound because the P7 claim pins the
source, so a small image simply becomes fully resident while a huge one
converges on the operation's locality — and **dirty state is never
dropped**: alteration is tracked at extent granularity, uncommitted
changes hold in memory within the bound and spill to private session
storage beyond it (P2), eviction moves them, only rollback discards
them, and commit projects them. A derived view's cache, where a session
models one, is an accelerator holding only clean state: its writes
complete into the layer below in the same act or alter nothing, a lower
write invalidates the overlapping derived extents above it, and eviction
regenerates from below.

The library may use threads to predict, prefetch, and offload —
speculatively reading ahead of an access pattern, deriving ahead of
demand, spilling ahead of pressure — with the standard library's threads
alone. Four rules keep the concurrency observationally invisible:
speculation produces only clean state; offload never gaps the truth (an
altered extent leaves memory only once its spill write has completed,
and every act that consumes the altered set joins the offloads in
flight); the work spends the declared budget with demand outranking
prediction; and speculation is silent — a failed speculative read caches
nothing and reports nothing, so results, evidence, and refusals are
identical with any number of threads, including none.

Commit, materialization, and recovery stream like everything else
through bounded buffers; identification probes read the bounded evidence
their claims name; private session storage takes the shape P9 gave the
journal — no user-owned file, no cleanup verb, discardable after
interruption — and the bound and its read-ahead are declared session
configuration with a stated default, never discovered behavior. Public
presentations carry the same rule: an operation whose result is
proportional to source content offers a bounded or streamed form in
Rust, C, and Python alike (P5), with whole-value conveniences beside it,
never as the only route. This principle constrains resources, not
semantics: behavior is identical at every source size, and peak memory
bounded independently of source size is the testable claim this entry
makes.

### P28 — Evidence may narrow authority without discarding readable evidence

Fail-closed is a rule about authority, not a command to discard every byte
whose complete intended interpretation cannot be proved. An image may be
recognizably incomplete, contradictory, or only caller-described, yet
still contain a bounded region that the library can read without inventing
bytes or concealing the defect. In that case the library retains the
evidence and offers only the operations whose preconditions it can
establish.

Every open therefore has one explicit **assurance outcome**: **verified**,
where the selected interpretation and every bound needed by the requested
operation are evidenced; **degraded**, where a material shortfall or
contradiction is known but a truthful read-only interpretation of a
bounded portion remains; or **refused**, where no bounded interpretation
exists or an operation needs the missing or contradictory fact. The
transition from verified to degraded is the confidence threshold. It is
not a second arbitrary score beside P4's recognition confidence: it is a
deterministic safety gate.

A declared size exceeding the source, contradictory required structure, a
caller assertion the source disproves, or a read reaching an unavailable
extent fails that gate. The report states the evidence, resulting bounds,
and withheld operations. An explicit caller selection is an interpretation
request, not a waiver of evidence. Thus a raw 1.44 MiB FAT12 floppy
declaration over a shorter source enters degraded read-only mode: the
library may list or extract only data whose directory traversal and full
cluster chain remain in the source. A chain entering the absent tail is a
named unavailable result, never zero-filled, shortened, or successful.

Degradation is not repair: the library does not fabricate missing sectors,
skip damaged structures, choose an unresolved interpretation, or continue
after it has lost the bounds that make a result meaningful. A malformed
boot record that prevents a safe prefix from being addressed remains a
refusal.

The degraded path is deliberately narrow: it applies only while
determining a catalog type or reading or writing through an already
selected catalog type. A catalog adapter may preserve uncertainty in the
image, layout, volume, filesystem, or file operation it owns when the
result remains bounded and evidenced. It does not apply to the library
machinery around that interpretation. Failure to acquire or use the host
claim, to read or write the session cache or private storage, to persist
the commit journal, to allocate a required resource, or to perform host
I/O is an immediate P6 failure. Such a failure cannot be re-described as
imperfect media evidence or yield a partial answer.

Degraded state revokes mutation authority for the session. A write-intent
open reports an evidence-driven effective read-only mode and a stable
condition; every write, commit, and mutation-capable derived operation is
refused with that condition. P7's no-silent-fallback rule still governs an
inability to acquire host access — this is a distinct restriction after a
safe claim has been made. A session never regains write authority without
a new verified open.

P3 and P6 remain intact: the library refuses an unclaimed interpretation
and stops the first operation it cannot account for. It does not turn a
known, bounded deficiency into an all-or-nothing loss of independently
readable evidence. P4 carries the reason, P10 carries the stable
condition, and P5 requires equivalent assurance outcome, evidence, bounds,
and effective mode in Rust, C, and Python.

#### The condition set, and where the gate is armed today

The conditions are an enumerated claim (P3) owned by this seam, and they
are the rule identities (P10) a withheld operation's refusal carries:
`source-truncated`, where the interpretation declares more bytes than the
source holds, and `evidence-conflict`, where required structure
contradicts itself so no safe bound can be stated for the shortfall
observed beside it.

Which interpretations the gate is armed for is a claim like any other,
and today it is one: a raw image whose leading sector is a FAT12/FAT16
boot record — the composition where a filesystem's own declaration bounds
the whole disk, because the caller selected the image's bytes as the
disk. A container format declares its own virtual size and answers for it
at its version gate (P8), so no automatic degradation rule is claimed for
qcow2, VDI, an archive, or a partition schema. This principle says what
an armed interpretation owes, not that every interpretation is armed;
arming another is a feature, and stating the shortfall as this one does
is what that feature would have to deliver.

### P29 — Mastering is declared, reproducible, and states its loss

**Mastering** is deriving a new artifact from evidence Remanence already
holds: solving several capture runs into one circular medium, choosing among
channels and observations, projecting one timebase onto another, and encoding
the result into a format that cannot carry everything the evidence holds. P13
already permits the act — choosing another authoritative layer is an explicit
conversion which creates a new image and names its loss. This principle says
what the act must carry, because a conversion that reduces evidence silently
is indistinguishable from one that preserves it.

**Mastering is requested, never incidental.** It is not a side effect of
opening, attaching, presenting, or saving. The sources are read and nothing
else: their authoritative layers, active layers, and provenance are unchanged
by the operation, and the result is a separate artifact with its own
authoritative layer.

**Every reduction is a named policy input.** Which channel supplies evidence;
which observation of a location is used and how several are reconciled; how
source location identity maps onto the destination's addressing; how the
source timebase projects onto the destination's; and how weakness, absence,
disagreement, and contradiction are expressed in the destination's vocabulary
— each is supplied by the caller or declared by the profile, and each travels
into the result as provenance. **A reduction that no policy names is a
refusal, not a default.** The flux capture already refuses to average timings,
deduplicate pulses, or select a cleanest pass inside itself; this forbids
performing those on the way out.

**Two owners, and neither infers the other's answer.** The family mastering
profile owns the physical reduction; the destination image-format adapter owns
its grammar, its version claim, its encoding, and its named refusals (P8, P12).
A profile does not decide what a container can hold, and an adapter does not
decide which revolution the disk was.

**The loss is declared before the write.** A mastering operation resolves in
two stages: a plan which computes the whole transformation and writes nothing,
and an execution which writes. The plan enumerates, in the source's own terms,
everything the destination will not carry — unselected channels and
observations, evidence outside the destination's addressable extent, marker
channels, foreign records, capture metadata, and timing resolution beyond the
destination's timebase. A count is not an account; loss reported after the
fact does not satisfy this; and a reduction the plan did not declare is a
defect, not a detail.

**The result is derived and says so.** Mastered content carries
selected-and-projected or synthetic provenance under P13, never
recovered-evidence provenance. Nothing in a mastered artifact is presented as
an observation of an original recording that was not one.

**Mastering is reproducible.** The same sources, policy, and declared seed
produce the same mastered state; where the destination encoding is itself
deterministic, the same bytes. A transformation which cannot state what makes
it vary is refused rather than shipped as approximately repeatable.

P2, P6, and P9 apply unchanged: the sources are never mutated, nothing is
written until every check has passed, and an interruption leaves a complete
destination artifact or none.

#### Knock-on requirements

This principle governs the direction *out* of the library's evidence into a
new artifact; P13 continues to govern write availability *back* to a source,
and P23 continues to govern which layer is active within a session. It pledges
no destination format by itself — each is a named claim under P3 delivered by
its own adapter — and it creates no public evidence iterator: the mastering
plan and its declared-loss account are the surface, and the evidence stays
behind them.

### P30 — Drive profiles are an independent seam

P22 and P23 both rest on a **media profile** and a **hardware profile**: the
authority that says whether a drive observes a selected revolution or a
seeded variation, and the authority that, with the image metadata and the
mastering rules, makes a downward synthesis honest rather than invented.
Neither principle names an owner for that knowledge, so today it is assumed
by two principles and held by none. This states the seam that holds it.

A **drive profile** consumes flux evidence and declared context and exposes
one family's recording conventions together with a recognition verdict over
that evidence. It owns how the family's source positions map onto its own
addressing and how many steps a location takes; its rotation rate and
reference clock; its density or zone map and what each zone claims; the
timing shape of its encoding landmarks; which surfaces it records; and the
selection or variation rule by which several observed revolutions become one
served medium. It owns the same knowledge above the medium: the mechanics
and read-channel rules by which a medium's pulses become clocked bit cells —
the window a transition is admitted by, and whether a transition restarts
the cell counter — and the family's group code, which is the table its
bytes are recorded as. Each is a declared fact of the family, carried with
its provenance — never arithmetic a capture is assumed to justify.

**Recognition is a probe that carries its evidence.** A profile is offered
the evidence and answers with a bounded, comparable confidence and the
observations that produced it (P4), so a verdict is auditable rather than
asserted. Several profiles may claim one capture and the verdict is ranked;
a capture no profile claims is a named refusal (P3), never a guess, a
default, or the single enrolled entry winning by being alone. **Discovery
proposes and never silently decides**: a caller may pin or override a
profile, and what the library chose travels into the result as provenance.

#### The recognition boundary

**A profile recognizes structure, never content.** It may read flux interval
lengths and the patterns they form — a run of shortest intervals is a
synchronization landmark whether or not anything ever decodes it — and it
may report a count, a density, an angle, a location, and an absence. It may
not resolve a bit value, assemble a byte, name a sector, or validate a
checksum.

The boundary is not fastidiousness. Those acts are the hardware bitstream
and the layers above it, and a probe that reached them to recognize a family
would make every recognition depend on a clock-recovery model, collapsing
the distinction between what a medium *is* and what a drive *makes of it*.
The test is what leaves the probe: **an angle, never a byte.** A protection
whose evidence is a deliberately wrong checksum is therefore invisible here
by design, and is carried faithfully by a layer that never interprets it.

#### What a profile is not

It is not P15 hardware emulation, which generates timed causality from state
a profile helped materialize, and it is not a P12 image-format adapter,
which owns a container's grammar and recognizes an encoding. A profile owns
what a family does to media. One composition may need all three and none
substitutes for another.

The profile catalog is wiring, as P12's is: every entry pairs a descriptor
with behavior, and adding a family changes its module, its tests, and one
mechanical enrollment. Central orchestration neither branches on a profile
identifier nor interprets string-named family rules.

#### Knock-on requirements

P29 is unchanged and is the reason this seam is safe. A policy input is
"supplied by the caller or declared by the profile", so recognition supplies
declarations with provenance rather than converting an unnamed reduction
into a silent one: a profile that cannot state a reduction still refuses.
This principle pledges no family — each is a named claim under P3 delivered
by its own feature — and it creates no public flux, pulse, or capture-run
iterator. The verdict and its evidence are the surface, and the evidence
stays behind them.
