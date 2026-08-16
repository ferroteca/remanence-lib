# ARCHITECTURE

The whole-system view, the application surface inventory, and the
architectural principles. This document describes the project **as it
exists today**; vision that has not arrived yet lives under
[planning/](planning/README.md).

## The system

One core, two bindings:

- **`crates/remanence`** — the analysis library, pure Rust, zero runtime
  dependencies. Everything the project knows lives here, in four groups.
  The **identification model** and the adapters beneath it: executable
  image formats, archives, partition layouts and
  filesystems, each enrolled in its own catalog. The **storage model**:
  the session's media pool and the devices beside it, the claim in its
  two classes — the library's own deny-write open, and the caller's
  handle honoured as it was afforded — the native qcow2 and VDI drivers with
  their backing and differencing chains, MBR partition discovery,
  FAT12/FAT16 volume read/write, the discovered geometry a medium's
  sector verbs address in — read from the sources that state one, with
  disagreement reported rather than settled — the assurance gate that
  meets a
  short source with a bounded read-only reading, and the commit-point
  session cache that keeps every write revocable until committed. And
  the **magnetic
  family** beside it, never crossing into it: the flux-capture and
  flux-medium models, the drive-profile seam, the gap-first
  reconstruction and the remanence image it answers with, the C1541
  presentation ladder — bitstream, bytestream, the
  sectors the recording states for itself, and the CBM DOS directory
  written across them — the C64 renditions, and the P64 container. A
  KryoFlux capture and a P64 load as media of the storage model — the
  collection-sourced and served-form reads — while the family's
  representations stay its own.
  [AGENTS.md](AGENTS.md) maps these onto modules.
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
  `Session`, its devices and its media pool, `Medium`, `MediaId` and `Format` — the
  medium being the pool-owned content handle, created by a declared
  reading over the caller's own opened file and carrying every content
  verb — `StorageDevice`, `DeviceView`,
  `AttachmentId`, `DeviceSlot`, `DeviceType`, `FloppyDrive` and
  `HardDrive` — the device being the slot the session holds, typed by
  the device that fills it, with `insert`/`eject` the one edge between configuration and
  state and device-type equality the check it makes —
  `Claim` beside the access mode,
  `Partition`, `PartitionView`, `PartitionScheme` and `PartitionType` —
  the partition pool being the medium's evidence, reached by the
  scheme's own ordinal, with the two vantage doors on the view —
  `StorageSpace`, `File` and the `Entry` vocabulary — the volume and
  filesystem being two vantage traits on one node, addressable I/O and
  namespace I/O, with the file verbs living there and nowhere else —
  `Identification` and the layer/layout types,
  `Assurance` and the outcome, condition and byte-range types beside it,
  `Error`/`ErrorCategory`/`Result` and the rule sets refusals name, and
  the remaining public disk and filesystem records. Defined by the crate's `pub` items; `cargo
  doc` output is a representation of it.
- **S2 — The C ABI.** Every `remanence_*` symbol exported by
  `crates/remanence-ffi`, with the generated `include/remanence.h` as its
  consumer-facing representation. Covers naming, ownership rules (who
  frees what), null/out-of-range behavior, and enum values — an ABI
  change is a surface change even when no Rust type changed. The
  hand-maintained `include/remanence.hpp` is a **second derived
  representation** of the same symbols for C++ consumers, carrying no
  capability of its own and no number of its own; it moves with the ABI
  in the same change, and where the two disagree the ABI governs.
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

**A principle states its claim, what it binds, and at most one line of
why.** Three things it does not carry, because each has a home that will
not drift: the *argument* that settled it, which is a decision entry
([planning/DECISIONS.md](planning/DECISIONS.md), cited by D-number where
it is worth finding); the *enumerated sets* a claim ranges over — error
categories, articles, device types, armed conditions — which the code owns, the code
being the norm; and a *restatement* of a neighbouring principle, which is
a cross-reference. Planning prose argues at whatever length the argument
takes; a principle in force is the settled rule. **A principle that
cannot be stated briefly is the first evidence that something is wrong
with the principle.**

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
skipping renumbers every volume after it.

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
mutation begins, so a mutating operation validates everything it can up
front; and the reason is a diagnostic — what was expected, what was
found, where. P2's commit point is the backstop, not the excuse.

### P7 — The file must never change under our feet

The library cannot support a file changing underneath it while it
works — not while writing, not while merely reading. **Denying write
permission to every other process is mandatory where the library opens,
and caller-owned where the caller opened.** Which of the two a medium
holds is the claim's class, and it travels on that medium's assurance.
A medium **created whole by its author** holds neither, and says so as a
third class: nothing was opened, so there is no file for this principle
to be about until an explicit encode gives it one.

**Where the library opens** — an artifact reached by name, every file of
a composed chain, every artifact it creates — the denial is mandatory
from the moment the file is opened, and a file for which it cannot be
obtained is not opened at all: fail fast, with the reason named. The
caller declares such a session's mode at open — read or write — and
the mode report echoes the declaration. A **writable session admits no
observers**: its claim excludes every other read and write for the
session's whole life, and a writable open that cannot secure that access
fails at the open rather than falling back silently. A read open takes no
stronger access than it needs, keeps admitting other readers, and refuses
every remanence write by name. An identification session, which only
reads, still takes the strongest access the file grants — read/write
preferred, read-only otherwise — with writes denied to others either way.

**Where the caller opened**, the claim is theirs: **whoever opens owns
the lock.** A local artifact arrives as the caller's own opened file, and
that open is their safeguard and the library's claim at once. The library
checks it for **exactly one thing** — may it write through it? — honours
the answer exactly, and never supplements it with a lock of its own or
escalates it through a name. A handle affording no write makes a
read-only medium whose write verbs refuse by name. A **name recovered
from a handle serves location only** — where a commit journal lands
beside the artifact, where a backing chain's parent is looked for next
door — under an identity check that the name still denotes the handle's
own file; a nameless handle refuses those journeys by name and serves
everything else.

The library-opened claim covers every file of a composed chain: the top
image per the declared intent, and every file behind it claimed
immutable — writes denied to others, the library's own access read-only.
Contention anywhere in the chain is an immediate named failure, never a
hidden wait. Every claim is held from open until the session is
completely done: no claim-on-modify, no release-on-save. Windows share modes are the native,
kernel-enforced mapping; on POSIX the advisory lock is the claim — shared
for a disk read open, exclusive otherwise — binding cooperating processes
and asserted as protocol against the rest.

### P8 — Versioned formats are supported by explicit version, or refused

Where an image container format or a filesystem declares its version — a version
field, a feature bitmap, anything the format provides for saying "this
is newer than you know" — the library validates it against the versions
it explicitly claims, **before touching anything else**, and a version
or feature bit beyond the claim fails immediately, naming what it found
and what it supports. Read and write alike.

Where the version is not stamped but versions are known to exist, the
library determines it by every available means, declares its ceiling all
the same, and fails fast above it; an undeterminable version on such a
format is itself a named refusal. Where a format genuinely carries no
versioning, the claim is structural and P3 governs. Supporting a new
version is a deliberate release, never an accommodation made at read time.

### P9 — Interruption never invents a third state

P2 makes commit the only moment the image changes; this principle
armors that moment. An interruption at any point during commit — a
killed process, lost power — leaves state the next open reconciles
**before exposing the disk**, and after reconciliation the image is
wholly the old state or wholly the committed new state, never a partial
third state.

The durable undo journal beneath the overlay is private transient
state: no user-owned file, no cleanup verb, no contract about its shape
or location. The evidence for this principle is a fault-injection harness
that terminates a separate process after each durability boundary in
commit; in-process rollback tests are not evidence for it.

### P10 — Every refusal is machine-addressable

A refusal's human diagnostic (P6) is not its interface. Every error
carries, beside its message, a stable machine-readable **category** from
one enumerated set — the same category in Rust, in C, and in Python
(P5) — so an embedder maps behavior without parsing text no release
promises to keep. The set is deliberately small and cross-cutting: it
answers *how should the caller behave*, and it answers it for the whole
library at once. It is part of the surface — adding a category is a
surface change; rewording a message never is.

What a category cannot answer is *which rule did this input break*. So
where a format, namespace, or grammar defines a bounded set of rules an
input must satisfy, the error carries one field beside the category — a
**rule identity**, a stable machine-readable value naming the rule that
was broken, from the set owned by the seam that defines those rules. The
category still says how to behave and remains the interface an embedder
maps; the rule identity says which rule, and never substitutes for the
category. A refusal belonging to no such rule set carries none, and that
absence is ordinary rather than an omission. Each rule set is part of the
surface that owns it, and every presentation carries the same identities
(P5). Because the sets belong to their seams rather than to the library,
the identity is a value the seam spells rather than a second library-wide
enumeration.

The rule identity is not a second diagnostic. It names the rule, and P6's
human diagnostic still says what was expected, what was found, and where.

### P11 — Portable Rust comes first

Remanence is written as portable Rust, not as a Windows implementation
with incidental reach elsewhere. Core behavior avoids host-specific
assumptions unless the operating system forces them, and any necessary
platform-specific behavior is isolated behind a small internal boundary.
Public semantics stay the same across platforms; where they cannot, the
difference is a named refusal rather than a silent divergence.

Windows is the directly tested and wheeled platform today; other systems
are a soft portability obligation — expected to remain buildable from
source, and eligible to become tested platforms when repeatable CI or
trusted native builders exist. A support claim names the host tuple it
covers rather than letting an operating-system name imply every
architecture that system can run.

### P12 — Image formats are implementations at representation seams

Every supported image format is an adapter at the seam matching the
representation it persistently encodes. The adapter owns its identity,
recognition and evidence, validation and refusals, variants,
interpretation, capabilities, decoding, and encoding where writing is
claimed. Raw sectors, logical-block images, encoded tracks, flux
recordings, and filesystem-level images are distinct image-format
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
rest.

### P13 — One image layer is authoritative

Every loaded image has exactly one authoritative image layer, declared by
the image-format adapter that recognizes it — a file tree or filesystem
structure, addressed sectors or logical blocks, encoded tracks, flux
transitions, or another representation its family claims. Persistent
container bytes are its encoding, not automatically its image layer. Every
other representation is derived from the authoritative one: decoding
toward logical meaning, or deterministic synthesis toward a lower-level
mechanism. A decoded view carries its evidence; synthesized detail is
identified as synthetic and is never presented as recovered evidence.

An authoritative layer does not imply that every other layer exists.
Derivation stops at the seam claimed for the image and its integration
contract: a virtual hard disk presented through LBA never gains inferred
platters, heads, tracks, or flux, while a sector-addressed legacy image
may infer a hardware-level representation where its format, media profile,
integration contract, and synthesis rules together claim the mapping —
synthesized from the sectors, and not evidence of the original recording.

Block and flux are disjoint representation families. No adapter or
composition converts a geometry-opaque logical-block device into flux, or
flux into a logical-block device; this is a prohibition, not merely the
absence of a current derivation. Either family may still expose derived
sector, volume, filesystem, and file presentations without that
interpretation changing the durable active layer into the other family.

The authoritative layer does not change during an open image's lifetime.
A writable composition is offered only when every derivation on the path
can project its changes back to that layer and its image format without
unclaimed loss; otherwise that composition is read-only or refused before
use. Choosing another authoritative layer is an explicit conversion that
creates a new image and names any loss (P29); it is never a side effect of
loading, attaching, or saving the original.

### P14 — Media is independent recorded state

A media instance is the independent mutable state between image formats
and drives. It names an immutable, family-specific **article** containing
passive compatibility facts; the recorded contents belong to the instance.
Magnetic flexible, optical, and logical-block media are families whose
state and compatibility facts differ too much for one schema, so a family
owns its own representation and small interface. The article catalog
may be declarative precisely because articles are passive; it is not a
language for behavior, and an article outside it is refused by name (P3).

Three facts stay apart. What the medium **is** belongs to the article;
what was **recorded** on it belongs to the instance; what a **drive does**
to it belongs to a P30 drive profile. The same disk carries different
recordings and is served by different drives, so any statement collapsing
two of the three makes one article answer twice. The test for an article
fact is whether it holds of a blank disk in its sleeve (D19).

Image-format adapters load and save media state; hardware and CHS
presentations operate on that state through their own seams. An article
contains neither image recognition nor hardware behavior and
cannot implicitly choose how far hardware emulation descends. Every medium
the library holds names one enrolled article: a block medium is named by
the image-format adapter that loaded its state, a flux medium by the
family declaration of the drive profile it was mastered under, and a
medium created whole by its author by the kind the author declared. An
author who names a manufactured article gets that article's published
facts; one who states coordinates instead names the **authored**
article, which declares no physical fact because nobody made one — the
virtual family's rule holding rather than bending, as the archive's
entry already showed.

**The recording side is the device type, and it is a catalog of its
own.** A medium carries one — the device its content is assumed recorded
by — enumerated in two levels: the **class** (`Floppy`, `HardDrive` and
`Optical`, with `Tape` reserved for the coming family), then the
**concrete type** within it. A type the library does not know fails to
compile; its display string survives in provenance, refusals and the
cross-language spellings. Archives were recorded by no device, and
neither was a medium its author created whole; `None` is the honest
answer rather than a gap, and a medium answering it goes in no drive.

The **granularity rule** cuts that catalog: a device type is the coarsest
name fixing the whole addressing surface and recording discipline without
per-media parameters. What the device fixes lives in the type — which is
why the hard-drive specs carry the partition scheme itself, and why every
partition pool populates under the spec of the device that recorded the
medium, with the schemeless types bearing the direct partition. What
varies disk to disk lives on the medium. A device type **composes** an
article and restates none of its facts, so D19's three homes hold: the
substrate in the article, the recording in the device type, the drive's
behavior in the P30 profile.

A device type's definition has **one home**: one spec shape per class,
one instance per concrete type — the enumeration is the instantiation,
its disciplines flat attributes of the spec — while the traits live on
the medium, each surface answering only where the spec's attribute
holds, with P30 declarations reached through the type rather than passed
as arguments. An image-format adapter declares the device types it
records: one means the format carries it bare, several mean the load
declares which, and a pairing no adapter declares is a named refusal at
the load (P3) even where the class is right.

### P16 — Partition layouts are an independent seam

A partition-layout adapter consumes one addressed device or region and
exposes its child regions with the layout metadata and identities it
claims. Layouts may nest, so a child region can be offered to the same seam
again. MBR, GPT, and BSD disklabels illustrate variation at this seam;
naming them does not promise their implementation.

The adapter owns recognition, evidence, validation, refusals, and the
meaning of its regions. It does not open filesystems or decide whether a
region is a volume. An image container such as qcow2 ends at its addressed
virtual device and does not absorb partition-layout semantics.

The partition-schema catalog enumerates layout adapters, not the
individual partition-type bytes or GUIDs within a layout. Those values
belong to, and are interpreted by, the adapter for their containing
schema.

### P17 — Volume composition is an independent seam

A volume-composition adapter consumes addressed storage regions and exposes
logical volumes through one volume interface. A whole unpartitioned medium,
a direct one-partition volume, and a volume assembled across regions are
different compositions. A partition and a volume may overlap one-to-one,
but they are never synonyms and neither implies the other.

The adapter owns membership, mapping, identity, validation, and refusals.
A filesystem receives a volume and does not know whether it came from one
region or nested layouts. A future composition needing several devices
must argue and design that capability when it is proposed; this seam
neither forbids nor prepares for it.

An addressed medium with no partition layout can form one direct volume.
This is the ordinary legacy-floppy case, not a missing partition scheme or
a special public path: the direct composition preserves the separate
interface while requiring no partition choice from the caller.

### P18 — Filesystems are an independent seam

A filesystem adapter consumes one volume and exposes a P19 namespace
view of the names, metadata, and data operations it claims. FAT, HDOS,
CP/M, NTFS, ext, and other filesystems illustrate variation at this seam;
their mention is not a support claim. The adapter owns recognition and
evidence, structural validation, version and feature ceilings, refusals,
and all filesystem semantics it implements.

A filesystem does not parse an image container, discover the partition
layout around its volume, or know how that volume was composed. The
filesystem catalog and interface remain independent of those adjacent seams.

### P19 — The namespace is the common file-access seam

A namespace exposes a rooted tree of named files and directories, with
their metadata and data operations, independently of what backs that view.
This is the high-level convergence point for file access: a
supported composition may pass through archives, image
formats, partition layouts, volume composition, or filesystems, and every
file-bearing result presents this interface, retaining
the layers, identities, and evidence that produced it. Multiple roots or
ambiguous paths are exposed or refused explicitly rather than flattened or
guessed.

**File access lives on one node and nowhere else.** The type that carries
the file verbs is the namespace itself; a medium, a partition or a volume
may be asked what it *composes*, and may not be told to act as something
it isn't — a medium bearing `get_file` would be a category error in the
type rather than a refusal waiting to happen.

**The walk is uniform.** Every medium bears a partition its content is
reached through, so one path serves whatever a medium turns out to be: a
medium recording no partition scheme bears the library's own composition
of the whole content, declared as such, and a caller who knows nothing
about partitions still takes the step every other caller takes.
Drive and mechanism emulation are not constructed merely to reach files.

P19 is the usual high-level destination, not a universal content model. A
partition or volume may validly contain boot data, swap, database or object
storage, volume-manager metadata, or another claimed structure with no file
namespace; that result remains visible at its applicable seam. Remanence
neither calls valid non-file data empty nor manufactures pseudo-files to
force it through P19, and opening specifically for file access returns a
named absence or refusal when no file-bearing interpretation is claimed.

Two provider forms meet here. **A medium may bear its namespace
directly** — an archive, a flat catalog on an unpartitioned disk — the
grammar that recognizes the artifact being a P12 adapter at this seam
rather than a serialized form of its own; and **filesystem adapters
(P18) consume volumes**. Either way the result preserves the identity
and provenance of its sources rather than flattening or copying them.

**The seam presents what a filesystem names, and never what a guest
called it.** A drive letter, a mount point, a volume-GUID path — the
name one operating system's own configuration gave a volume — is a fact
about a *guest*, and this library composes no namespace over a machine's
several filesystems and derives no such name. Neither the reading nor
its refusal is offered: the question is outside the claim rather than
answered undetermined, and a consumer that wants it holds the volume
identity this library issues and maps it in its own terms. What a caller
gets here is one filesystem's own tree, reached through the volume that
composed it.

The namespace view is not a disk representation and declares no image
layer, media, geometry, partition layout, or volume semantics. Raw
partitions and volumes do not satisfy it. Selecting a file yields a byte
stream; only independent P12 recognition can make that file an image and
declare its authoritative layer under P13.

### P21 — Device identity is assigned, scoped, and unobtrusive

Every addressed virtual device receives an opaque identity when Remanence
composes it. The library assigns that identity; an ordinary single-image
open never asks the caller to provide or choose one. The identity is unique
within its containing open or composition, and implies no globally stable
or user-meaningful identity unless a later interface explicitly claims one.

Device identity qualifies provenance and otherwise-local identifiers only
where more than one device makes that distinction necessary. An interface
already scoped to one disk may continue to accept a disk-local identity
without exposing the device identity. A presentation may report the
assigned identity, but never makes callers echo a value that does not
affect the requested operation.

An attachment identity such as `hdd0` is distinct from device identity. A
caller supplies placement only when placement changes semantics and cannot
be inferred. This principle adds neither multi-device opening nor
multi-device volume composition; those require their own proposal.

### P22 — Magnetic recording can descend to timed flux transitions

For a magnetic-media family that claims a physical recording path,
Remanence can represent recorded state at the granularity of individual
timed flux-transition events, each carrying its timing and any
detection-strength semantics its layer claims. This is the lowest modeled
magnetic-data layer: analog head voltages, amplifier waveforms, magnetic
field shapes, and transistor-level behavior remain outside the model.

Where a composition claims that path, the flux layer is durable mutable
media state for the session, not a stream regenerated per read.
Track-relative flux and marker state survives controller interactions and
receives modeled writes; the timing mechanism projects that circular state
into ephemeral absolute-time read-channel events as rotation advances,
storing no second event history.

Flux is one channel, not the whole medium. Index and hard-sector holes and
other mechanical or sensor observations are separate timed state or event
channels, never folded into the flux-transition stream. Every adapter
states which timing, markers, revolutions, and weak-event semantics it
preserves, normalizes, synthesizes, or cannot represent. P64 is the
concrete lower-bound test: a P64 path preserves stored pulse position and
strength into the flux medium and a read-channel simulation consumes that
state with its weak-event semantics intact, so flattening the image to one
deterministic bitcell or byte stream does not satisfy this principle.

#### The family holds two models, capture and medium

**Flux capture** is timed transition evidence as an instrument recorded
it: several capture runs over one source location, the instrument's own
timebase, the source's own location identity, and parallel marker
channels. It asserts nothing about which revolution the disk *was*, and
never averages, deduplicates, or selects inside itself.

**Flux medium** is one circular pulse stream per location the family
addresses, in a declared rotational frame against a declared reference
clock, each pulse carrying the family's strength semantics. It asserts
exactly what a drive would read. What it adds beyond the flux — the
rotational frame, the addressing, the reference clock, the strength
vocabulary, and which surface is the disk — is declared by a P30 drive
profile, which is why it is a second model rather than a tidier first one
(D14).

**The boundary is one sentence: disagreement across observations is a
capture fact, and strength is a medium fact.** Turning the first into the
second is a P29 reduction, performed by neither model on its own
initiative. What the medium must **not** hold keeps it below the layer
above: no bitcell, no recovered clock, no synchronization, no symbol, no
byte.

The flux floor is an internal modeling capability, not a universal public
interface, and this principle creates no public flux or pulse iterator.

### P23 — One durable layer is active

Every independently mutable open state instance has exactly one **active
layer**: the durable representation against which all of its current
presentations read and, when permitted, write. The active layer is runtime
artifact or media state — not an image-format choice, not a derived cache,
and not the hardware emulation layer. Several presentations over one
instance share it; they never maintain independently mutable file, sector,
track, and flux copies of the same state. P33 governs how an instance's
active layer is chosen and changed; this principle states what one is.

Here **durable** means the representation survives runtime interactions as
the instance's continuing mutable truth and is the source offered to P2
commit. It does not mean already serialized, crash-durable, or necessarily
encodable by the source image format, and it does not fix residence: under
P27 the state may be resident or spilled to private session storage.

The durable active-layer vocabulary is exactly:

| Active layer | Durable session state | Claim |
|---|---|---|
| **namespace** | a rooted tree of named entries and nested directories, entry bytes, and claimed metadata | namespace structure, not disk allocation or recording |
| **flux medium** | circular track-relative flux transitions and strength semantics, with marker/sensor channels and provenance | a modeled magnetic recording surface |
| **hardware bitstream** | circular track-relative clocked bit state, with the timing and provenance its declared drive family requires | what a family's read channel resolved, not what it means |
| **encoded bytestream** | the circular track-relative byte sequence a declared family codec materializes from that bit state | the recording's own bytes, before any of them is a header, a sector, or a file |
| **CHS** | records addressed by cylinder, head, and sector under a declared geometry | geometry and records, but not their physical encoding |
| **block** | geometry-opaque logical blocks addressed by number | no cylinder, head, track, recording, or mechanism claim |

These are six family-owned representations, not variants of one universal
schema. CHS and block both carry record bytes, but CHS's declared geometry
is observable and load-bearing while block deliberately hides it. Hardware
bitstream is pre-synchronization and pre-decoding — a bit cell is not a
symbol, a byte, a sector, or a file — and encoded bytestream is what one
declared family codec resolves out of it, having located only the family's
declared framing landmark and claiming nothing about what follows it.

**Flux capture takes no row.** It is an authoritative image layer under
P13, read by inspection and by mastering, and it never carries a session's
mutable truth, because a drive writing to a capture would have to choose
which of several disagreeing observations to overwrite (D14). A capture
becomes a medium by mastering (P29), never by an unnamed normalization.

Encoded tracks, bitcells, nibbles, and filesystem structures may be
authoritative image layers or derived representations, but they are not
additional durable active layers; a composition materializes them into the
applicable flux, CHS, or block state before service begins. P19's
namespace interface does not by itself make namespace the active
layer: over a filesystem it is a derived presentation whose mutations
project into the media's active state, while over an archive
the named-entry state itself is active. Nested artifacts have one active
layer per independently mutable instance, not one for the whole object
graph — opening `archive.zip/disk.d64` can leave the outer ZIP active as a
namespace while the entry is a child disk image with its own active
media instance.

P13's authoritative layer and the active layer answer different questions:
the first states what the loaded artifact records and what its format can
persist, the second states which representation currently carries the
session's mutable truth. They may coincide or differ, and changing the
active layer neither promotes synthetic state into recovered evidence nor
changes the authoritative image layer.

Writes land in the active layer, and commit remains governed by P2 and
P13: the original image is updated only when every change projects back to
its authoritative layer and encoding without unclaimed loss. Each active
layer caches under P27.

### P27 — Sessions stream; memory holds a bounded working set

Remanence is sized by the operation, never by the artifact. A source may
be a floppy image of a few hundred kilobytes or a virtual disk of a
hundred-plus gigabytes; the same journeys serve both, so no
representation — a source's encoding, the session's durable state, a
derived view, or the uncommitted write set — is ever loaded whole as a
design assumption. An operation may visit bytes in proportion to its task;
it may hold only a bounded working set. A whole layer may be held only
when its format bounds it beneath the working set; every other path
streams, and a format that resists streaming is materialized to private
session storage, never to memory.

Every session's durable state has one backing. It is **source-backed**
when bounded random access is served directly from the source encoding,
reads streaming on demand through the session cache. It is
**session-backed** when it cannot be — a decoded representation whose
encoding permits only sequential access — and is then produced once by a
streamed transform into private session storage and served from there
through the same cache.

Caching is per modeled durable layer, under one declared session budget.
The active state's cache carries the session's mutable truth in two
residency classes. **Clean state is always evictable**, droppable and
re-read from its backing at will, which is sound because the P7 claim pins
the source; a small image simply becomes fully resident while a huge one
converges on the operation's locality. **Dirty state is never dropped**:
alteration is tracked at extent granularity, uncommitted changes hold in
memory within the bound and spill to private session storage beyond it
(P2), eviction moves them, only rollback discards them, and commit
projects them. A derived view's cache holds only clean state: its writes
complete into the layer below in the same act or alter nothing, a write
landing in a lower layer invalidates the overlapping derived extents above
it, and eviction regenerates from below.

The library may thread its work — prediction, prefetch, offload — and
P34 governs that concurrency: the budget its threads spend is this
principle's, and nothing they do is observable.

Commit, materialization, and recovery stream through bounded buffers;
identification probes read the bounded evidence their claims name; private
session storage takes the shape P9 gave the journal; and the bound and its
read-ahead are declared session configuration with a stated default, never
discovered behavior. Public presentations carry the same rule: an
operation whose result is proportional to source content offers a bounded
or streamed form in all three presentations (P5), with whole-value
conveniences beside it, never as the only route. This principle constrains
resources, not semantics — behavior is identical at every source size, and
peak memory bounded independently of source size is its testable claim.

### P28 — Evidence may narrow authority without discarding readable evidence

Fail-closed is a rule about authority, not a command to discard every byte
whose complete intended interpretation cannot be proved. An image may be
recognizably incomplete, contradictory, or only caller-described, yet
still contain a bounded region the library can read without inventing
bytes or concealing the defect. In that case the library retains the
evidence and offers only the operations whose preconditions it can
establish.

Every open therefore has one explicit **assurance outcome**: **verified**,
where the selected interpretation and every bound the requested operation
needs are evidenced; **degraded**, where a material shortfall or
contradiction is known but a truthful read-only interpretation of a
bounded portion remains; or **refused**, where no bounded interpretation
exists or an operation needs the missing or contradictory fact. The
transition from verified to degraded is a deterministic safety gate, not a
second score beside P4's recognition confidence. A declared size exceeding
the source, contradictory required structure, a caller assertion the
source disproves, or a read reaching an unavailable extent fails that
gate, and the report states the evidence, the resulting bounds, and the
withheld operations. An explicit caller selection is an interpretation
request, not a waiver of evidence.

Degradation is not repair. The library does not fabricate missing sectors,
skip damaged structures, choose an unresolved interpretation, or continue
after losing the bounds that make a result meaningful; a read entering an
absent extent is a named unavailable result, never zero-filled, shortened,
or successful.

The degraded path is deliberately narrow: it applies only while
determining a catalog type or reading or writing through an already
selected one, and never to the library machinery around that
interpretation. Failure to acquire or use the host claim, to read or write
the session cache or private storage, to persist the commit journal, to
allocate a resource, or to perform host I/O is an immediate P6 failure,
which can never be re-described as imperfect media evidence.

Degraded state revokes mutation authority for the session. A write-intent
open reports an evidence-driven effective read-only mode and a stable
condition, and every write, commit, and mutation-capable derived operation
is refused with that condition; a session never regains write authority
without a new verified open. P7's no-silent-fallback rule still governs an
inability to acquire host access — this is a distinct restriction after a
safe claim has been made. The outcome, its evidence, the resulting bounds,
and the effective mode appear equivalently in all three presentations (P5).

The conditions the gate names are an enumerated claim (P3) owned by this
seam, and they are the rule identities (P10) a withheld operation's
refusal carries. Which interpretations the gate is armed for is likewise a
claim: this principle says what an armed interpretation owes, not that
every interpretation is armed, and arming another is a feature.

### P29 — Mastering is declared, reproducible, and states its loss

**Mastering** is deriving a new representation from evidence Remanence
already holds: solving several capture runs into one circular medium,
choosing among channels and observations, projecting one timebase onto
another, and expressing the result where the destination cannot carry
everything the evidence holds. **Only the destination varies** — usually a
new artifact, equally an active layer materialized inside a session
(P33) — and the policy inputs, the plan, and the declared-loss account are
the same either way (D14). P13 already permits the act; this principle
says what the act must carry, because a conversion that reduces evidence
silently is indistinguishable from one that preserves it.

**Mastering is requested, never incidental.** It is not a side effect of
opening, attaching, presenting, or saving. The sources are read and
nothing else — their layers and provenance are unchanged — and the result
is separate state, carrying its own authoritative layer where it is an
artifact.

**Every reduction is a named policy input.** Which channel supplies
evidence; which observation of a location is used and how several are
reconciled; how source location identity maps onto the destination's
addressing; how the source timebase projects onto the destination's; and
how weakness, absence, disagreement, and contradiction are expressed in
the destination's vocabulary — each is supplied by the caller or declared
by the profile, and each travels into the result as provenance. **A
reduction that no policy names is a refusal, not a default.**

**Two owners, and neither infers the other's answer.** The family
mastering profile owns the physical reduction; the destination
image-format adapter owns its grammar, version claim, encoding, and named
refusals (P8, P12). A profile does not decide what a container can hold,
and an adapter does not decide which revolution the disk was.

**The loss is declared before anything is produced.** A mastering
operation resolves in two stages: a plan which computes the whole
transformation and produces nothing, and an execution which produces the
result. The plan enumerates, in the source's own terms, everything the
destination will not carry. A count is not an account; loss reported after
the fact does not satisfy this; and a reduction the plan did not declare
is a defect, not a detail.

**The result is derived and says so.** Mastered content carries
selected-and-projected or synthetic provenance under P13, never
recovered-evidence provenance.

**Mastering is reproducible.** The same sources, policy, and declared seed
produce the same mastered state, and the same bytes where the destination
encoding is itself deterministic. A transformation which cannot state what
makes it vary is refused rather than shipped as approximately repeatable.

P2, P6, and P9 apply unchanged: the sources are never mutated, nothing is
written until every check has passed, and an interruption leaves a
complete destination or none. This principle pledges no
destination format and creates no public evidence iterator: the mastering
plan and its declared-loss account are the surface, and the evidence stays
behind them.

### P30 — Drive profiles are an independent seam

A **drive profile** consumes flux evidence and declared context and exposes
one family's recording conventions together with a recognition verdict over
that evidence. It owns how the family's source positions map onto its own
addressing and how many steps a location takes; its rotation rate and
reference clock; its density or zone map; the timing shape of its encoding
landmarks; which surfaces it records; the selection or variation rule by
which several observed revolutions become one served medium; the
read-channel rules by which a medium's pulses become clocked bit cells; and
the family's group code. Each is a declared fact of the family, carried
with its provenance — never arithmetic a capture is assumed to justify.
This is the seam holding the knowledge P22 and P23 both rest on and
neither owns.

**Recognition is a probe that carries its evidence.** A profile is offered
the evidence and answers with a bounded, comparable confidence and the
observations that produced it (P4). Several profiles may claim one capture
and the verdict is ranked; a capture no profile claims is a named refusal
(P3), never a default or the single enrolled entry winning by being alone.
**Discovery proposes and never silently decides**: a caller may pin or
override a profile, and what the library chose travels into the result as
provenance.

**A profile recognizes structure, never content.** It may read flux
interval lengths and the patterns they form, and report a count, a
density, an angle, a location, or an absence; it may not resolve a bit
value, assemble a byte, name a sector, or validate a checksum. The test is
what leaves the probe: **an angle, never a byte** (D12). A protection whose
evidence is a deliberately wrong checksum is therefore invisible here by
design, and is carried faithfully by a layer that never interprets it.

A profile is not hardware emulation, which generates timed causality from
state a profile helped materialize, and not a P12 image-format adapter,
which owns a container's grammar. A profile owns what a family does to
media. The profile catalog is wiring, as P12's is: every entry pairs a
descriptor with behavior, and central orchestration neither branches on a
profile identifier nor interprets string-named family rules. This
principle pledges no family and creates no public flux, pulse, or
capture-run iterator: the verdict and its evidence are the surface.

### P33 — Active-layer descent is requested, atomic, and never reversed

P23 states what an active layer is; this principle states which one an
instance starts at and how it may change.

**The ladder is family-owned and one-directional.** The magnetic ladder
reads: flux capture → flux medium → hardware bitstream → encoded
bytestream → CHS → filesystem. Block is terminal and disjoint from all of
it. No descent crosses between the block and flux families in either
direction, and derived filesystem or file access over either family is a
presentation over existing active state rather than an intermediate
conversion.

**A composition starts as high as it can.** For a disk, the initial active
layer is the least physically expressive durable media layer which
faithfully serves every presentation requested when the composition is
formed. A Commodore DOS device over a standard sector image can use CHS,
generating no track, flux, head, or rotation state; an LBA device uses
block and cannot be lowered merely because another family knows CHS or
flux.

**Descent is requested, never incidental.** Service below the active layer
requires a new one to be materialized first, and nothing about opening,
attaching, or presenting descends on its own initiative. Materializing
downward is a P29 mastering act whose destination is an active layer
rather than an artifact, so P29's named policy inputs, its plan, and its
declared-loss account govern it unchanged (D14). **Generate-flux is
generate-medium**: it synthesizes a flux medium and never a capture,
because fabricating instrument evidence from sectors would be a false
provenance claim. It materializes circular, track-relative media state and
not runtime pulse occurrences — mechanism state such as head position,
motor speed, rotational phase, settling, and read-channel history never
becomes part of the active media layer.

**Every descent is atomic**: it is whole or it refuses, leaving the old
active layer in place. Once the new state is validated it replaces the old
one as the single durable mutable session state; existing higher
presentations are rebound as derived views and their caches invalidated,
and they may decode upward but cannot continue mutating the former copy.
Each descent carries the profile, the codec, and the source's own policy
as provenance.

**The layer never rises again.** It does not rise during that open
lifetime merely because a lower presentation closes, since doing so could
discard state the higher layer cannot express. Returning to a higher
representation requires closing the composition, or a family-permitted
explicit conversion which names its loss; no such conversion exists
between block and flux. None of flux medium, hardware bitstream, or
encoded bytestream is writable today, which is why a medium and the
bitstream above it may both be held without either becoming a second
mutable instance.

A descent changes where writes land, never what the artifact records.
P13's authoritative layer is unchanged by it, and a writable composition
whose lowered state would acquire a change that layer cannot represent is
refused in advance rather than silently flattened.

### P34 — Concurrency is observationally invisible

The library may use threads to predict, prefetch, and offload —
speculatively reading ahead of an access pattern, deriving ahead of
demand, spilling ahead of pressure — with the standard library's threads
alone. Four rules keep every thread undetectable:

- **Speculation produces only clean state.** A speculative read installs
  evictable state or nothing; dirty truth is never created ahead of a
  caller's act.
- **Offload never gaps the truth.** An altered extent leaves memory only
  once its spill write has completed, and every act that consumes the
  altered set joins the offloads in flight.
- **Demand outranks prediction.** The threads spend P27's declared
  budget, and a caller's read is never starved by a guess.
- **Speculation is silent.** A failed speculative read caches nothing
  and reports nothing.

The testable claim: results, evidence, and refusals are identical with
any number of threads, including none. Concurrency is a resource
strategy, never a semantic one — a caller cannot learn the thread count
from anything the library says or does.
