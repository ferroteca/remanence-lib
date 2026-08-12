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
F53's declared set does not carry, because a P64 answers with a flux
medium and that is this feature's own substance (D31). `bitstream()` and
`bytestream()` become argument-free — the type carries the channel and
codec (P30 reached through the type) — and the standalone `CaptureSet`
and `P64Image` roots fold into the model, closing the second root.
Capture-inspection reporting and plan preview stay out, with the
question tier.

Touches: S1, S2, S3. Supports: the pledged design; in-force P7, P13,
P22, P27, P29, P30, P31; U23, U25, U26, U33.

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
P27; U32. Needs nothing: the coordinates an authored blank states are
the delivered geometry's own, and authorship is the third fact class
beside the discovery that fills them today.

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
