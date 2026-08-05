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
`Machine`, `StorageDevice`, `Medium`, `Volume`, `Filesystem`, and `File`
— each owning its handle's lifetime through the ABI's free functions,
with families as enum values and every refusal surfacing as a typed
error carrying the delivered category and rule identity (P10). No
compiled C++ artifact exists — C++ has no stable cross-compiler ABI,
which is why the boundary stays C — so the deliverable is a header, its
tests, and a C++ example consumer beside the C one. The wrapper
documents view lifetimes — a filesystem borrowed from its medium, a
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

## F46 — The Machine scope, and one storage handle

Two structural changes to the delivered types, no behavior change.

**`Session` keeps its name and gains `Machine` beneath it.** The session
stays what the principles already call it — the P7 claims, the P27 cache
budget and private session storage — and a machine becomes the device set
within it, owning attachment identities and attachment order. A session
holds machines; a machine holds devices. The `machine.rs` module's
contents split accordingly.

**`Disk` merges into `StorageDevice`.** A caller never holds a medium
outside a device, so the delivered two-type shape
(`session.medium(attachment) -> &mut Disk`) becomes one handle carrying
both nodes' data: slot-side facts and content-side facts on the same
object, content verbs refusing by name when the slot is empty. The medium
survives as a model node and as data (`media_type` and its profile), not
as a type. The "disk stack" prose naming (D2) follows.

The C symbols and Python classes move in the same change; pre-1.0, the
old shapes are deleted, never aliased.

Touches: S1, S2, S3. Supports: the U2 amendment; the P32 amendment; the
storage model design
([design/storage-model-and-vocabulary.md](design/storage-model-and-vocabulary.md)).
F48–F51 read better after it but do not require it.

## F48 — The Filesystem node

Move file verbs onto the one namespace node: `Filesystem`, with
`get_file` and its kin living there and nowhere else.
`device.filesystem()` is the resolve-or-refuse transparency
method; `device.volume(id).filesystem()` selects where several
candidates exist; `list_hdos_files` is regularized as the resolver's
transparent form. The device exposes no file access — a device holding a
partitionable medium and bearing `get_file` would be a category error,
not a refusal waiting to happen. A device may be asked what it resolves
to; it may not be told to act as something it isn't.

Touches: S1, S2, S3. Supports: the U2 amendment; the P19 amendment;
P35; in-force P10 (the resolver's refusals are categorized and
rule-identified).

## F49 — Uniform archive open

An archive enters the machine as every medium does: an archive-family
virtual device, `load_media("games.zip")` into it, content walked
through the `Filesystem` that device resolves. The `archive[/entry]` path syntax and the standalone
`Archive` journey fold into the model; the archive catalog itself is
unchanged, becoming the family's adapters at the namespace seam.
Recursion is the same journey again from a file, **in a machine of its
own**: an entry recognized as an image is loaded into a device in a
separate scope, so reaching `games.zip/boot.h8d` is an archive device in
one machine and a drive in another. That is the honest shape twice over —
a disk needs a drive to be served, and the host's archive was never part
of the machine the disk belonged to. It also keeps each device set
holding only that machine's own configuration, which is why P35's
composer cannot letter an archive slot: the archive is not in the machine
being reasoned about.

Two things this feature settles. Whether an archive slot is visible in
its machine's attachment namespace or stays behind the report. And the
backing relationship across scopes: a stored entry is source-backed
through the parent machine's P7 claim, so that machine must outlive the
child, while a compressed entry is session-backed and free-standing once
materialized (P27) — the feature states which, rather than copying every
source to avoid saying so.

Read-only, as archives are today; a write claim is its own future
feature.

Touches: S1, S2, S3. Supports: the U2 amendment; the P14 amendment;
P35; the P19 amendment; in-force P7, P12, P27. Needs F48, whose
`Filesystem` node is what an archive's content is reached through.

## F50 — The two-act access path and concrete device families

Replace one-act `attach` with the two acts the P32 amendment names:
`add_device`, taking a device family that is a concrete leaf of the
lineage-bearing catalog — interior names classify but never
instantiate — and `load_media` on the device, plus eject, with an empty
device as first-class configuration. `add_device` returns the device,
which is the one storage handle (F46); `load_media` places a medium in
it and hands back nothing to hold. A family mismatch at `load_media`
refuses naming both sides, which is the check a concrete family exists
to make possible. Content verbs on an empty device refuse by name, and
views taken through a device invalidate when its medium is ejected.
Attachment order becomes an explicit machine fact the DOS composer reads
from the device set.

The device-family catalog gains its lineage: entries as concrete as the
machine fact they assert, each stating what it is a kind of, as data
rather than as a type hierarchy. Enrolling the families the claimed
media need is part of this feature; enrolling one for a family the
project does not claim is not.

Touches: S1, S2, S3. Supports: the U2 amendment; the P32 amendment;
in-force P14, P21, P22 (a claimed flux path is a family declaration).
The archive slot arrives with F49, not here.

## F51 — Discovery, declared defaults, and the one-step conveniences

`discover_media(path)` lands as a first-class library function on no
handle: the claim for the read, identification, and a report of the
exact medium, the device families that accept it (derived from the
families' own declarations), and the image format's declared default
device. The discovery is a consumable claim-scope handle carrying the
work done — parsed capture state, probe verdicts — and `load_media`
accepts a path, a file view, or a discovery, consuming the last so
nothing expensive runs twice and the claim never lapses between question
and load (P7). Accepting a file view is what makes a nested artifact the
same journey: an entry inside an archive loads into a device of its own.

Image-format adapters gain the default-device declaration this rests on:
a recording-side fact the media type cannot hold, declared by the format
that records the ecosystem's disk. A format that declares none is
ordinary, not deficient. One machine-level convenience then sits over
discovery — `add_device(path)`, adding a fresh device of the declared
default family, loading the medium into it, and returning that device —
refusing by name where a format declares no default. There is no
media-first spelling: with one storage handle it would return the same
device.

Touches: S1, S2, S3. Supports: the U2 amendment; the P32 amendment;
in-force P3, P4, P7, P12 (the default is a format adapter's
declaration), P14. Needs F50, whose two acts it composes.
