<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# ARCHITECTURE (proposed)

> **Status:** drafted at the owner's direction. Nothing here binds;
> a principle is pledged by moving it to `planning/pledged/` and is
> armed only when it reaches root
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
module. Its catalog is wiring: every entry pairs a descriptor with
behavior, and adding an ordinary image format changes its module, tests,
and one mechanical enrollment. Central orchestration neither interprets
string-named format rules nor branches on an image-format identifier.

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
Derivation stops at the seam claimed for the image and drive family. A
virtual hard-disk image presented through LBA never gains inferred
platters, heads, tracks, flux, or other hardware state. A sector-addressed
legacy image may infer a hardware-level representation when its image format,
media profile, and drive family together claim the mapping. That
representation is synthesized from the sectors; it is not evidence of the
original medium's physical recording.

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

Image-format adapters load and save media state. Drives operate on that
state through their own seams. A media profile contains neither image
recognition nor drive behavior and cannot implicitly choose how far drive
emulation descends. Its catalog may be declarative because media is
passive; it is not a language for recorded behavior.

## P15 — Every drive family declares its emulation seam

Every emulated drive is an adapter whose family declares the interface it
presents. The default seam is the common drive interface presented to the
system, with the device beneath it opaque. LBA hard drives and modern
optical or large-storage drives stop there: physical geometry, pickups,
servos, integrated controllers, firmware, error correction internals, and
microcode are not inferred or emulated.

Selected legacy families may instead claim a mechanism-level seam.
Remanence then accepts the applicable motor, head-select, direction, step,
write-gate, and timed write-data inputs; models rotation, head position,
settling, and head/media interaction; and returns read-data transitions
and sensor levels. Controller functions such as data separation, encoding,
and sector recognition remain outside the drive mechanism. The Commodore
1541 illustrates the required generality; old MFM or RLL mechanisms are
another possible family.

Older drive families also have a CHS interface that presents their
recorded cylinder, head, and sector layout without emulating a particular
controller. A family may provide CHS and mechanism adapters over the same
media instance. CHS never exposes or invents geometry beneath an LBA
drive. The selected drive family, never its image format or media profile,
chooses the seam.

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
layout around its volume, or know how that volume was composed. Its
catalog and interface remain independent of those adjacent seams.

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
