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
