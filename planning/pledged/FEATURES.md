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

## F49 — Uniform archive open

An archive enters the machine as every medium does: an archive-family
virtual device, `load_media("games.zip")` into it, content walked
through the `Filesystem` that device resolves. The `archive[/entry]` path syntax and the standalone
`Archive` journey fold into the model; the archive catalog itself is
unchanged, becoming the family's adapters at the namespace seam.
Recursion is the same journey again from a file, **in a machine of its
own** where a machine is being reconstructed: an entry recognized as an
image is loaded into a device of its own, and reaching
`games.zip/boot.h8d` while modelling the disk's machine means an archive
device in one machine and a drive in another, because the host's archive
was never part of the machine that disk belonged to. Where nothing is
being reconstructed, both may sit in the session's anonymous machine; a
composer passes over an archive device either way, by family rather than
by scope, since an archive has no partitions or volumes for an assignment
rule to reach (D23).

Two things this feature settles. Whether an archive slot is visible in
its machine's attachment namespace or stays behind the report. And the
backing relationship across scopes: a stored entry is source-backed
through the parent machine's P7 claim, so that machine must outlive the
child, while a compressed entry is session-backed and free-standing once
materialized (P27) — the feature states which, rather than copying every
source to avoid saying so.

Read-only, as archives are today; a write claim is its own future
feature.

This feature finishes the container retirement the `Filesystem` node
began: that change purged the word from the code and the surfaces, and
what survives is the archive journey's own shape — the `archive[/entry]`
path resolution and the archive catalog's prose — which only folding
that journey into the model removes.

It also finishes the P19 amendment, which landed only as far as the
delivered code honored it (D25): the "serialized-container adapter"
provider form dissolves when an archive becomes a medium whose grammar
is a P12 adapter at the namespace seam, and the composer's move to P35
waits for the machine namespace that principle claims.

Touches: S1, S2, S3. Supports: the U2 amendment; the P14 amendment;
P35; the P19 amendment; in-force P7, P12, P19, P27. The `Filesystem`
node an archive's content is reached through is delivered.

## F52 — StorageSpace: one object, two vantage traits

F48 delivered `Volume` and `Filesystem` as two types with a hop between
them (`device.volume(id).filesystem()`). The storage model says they are
**one node seen from two vantages**, and this feature makes the types say
it: one object, **`StorageSpace`**, carrying two capability traits —
**addressable I/O** (the volume vantage: reads and writes by position
within the space it names) and **namespace I/O** (the filesystem vantage:
entries, stat, get_file and the rest F48 already delivered).

**Traits succeed where a type merge fails.** Merging the two *types* was
weighed and refused, because two of the three providers have no volume at
all — an archive's content is a namespace with no space beneath it, and a
machine namespace is composed over several filesystems — so a merged type
would have had to invent a phantom volume for each. An object implementing
what it claims has no such problem, and it is the rule this project
already applies one level up, where a device's vantages are capability
traits rather than a hierarchy:

| StorageSpace over | addressable | namespace |
|---|---|---|
| a FAT volume | yes | yes |
| swap, an unformatted volume, a raw database extent | yes | — |
| an archive medium's content | — | yes |
| a machine's composed namespace (P35) | — | yes |

**The 0..1 stops being prose and becomes trait presence.** "A volume bears
at most one filesystem" is carried by the type rather than asserted beside
it, and asking namespace questions of an addressable-only space is the
`no-namespace` refusal F48 already enrolled. The delivered `NamespaceRule`
set fits unchanged.

**Addressable I/O scoped to a space is the new capability**, and it is the
reason this is a feature rather than a tidy-up. Today the only addressed
reads are whole-medium; there is no way to read a volume's boot sector,
its unallocated extents, or the bytes behind a file just listed, without
computing offsets against the medium by hand. The addressable trait closes
that on the same object that hands over the files, which is the work this
project exists for. Writes follow the delivered rules unchanged: they land
in the active layer (P23) and commit through P2.

Naming: `StorageSpace` rhymes with `StorageDevice`, which is the point —
the two nouns a caller holds. The compound is its own term even though
bare "space" is the addressable vantage's word, exactly as D19 kept "hard
disk" as a device-family name while "disk" stayed media-centric. The
vantages keep their words: *volume* is the space vantage, *filesystem* the
namespace vantage, and the object needs neither name because it is both.

`device.volume(id)` and `device.filesystem()` both answer with a
`StorageSpace` — the first selecting, the second resolving — and the hop
between them disappears. The resolver's behavior, its refusals, and the
transparency clause it implements are F48's and are unchanged.

Touches: S1, S2, S3 — `Volume` and `Filesystem` become one type across
all three, the C ABI keeping `remanence_volume_*` and
`remanence_filesystem_*` as the two function families over one opaque
handle, capability-checked, so the vantages stay legible at the seam
where types are thinnest. Supports: in-force P17, P18, P19, P10 (the
capability refusals are categorized and rule-identified), P27 (the
addressable reads are bounded and streamed); the storage model design.
Independent of F45 and F49; F49's archive namespaces present as
`StorageSpace` without namespace-only being a special case.
