# AGENTS.md — repository guidance

This is the canonical, agent-agnostic guidance for working on remanence-lib.
Human usage documentation belongs in [README.md](README.md); keep this file
focused on repository structure, engineering constraints, verification, and
maintenance context.

## Project state and layout

remanence-lib is the core disk image analysis library for Remanence
Workbench, ported to Rust from the workbench's C++ `lib/` (itself a port of
an earlier Rust prototype). The C++ front-ends (CLI, GTK4 GUI) remain in
the separate `remanence` project and will consume this library through the
C ABI. The Rust code here is now the authoritative implementation.

- `crates/remanence/` — the core library. `error.rs` owns the error
  taxonomy (`Error`, five variants, display messages that the front-ends
  print verbatim); `registry.rs` the format-definition parser and
  `FormatRegistry` (BTreeMap-keyed, so detection iterates in stable id
  order); `image.rs` size validation against a container format;
  `container.rs` / `filesystem.rs` the detection heuristics (crate-private,
  reached through `Session::identify`); `session.rs` the session model,
  the layered identification result, and the P7 claim held for the
  session's lifetime; `hdos.rs` the HDOS directory lister and file
  extractor; `archive.rs` `.zip[/entry]` path resolution under the claim;
  `zip.rs` + `inflate.rs` the self-contained ZIP reader and DEFLATE
  decompressor; `device.rs` the block-device seam, the P7 claims
  (declared intent for the disk stack, the discovery ladder for
  identification sessions), and the P2 commit-point overlay;
  `qcow2.rs` the native qcow2 v2/v3
  driver (P8 version gate first; write path refuses snapshots and
  non-16-bit refcounts by name); `mbr.rs` partition discovery with
  pinned types; `fat.rs` FAT12/16 volume read/write; `disk.rs` the
  public `Disk` API (open/geometry/entries/read/write/mkdir/
  commit/rollback). `formats/` holds the starter container/filesystem
  definitions, embedded with `include_str!`. Unit tests live in their
  modules; integration tests in `tests/` — synthetic FAT/MBR/qcow2
  images built in-test, plus the fixture-driven HDOS tests.
- `crates/remanence-ffi/` — the C ABI (`rmn_*` symbols): opaque handles,
  accessor functions, borrowed strings owned by their handle. `build.rs`
  regenerates `include/remanence.h` with cbindgen on every build; the
  header is generated output, never edited by hand.
  `examples/identify.c` is the example C consumer and doubles as the ABI
  smoke test (build instructions in its header comment).
- `crates/remanence-py/` — the Python module (PyO3, abi3, Python ≥ 3.10),
  excluded from default workspace members so plain `cargo build`/
  `cargo test` never needs a Python toolchain. Distribution artifacts
  are built with **uv** (`uv build crates/remanence-py` → sdist + abi3
  wheel in its `dist/`), which drives the maturin build backend in an
  isolated environment; publishing is `uv publish` and is owner-gated.
  **The Python package claims Windows only** (the tested host; the
  classifiers state it) — keep POSIX paths correct but never state or
  imply support the project has not tested.
- `planning/README.md` is the map of the maintainer-facing planning
  machinery, and the place to start. `planning/SURFACES.md` is the
  surface-change rule; the application surface inventory it scopes over is
  S-numbered in root [ARCHITECTURE.md](ARCHITECTURE.md) "The application
  surfaces", where the housekeeping lookup answers by checklist.
  `planning/DECISIONS.md` is the adjudication record — **search it before
  a governed act** (drafting a proposal, pledging one, changing a norm)
  and report what you found, including nothing. `planning/TASKS.md` is the
  pre-approved task queue: **agents do not add tasks on their own
  initiative, and ask before editing that file at all**; anyone may pick
  up what is already there.
- **The vision is in force.** Use cases U1–U5 (root
  [USE-CASES.md](USE-CASES.md)) and architectural principles P1–P8
  (root [ARCHITECTURE.md](ARCHITECTURE.md)) are armed: every entry is
  met or honored by the code today, and a divergence is a bug. Triage
  cites them by number; the surface-change rule in
  [planning/SURFACES.md](planning/SURFACES.md) is fully operable.
