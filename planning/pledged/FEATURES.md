<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged — owed by the project and not yet delivered. This
> file says nothing about when, and nothing about the order they are
> worked in. Feature numbers record order of issue; a delivered feature
> stops existing as an item and its number retires rather than being
> reused.

## F45 — An idiomatic C++ presentation, derived from the C ABI

Provide C++ consumers an idiomatic surface — RAII, namespaces, typed
errors — without the project acquiring a fourth application surface. The
wrapper is a single header-only layer over S2, and **S2 remains the
norm**: the C++ header is a derived representation of the C ABI exactly
as `include/remanence.h` is a derived representation of the Rust
`extern "C"` items, and it moves with S2 in the same change, never
independently. This feature deliberately amends nothing: P5's
three-presentation rule stands, no S-number is issued, and the wrapper
claims no capability the C ABI does not already provide — C++ programs
consume `remanence.h` today, and this adds ergonomics, not reach.

Shape: one move-only RAII class per node kind — the storage model's
`Session`, `Machine`, `StorageDevice`, `Volume`, `Filesystem`, and
`File` — each owning its handle's lifetime through the ABI's free
functions, with families as enum values and every refusal surfacing as a
typed error carrying the delivered category and rule identity (P10). No
compiled C++ artifact exists — C++ has no stable cross-compiler ABI,
which is why the boundary stays C — so the deliverable is a header, its
tests, and a C++ example consumer beside the C one. The wrapper
documents view lifetimes — a filesystem borrowed from its device, a
file from its filesystem — under the ABI's existing "borrowed, owned by
their handle" discipline; C++'s inability to enforce them is documented
rather than papered over.

Open to the implementation: whether errors present as exceptions, an
`expected`-style result, or both; whether the header is generated from
S2's shape or hand-maintained thin; and the header's name and install
path.

Touches: S2 only — a new derived representation beside the generated
header, with the ABI itself unchanged; S1 and S3 are unaffected because
nothing crosses the C boundary differently. Supports: S2, P5, P10 — no
U-number demands idiomatic C++, the demand being developer experience
at an existing surface. Wraps whatever S2 is when it lands, so it
neither requires nor blocks the features below.

## F56 — The partition pool and the vantage doors

Partitions become the medium's evidence pool: `partition(n)` by the
scheme's own ordinal, `partitions()`, the `partition_scheme` attribute,
and the **direct partition** — the library's composition of the whole
content where no scheme exists, declared as synthetic in provenance and
never as evidence, extent-less over namespace-native media. A partition
carries its raw type byte beside a reading (U4), `active()`, and
`as_type(PartitionType::…)` as a declared reading checked against the
byte. The vantage doors land: `volume()` and `filesystem()`, each
`Option`, both handing out the **one** `StorageSpace` the partition
composes — and everything behind them is specified, never probed: the
pool populates under the device spec, kind-determined for every type —
the hard-drive class by its spec's scheme, checked at load, the
schemeless types by the direct partition — and the namespace vantage
opens under the declared partition type where it determines one, or
`filesystem_as` where nothing does, P18's recognizers verifying
declarations rather than probing for readings.
The `medium.filesystem()` resolver and `volume(id)` selector are
deleted; in-force P19's transparency clause is amended in the same
change — uniformity of the walk replaces resolve-without-selecting —
and `DiskReport` demotes to a derived view.

**It rehomes the one delivered door that has nowhere else to be.** The
CBM DOS filesystem reads today through `C1541Sectors::filesystem()`,
that layer being where a flux recording could be reached at all before
media were pool-held; the adapter, its label and its evidence are
delivered and move with the door rather than being rewritten, and the
sector layer keeps carrying no file verbs of its own either way.

Touches: S1, S2, S3. Supports: the pledged design; in-force P4, P16,
P17, P18, P19 (as amended here), P21; U4, U26–U29, U31, U34.

## F57 — Device types, and the articles they compose

