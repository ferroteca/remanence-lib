<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Image-format modules and the built-in catalogs

Design for
[F19](../FEATURES.md#f19--image-format-modules-and-the-built-in-catalogs),
serving pledged P12–P19, P21–P23, and P27 in
[ARCHITECTURE.md](../ARCHITECTURE.md).
This document specifies the destination and delivery cut for the pledged
feature; it is not an implementation record.

## The problem

The registry was introduced to make an image format cheap to add. Its text
language can presently express names, extensions, one fixed geometry, a
leading signature, filesystem candidates, and string-named detection
heuristics. Unknown attributes are retained but have no behavior. The
library interprets the known strings centrally, while implementations
that do real work already sit elsewhere: qcow2 virtual-disk walking is
selected by its identifier, HDOS operations live in their own module,
and the disk stack selects raw and qcow2 independently of the registry.

That shape makes the easy case declarative by making every harder case a
change to a public language and its interpreter. It also separates the
claim from the implementation: a definition can name a format for which
the library has no behavior beyond generic scoring. The public cost is
larger than the leverage — S1 and S3 present the registry, S4 is the
definition language itself, and the C presentation has no corresponding
capability.

The objective is retained and made stricter: adding an ordinary image format
must be a local addition. Code is not the failure; dispersed knowledge is.

## The seams are by role

`image format` is a domain category, not one universal Rust trait. Image
formats enter at different semantic seams and produce different things:

- a **serialized-file-container adapter** (P19) recognizes a container and
  exposes a file-container view;
- an **image-format adapter** (P12) recognizes an image container and
  exposes its media or guest-visible byte device;
- a **partition-layout adapter** (P16) recognizes a partitioning scheme
  and exposes addressed child regions;
- a **volume-composition adapter** (P17) recognizes how one or more
  addressed regions form logical volumes;
- a **filesystem adapter** (P18) recognizes a volume and exposes a P19
  file-container view; and
- a **namespace-composition adapter** (P19) maps file containers into one
  composed file-container view.

Each adapter role has its own small input interface. Filesystem,
serialized-container, and namespace-composition adapters deliberately
converge on the P19 file-container output because callers perform the same
file operations there. Nothing else shares an `open() -> AnyMedia`
interface or a bag of optional methods. ZIP, qcow2, MBR, FAT, H8D, HDOS,
and CP/M remain free to need different inputs and implementations.

Partition layout, volume composition, and filesystem are real seams, not
merely stages named by central orchestration. They are related but not
interchangeable:

- a partition-layout adapter consumes one addressed device or region and
  returns addressed child regions; MBR, GPT, and BSD disklabels are
  examples, and one layout may occur inside a region produced by another;
- a volume-composition adapter consumes one or more addressed regions and
  returns logical volumes; a direct one-region volume and an
  LVM-like volume assembled across regions share this output interface;
  and
- a filesystem adapter consumes one volume and returns a P19 file-container
  view of the namespace, metadata, and data operations it claims.

A partition is therefore not a modern spelling of volume. They often
overlap one-to-one, especially in simple images, but neither implies the
other: an unpartitioned medium may be one direct volume, a partition may
carry no claimed volume, and one volume may span several regions.
Composition passes only the preceding seam's output interface forward, so
no adapter knows an adjacent adapter's identifier or implementation.

VHDX, qcow2, and comparable image containers end at an addressed virtual
device. A device they expose may contain partition layouts and volume
metadata; none of those semantics belongs to the image container.

## P21 assigns device identity inside composition

An image-format adapter that exposes an addressed virtual device hands it
to composition with a new opaque device identity. Composition creates the
identity after resolving the image format; source paths, format identifiers,
catalog order, and attachment names never become the identity. It need be
unique only within the loaded composition. A one-device composition assigns
its sole identity silently.

Device identity is internal structure and provenance, not a required caller
datum. An interface already scoped to one disk keeps its disk-local volume
identifiers, so callers may continue to use `partition:1` without spelling a
device qualification. A presentation may report a device identity when it
helps explain a composed result, but ordinary opening and file access never
ask the caller to echo it.

Hardware placement is separate. A later topology may attach a device at
`hdd0`, but that attachment name does not become its identity and is asked
for only when placement affects the requested result. F19 establishes
identity, not multi-device loading, volume assembly, or cross-source commit.

The existing block `Device` seam remains the common representation for
direct random-access disk and filesystem work. It is also the right media
representation at an LBA seam. It is not the representation for a
signal-level floppy drive: sectors cannot express rotational position,
transition timing, index and sector holes, damaged or neutral regions, or
other facts visible to a physical head and its sensors.

## P13 and P23 separate authoritative and active layers

Loading an image does not make every representation equally true. The
recognizing image-format adapter declares exactly one authoritative image
layer: the semantic level at which the persistent image actually records
information. Examples include:

- a file tree or filesystem structures for a filesystem-level image;
- addressed sectors or logical blocks for a sector or block image;
- encoded tracks or bitcells for a track image; and
- timed flux transitions for a flux image.

The serialization that stores this information may add a header,
compression, or allocation metadata. Those persistent bytes are the
format encoding; they are not automatically another image layer. A P19
file-container view is separate: selecting one of its files produces a new
byte stream, which must independently enter P12 before it is an image or
has an authoritative layer.

Other representations are derived through family-specific adapters. A
lower-level recording can be decoded toward sector, filesystem, and file
presentations, with ambiguity and evidence preserved under P3 and P4. A
legacy magnetic image may be materialized toward tracks, bitcells, or flux
only by synthesis inside its declared family. The chosen geometry,
encoding, gaps, timing, and other defaults are synthetic provenance, not
observations recovered from the image. Geometry-opaque logical blocks and
flux are mutually non-convertible in either direction. There is no
project-wide total ordering forced across magnetic, optical, and
logical-block families; each family owns only its permitted derivations.

Each independently mutable open state also has exactly one active durable
layer: file container, flux, CHS, or block. This is the state against which
all current presentations read and write. The active and authoritative
layers commonly coincide, but answer different questions. The authoritative
layer says what the source image persistently records; the active layer says
which durable representation currently owns mutation.

A request below an image's information floor triggers one explicit
materialization to the most honest lower layer available. For example, a
sector-authoritative C64 image may synthesize timed flux under a declared
media and encoding profile, after which flux is active while sectors remain
authoritative. Provenance identifies every synthesized fact. Sector,
filesystem, and file views over that flux are derived presentations, never
independently mutable peer copies.

The graph is deliberately incomplete. Opening a typical virtual hard-disk
image through an LBA drive ends at logical blocks. Remanence never
materializes speculative platters, heads, tracks, timing, or flux beneath
it, and no flux image is converted into an LBA device. This is a permanent
family separation rather than a missing adapter. By contrast, a raw sector
C64 image may be attached to a 1541 composition that claims a
sector-to-track and track-to-signal derivation.
Its track, bitcell, or flux state is then inferred by deterministic
synthesis from the authoritative sectors plus the selected media and drive
rules. It is suitable for hardware-level emulation, but it remains
synthetic provenance rather than a recovered observation of the original
disk.

The authoritative layer is fixed for an open image. Read-only derivation
may be lossy when the result says what was omitted or synthesized. Writes
land in the active layer. A writable composition is assembled only when
every adapter on the path claims that those changes can be projected back
through the authoritative layer and encoded by the original format without
unclaimed loss. If arbitrary hardware-level writes cannot be represented by
a sector image, for example, that image cannot be attached writable through
that path; the refusal occurs before emulation begins. An explicit
conversion may create a new image whose authoritative layer differs, with
losses named. Loading, attaching, writing, and saving an existing image
never change its authoritative layer silently.

## P27 sizes every access by the operation

Every interface in this design is stream-shaped. An image-format adapter
exposes its addressed device or media state through bounded range access,
never a whole-source buffer; a serialized-file-container adapter exposes
entry bytes the same way; recognition probes read the bounded evidence their
claims name. Decoding, encoding, and materialization are streamed
transforms. The bounded session cache, its extent-granular alteration
tracking, and private spill storage are shared mechanisms owned once — one
cache per independently mutable state instance, shared by every presentation
over it — while each adapter owns the mapping that makes its encoding
randomly accessible, or the declaration that it is not, which is what routes
its layer to source-backed or session-backed service under P27.

Delivering F19 with whole-image residency assumed anywhere would leave P27
unarmable without reopening every adapter interface, so the constraint binds
the first implementation, not a later optimization pass.

## P19 makes file containers the common file-access seam

The file-container interface is a semantic view, not a storage format. It
provides a rooted namespace of named files and containers, metadata, and
data operations. It is the deep, common answer to the user's request to
open an artifact and reach the files inside, however many lower seams are
needed to get there. Three distinct adapters demonstrate that the seam is
real:

- a serialized-file-container adapter turns a ZIP-like byte stream into a
  file-container view;
- a P18 filesystem adapter turns a recognized filesystem on a P17 volume
  into the same view; and
- a namespace-composition adapter maps file containers at explicit roots
  or paths and exposes the result as another file container.

The last form models a Windows namespace with drive letters, volume mount
points, and folder mappings, or a Unix root with mounted filesystems. A
source container's identity and backing volume remain available as
provenance; composition does not flatten the sources or pretend they share
a filesystem. A raw partition or volume cannot enter P19 directly. It
first needs P18 filesystem semantics, while a serialized container needs
no disk representation at all.

Opening for file access returns the applicable file-container view or a
named refusal. A simple image may return one filesystem root directly. A
multi-volume image or composed machine namespace may expose stable child
roots without flattening them. Recognition ambiguity remains visible with
its P4 evidence; orchestration never chooses a filesystem, volume, or
contained image merely to manufacture a convenient answer. Lower-layer
inspection remains available alongside this high-level view.

For a simple legacy floppy, distinct seams impose no user ceremony. Its
image-format adapter exposes the recorded sector or block representation;
the whole addressed medium forms one direct P17 volume; and its P18
filesystem adapter exposes the P19 file container. If each result is
unique, opening for file access composes them automatically and returns the
filesystem root. The layer report still names every decision and its
evidence. P14 media and a P15 drive are composed only for an operation that
needs drive behavior, not for ordinary listing or extraction.

The convergence is conditional on file semantics. Valid boot regions,
swap, databases or object stores, volume-manager metadata, and other
structured payloads may terminate at P16, P17, or a later purpose-specific
seam without producing a file container. They remain identified data, not
failed or empty filesystems. The composition layer does not invent a
directory of pseudo-files to make P19 universal. A request specifically
for files receives a named absence or refusal while lower-layer inspection
continues to expose the recognized content and its evidence.

F19 establishes the common output for its existing serialized-container
and filesystem paths. System-wide namespace reconstruction and mapping are
architecturally admitted by P19 but require a later feature and surface
vetting.

## P14 makes media an independent composition

A media instance is the mutable state between an image-format adapter and a
drive. It names an immutable profile from the catalog and carries the
recorded state that changes when the drive writes. The media catalog is
divided by family rather than forced into one schema:

- magnetic flexible media profiles describe physical compatibility such
  as form factor, sides, magnetic characteristics, and index or sector
  holes; their instances carry magnetic surface state as timed flux-data
  transitions and strength semantics, with index and hard-sector topology
  represented through separate marker/sensor channels carrying their own
  captured or fabricated provenance;
- optical profiles describe the applicable CD, DVD, dual-layer DVD,
  Blu-ray, or later variant; for modern drives their instances carry only
  the tracks, sectors, subchannels, and other recorded state visible at
  the common drive interface, not invented pickup or servo internals; and
- logical-block profiles describe the block sizes and bounds visible at
  an LBA seam; their instances carry the logical block address space and
  no invented platter geometry.

These are passive profiles, so their catalog entries may be declarative.
They do not contain recognition heuristics or programs. A user-facing
name such as a 1.44 MB floppy may be a creation preset combining a 3.5
inch high-density, double-sided profile with a conventional recorded
layout; the sector count and encoding are not intrinsic physical
properties of the medium.

## P15 supplies one common hardware-emulation layer

Remanence does not own a catalog of modeled drive products. It owns one
timed-causality hardware layer whose reusable timing, mechanism,
read-channel, and electronics modules are composed behind typed integration
contracts. A contract selects a useful real seam and its physical profile;
the image format and media profile never select that seam implicitly.

Every typed hardware presentation uses the same timed-causality lifecycle:
reset, current time, next outward deadline, advance, one timestamped
interaction, and side-effect-free inspection. The contract binds that
lifecycle to typed stimuli, responses, inspections, events, time, media
slots, and configuration. The common contract is therefore neither a
universal pin map nor a public byte, bit, pulse, or sector stream.

The Commodore 1541 fixes the lowest required public cut. The emulator owns
the drive CPU, memory, firmware, IEC bus, both 6522 VIAs, and their register
and interrupt behavior. Remanence owns the drive-side electronics below the
disk VIA, including data separation, sync detection, byte assembly, weak-
signal behavior, mechanism, and medium. Timestamped motor, stepper, density,
byte-ready-enable, read/write-mode, and port-A-drive state cross into
Remanence; port-A data, sync, byte-ready, and write-protect signals cross
back at their causal times.

Other typed presentations may place the public cut elsewhere while retaining
the same lifecycle. A Disk II presentation may include its controller and
expose slot-bus transactions; H17 and MITS 88-DCDD presentations may expose
timed programmed-I/O transactions. These examples test the common layer's
required generality; they are neither drive-catalog entries nor pledges of
the named support.

Below every public cut, Remanence may compose private mechanism, medium,
read-channel, and controller modules as one signal graph. Those private
modules can exchange timed flux transitions, marker/sensor changes, motor
and head controls, recovered bits, or latched bytes without making any of
those an additional public interface. Rotational phase, head position,
settling, controller continuation, and pending causal effects are ephemeral
hardware state and never become an image layer.

One hardware composition accepts zero or more typed media attachments.
Each occupied slot owns a separate claim and independently mutable P23
active-layer instance; drive selection remains ephemeral hardware state.
The composition may mutate only the selected active media state, and P2
commit still decides whether that change projects honestly to the image's
P13 authoritative layer.

For LBA storage, the block interface itself is the seam:

1. an image or container adapter exposes a logical-block media instance;
2. the common block-hardware presentation exposes the claimed block sizes,
   bounds, operations, and externally observable refusals; and
3. the emulator consumes that LBA interface.

Remanence does not derive cylinders, heads, zones, platters, servo
behavior, or transition streams underneath those logical blocks. It does
not emulate the integrated controller's algorithms, firmware, or
microcode. Those internals may vary while the external block contract
remains useful and stable.

CHS is a required common interface for older storage contexts. A CHS adapter
addresses the recorded cylinder, head, and sector layout without emulating
controller registers, commands, encoding, firmware, or microcode. Where
the same composition also supplies a hardware presentation, both
presentations operate on the same media instance, so activity at either seam
changes one recorded state. CHS is not a physical-media profile and is never
inferred as hidden geometry beneath an LBA drive.

Modern large storage and optical hardware follows the same default. CD, DVD,
dual-layer DVD, Blu-ray, and their variants are exposed at the applicable
common command, track, sector, and subchannel interface. Remanence does not
descend into laser pickup behavior, focus and tracking servos, physical mark
detection, internal error correction, firmware, or microcode. Those facts
may distinguish media profiles without becoming emulated internals.

There is no drive descriptor, drive enrollment, or drive catalog. Typed
integration contracts and physical profiles configure the one common layer;
they do not form a product-discovery mechanism. Shared electromechanical,
connector-signalling, controller, and block behavior stays inside that deep
module rather than being presented as separately selectable drives.

Writes made through a hardware presentation alter its media instance inside
the explicit writable session. They reach the image only at its commit point,
can be rolled back before then, and retain the file claims and reconciliation
guarantees of P2, P7, and P9. A hardware-integration seam is not an excuse to
invent a second mutation policy.

This design establishes the media and hardware boundaries but F19 does not
add media-family modules, the common hardware-emulation layer, typed hardware
presentations, or an emulator presentation. Those later capabilities need
their own features and surface vetting. Building them against P14, P15, P22,
and P23 will not require reopening the image-format architecture.

## One module owns one image format

An image-format module owns every rule that distinguishes that image
format from its peers, as applicable:

- stable identifier, display name, extensions, and capabilities;
- variants and the context that can disambiguate them;
- recognition, confidence, and human-readable evidence;
- structural validation, version ceilings, and named refusals;
- its authoritative image layer and the derivations it claims;
- decoding into the representation at its seam;
- encoding, including whether saving is supported and what cannot be
  represented without loss;
- focused unit tests and the construction of synthetic specimens.

Relations that belong to an image format stay with it. For example, an H8D
adapter owns its supported geometries and how their ambiguity is reported;
an HDOS adapter owns the observations that identify HDOS. Central code may
compose image candidates with filesystem candidates, but it does not know
what either candidate means.

The source layout should make that ownership visible. Whether an image
format is one file or a directory is an implementation-size choice;
neither splits its descriptor, probe, and validation among central tables.
Existing deep implementations such as `qcow2.rs`, `fat.rs`, and `hdos.rs`
may remain where they are and satisfy adapters locally. Directory movement
earns its keep only where it improves ownership; it is not a goal of F19.

## Shared mechanisms stay deep

Repetition is removed behind internal helpers only after at least two
image formats demonstrate the same rule. Likely shared modules include:

- descriptor and capability vocabulary;
- probe aggregation and evidence handling;
- exact byte-reading and checked-range helpers;
- a fixed-sector-layout helper once multiple raw formats share it;
- a physical magnetic-media representation for claimed legacy mechanisms;
- the P27 session cache, alteration tracking, and private spill storage.

The helper owns the mechanism; the image format owns every value and
choice that gives it meaning. A helper for a coherent image-layout or
track-encoding family may therefore be table-driven and deep. An image
format that does not fit uses ordinary code behind the same seam. It does
not add optional fields or new opcodes to a project-wide format schema
merely to avoid having a module of its own.

## Catalogs are role-specific wiring

The catalog families are role-scoped:

- the **image-format catalog** and **filesystem catalog** enumerate executable
  adapters which recognize, validate, and interpret their inputs;
- the **partition-schema catalog** enumerates MBR, GPT, BSD disklabel, and
  other layout adapters—not their individual partition-type codes or GUIDs;
- serialized-file-container recognition uses its own adapter catalog rather
  than entering the image-format catalog; and
- the **media-type catalog** enumerates passive compatibility profiles, not
  recognition programs or hardware behavior.

For every adapter catalog, a descriptor and its behavior are enrolled
together; there is no descriptor-only entry. Enrollment is the one expected
central edit when an implementation is added, and it is mechanical: central
orchestration iterates adapters and never switches on an implementation
identifier. Passive media-type entries may instead be declarative because
their profiles contain facts rather than behavior.

P15 deliberately does not give hardware presentations or modeled drive
products a catalog; typed contracts configure one common hardware-emulation
layer outside F19.

Catalogs are internal. F19 introduces no plug-in mechanism, dynamic
loading, or caller-authored catalog. If a presentation needs to list
supported image formats later, it may receive a read-only projection of the
built-in descriptors as a separately vetted generic capability. That is
not required to identify, open, or use an image and is not part of F19.

Tests may construct a catalog from an explicit adapter slice. This is an
internal test seam, not the return of `Session::open_with_registry`: it
admits executable implementations compiled with the crate, never
declarations whose semantics the crate does not know.

## Probing and ambiguity

An adapter returns one of three semantic results:

- **no match** — the format has no affirmative evidence and makes no
  claim;
- **match** — confidence plus the human-readable observations that
  support it;
- **recognized but invalid** — affirmative format evidence was found,
  followed by a structural, version, or feature refusal carrying its
  category and diagnostic.

Filename extension is supporting evidence only. It cannot turn bytes an
adapter rejected into a match. Evidence strength is comparable across
adapters at one seam; the catalog selects a result only when one strongest
match exists. A tie is not broken by enrollment order or identifier
spelling. The layer remains unknown and its evidence names every tied
candidate and the observations supporting each, satisfying U1 without a
guess.

Once evidence specific enough to recognize a format has been followed by
an invalid or unsupported structure, the refusal is not discarded so a
weaker adapter can claim the same bytes. P6 applies at recognition just as
it does during an operation: the known cause is reported.

The current public `confidence: u8` and human-readable evidence can remain
the presentation of this decision. F19 need not expose the internal probe
types. Any public change needed to represent a recognized refusal or an
ambiguous layer lands across S1, S2, and S3 under P5 in the same feature;
otherwise the existing unknown layer plus its evidence is used.

## Public-surface landing

The format-definition experiment is removed rather than deprecated:

- **S1:** remove `FormatRegistry`, `ContainerFormat`,
  `FilesystemFormat`, the embedded definition constants and parser entry
  points, `Session::open_with_registry`, `Session::registry`, and public
  construction coupled to a registry record. Keep only public types and
  operations serving the in-force use cases.
- **S2:** no registry capability exists, so no removal is required. Any
  generic identification change discovered during implementation still
  follows P5.
- **S3:** remove the Python registry and definition classes,
  `default_format_registry`, and `Session.with_registry` in the same
  landing as S1.
- **S4:** delete the grammar and built-in definition files, remove S4
  from the application-surface inventory, and remove descriptive claims
  that formats are added without code.

This is pre-1.0: there are no aliases, compatibility parsers, deprecation
periods, or inert attributes carried forward. Tests, examples, generated
representations, README, root architecture, and repository guidance all
move to the implemented shape in the delivery change.

## Delivery cut

F19 is one coherent replacement, in this internal order:

1. Introduce role-specific adapter interfaces, the shared probe result,
   and internal catalogs.
2. Put H8D, qcow2, HDOS, and CP/M identification behind their adapters;
   compose the existing ZIP, MBR, FAT, qcow2, and HDOS implementations at
   their existing seams rather than duplicating them.
3. Route `Session` and `Disk` through the applicable catalogs and remove
   format-specific identifiers and heuristic interpretation from central
   orchestration.
4. Carry each loaded image's authoritative layer, each independently
   mutable open state's active layer, and derivation provenance through
   identification and opening.
5. Assign every addressed virtual device an opaque, composition-scoped
   identity without adding a caller-supplied identifier.
6. Delete the registry parser, definition files, and public reflections;
   land every affected document and test on the new shape.

This cut is one coherent, bounded push within the project's one-sprint
bound. It is delivered as a whole rather than implemented piecemeal.

## Acceptance

The feature is delivered only when:

- no central dispatcher branches on an image-format identifier or interprets a
  string-named format rule;
- every catalog entry pairs an implementation with its descriptor;
- a test-only adapter is recognized through a test catalog without an
  edit to orchestration;
- every loaded image names exactly one authoritative image layer, every
  independently mutable open state names exactly one active durable layer,
  and derived views distinguish decoded evidence from synthesized detail;
- every addressed virtual device has a library-assigned identity unique
  within its loaded composition;
- a supported single-volume legacy floppy reaches its filesystem's P19
  file container without caller configuration, a caller-provided device
  identity, or drive emulation;
- disk-local volume identifiers remain sufficient through an interface
  already scoped to one disk;
- H8D, qcow2, HDOS, CP/M, ZIP, MBR, and FAT behavior remains covered
  through the interfaces callers use;
- a recognition tie is reported as unknown with evidence naming the
  alternatives, and recognized-invalid input keeps its refusal;
- S1, S3, S4, README, repository guidance, examples, and tests agree that
  image formats are implemented modules rather than caller-authored text;
- no adapter interface requires, and no delivered path assumes, a whole
  source resident in memory, and peak memory stays bounded independently of
  source size (P27);
- S2 remains semantically aligned under P5; and
- the core remains runtime-dependency-free.

## Deliberately absent

- A public or dynamic plug-in system.
- A replacement universal declaration language.
- A new disk-image or filesystem format merely to demonstrate the seam.
- The media-family modules, common hardware-emulation layer, typed hardware
  presentations, complex volume-composition implementations and catalog,
  and emulator-facing compositions; P14, P15, P17, P22, and P23 fix their
  seams and state model, but later features must deliver them.
- System-wide file-container namespace composition and mount mapping; P19
  fixes the seam, but a later feature must deliver them.
- A generic public open-to-files capability across S1, S2, and S3; P19
  fixes its semantic result, but a later feature must vet and deliver the
  presentations.
- Caller-authored device identities or attachment topology; P21 supplies
  internal identity but promises neither multi-device composition nor
  hardware placement.
- Backward-compatible aliases for the removed pre-1.0 surfaces.

If a future use case requires users to author instances of a demonstrably
uniform format family, that family may gain a narrow declaration parsed
inside its own adapter. It does not reopen a universal registry.
Executable third-party formats or a general plug-in system would be new
application capabilities and require their own use case and surface
design.

MAME informs the private mechanism-and-media branch: persistent image formats
convert to and from physical media state, and timed signals compose drives
and controllers. It does not determine a family's public hardware seam or
impose physical emulation on LBA drives, whose logical-block interface is the
deliberate Remanence boundary. No MAME source enters this project.