- **There is no roadmap**, and no issue tracker yet — until one exists the
  task lane has no proposed state at all (see `planning/TASKS.md`).

## Required invariants

### Pre-release: no backward compatibility

remanence-lib is pre-1.0 and maintains no backward compatibility: when a
surface changes, change it coherently and completely — every binding,
document, example, and test moved to the new shape, the old one deleted.
No aliasing, no deprecated shims. Compatibility guarantees are defined no
earlier than 1.0.

### Surface changes are vetted

The Rust crate API, the C ABI, the Python module, and the format-definition
text format are the application surfaces (S1–S4, root
[ARCHITECTURE.md](ARCHITECTURE.md)). Any decision that changes one follows
[planning/SURFACES.md](planning/SURFACES.md). With no use-case or principle
lists in force yet, the triage there cannot be run to completion — flag
surface-changing proposals to the owner instead of self-approving them.

### The bindings track the core in the same change

A public-surface change in `crates/remanence` lands with its C ABI and
Python reflections in the same change, never deferred: the cbindgen header
regenerates on build (commit the result), and `remanence-py` mirrors the
public surface explicitly. The example consumer and tests move in the same
change.

### The core stays dependency-free at runtime

`crates/remanence` has no runtime dependencies, deliberately — its ZIP
reader and DEFLATE decompressor are its own. That is a property the
licensing tiers below make load-bearing, not just tidiness. Discuss before
adding any dependency anywhere in the workspace; for the core the answer
is expected to stay no.

## Licensing

The project is **GPL-3.0-only** and follows REUSE conventions. The name
**Remanence** is reserved to Paul Galbraith under [TRADEMARKS.md](TRADEMARKS.md) — a reservation GPL section 7(e)
expressly permits; do not weaken or contradict that policy in docs or packaging metadata.

Every new file authored for the project needs:

```text
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
```

Use the appropriate comment syntax for the file type. Files that cannot or
should not carry headers must be covered by `REUSE.toml`.

### The relicensing reservation, and what it constrains

Paul holds copyright in the whole work and **reserves the right to
relicense the project on any terms**. Nothing is planned; the reservation
exists so the option is not lost by default. Two consequences bind
everything below, and neither is negotiable at the level of an individual
change:

- **The project must own every line it ships.** Relicensing is only
  available to a party holding rights in the whole work, and enforcing
  copyleft requires standing that only an owner has. One file the project
  cannot account for forecloses both, permanently and silently.
- **Assignability, not licence compatibility, is the test for incoming
  code.** GPL-compatible is not good enough. Code the project cannot
  acquire *title* to cannot enter, whatever its licence.

**Vet against a commercial dual licence, and say only "relicensing" out
loud.** What the project *states* — in README.md, CONTRIBUTING.md, and
CLA.md — is that relicensing is reserved and nothing is planned, which is
true and is all the disclosure the reservation needs. What the project
*vets against* is the strictest realistic outcome, which is a commercial
dual licence, because vetting to a weaker bar would forfeit the reserved
option invisibly. The question to ask of any external source is **"could
this ship inside a proprietary product?"** — never "is this
GPL-compatible?"

Contributions are accepted only under the copyright assignment in
[CLA.md](CLA.md). Once assigned, a contributor's files carry Paul's
copyright notice, because he is then the actual owner — the REUSE record
states ownership, not authorship, and authorship credit lives in the git
history. Keep the human submission terms in
[CONTRIBUTING.md](CONTRIBUTING.md) synchronized with this policy.

**Never merge third-party source.** Not permissively licensed source, not
public-domain-looking snippets, not vendored files. The contributor cannot
assign what they do not own, and neither can the project. Third-party code
enters as a declared dependency or not at all.

### Dependency licence tiers