The catalog gains the **device type**: one identity per medium naming
the device its content is assumed recorded by, enumerated in two
levels — the **class** (`Floppy`, `HardDrive`; `Optical` and `Tape`
reserved for the coming families), then the **concrete type** within
it: `Commodore1541` is a `FloppyDrive`, and is a `DeviceType`. The
floppy class: `FloppyDrive::Commodore1541`, the flux product class
(encoding, speed zones, timings, tracks); `FloppyDrive::HeathH17`
and `FloppyDrive::HeathH37`, the Heathkit product classes (hard- and
soft-sectored); `FloppyDrive::Sector`, the generic schemeless sector
floppy, geometry per-media. The hard-drive class, whose specs carry
the partition scheme itself: `HardDrive::MbrSector`,
`HardDrive::MbrBlock` and `HardDrive::Gpt`, GPT implying block
addressing by its own definition. `device_type()` answers `Option` —
archives were recorded by no device, and `None` is the honest answer.
The granularity rule cuts the catalog: a device type is the coarsest
name fixing the whole addressing surface and recording discipline
without per-media parameters. A type the library does not know fails
to compile; the display strings (`c1541`, `mbr-block-hd`) survive in
provenance, refusals, and the S2/S3 spellings — integer constants in
C, enums in Python (P5). `article()` answers the substrate
(`flexible-5.25-soft`, `flexible-5.25-hard-10`, `logical-block-512`,
`virtual`); D19's three facts keep their three homes, the recording
living in the device type. A format that admits one device type
carries it bare (`Format::H8d` → `FloppyDrive::HeathH17`,
`Format::P64` → `FloppyDrive::Commodore1541`); one that records many
declares it, the field typed by the class its adapter records —
`KryoFlux { device: FloppyDrive }`, `Qcow2 { device: HardDrive }`,
`Vdi { device: HardDrive }`, `Raw { device: HardDrive, block_bytes }`
— so a flux capture of a hard drive fails to compile, and a pairing
no adapter declares within the class is a named refusal at load.
Insert's check is device-type equality naming both sides, so a 1541
refuses an H17 disk it could physically hold but never serve. In S1 a
device type's definition has **one home**: one spec shape per class,
one instance per concrete type — the enumeration is the
instantiation, its disciplines flat attributes of the profile —
while the **traits live on the medium**, where the actions
(`read_blocks`, `put_sector`, `partition`) take shape, each trait
surface answering only where the profile's attribute holds, P30
declarations reached through the type (rule 8).

Touches: S1, S2, S3. Supports: the pledged design; in-force P3, P14
(gaining the device-type catalog and its granularity rule); U23,
U25–U28, U32, U34.

## F58 — Discovered geometry and recording coordinates

Geometry becomes discovered instance evidence with provenance: the
format's declaration where one exists, the FAT BPB's recorded
sectors-per-track and heads, MBR end-tuple inference, extent
arithmetic — and **`Undetermined`** where sources disagree, reported
with both readings and settled by neither. `get_sector` / `put_sector`
answer on geometry-bearing types in the recording's own coordinates,
refuse by name toward the evidence state otherwise, and writes buffer
until commit (P2). Nothing is ever declared onto an existing medium.

Touches: S1, S2, S3. Supports: the pledged design; in-force P2, P4;
U4, U28, U32. Needs F57.

## F59 — Collection sources, and the flux family folds in

`load_media` gains its source shapes: a collection of the caller's
opened files, a `File` from another medium's namespace, and a
collection of `File`s — each format declaring which shape it reads. `Format::KryoFlux { disk }`
is the first collection-sourced format: member grammar, completeness,
stream grammar and the profile claim checked whole, then the reduction
under the profile's declared `Materialization` defaults — a choice no
family convention can make refuses by name and the answer grows the
declaration (P29, nothing unnamed). The result is a `Commodore1541` medium with
the verdicts, policy and declared-loss account as provenance.
`Format::P64` loads the served form straight in — the one format id
F53's declared set did not carry, because a P64 answers with a flux
medium and that is this feature's own substance (D31). `bitstream()` and
`bytestream()` become argument-free — the type carries the channel and
codec (P30 reached through the type) — and the standalone `CaptureSet`
and `P64Image` roots fold into the model, closing the second root.
Capture-inspection reporting and plan preview stay out, with the
question tier.

Touches: S1, S2, S3. Supports: the pledged design; in-force P7, P13,
P22, P27, P29, P30, P31; U23, U25, U26, U33. Needs F57.

## F60 — Authored media

`new_media(kind)` creates blank media whole: the blank article kinds
and `ChsDisk { geometry }`, session-backed, authored provenance —
authorship being the third fact class, the author's facts becoming the
medium's original facts. An authored blank assumes no device —
`device_type()` answers `None`. The authored-to-recorded arc (a
partition editor consuming authored geometry into MBR end-tuples and
BPBs, binding a device type) remains reserved in the partition pool's
create/release slots.

Touches: S1, S2, S3. Supports: the pledged design; in-force P2, P13,
P27; U32. Needs F57, F58.

## F67 — Discovery holds the claim and builds no cache

The constraint D30 reinstated the discovery surface *on*, made real.
`discover_media` opens the artifact, takes the P7 claim, probes for the
type, and stops: no media state, no session cache, no spilled backing —
where the delivered verb opens a whole `MediumState` under a declared
bound today, which is a load's work done before anyone asked for one.
The `Discovery` stays consumable, so a load still takes the open handle
out of it and nothing runs twice. `discover_media_with_cache` and the
bound travelling into the device with a discovery go with the
materialization: a cache bound is the load's declaration, and a verb
that creates nothing has nothing to bound. The probe reads the bounded
evidence its claims name (P27), as identification always has.

Touches: S1, S2, S3. Supports: D30; in-force P4, P7, P27. Needs
nothing — the delivered surface is what it changes.
