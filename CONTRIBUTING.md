# Contributing to remanence-lib

Thank you for helping improve remanence-lib. Bug reports, documentation
fixes, tests, and code changes are welcome when they preserve the project's
GPL licensing and its role as a reusable disk image analysis library.

Code contributions carry a licensing requirement that is stricter than most
projects': every accepted contribution is assigned to the project owner.
Read [Contribution licensing](#contribution-licensing) before you write
code — it is a real condition, not a formality, and it is better learned
before the work than after.

We know your time is worth something, and we're glad you're spending some
of it here. This project has a firm sense of what it's for and what it's
trying to be, and we weigh contributions against that, to keep it coherent
for everyone who relies on it. Most contributions fit without any fuss.

And when one doesn't, that's not the end of the conversation. It might mean
the idea's a poor fit — or that our sense of the project is too narrow and
should change. Tell us either way.

## Before you start

For a substantial change, raise it before investing significant work, so we
can agree on the problem, scope, and approach. Small, focused fixes may go
directly to a pull request. Keep changes narrowly scoped and avoid
unrelated cleanup.

Maintainer-facing planning — how ideas enter, what is pledged, and how
decisions are recorded — is mapped in [planning/README.md](planning/README.md).

## Development setup

remanence-lib is a Cargo workspace, pinned to one toolchain by
`rust-toolchain.toml` so every host formats and lints alike. A stable
Rust toolchain is all the core needs.

```bash
cargo build                 # the Rust core and the C ABI; nothing but rustc is needed
cargo test
cargo build --workspace     # every surface; regenerates c/include/remanence.h
cargo test --workspace      # the Rust-level tests only
task test-ffi                 # the C/C++ surface: needs CMake and a C/C++ compiler
task test-py                  # the Python surface: needs Python 3.10+ with uv
```

`crates/remanence` and `crates/remanence-ffi` are default members; only
`crates/remanence-py` is not, since its lifecycle depends on an external
Python that the other two no longer need at all (D68). The bare commands
therefore ask nothing of you but rustc, and already regenerate
`crates/remanence-ffi/c/include/remanence.h` in the process. **A
contributor runs all four of the rest**: `cargo build --workspace`
additionally reaches `remanence-py`'s Rust code, `cargo test --workspace`
checks the Rust-level tests of every surface, and `task test-ffi`/
`task test-py` are what actually check the C ABI and the Python module
respectively — neither is reached by `cargo test` in any form. `task`,
not `just` — [Task](https://taskfile.dev) embeds its own shell
interpreter, so a task runs the same wherever it's invoked from with no
external shell dependency at all, unlike `just`'s recipes, which needed
git-bash. Extra `ctest` arguments pass through `task test-ffi`, e.g.
`task test-ffi -- -LE "rigs|fixtures"` to skip what needs a downloaded or
generated fixture (one regex — `ctest -LE` does not compose across
repeated flags). Distributable Python artifacts are built with
`uv build crates/remanence-py`, which drives the maturin build backend
in an isolated environment. See [README.md](README.md).

Some `remanence` unit tests need fixtures that are not checked in;
`integration-tests/prep_fixtures.py` prepares them. It pins reliquary
(Python 3.12+) as inline script metadata rather than through a
project of its own, so uv provisions the environment straight from
the file — there is nothing to sync or activate first:

```bash
uv run integration-tests/prep_fixtures.py
```

See [test-fixture-prep/test-rigs/README.md](test-fixture-prep/test-rigs/README.md)
for what it builds, prerequisites (QEMU), and how the FreeDOS rig works.

- The core crate (`crates/remanence`) is **dependency-free at runtime**,
  deliberately — it carries its own ZIP reader and DEFLATE decompressor.
  Discuss any new dependency before adding it; the licensing tiers in
  [AGENTS.md](AGENTS.md) govern what may be depended on at all.
- Match the existing style, add or update tests for changed behavior, and
  keep the C header and Python surface in step with the core when the
  public API changes. The C header regenerates on build; the C++ wrapper
  beside it (`crates/remanence-ffi/c/include/remanence.hpp`) is written by
  hand and moves with the C ABI in the same change.
- Add an entry to [CHANGELOG.md](CHANGELOG.md) under `Unreleased` when
  public behavior changes. Released sections are history and are never
  edited; a correction is a new entry.
- Run `git diff --check` before handing work back.

## Contribution licensing

remanence-lib is licensed under the [GNU General Public License v3.0
only](LICENSE). It is copyleft: anyone may run, study, modify, and
redistribute it, and any distributed work incorporating it must also be
GPL-3.0-only. It cannot be taken into a proprietary product.

### The reserved right, stated plainly

Paul Galbraith holds copyright in remanence-lib and **reserves the right to
relicense it**, on any terms, at any time. No relicensing is planned or in
preparation. The reservation exists so that the option is not lost by
default — not because there is a plan behind it.

Two things follow, and both are worth being explicit about:

- **Nothing is taken back.** Every version published under the GPL stays
  under the GPL, permanently and irrevocably. A relicensed edition could
  only ever sit *alongside* what has already been released, never replace
  it, and could not reach backwards into published history. Your right to
  use and fork what exists does not depend on the owner's goodwill.
- **The owner would be the only party able to do it.** Relicensing
  requires the licensor to hold rights in the whole work. That is the
  reason for the assignment below, and it is the honest reason — not
  administrative tidiness. It is also what keeps the GPL on this project
  enforceable: only a copyright owner can bring an infringement action.

If you are not comfortable with that reservation, that is a legitimate
position and we would rather you know it now than discover it at merge
time. Bug reports, discussion, and review need no assignment at all.

### Copyright assignment

**Copyrightable contributions require a signed copyright assignment**
before they can be merged. This covers code, documentation, format
definitions, and test fixtures of any substance. It does not cover bug
reports, feature requests, review comments, or discussion.

The instrument is [CLA.md](CLA.md), signed separately and once. A statement
in a pull request or a commit trailer is **not** a substitute: an
assignment must be executed as its own agreement, and the project keeps a
durable record linking each accepted contribution to it.

Where the law of your jurisdiction does not permit copyright to be assigned
between living persons — Germany is the usual example — the agreement falls
back automatically to the fullest exclusive licence that jurisdiction does
allow. You do not need to work out which case you are in; the document
handles both.

If you contributed the work in the course of employment, or anyone else has
a claim on it, **their consent is required too**, on the entity form in the
same document. In most jurisdictions an employer owns what its employees
write, and an individual signature alone would grant nothing.

Contributions whose ownership cannot be established completely and on the
record are declined. This is not a judgement about the contributor — it is
that unclear title cannot be repaired later, and the project prefers a
clean reimplementation by the owner over code it cannot account for.

### Third-party material cannot be accepted

**Do not submit code you did not write**, even when its licence is
permissive and even when it would be GPL-compatible. You cannot assign
copyright in work you do not own, so third-party material — however freely
licensed — cannot pass through this process. That includes snippets from
Stack Overflow, blog posts, other projects, and vendored files.

This applies with particular force to code from **GPL-licensed projects**.
GPL compatibility is not the test here; assignability is, and copyleft code
from another author fails it.

If a third-party component genuinely belongs in remanence-lib, it comes in
as a **declared dependency** with its own licence intact, never as copied
source, and only after discussion. See [AGENTS.md](AGENTS.md) for the rules
governing which licences may be depended on and on what terms.

### Reference projects and clean-room work

Studying published format documentation — HDOS internals references, the
ZIP application note, RFC 1951 — is expected and welcome. Reading another
project's *implementation* for reimplementation is not: a close translation
is a port no matter what the source licence permits. If you have read
another project's implementation of something, say so before submitting
work in that area — that is a normal and welcome thing to disclose, not an
accusation to avoid.

### The project name

The name **Remanence** is owned by Paul Galbraith and is not part of the
GPL grant — a reservation the GPL expressly permits at section 7(e).
Forks and redistributions must use a different name; see
[TRADEMARKS.md](TRADEMARKS.md).

### SPDX headers

Use accurate SPDX copyright information in each new file:

```text
SPDX-FileCopyrightText: YEAR COPYRIGHT HOLDER
SPDX-License-Identifier: GPL-3.0-only
```

Use the appropriate comment syntax for the file type. Files that cannot or
should not carry comments must be added to `REUSE.toml` with their actual
copyright holder.