Every dependency that reaches a **distributed artifact** — the published
crate a consumer compiles in, the staticlib/cdylib, the Python wheel —
sorts into exactly one tier, drawn against the commercial-dual-licence bar
above rather than against GPL compatibility. Verify a new dependency's
whole transitive closure, not just the package named.

| Tier | What qualifies | Standing |
|---|---|---|
| **1 — Sublicensable** | MIT, BSD-2/3-Clause, Apache-2.0, ISC, Zlib, Unicode | Freely dependable. Attribution obligations carry into any redistribution. |
| **2 — Arm's length only** | LGPL as a separately installed, replaceable library; GPL invoked as a separate process | Permitted, never combined. Static linking — Cargo's default — is **not** arm's length, which makes this tier nearly unreachable for Rust dependencies. |
| **3 — Refused** | Any GPL/AGPL code that would be linked, imported, or copied in | Never. Compatible with the GPL arm and fatal to the reservation. |

Build-time and development dependencies are out of scope — they are not
distributed. cbindgen (MPL-2.0) is build-time only. The current
distributed closure: the core crate has **zero** dependencies;
`remanence-py` adds pyo3 and its closure (MIT/Apache-2.0 — tier 1),
compiled into the wheel.

### Prior art and provenance notes

- `crates/remanence/src/inflate.rs` follows the structure of Mark Adler's
  "puff" reference DEFLATE implementation, as did the C++ file it ports
  (the C++ described puff as public-domain; puff ships in zlib's contrib
  under the zlib licence — tier 1 either way). The Rust is an original
  implementation of RFC 1951 following that published structure, written
  from the project's own C++ lineage, not from puff.c. Keep the
  attribution comment in the file.
- `crates/remanence/tests/fixtures/` is **local-only test data** and is
  deliberately empty in a fresh checkout: the vintage HDOS distribution
  disk images used by the integration tests are third-party material
  the project cannot account for, so per D1 (`planning/DECISIONS.md`)
  they are excluded from git (history rewritten before any remote
  existed), from cargo packages (`package.exclude`), and from Python
  sdists. The fixture-driven integration tests fail without the local
  files — a knowingly accepted state; `planning/TASKS.md` T5 tracks
  the repair. Never re-add the images to git or to any published
  artifact.

## Versioning and releases

The **workspace SemVer is the single upstream version** —
`workspace.package.version`, inherited by every crate (currently
`0.0.1-alpha.2`). Pre-releases follow SemVer's ladder (`-alpha.N` →
`-beta.N` → `-rc.N` → bare); nothing below `-alpha.1` is ever
published to a registry — unpublished git is the dev channel.

The **PyPI version is derived, never hand-written**: pyproject
declares `dynamic = ["version"]` and maturin converts the Cargo
version to PEP 440 (`0.0.1-alpha.1` → `0.0.1a1`). Do not put a static
version back in pyproject.

**Repackaging an unchanged upstream** (distro-style revision — the
wheel changed, the library did not) is spelled as a PEP 440
post-release: give `crates/remanence-py` its own Cargo version with
`.post.N` appended to the workspace version (e.g.
`0.0.1-alpha.1.post.1` → PyPI `0.0.1a1.post1`), and return it to
`version.workspace = true` at the next upstream bump. **The decision
that a repack is warranted is the releaser's judgment — only the
spelling is mechanized.** PEP 440 discourages post-releases of
pre-releases; the distro-revision model is chosen deliberately over
that advice (D3), and PyPI's local-version syntax — the truer
analog — is rejected by the index outright.

## Required checks

```bash
cargo build      # also regenerates crates/remanence-ffi/include/remanence.h
cargo test
git diff --check
```

When the C ABI changed, rebuild and commit the regenerated header, and
recompile `examples/identify.c` against it (instructions in the file
header). When the Python surface changed, build `-p remanence-py` (needs
Python ≥ 3.10) and smoke-test the module; for release artifacts,
`uv build crates/remanence-py` produces the sdist and abi3 wheel.
