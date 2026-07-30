# Contributing to remanence-lib

Thank you for helping improve remanence-lib. Bug reports, documentation
fixes, tests, and code changes are welcome when they preserve the project's
GPL licensing and its role as the core disk image analysis library for
Remanence Workbench.

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

remanence-lib is a Cargo workspace; a stable Rust toolchain is all the core
needs.

```bash
cargo build      # core + C FFI; regenerates crates/remanence-ffi/include/remanence.h
cargo test      # the full suite
```

The Python bindings (`crates/remanence-py`) are excluded from default
workspace builds; building them needs Python 3.10+. Distributable
artifacts are built with uv (`uv build crates/remanence-py`), which
drives the maturin build backend in an isolated environment. See
[README.md](README.md).

- The core crate (`crates/remanence`) is **dependency-free at runtime**,
  deliberately — it carries its own ZIP reader and DEFLATE decompressor.
  Discuss any new dependency before adding it; the licensing tiers in
  [AGENTS.md](AGENTS.md) govern what may be depended on at all.
- Match the existing style, add or update tests for changed behavior, and
  keep the C header and Python surface in step with the core when the
  public API changes.
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

### SPDX headers

Use accurate SPDX copyright information in each new file:

```text
SPDX-FileCopyrightText: YEAR COPYRIGHT HOLDER
SPDX-License-Identifier: GPL-3.0-only
```

Use the appropriate comment syntax for the file type. Files that cannot or
should not carry comments must be added to `REUSE.toml` with their actual
copyright holder.
