# DECISIONS

The adjudicated design-decision record. Each entry records what was
decided, by whom and when, what was weighed and declined, and where
it folded. The normative homes are elsewhere — root
[ARCHITECTURE.md](../ARCHITECTURE.md) and, once dictated, the
use-case and principle lists. This file is the adjudication trail,
and the guard against re-litigating: **anything recorded here as
killed, declined, or superseded is not revisited without new
evidence**, argued through the surface-change rule
([SURFACES.md](SURFACES.md)).

Decisions are numbered in the order first recorded — D1 the
earliest — and **a number is never reused**; the list reads
newest-first, so the top entry carries the highest number and a new
entry prepends with the next free one. The D-number is the
decision's citation handle everywhere: a decision names the vision
it supports — use cases (U-numbers), principles (P-numbers),
surfaces (S-numbers) — and it is citable downstream in design
documents, specifications, and code commits.

**The supports clause is not optional, and "none" is an answer.** A
decision genuinely demanded by nothing — a vocabulary or naming
choice — records `Supports (none)` and why. Prose in place of a
handle is the same gap wearing a sentence: a citation that resolves
to no number is not a citation, and only a numbered one can be
audited.

**A lifecycle act alone earns no entry.** Proposing, pledging,
promoting, delivering: location states the status and the commit
that moves the item is the record, so delivery evidence belongs in
that commit's message. Only a ruling made in the act's course — a
contested clause reading, a scope call, a withdrawal — is recorded
here, slim, as the ruling rather than the promotion around it.

An overruled or no-longer-relevant decision moves, number and text
intact, to the Retired decisions section at the bottom, its note
naming what overruled it — a retired decision binds nothing but
remains the record. **Entries keep the spellings of their time**: an
entry only partly overruled is annotated, never rewritten, and
correcting an entry's prose in place is never the answer — an error
and its discovery are part of the record.

## Open questions

Questions awaiting adjudication — the front of this record rather
than a separate one. Nothing here binds anything; a question leaves
this section when it is adjudicated — as a D-number only where the
ruling has no normative home, otherwise absorbed by the pledged or
in-force entry whose text carries the ruling — and the commit that
removes it is the record either way.

- **CLA legal review** — [CLA.md](../CLA.md) states intended terms
  but has not been reviewed by a lawyer, and its governing-law
  clause is deliberately unfilled. What turns on it: no external
  contribution can be accepted under it until reviewed. Settled by
  that review.

## Decisions

### D72 — `remanence-ffi`'s `src/lib.rs` splits into groups plus a root, on the core crate's shape

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-19. **Supports** S2, changing nothing it promises.
`DECISIONS.md` was searched first and returned D69 (the grouping this
extends inward, having settled `c/{include,examples,tests}` by audience)
and its "C and C++ do not split, on either crate" clause, which governs
the *header* layout and is untouched here — one `remanence.h`, one
`remanence.hpp`, one `-I` flag, exactly as before. Nothing else in the
record spoke to the crate's own Rust layout.

**The file had become the one place in the workspace with no grouping at
all.** `src/lib.rs` held 9,814 lines: 447 `extern "C"` functions, some
forty-five handle and view types, and no `mod` but the leak probe and
the tests. The core crate had been laid out in eight groups plus a root
since D49, each group's `mod.rs` stating its own seam; the ABI crate,
which mirrors that surface function for function, had nothing. What the
flat file cost was not aesthetic: `RemanenceP64Report` was defined 3,800
lines from the two functions that construct it, `remanence_device_*` sat
1,300 lines from the `RemanenceDevice` it reads, and the nesting-layer
view was named `NestedLayerView` only because the unrelated flux one had
already taken `LayerView`.

**A function's group is its ABI name prefix.** `remanence_partition_*`
is `storage/partition.rs`, `remanence_bytestream_*` is
`flux/stream.rs`, and so on through `abi`, `session`, `device`,
`discovery`, `medium`, `identify`, `assurance`, `geometry`, `catalog`,
`report`, `storage/{partition,space,entries,file}` and
`flux/{image,stream,c1541,ibm,rendition}`. That rule is deliberate and
does the same work D54's total-coverage rule does for the C++ wrapper:
where a function belongs is a lookup rather than a judgement, so the
grouping cannot drift into taste. `lib.rs` keeps the crate's
conventions, the leak probe, and the three verbs belonging to no group.

**Bitstream and bytestream stay in one file, against the prefix rule.**
`flux/stream.rs` is the largest group at 840 lines because the two rungs
are genuinely one seam: `remanence_bitstream_materialize_bytestream`
crosses between them, both backings resolve through the same pooled
medium, and both handles answer their strings from one shared
`LayerView`. Splitting on the prefix would have put a type and its only
constructor in different files to satisfy a rule whose whole purpose is
to make lookups cheap.

**The exported ABI cannot notice.** The symbols are
`#[unsafe(no_mangle)]` and carry no module path, so every one of the 447
exports is unchanged in name, signature and behaviour — verified by
`task test-ffi`'s 25 CTest tests, which compile the header standalone,
compile both `identify` examples against it, run a C++ caller through
the wrapper, and prove the `_free` discipline with the leak probe.
`cargo test --workspace` and `cargo clippy` are unchanged too, the
latter at exactly the 450 warnings it emitted before.

**What the split does reach is the generated header's order, and only
its order.** cbindgen emits in module-declaration order under `[fn]
sort_by = "None"`, so `remanence.h` is reordered: 2,004 lines moved,
with the sorted content of the two files byte-identical — the same 446
declarations and 26 typedefs, none added, none lost. `sort_by = "Name"`
would have made the header order-independent for good, and is rejected:
it would sort 446 functions alphabetically and destroy the grouping that
makes the header readable, to spare a diff that only appears when a
function changes groups. rustfmt keeps the module declarations
alphabetical, so the header now groups by module and orders those groups
by name.

**Two things could have failed quietly and were made loud instead.**
`build.rs` watched `cargo:rerun-if-changed=src/lib.rs`, which after the
split would have stopped regenerating the header whenever a submodule
changed — it now watches `src/`. And moving code across module
boundaries needed the private seams named: 62 declarations were raised
to `pub(crate)`, driven by the compiler rather than by hand, and no
field on a `pub` handle type was widened past `pub(crate)`, which is
what keeps cbindgen emitting `RemanenceSession` and its kin as opaque
typedefs rather than as C structs.

### D71 — The core crate's fixture/rig-gated integration tests move to their own crate under integration-tests/

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-19. **Supports** S1; amends D65's "the core crate's
`fixtures`/`rigs` features are untouched" clause, narrowing rather than
reversing it — the features themselves, and the in-source unit tests
they still gate, are untouched; only the nine `tests/*.rs` targets they
also gated are not. `DECISIONS.md` was searched first and returned D49
(the split's origin), D65 (the FFI/Python precedent this extends, and
the clause it amends) and D69 (the `crates/*/tests` grouping principle
this stays inside rather than overrules).

**D65 left the core crate alone for a real reason, not an oversight.**
Its complaint was that `cargo test` had come to drive CMake, `uv`,
pytest and mypy from inside `#[test]` functions — non-Rust toolchain
work wearing a Rust test's shape. Nothing in `crates/remanence/tests/`
ever did that: `flux_media.rs`, `freedos_qcow2.rs` and the rest are
plain `#[test]` functions that open a real file, the same shape as every
synthetic-image suite beside them. That distinction stands, and this
entry does not relitigate it.

**The reason to move them anyway is co-location, not the toolchain
complaint.** `integration-tests/` is already the one place a contributor
looks for everything `prep_fixtures.py` provides and everything that
reads what it provides — the fixtures themselves, the C/C++ CTest suite
that opens them, the prep script that writes them. The nine Rust suites
were the one reader left outside it, findable only by knowing to grep
`crates/remanence/tests/` for `required-features`. `git mv`-ing them to
`integration-tests/rust/tests/` puts every reader of a downloaded or
generated fixture under one directory, and — a second-order benefit
`crates/remanence/Cargo.toml`'s own `exclude = ["tests/**"]` comment
already named as the goal — a fresh clone's `cargo package` output is
never in a position to ship them, `integration-tests/` never being
inside a crate cargo packages from at all.

**Mechanically, a new workspace member, not a relocated `path`.**
`integration-tests/rust/` is `remanence-integration-tests` — a workspace
member and never a default one, on the same footing as `xtask` and
`remanence-py` — carrying its own `fixtures`/`rigs` features and the
same nine `[[test]]`/`required-features` declarations
`crates/remanence/Cargo.toml` used to. `tests/common/mod.rs` (`open_read`,
`open_write`, `manifest_dir`, `repo_root`, `fixtures_dir`,
`ensure_fixture`) moved whole rather than being re-derived — its
`repo_root()` climbs two parents from `CARGO_MANIFEST_DIR` either way,
`integration-tests/rust` sitting exactly as deep as `crates/remanence`
did. `crates/remanence/tests/common/mod.rs` keeps a copy trimmed to
`open_read`/`open_write` alone, for the fourteen suites that stayed and
never called `ensure_fixture` or `fixtures_dir`. A new task,
`task test-rust`, is the entry point — it runs
`uv run integration-tests/prep_fixtures.py` itself first (idempotent, so
a machine that already has everything pays only the existence checks),
then builds both feature tiers by default; extra arguments replace that
default rather than appending to it
(`{{.CLI_ARGS | default "--features fixtures,rigs"}}`, confirmed against
Task's actual substitution rather than assumed), so
`task test-rust -- --features fixtures` genuinely runs the smaller tier
rather than silently unioning both — though the prep step itself still
prepares everything either way, `prep_fixtures.py`'s `main()` taking no
flag for less.

**`rigs` leaves `crates/remanence` entirely; `fixtures` stays, narrower.**
`freedos_qcow2.rs` was the only place in the core crate that named
`rigs` at all, in either `src/` or `tests/`, so nothing is left there for
the feature to gate — declaring it would be exactly the drift D49's own
"a target that calls the helper without declaring a feature now fails to
compile" guard exists to catch, aimed at a feature instead of a call
site. `fixtures` stays, because `flux/c1541/renditions.rs`,
`flux/drive_profile/verdict.rs` and `flux/remanence/reconstruction.rs`
are in-source `#[cfg(test)]` modules that reach `pub(crate)` internals no
external crate can — the same reason D65 never moved the FFI crate's own
in-source tests out from behind its mirrored `fixtures` feature (D54).

**This stays inside D69's principle rather than overruling it.** D69
settled that `crates/*/tests` holds Rust tests and nothing else, arguing
from what was found in `crates/remanence-ffi/tests/c/` and
`crates/remanence-py/tests/` — non-Rust content sitting under a
Rust-suggesting name. `crates/remanence/tests/` never had that problem
and still doesn't: every file left in it is a Rust test, same as
`integration-tests/rust/tests/` is now. What moved is *which* directory
a Rust suite lives under, on the same "group by what shares an audience"
logic D69 used for `crates/remanence-ffi/c/` — the audience for a
fixture-gated Rust suite is `integration-tests/`, not `crates/remanence`.

**Weighed and declined:** relocating only the nine files' `path`s inside
`crates/remanence/Cargo.toml`'s existing `[[test]]` blocks, leaving
`cargo test --features fixtures`/`--features rigs` at the workspace root
as the invocation — kept the smallest possible diff and cargo genuinely
permits a `path` outside the package root (confirmed: `cargo package`
warns and excludes such a target rather than erroring), but left the
core crate answering to two different reachability rules for its own
`[[test]]` declarations depending on which crate the file physically
sat in, a distinction with no reader-visible reason once the file was
already gone from `crates/remanence/tests/`. Declared explicitly as the
question this entry answers, rather than assumed.

**Folded into:** `integration-tests/rust/Cargo.toml`;
`integration-tests/rust/tests/{cpm_files,flux_media,freedos_qcow2,
geometry_fixtures,hdos_files,identify_hdos_image,media_sources,
pcdos_files,sevenzip_catalog}.rs`; `integration-tests/rust/tests/common/
mod.rs`; `crates/remanence/Cargo.toml`; `crates/remanence/tests/common/
mod.rs`; `crates/remanence/tests/geometry.rs`; `Cargo.toml` (workspace
members); `Taskfile.yml`; `AGENTS.md`; `CONTRIBUTING.md`; `README.md`;
`REUSE.toml`; `planning/SEQUENCES.md`.

**No changelog entry.** Where source and test files live is not
release-facing.

### D70 — The fixture-prep script drops its uv project for inline metadata, and moves to integration-tests/

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-19. **Supports** (none) — a packaging and invocation choice
for tooling outside every application surface. `DECISIONS.md` was
searched first and returned D1, whose same-day amendment moved
`fixtures/` and `downloads/` to `integration-tests/` but left the
script that writes them, and the `test-fixture-prep/` project around
it, untouched.

**The uv project was solving a problem PEP 723 already solves.**
`test-fixture-prep/pyproject.toml` and its lock file existed for one
reason: to pin `reliquary` somewhere `uv run --directory
test-fixture-prep` could find it. A single dependency, on a single
script, does not need a project of its own — `uv run` reads a
`# /// script` block straight out of the file it is running and
provisions an ephemeral environment from it, no `pyproject.toml`, no
lock file, no `.venv` beside it. `prep_fixtures.py` now carries that
block itself, pinning `reliquary==0.1.0a2` and
`requires-python = ">=3.12"` exactly as the dropped `pyproject.toml`
did.

**The script moves to where its output lives.** `integration-tests/`
already holds `fixtures/` and `downloads/`; `prep_fixtures.py` writes
both and reads neither anywhere else, so `test-fixture-prep/` was the
only thing left keeping the writer apart from what it writes. It runs
now as one command from the repository root, provisioning itself:
`uv run integration-tests/prep_fixtures.py`.

**`test-fixture-prep/` stays, smaller.** `test-rigs/` — the
checked-in blueprint and install script, and the reliquary home the
rig cache derives from — is not fixture material to relocate; it is
authored rig infrastructure that happened to sit beside the fixtures
it shared a directory with. Moving it would have reopened the
cache-location clause D1's amendment settled the same day. So
`test-fixture-prep/` now names that directory alone.

**The recovery commands lose their free ride.** `rlq
destroy-machine`/`clean-media`, run by hand against a stuck build,
previously resolved through the project's own environment via
`--directory test-fixture-prep`. With no project there, they now name
reliquary explicitly and take the rig home's real path from the
repository root — `uv run --with reliquary==0.1.0a2 rlq ... --home-dir
test-fixture-prep/test-rigs` — which puts the version pin in two
places instead of one. Accepted: these are break-glass commands read
straight from the README while they are typed, not the pin the script
resolves against on every run.

**Weighed and declined:** moving `test-rigs/` into
`integration-tests/` as well, so `test-fixture-prep/` disappeared
outright (declined above, on the same grounds); keeping the uv
project and only relocating the script into `integration-tests/` with
a `--directory` pointed back at `test-fixture-prep/` (kept the extra
project and its lock file for no reason once the script itself can
carry the pin).

**Folded into:** `integration-tests/prep_fixtures.py` (moved from
`test-fixture-prep/prep_fixtures.py`, `test-fixture-prep/pyproject.toml`
and `test-fixture-prep/uv.lock` removed); `test-fixture-prep/test-rigs/README.md`;
`REUSE.toml`; AGENTS.md; CONTRIBUTING.md; `crates/remanence/Cargo.toml`;
`crates/remanence/tests/freedos_qcow2.rs`;
`crates/remanence/tests/geometry_fixtures.rs`;
`crates/remanence/tests/common/mod.rs`;
`crates/remanence/src/flux/remanence/reconstruction.rs`;
`crates/remanence-ffi/c/tests/check_fixture.cmake`.

**No changelog entry.** How the fixture-prep script is packaged and
invoked is not release-facing.

### D69 — `c/{include,examples,tests}` groups the whole C/C++-facing surface, apart from the crate's own Rust scaffolding

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-19. **Supports** S2, S3; amends D69's own predecessors on path
alone — D46, D47, D50, D53, D54, D65, D66, and D48/D51's Python-side
counterparts describe mechanisms that all stand unchanged, only relocated.
`DECISIONS.md` was searched first and returned all of these, none of
which this overrules in substance.

**The forcing question was narrower than "reorganize by language."**
`crates/*/tests` should hold Rust tests (a plain statement of what the
name already promises everywhere else in this workspace) — and after
D65, `crates/remanence-ffi/tests/c/` held zero Rust files, entirely C/C++
content sitting under a name that no longer described it.
`crates/remanence-py/tests/` was a genuine mix: `stub_matches_module.rs`
(Rust, cargo-driven) beside five pytest files and `mypy_fixtures/`
(neither). Only that mismatch needed fixing — nobody had raised a
problem with `include/` or `examples/`, which were already correctly
named for what they hold.

**C and C++ do not split, on either crate.** `remanence.hpp` has a direct
`#include <remanence.h>` — splitting `include/` into `c/include/` and
`cpp/include/` would force every C++ compile site (the CMake project,
and any real external C++ consumer) to carry two `-I` flags forever,
where one suffices today, for no offsetting benefit: nothing about
testing or examples needed C and C++ kept apart, only kept apart from
Rust. `examples/` and `external-tests` (this entry's own transient name
for what became `c/tests/`) *could* have split cleanly at the file
level — no cross-file dependency forced them together — but doing so
while `include/` stayed unsplit would have left the crate with C/C++
mixed in one folder and separated in two others, a worse inconsistency
than the one being fixed.

**So `remanence-ffi` groups by audience, not language:**
`crates/remanence-ffi/c/{include,examples,tests}` is everything a C/C++
consumer or contributor to that surface touches, C and C++ still
combined within each exactly as before, sitting apart from `src/`,
`Cargo.toml`, `build.rs`, `cbindgen.toml` — this crate's own Rust
machinery. `tests/` at the crate root no longer exists at all, having
held nothing but this. `Cargo.toml`'s `exclude` narrows from
`tests/**` to `c/tests/**` specifically, since `c/include/` and
`c/examples/` still ship (the header a consumer builds against,
`identify.c`/`identify.cpp` as documentation) — only the part that tests
*this repository* stays out.

**`remanence-py` groups by the same principle, expressed as Python's own
convention.** `crates/remanence-py/python/src/remanence/` (was
`python/remanence/`) and `crates/remanence-py/python/tests/` (was mostly
`crates/remanence-py/tests/`, minus the one Rust file) are the standard
Python src-layout — package source and its tests as siblings under one
project root — chosen over inventing a workspace-specific shape.
`pyproject.toml`'s `python-source` moves from `"python"` to `"python/src"`
accordingly, which is not a cosmetic rename: maturin bundles into the
wheel only what sits inside `python-source`, so `python/tests/` as a
*sibling* to `python/src/` rather than a child of the old `python/` is
what makes the pytest suite structurally impossible to ship in the
wheel, rather than merely a convention nobody violates yet. Verified
directly — built both the sdist and the wheel after the move: the sdist
carries `python/tests/*.py` (D48 still holds) and correctly omits
`python/tests/mypy_fixtures/**`; the wheel carries exactly
`remanence/{__init__.py,__init__.pyi,py.typed,remanence.pyd}` and
nothing else, no test file present. `crates/remanence-py/tests/` keeps
only `stub_matches_module.rs`, the one thing there that was ever
actually a Rust test.

**Two real defects caught by insisting on a clean rebuild rather than
trusting a moved file's stale mtime:** `workspace_dir()`'s
`.canonicalize()` prefixes paths with `\\?\` on Windows, which broke
MSVC's SARIF diagnostics parser the moment `xtask` needed to hand CMake
a path built from it (`Invalid URI: The hostname could not be parsed`
for a source file that plainly existed) — fixed by using `.parent()`
instead, needing no `..` resolution since the manifest directory is
already absolute. And `crates/remanence-ffi/c/tests/CMakeLists.txt`'s
`REMANENCE_WORKSPACE` relative-path climb needed recounting for the new
depth (`../../../..`, matching `tests/c/`'s original depth — `c/tests/`
sits exactly as deep, just under a different parent) — caught because a
stale CMake cache from the old source path failed loudly
("does not match the source ... used to generate cache") rather than
silently reusing the wrong tree.

**Weighed and declined:** splitting `include/`/`examples/`/`tests` by
language on both crates for uniformity with the `c/` grouping —
declined because `include/`'s cross-file dependency makes that split
strictly worse there, and applying it only where it's free (`examples/`,
`tests`) while `include/` stays combined trades one inconsistency for
another; and nesting `remanence-py`'s tests inside the pre-existing
`python/` directory without introducing `src/` — declined because
`python-source = "python"` would still cover the whole directory,
leaving test-file wheel-exclusion a convention rather than a structural
guarantee.

**No changelog entry.** Where source and test files live is not
release-facing.


### D68 — `remanence-ffi` rejoins `default-members`; `remanence-py` stays out for what it alone still needs

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-19. **Supports** S2, S3; amends D52, narrowing rather than
reversing it. `DECISIONS.md` was searched first and returned D52, whose
"name neither" resolution this partly undoes — for a reason D52 itself
did not have available to it.

**D52 objected to an asymmetry with no cause a caller could see:**
`remanence-ffi` was a default member and `remanence-py` was not, for no
reason tied to what either crate actually cost to build — CMake and a
C/C++ compiler were still needed for `remanence-ffi`'s own `cargo test`
at the time, same as `remanence-py`'s Python toolchain. Naming neither as
default was the fix available then, because the two crates' real costs
were not actually different.

**They are different now.** D65 moved every C/C++ check out of
`cargo test` entirely, reached only by `task test-ffi`. Verified
directly, from a clean state: `cargo build -p remanence-ffi` and
`cargo test -p remanence-ffi` need nothing beyond rustc — cbindgen only
writes the header's text, compiling nothing, and the in-source unit
tests are plain Rust. `remanence-py` has no equivalent change available
to it: pyo3's build script resolves against a real Python interpreter to
build against at all, a dependency this migration never touched and
could not remove by relocating checks, because it is not a check —
it is what compiling the crate itself requires.

**So default membership now tracks a real difference, not an arbitrary
one.** `crates/remanence` and `crates/remanence-ffi` are default
members; `crates/remanence-py` is not, because its lifecycle depends on
an external Python in a way the other two no longer depend on anything
external at all. This reintroduces the *shape* D52 objected to — one
crate default, a sibling not — without reintroducing what D52 actually
objected to, which was the absence of a reason.

**The asymmetry stays smaller than the one D52 fixed, on purpose.**
Default membership only ever governed the free (rustc-only) portion of
each surface's checks — neither the C/C++ suite nor the Python one was
ever reached by it, before or after this entry; both need their own
`task` invocation regardless. What default membership decides now is
narrower than what it decided when D52 was written, which is part of why
reintroducing the asymmetry is safe to do.

**`cargo build --workspace` and `cargo test --workspace` are still
required checks for everything**, unchanged from D52: what is optional
is being the person who remembers to ask for them beyond the default,
not whether they are asked for at all.

**No changelog entry.** Which crates a bare `cargo build` reaches is not
release-facing.


### D67 — `task` replaces `just`, dropping the last MSYS2/bash dependency

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-19. **Supports** S2, S3; amends D65 to name Task rather than
`just` as the runner — the CMake/CTest registration and `xtask`'s role
both stand, unchanged. `DECISIONS.md` was searched first and returned
D65 and D66, the second of which this follows directly: having dropped
MSYS2 as a supported build *toolchain*, `just`'s own recipes still had a
hard, unavoidable dependency on a real shell to run them at all.

**`just` has no shell of its own.** Every recipe in the `justfile` this
replaces carried a `#!/usr/bin/env bash` shebang, because `just`
delegates a recipe body to whatever the host provides — on Windows, that
means git-bash/MSYS2, or nothing runs. Dropping MSYS2 as a toolchain
while still requiring it to invoke the tasks that check the toolchain
was the asymmetry this closes. Task embeds its own shell interpreter
(`mvdan.cc/sh`), so a task runs identically wherever it is invoked from,
with no external shell dependency at all.

**Task's own bundled tools are minimal, and that reshaped `xtask` rather
than just relocating text.** Task ships a small Go-native file-operations
set on Windows (`cp`, `mv`, `mkdir`) — not `sed`, `grep`, `xargs`, or
shell arrays. The `just`-era design had `xtask` print `SLUG=`/`CMAKE_ARG=`
lines for the recipe's shell to parse and reassemble into an argument
list; porting that parsing to Task would have silently depended on
coreutils being on `PATH` again, the exact dependency this is meant to
drop. So the parsing moved instead of the text: `xtask ffi` now runs
`cmake` configure and build itself (`std::process::Command`, real argv,
no shell involved) and prints one line — the build directory — for
`task test-ffi` to hand straight to `ctest`. `xtask py-stage` is new for
the same reason, taking over the compiled-module staging `python_suite.rs`
used to do before D65 deleted it. This is a genuine simplification, not
a workaround: it also removes every shell-quoting hazard the `just`
version carried.

**Three things broke while proving this, each fixed and worth recording
so they are not rediscovered:**

- `workspace_dir()`'s `.canonicalize()` prefixes its answer with `\\?\`
  on Windows — a verbatim path CMake's own tools mostly tolerate but
  MSVC's SARIF diagnostic output does not, failing a `try_compile` with
  `Invalid URI: The hostname could not be parsed` for a source file that
  plainly exists. Fixed by using `.parent()` instead: the manifest
  directory is already absolute, so there is no `..` left to resolve and
  no prefix to trip over.
- `{{.CLI_ARGS}}` is *already* shell-quoted by Task for direct use
  (confirmed directly: `-LE "rigs|fixtures"` becomes the literal text
  `-LE 'rigs|fixtures'`) — the opposite of `just`, whose equivalent
  needed defensive quoting because it is raw text substitution. Applying
  the fix `just` needed here — capturing it into a variable and
  re-quoting — double-quoted it instead, so the embedded shell saw
  literal `'` characters as part of the value and the label filter
  silently excluded nothing. Caught by testing the exclusion case
  directly, not by inspection.
- Task's `cmd:` scripts do not default to `errexit`. `set -euo pipefail`
  (confirmed supported by `mvdan.cc/sh`) is stated explicitly at the top
  of both tasks now; without it a failed step would not stop the ones
  after it, which is the exact "quietly does not run" shape D64 exists to
  refuse.

**Also found, unrelated to Task itself: running the whole `test-py` task
from `crates/remanence-py` made `uv run -- cargo build` treat that
directory as a uv project and create a stray `uv.lock`.** Fixed by
running that one step from the workspace root, as the original design
did, and changing directory only for the later pytest/mypy steps that
need `pyproject.toml`'s relative paths.

**Weighed and declined:** keeping `just` for the C/C++ and Python tasks
specifically since its own shell-script bodies already worked — declined
because "drop MSYS2 everywhere" (D66) means the tooling too, not only the
toolchain being tested, and `just` cannot reach that without a shell
`Taskfile.yml` does not need.

**No changelog entry.** How the suite is invoked is not release-facing.


### D66 — Windows means MSVC only; a MinGW/MSYS2 build is refused, not accommodated

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-18. **Supports** S2, S3; overrules D46's toolchain-matching
accommodation and the commit `a58247c`'s "a local MSYS2 build is
legitimate" stance, in favor of MSVC as the one supported Windows
toolchain. Genuine Linux/macOS support — `Toolchain::Native` in `xtask`,
unrelated to MSYS2 at all — is untouched and stays wanted. `DECISIONS.md`
was searched first and returned D46 and D65, whose accommodation this
overrules, and named the commit the search also found.

**D46 solved the wrong problem, in hindsight.** It read what toolchain
built the library and matched CMake to it, on the premise that a
developer working from an MSYS2 shell was someone the project should
keep working smoothly for. Living through what that premise actually
costs — D63's uv-interpreter routing, D64's fail-rather-than-skip policy,
D65's whole CMake/CTest migration, and this session's own `libpython3.dll`
diagnosis — is what overturns it: matching two Windows toolchains buys a
developer nothing an MSVC-only shell does not already give them, and
every one of those decisions exists to manage a mismatch that a second
supported toolchain creates and a single one cannot.

**So the two now refuse rather than adapt, symmetrically.** `xtask`
(`xtask/src/main.rs`) still reads which toolchain a build actually
produced — that read is right and stays, per D65's own reasoning — but
where it once selected a matching CMake generator and compiler, it now
panics with a clear message naming the fix (build with the native
toolchain) before CMake configures at all.
`crates/remanence-py/build.rs`, deleted by D65 for having become a bare
diagnostic recorder, is reinstated with an actual job: it reads
`pyo3_build_config::get().lib_name()` and panics if the name is
`lib`-prefixed, which is exactly the claim an MSYS2-built Python's import
library makes about itself (`libpython3` rather than the native
convention's `python3`) — read from what pyo3 resolved, not guessed from
the host, the same principle D46/D65 already established.

**The Python-side refusal closes an incident at its actual source.**
0.0.1a4 shipped a MinGW-tagged wheel to PyPI, and failed at `import` for
every consumer of it, because nothing stood between `uv build`'s internal
`cargo build` (via maturin) and a published artifact. `build.rs` runs for
*any* compile of the crate — a bare `cargo build -p remanence-py`, `uv
build`, `task test-py` — so that incident is no longer reachable at all,
not merely caught later by a test.

**No equivalent build.rs-level refusal for `remanence-ffi`, deliberately
asymmetric.** `remanence-ffi` publishes only source to crates.io — no
compiled cdylib ever ships — so there is no artifact-reaches-a-registry
risk to guard against the way there was for `remanence-py`'s wheel. A
consumer building it from source uses their own toolchain, unrelated to
whatever built a maintainer's dev copy. The only real consequence of a
MinGW dev-build here is an untestable `task test-ffi`, which `xtask`
already refuses at exactly the point that matters.

**Weighed and declined:** keeping the accommodation as a courtesy to
whoever develops from an MSYS2 shell — declined because the courtesy
assumed matching two toolchains was worth its cost to someone, and this
project's own experience is the disproof: verifying it took a session's
worth of diagnosis, three prior decisions, and machinery nobody asked
for by name; and refusing only inside `task test-py`'s own build step
rather than in `build.rs` itself, declined because that would leave
`uv build`/maturin's internal build — the actual 0.0.1a4 path — unguarded.

**No changelog entry.** Which toolchains a contributor's local build is
checked against is not release-facing; the classifiers `pyproject.toml`
already states (Windows, native) do not change.


### D65 — `just` runs the C/C++ and Python checks now; `cargo test` runs only Rust's own

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-18. **Supports** S2, S3; overrules D46's and D48's "the Rust
tests drive this" clauses, and D49's/D54's Cargo-feature gating for the
FFI crate specifically — the core crate's `fixtures`/`rigs` features are
untouched. Amends D63 to name `just test-py` rather than `cargo test` as
what routes through uv. `DECISIONS.md` was searched first and returned
all of these, each turning on the same premise this reverses: that a
check reached from `cargo test` is a check that keeps running.
[Superseded on the runner by D67 — `just` is replaced by `task`
throughout this entry; the CMake/CTest registration and the choice to
keep toolchain-matching in `xtask` both stand unchanged.]
[Narrowed by D71 — "the core crate's `fixtures`/`rigs` features are
untouched" no longer holds for the `tests/*.rs` targets those features
gated: those nine moved to `integration-tests/rust/`, reached by the
same `task test-*` pattern this entry set for the FFI and Python
surfaces, for a different reason than this entry's own (D71 says which).
The features themselves, and the in-source unit tests still behind
`fixtures`, are exactly as this entry left them.]

**The premise was true and the mechanism was the cost.** Nothing here
disputes D48's or D64's reasoning — a check nobody remembers to run is a
check that has already failed, silently — only where the discipline
should sit. Driving CMake and `uv` from inside `#[test]` functions meant
`cargo test` itself carried the toolchain-matching logic (D46: which
import library a C caller can link), the leak-probe build, the pytest
staging, and the mypy invocation — none of it about testing Rust, and
increasingly the reason `cargo test -p remanence-ffi`/`-p remanence-py`
needed explaining rather than running.

**So the two move to where they already belong.**
`crates/remanence-ffi/tests/c/CMakeLists.txt` now registers its own
CTest tests — `enable_testing()`, `add_test()`, CTest `LABELS` for what
D49/D54 gated behind Cargo features — and `just test-ffi` drives
configure, build and `ctest` in one step. `just test-py` builds through
uv, stages the module, and runs pytest and mypy directly; neither needs
a Rust `#[test]` to exist at all.
[Superseded on paths by D69 — `tests/c/` is `c/tests/` throughout this
entry (and every other `tests/c/...` mention below), `include/`/
`examples/` are `c/include/`/`c/examples/`, and `crates/remanence-py/
python/remanence/` is `crates/remanence-py/python/src/remanence/`. The
CTest registration and everything else this entry describes stand
unchanged — only where it all lives moved.]

**One piece stayed in Rust, deliberately: which toolchain built the
library** (D46's "read from what a build produced rather than the
host's default"). A new crate, `xtask` — unpublished, and a workspace
member but never a default one — is the only place that logic lives
now, extracted rather than reimplemented: reading a claim about one file
instead of guessing from the host or from CMake's own compiler search is
the exact distinction D46 already insisted on, and putting the same
logic in CMake instead would have reintroduced the gap that guarding was
written to close.

**The same fail-rather-than-skip policy holds, moved.** D64 declined
`REMANENCE_SKIP_*` variables because a silenced check reads exactly like
a passing one. `just test-ffi`/`just test-py` carry no equivalent: a
missing CMake, compiler or `uv` still fails the recipe outright (the
justfile runs under `set -euo pipefail`, and CMake's own
`message(FATAL_ERROR ...)` covers a missing tool at configure time), and
choosing not to run a recipe leaves its own trace — an unrun
`just test-ffi` is a `just` invocation nobody made, findable the same way
an unrun `cargo test -p remanence-ffi` used to be. What no longer holds
automatically is *reach*: `cargo test --workspace` used to be the one
command that touched every surface, and after this it touches only the
Rust ones. Two new commands are required checks now instead of being
folded into that one; `AGENTS.md`'s "Required checks" says so, and lists
both.

**Converged the `rigs` gate onto two more tests, closing a live instance
of D49's bug.** `abi_leaks` and `wrapper report` (`cpp_wrapper.rs`) each
needed `freedos-parttest.qcow2` without declaring it — the same shape
`c_abi_rig.rs` was split out to fix, reached past that guard on two
tests nobody had audited since. All three now share one CTest label
(`rigs`) and one `FIXTURES_SETUP` existence check, in
`tests/c/CMakeLists.txt`.

**A real loss, named rather than hidden:** `stub_typechecks.rs`'s
line-and-code cross-check of `rejects.py` against its `# expect:`
markers is not reproduced in `just test-py`. The recipe checks that
mypy refuses the fixture as a whole; it no longer checks that each line
is refused for the *specific* code its marker names. Reproducing that in
shell was judged not worth the script it would take — stated here so a
future reader does not assume a parity that is not there.

**Weighed and declined:** reimplementing the toolchain classification in
CMake (`CMAKE_C_COMPILER_ID`, or a host-triple guess) — declined for the
reason above, being exactly the artifact-vs-environment gap D46 already
refused; and putting `xtask`'s logic in `crates/remanence-ffi/src/bin/`,
declined because that crate publishes to crates.io and states its own
principle that a released artifact carries what a consumer runs and
nothing else — a `[[bin]]` there would ship in the tarball and force a
dev-only dependency into a real one.

**No changelog entry.** How the suite is invoked is not release-facing.


### D64 — A test run that reaches a surface runs all of it; there is no variable that excuses a check

**Decided** Paul Galbraith, 2026-08-18. **Supports** S2, S3; overrules
the escape-hatch clauses of D45 and D48. `DECISIONS.md` was searched
first and returned both, each of which paired "an absent tool fails
rather than skips" with a variable that skips anyway.

**The pairing was self-cancelling.** `REMANENCE_SKIP_CC`,
`REMANENCE_SKIP_MYPY` and `REMANENCE_SKIP_PYTEST` each existed so that
not running a check would be "a decision somebody made and can be
found" — but a variable set once in a shell profile or a CI job is not
found by anyone afterwards, and the check it silences reads exactly like
the passing check the surrounding rule was written to prevent.

**Choosing not to test a surface is already expressible, and that is
where the choice belongs.** `remanence-ffi` and `remanence-py` are
selected by `-p` and by `default-members`; a reader who does not want
the C or Python toolchains simply does not build those crates. What the
variables added was a second, weaker way to opt out — one that opts out
of a *check* while still claiming to have tested the crate.

**So the rule is the whole of it: a run that reaches a surface runs
every check that surface has.** The only tests that stay optional are
those needing an external fixture download, which is a dependency no
local toolchain can satisfy.

**Weighed and declined:** keeping the variables for CI convenience,
which is the case that most wants them and is exactly where a silenced
check does the most damage; and converting them to `#[ignore]`, which
would still let `cargo test` report success with checks unrun, differing
only in that the omission is printed.

**No changelog entry.** How the suite is invoked is not release-facing.


### D63 — uv chooses the interpreter that runs the Python suite, always

**Decided** Paul Galbraith, 2026-08-18. **Supports** S3; amends D51.
`DECISIONS.md` was searched first and returned D51, whose "pytest is
found the way mypy is" clause this overturns.

**D51's search was mitigating a variable that does not exist.** It tried
`python -m pytest`, then `pytest`, then `uv run --with pytest`, pinning
the last to the interpreter `build.rs` recorded so uv could not drift
onto another. But the module is `abi3-py310`: it links `python3.dll`,
the stable-ABI forwarder every CPython 3.10 and later ships, so any of
them can import it. There was no mismatch to prevent — and the failure
message closed by asking the reader to check whether the two Pythons
matched, which is a harness admitting it did not know what it had run.

**So nothing here selects an interpreter.** One command,
`uv run --with <tool> --no-project <tool>`, for pytest and mypy alike,
with no `--python`, no version request and no implementation request.
What uv picks is not this crate's business, and the constraints the
artifact does carry are stated once, in `pyproject.toml`.

**The MSYS2 build is the case this gives up, knowingly.** A module built
from an MSYS2 shell links `libpython3.dll`, which exists only inside
MSYS2, and uv cannot supply that interpreter: it discovers Pythons
through its own registry and managed installs, and does not see MSYS2's
even when that one is first on `PATH` (checked against uv 0.11.25). So
the Python suite fails for such a build. `build.rs` still records which
interpreter the module was built for, and the failure names it, because
the bare error is `DLL load failed` and explains nothing.

**D51's claim that the two finders became one helper is true as of
here.** `stub_typechecks.rs` still carried its own copy of the search;
it now uses the shared helper, which needs no argument — mypy reads the
stub and never imports the module, so no build fact bears on it.

**Weighed and declined:** classifying the build in `build.rs` and
running an MSYS2 build under its own interpreter, which keeps that case
working at the cost of a second command, a flavor enum, and an
implementation guess — the guess being the weak point, since "not
MinGW" is not the same claim as "CPython", and a PyPy or GraalPy build
would fall through it; and asking uv for `>=3.10` or `cpython>=3.10`,
declined as specification restating what `requires-python` and the
classifiers already declare.

**Reopened by** uv gaining MSYS2 support, which would make the given-up
case work with no change here.

**No changelog entry.** How the suite is invoked is not release-facing.


### D62 — A flux recording's sectors compose an addressed extent, and a hole refuses only the reads that touch it

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-17. **Supports** S1, S2, S3; in-force P3, P4, P13, P16–P19, P28.
Shapes pledged F78.

**The CBM DOS layer composes no addressed extent, and that was read as a
property of flux rather than of CBM DOS.** A recording's blocks are
addressed by the recording, so the partition over the 1541's sector layer
backs onto blocks and has no linear extent at all; the one namespace
declarable over it is `cbmdos`, and `filesystem_as("fat")` there is
refused by name. That is right for CBM DOS and wrong as a general rule.
FAT, HDOS and CP/M all read a linear space, and an FM or MFM recording
*has* one: its sectors state a cylinder, a head and a sector number that
compose exactly the geometry ordering every one of those filesystems was
written against.

**So the IBM sector layer composes an addressed extent, and the reach is
free.** A FAT or HDOS volume on an MFM floppy opens through the same
`Device` seam a hard-disk image opens through, with neither side learning
about the other — no flux vocabulary reaches the filesystem adapter and
no filesystem vocabulary reaches the recording. This is the payoff F78
pledged, and it is delivered by presenting an extent rather than by
teaching any adapter what a recording is.

**The geometry is derived from the claims and refused where it is not
uniform.** A linear image needs one sector size, one contiguous run of
sector numbers per track, and every cylinder and head present. All three
are read off what the records state for themselves — not off the drive
profile, which declares a nominal geometry the recording may not match.
A recording that fails any of them is refused by name showing what it
states, rather than being flattened into an ordering that would put every
file's contents somewhere other than where they are. Non-uniform
recordings are proposed F80's subject and are not read here.

**A hole refuses only the reads that touch it.** A sector the recording
never stated, or one whose CRC disagrees, is a hole in the extent. The
extent still composes, its length is still the geometry's, and a read
covering the hole is refused naming the address — every other read
answers. The alternative, refusing the whole extent, would mean one bad
sector anywhere costs the entire disk; under this rule the directory
still lists and every file that does not live on the damaged sector still
reads whole. That is P28's degraded reading applied at the seam where the
damage actually is, and it never fills: nothing is zeroed, and the
refusal carries both checksums.

**Weighed and declined:** materializing the whole recording into a linear
buffer and handing that to the adapter, which reads well until the hole —
the buffer has to hold *something* there, and every value is a lie the
layers above cannot see through; and giving the adapters a sector-shaped
door beside the byte-shaped one, which is a second seam beside a working
one for no gain, and is what F78's design already declined.

### D61 — Step pitch is declared as a rational pair, and consumers never divide it

**Decided** Paul Galbraith, 2026-08-06, in conversation; recorded here
2026-08-16. **Supports (none)** — a representation choice inside a
private struct, disturbing no use case and no principle. Shapes
[design/drive-profile-strata.md](design/drive-profile-strata.md).

**A drive's step pitch and a recording's are two numbers, and what
matters is their ratio.** How many steps a mechanism takes per recorded
track is not a property of the mechanism: a 96 TPI head takes two steps
over 48 TPI media and one over 96 TPI media, and a 96 TPI *instrument*
capturing 100 TPI media stands in the ratio 24/25. A single count
answers for exactly one pairing.

**The pair is rational — `tpi_numerator` over `tpi_denominator` — and
not a float.** A double cannot represent 96/100 = 24/25, so every
comparison would need an epsilon, and an epsilon is undeclared policy
wearing a number. That is the deciding argument rather than a
preference about types.

**What makes the rational form cheap is that consumers never divide.**
Comparisons cross-multiply; admission is a divisibility check; and the
one true division — projecting one frame onto another — observes its
remainder into the declared-loss account rather than discarding it. A
representation that is only ever multiplied and compared does not need
the precision a division would.

**Weighed and declined:** a float pitch with epsilon comparison (the
undeclared policy above); and a bare integer step count, which is what
the code held and which silently bakes the *capture* drive's pitch into
a constant belonging to no declared owner.

**Delivered in part.** The pair is stored and the 1541's documented two
steps derive from 96 over 48 rather than being asserted. The arithmetic
is still integer: a non-integer ratio answers zero steps and refuses
every location, where 24/25 should address every twenty-fourth. The
gap is recorded in the design above rather than left in the code alone.

> **Annotation (2026-08-16, same day):** the remaining half landed.
> `Stepping` now reports a cadence — steps taken against tracks covered,
> reduced by the common divisor — so 96 over 100 addresses every
> twenty-fourth step and advances twenty-five tracks. Finishing it also
> corrected a claim made above it: a mechanism *coarser* than its
> recording was said to be unable to address it, and in fact reaches
> every other track. The clause stands as what was believed when the
> entry was written.

### D60 — Sector ordering is resolved by the image format, and only where the format states it

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-16. **Supports** S1; in-force P4, P12, P13, P18. Shapes pledged
F68.

**The same recording arrives in two orders.** An ImageDisk track stores
its sectors in the physical order they were recorded and states each
one's id separately; a raw dump of that disk holds them in id order,
because whoever dumped it flattened the interleave. Something has to say
which order a byte offset is in, and the choice is not free: every layer
above — the CP/M layout's skew table above all — is written against one
answer or the other.

**It belongs to the image format, because that is where the evidence
is.** The sector-id map is inside the ImageDisk file. A filesystem
adapter resolving the order would be applying a rule it cannot see the
basis for, which is exactly the arrangement P12 and P18 keep out of the
seams: the module making a claim is the module that knows why. So an
adapter presents sectors in the order the recording numbers them, and it
does so *only* where the format states that numbering.

**The converse is half the ruling.** A raw dump states no ids and no
interleave, so nothing is resolved for one and nothing is guessed:
whatever ordering remains is a declaration some layer above makes and
takes responsibility for. This is what keeps the rule from becoming
"adapters normalize", which would have them inventing an answer for
formats that supply none.

**The Heath CP/M disks are what forced it and what checks it.** The
hard-sectored dumps need a four-way skew declared in the CP/M layout,
the interleave having lived in the drive's BIOS. The soft-sectored
ImageDisk images of the *same release* need none: the interleave is in
the sector numbering, and the format states it. Under a single rule that
put ordering in one place for both, one of those two must read wrongly —
and wrongly in this format's characteristic way, where the directory
still lists and only file contents come back interleaved.

**Weighed and declined:** resolving ordering in the filesystem layout,
which would make a CP/M block depend on which container carried the disk
and put the image format into the namespace's vocabulary; and
normalizing every adapter to some canonical order, which reads well until
a format that states nothing has to be normalized, at which point it is
guessing under another name.

### D59 — The flux rungs stop naming their family, which takes D39's surviving qualifier with it

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-16. **Supports** S1, S2, S3; in-force P5, P12, P13, P30; pledged
F76. Partly overrules **D39**.

**D39's qualifier ruling is overturned by its own reasoning, not
against it.** That entry dropped `c1541` from
`C1541Bitstream::materialize_c1541_bytestream` and kept it on
`FluxImage::materialize_c1541_bitstream`, and the test it applied was
whether the word does work: a receiver that is nothing but a 1541 type
restates the family for nothing, while a `FluxImage` is no c1541 type,
so there the word said which family was being materialized. Both halves
were right at the time.

F76 changes what the word can honestly say. With the rungs named
`Bitstream` and `Bytestream`, the family is no longer in any receiver on
the path, and the image states for itself which family it holds — so the
verb reads that declaration and refuses by name where nothing enrolled
matches it. A `c1541` in the spelling would then be a claim about the
result that the call does not make, which is worse than a redundant
word: the first defect D39 was fixing was one word meaning two things,
and this would be one word meaning something untrue.

**What is not overturned is the test.** D39 asked whether a qualifier
does work; this entry applies the same test to a receiver whose meaning
changed underneath it. The ruling would be identical on the old surface,
which is why this supersedes one clause and leaves the rest of D39
standing.

**Weighed and declined:** keeping the qualifier and reading it as "the
c1541 case of a general verb" (it is not a case of anything — the verb
is general and would be advertising one family's name on every other
family's call); spelling it `materialize_declared_bitstream` to say the
family comes from the artifact (accurate, and it names the mechanism
rather than the result, which no sibling verb here does).

### D58 — The machine tier is withdrawn, the session being the device scope until nesting needs otherwise

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-16. **Supports** S1, S2, S3; in-force P19, U3, U4, U33; pledged
P32. Follows **D57**.

**D57 took the tier's only consumer.** The pledged P32 amendment
inserted a machine between the session and its devices, and its
strongest justification was written into its own text: a machine's
namespace composes over its own devices and no others, *so a composer
never letters a slot that belongs elsewhere*. With guest volume mapping
withdrawn, nothing in the library read a device set as a set or read
attachment order at all. What remained was a scope whose every
mechanism — attachment identities scoped per machine, the link address
carrying a machine name, the teardown cascade — existed only because
there could be more than one machine, and nothing needed two.

**Structure ahead of demand is not the same as structure ahead of
plumbing.** Pre-building a seam is right when the demand is known and
only the implementation is missing; it is wrong when the demand is what
would *shape* the seam. The tier's surviving justification is artifact
nesting — a host's archive in one machine and the disk inside it in
another — and that journey is unbuilt: nesting is still special-cased to
ZIP and 7z by file extension and resolves one level deep. A tier built
against a journey nobody has walked is a guess about what that journey
will need, and it was being carried on every surface, in three
languages, at a cost paid per call.

**The code stops anticipating it, and so does the shelf it sat on.**
P32's own base text already says there is no separate machine object and
that the session is the scope, so the code now honors the principle as
written. The pledged P32 amendment that inserted the tier is **split**:
what it claims about devices — concrete entries, the family lineage, the
addressing nature, the archive's virtual slot — is delivered and stays
pledged, and the machine tier itself is **withdrawn to `proposed/`**. A
pledge says the project will do it; the tier's own justification is a
nesting journey that is unbuilt and whose recursion (P25) is itself only
proposed, so pledging the tier would rest a pledge on an unmade
decision. Argued and binding nothing is exactly what `proposed/` is
for.

**What a caller loses is one word.** Every device verb kept its
spelling: `add_device`, `device`, `devices`, `release_device` were
already on `Session`, delegating to the anonymous machine, so a
single-machine caller's code is unchanged. What goes is `Machine`,
`MachineView`, `add_machine`, `machine`, `machines`, `release_machine`
and their C and Python mirrors — the surface only a caller who wanted
two device sets ever touched, and no journey in the use cases wanted
one.

**Weighed and declined:** leaving the tier standing as pre-built
structure (the cheapest option, and it leaves the documents claiming a
load the code does not carry — S1 naming types no journey reaches, and
tests asserting a separation nothing consumes); keeping `Machine` as a
deprecated alias for `Session` (pre-1.0 promises no compatibility, and
an alias is a second name for one thing, which is what the vocabulary
rules refuse); and striking the pledged amendment outright rather than
banner-flagging it (the nesting argument is still good and still
unanswered — what was wrong was building against it early, not making
it).

**Reopens when:** an artifact-nesting journey needs two device sets in
one claim scope — which is the proposed amendment's own argument, and
the point at which the tier's shape can be read off a real journey
rather than guessed. The names are spent either way: `Machine` and
`MachineView` were issued and withdrawn, and the tier that returns
should be named for what that journey shows it to be.

### D57 — Guest volume mapping and drive lettering leave the claim, and the seam stops at one filesystem's own tree

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-16. **Supports** S1, S2, S3; in-force P19, U3, U4. Strikes
pledged **U16**, **U30** and **P35**, and the pledged P19 amendment that
re-homed the composer; retires **D55**.

**The claim was that this library could say what a guest called a
volume.** In-force P19 carried a namespace-mapping composer with three
constraints; pledged P35 owned the machine namespace it fed; and pledged
U16 and U30 were the consumed and derived journeys over it. The
delivered half was DOS: an assignment rule
chosen from the installation read off the booting volume, applied over a
machine's device set, answering letters with provenance.

**What is withdrawn is the question, not an answer to it.** The
composer's constraints were good ones and the DOS derivation honored
them; nothing here says the letters it produced were wrong. What the
project no longer claims is that deriving them is *its* job. A guest-side
name is a fact about an operating system's own configuration, one seam
above the storage this library reads, and every consumer that wanted a
letter already held the volume identity the inspection report issues —
which is the durable half, and the half a consumer can map in whatever
terms its own world uses. Carrying the other half obliged the project to
claim an OS-configuration seam, per family and per version, for a
correspondence the caller was better placed to state.

**The refusal is silence rather than an undetermined reading.** P19 now
says the question is outside the claim, which is a different answer from
the composer's `Undetermined`: undetermined asserts that a letter exists
and could not be settled, and that assertion is itself a claim about a
guest. A seam that answers nothing cannot be read as answering wrongly.

**The machine survives; only what it was asked survives less.** A
`Session` still holds machines, machines still hold devices in
attachment order, and a device still links one medium — P32's model is
untouched, and U4's per-disk `inspect()` is the whole of what a caller
walks. What went with the letters is what only they demanded: DOS
installation recognition, the boot outcome, the machine-level report,
and `declare_boot_device`, whose one purpose was settling which
installation's rule applied. An empty drive stays first-class
configuration on its own account — the machine held it — rather than
because a letter reached it.

**Weighed and declined:** keeping the machine report and the boot
outcome without the letters (the report's content past the device list
was the installation recognition, which existed to choose an assignment
rule; kept, it would be a seam claiming to recognize operating systems
with nothing downstream asking); keeping the derivation crate-private
against a later consumer (unreachable code is not a capability, and the
git history holds it); and answering every letter `Undetermined`
(that is a claim about a guest wearing a refusal's clothes, and it is
the reading this entry specifically refuses).

**What this withdraws is a pledge, not an argument.** Proposed **U13**
stands, and the owner kept it there deliberately: it is the Windows form
of the case for reading a *persisted* mapping — a hive that records the
assignment outright, which is a different claim from the derivation
struck here and was never the reason for striking it. `proposed/` is
where a live argument belongs, and one contradicting the amended vision
is exactly what that shelf is for. U15 is its pair and is unchanged.

**Reopens if:** a use case arrives that cannot be served by the volume
identity plus the consumer's own mapping — the case to make is that the
correspondence is unavailable to the caller, not merely inconvenient for
it. U13 is where that argument would be won or lost; pledging it would
put back a machine-namespace seam, so it takes the surface-change rule's
hard route rather than following from this entry.

### D56 — Only the core is a default member, because the audience stopped being contributors alone

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-14. **Supports** S1, S2, S3; P3. **Reverses D52** and annotates
it. `DECISIONS.md` was searched first and returned D52, which set what
this reverses, and D44, D45, D49, D50 and D51, which are the sequence
D52 completed; D49's claim is corrected here as well.

**D52 was right about its audience and that audience has widened.** Its
acceptance rested on one answer given directly (Paul, 2026-08-13):
*anyone contributing is expected to run the tests*. A contributor needs
whatever the suite needs, so a toolchain the build alone could have
avoided bought them nothing. That is still true, and nothing below
disputes it. What has changed is that a contributor is no longer the
only person who builds this tree. `remanence` and `remanence-ffi` go to
crates.io as source; a distribution packager fetches the tarball and
builds it, and so does anyone who wants to read or use the Rust core
without touching the C ABI or the Python module. Asking that person for
CMake, a C++ compiler, a Python interpreter, uv and mypy is asking for
four toolchains to compile one dependency-free library.

**D52 weighed this option and declined it, on a ground that no longer
holds.** It declined "documenting `cargo test --workspace` as the
run-everything command and changing nothing", because such flags do not
get typed. The flag now has somewhere to be typed that a person cannot
skip and still claim to have run the checks: `cargo build --workspace`
and `cargo test --workspace` are two of the six *required checks* in
AGENTS.md, and CONTRIBUTING.md says the `--workspace` pair is what a
contributor runs. D52's objection was to a flag documented in prose as
an option; this is a flag that is the obligation.

**The asymmetry D52 removed stays removed.** Its complaint was that
`remanence-ffi` sat in `default-members` and `remanence-py` did not, so
S2's checks ran for everyone and S3's for nobody. That is not repaired
by restoring the old list. It is repaired by naming neither: both
surfaces are reached the same way, by the same command, and neither is
privileged over the other.

**What it costs, stated rather than hidden.** A bare `cargo build` no
longer regenerates `crates/remanence-ffi/include/remanence.h`, the
build script that writes it running only when its own crate is built —
so the workspace build is what carries that, and the required checks say
so.
[Superseded by D68 and D69 — a bare `cargo build` regenerates it again,
`remanence-ffi` having rejoined `default-members` once its checks needed
nothing beyond rustc; and the path itself is now
`crates/remanence-ffi/c/include/remanence.h`.]
And the risk D52 named is real and unchanged: a check reached only
by a flag is a check that stops being run. Nothing here disproves that;
the answer is the obligation above, and if it turns out flags are not
typed even when required, this entry is the wrong one rather than the
requirement.

**D49's claim is corrected in the same change**, having been false when
made and false for the same kind of reason. It held that a fresh clone
is testable immediately; `media_sources` and `sevenzip_catalog` called
the fixture helper while declaring no feature, and seven tests inside
`crates/remanence/src/` did the same, so a clone met a panic on eight
suites. All are declared now, and `ensure_fixture` is itself gated on
the features that declare a fixture is wanted, so a target that reaches
for one without saying so fails to compile in the default run rather
than failing on whichever machine has not downloaded. The declaration
can no longer drift from the fact silently, which is the part worth
keeping.

**The fixture feature splits in two while the tiers are being drawn.**
`fixtures` names what the project acquires from elsewhere against pinned
SHA-256s; `rigs` names the one artifact it generates,
`freedos-parttest.qcow2`, which reliquary produces by booting a machine
and installing FreeDOS. The costs differ in kind — a network round trip
against an emulator, a Python toolchain and an operating-system install
— and the gap widens with the system installed, so holding the
downloads does not oblige owning the rig toolchain.

**Weighed and declined:** leaving `default-members` alone and telling
the core-only reader to type `-p remanence` (it works today, and it
makes the narrow run the thing you must know to ask for, when it is the
one more people want); and gating the bindings' heavy test targets
behind features instead (it hides the requirement inside the crates that
own it, where `default-members` states it in one place a reader of the
workspace meets first).

**No changelog entry.** How the suite is invoked is not release-facing.

### D54 — The C++ header covers the whole ABI, so an unwrapped function is a defect rather than a boundary

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S2, P5, P10; amends D53, and stands on D47 and
D49. `DECISIONS.md` was searched first and returned D53, which drew the
boundary this removes and stated the reasoning for it.

**The instruction is the argument, and D53's reasoning was not wrong.**
D53 scoped the C++ presentation to the storage model and left the 136
flux functions to the C header, on the grounds that they are no
storage-model node and carry their own presentation ladder. The owner
directed the rest wrapped, the same day, before that entry had aged an
hour. Nothing about the earlier reasoning was found faulty; what changed
is that partial coverage was not what was wanted, and the owner is the
authority on that.

**What it buys is a rule with a yes-or-no answer.** D53's boundary had
to be described — which families are in, which are out, and why — and
every future ABI addition would have had to be triaged against that
description. Full coverage replaces it with something a script can
check: **every `remanence_*` function the header declares is wrapped, so
one that is not is a defect.** The header says so in those words, and
the count is 470 of 470.

**No F-number is issued, and that is the ruling this entry exists for.**
D53 said wrapping the flux layer "would need a fresh F-number", which
was right about a *pledge*: a feature is capability argued and owed
before it is built. This was neither argued nor owed — it was directed
and delivered in one motion, and [TASKS.md](TASKS.md) already states the
principle for that case ("work that arrives already done never appears
here: there is nothing to schedule, only a decision to make"). Issuing a
number to retire it in the same commit would be ceremony. The commit is
the record, and this entry is the decision it points at.

**The ABI's own records are aliased, not restated.** A half-track, a
bitstream location, a sector claim, an orbit and a hole are
`#[repr(C)]` plain numbers the ABI copies into an out-parameter — no
strings, no ownership, nothing for a wrapper to own or free. So the
header spells them `using SectorClaim = RemanenceSectorClaim;` rather
than declaring ten C++ structs beside ten C ones, which would add a
conversion, a maintenance burden, and a place for the two to disagree.
The copying rule D53 set is about *strings a handle owns*, and these
carry none.

**Six handle types answer the same three shapes**, so the shapes are
written once: a declared-loss account, an evidence list, and a list of
records copied into an out-parameter. Three small function templates in
`detail` take the ABI functions as arguments, and each class's accessor
is a line. `DeclaredLoss` is the one new struct — a code, a detail and
an amount — because every rung of the ladder and every rendition off it
accounts for what it did not carry.

**The ladder is tested without a fixture, which the flux layer has never
managed before.** The remanence format's own worked example is
twenty-one bytes — one index hole at 3/8 of a turn, one orbit at
57,150 µm, two points — and the artifact around it is a magic string, a
sentinel, a version byte and one stored DEFLATE block in a zlib stream.
The C++ caller lays that on disk itself and opens it, so the image, its
shape, its round trip, the write refusal on an occupied destination, the
bitstream, the bytestream and all three renditions are checked on a
fresh clone. **Two points frame no record, and the sector layer's
refusal is a check rather than a gap**: nothing is manufactured to stand
in for a recording.

**One test needs a real capture, and it is gated as the core's are.**
The sector layer, its claims, the BAM and the CBM DOS catalog above it
need a recording that frames records, so `cpp_flux.rs` walks the
KryoFlux capture the prep script fetches, behind a `fixtures` feature on
`remanence-ffi` that mirrors the core crate's (D49). It takes about two
and a quarter minutes, which is the gap-first reduction over
eighty-four step positions rather than the boundary being slow. The
default run is untouched.

**D53's dangling guard caught its own author, which is the evidence for
it.** Writing the capture group, `catalog.entries().entries()` was typed
without a second thought and did not compile — the deleted rvalue
overload doing exactly the job it was added for, on a line a reviewer
would have read straight past. The leak probe gained a flux cycle for
the same reason: the ladder's rungs each own private session storage,
and a C++ caller writes no free for any of them.

**Weighed and declined:** leaving the boundary where D53 put it and
recording the owner's instruction as a pledge to be worked later (the
work is mechanical, it was directed, and a pledge nobody means is what
`planning/README.md` says to withdraw rather than write); declaring C++
structs for the ABI's plain records (above); and gating the whole flux
group behind `fixtures` (it would have left the flux half of the header
unchecked on a fresh clone, which is the property the worked example
exists to avoid).

### D53 — The C++ presentation is a hand-maintained header that copies its strings, and it wraps the storage model rather than everything

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S2, P5, P10; D44, D45, D46, D47; delivers
pledged F45, whose number retires with it. `DECISIONS.md` was searched
first and returned D46, which anticipated a C++ framework here, and D47,
which named this presentation as the thing its leak probe would serve.

**No surface is issued and none is amended.** `include/remanence.hpp` is
a derived representation of the C ABI exactly as `include/remanence.h`
is a derived representation of the Rust `extern "C"` items: it is
header-only, it links nothing of its own, and every line is a call a
caller could have made. P5's three presentations stand, S2 stays the
norm, and the header moves with S2 in the same change. What follows is
the four rulings F45 left open and the one place its own shape sentence
could not survive contact with today's ABI.

**Refusals are exceptions, and only exceptions.** F45 left open whether
they present as exceptions, an `expected`-style result, or both. The
CMake project compiles at C++17 (D46), which has no `std::expected`, so
"both" means writing a result type as well as the wrapper and doubling
the surface a reader has to learn — for a library whose refusals are
genuinely exceptional and whose C door is still there for a caller who
wants a status code. `remanence::Error` derives `std::runtime_error` and
carries the delivered category and the rule identity beside the
diagnostic (P10), so a caller with one handler at the bottom of `main`
gets the classification without parsing text.

**The header is hand-maintained, not generated, and compiling it is what
catches that.** cbindgen generates the C header from the Rust; a second
generator over the *generated* header would be a tool that reads a file
another tool owns, and would have to be taught the ABI's conventions —
the three out-parameters, who frees what, which nulls are answers — none
of which is expressed in the C declarations it would parse. Those
conventions are exactly the knowledge a hand-written wrapper carries.
The cost is real and is paid where it can be seen: unlike the C header,
this one *can* fall behind the Rust, so `cargo test` compiles it
standalone and compiles the C++ example against it, and the surface
tests below run a C++ caller through it.

**Ownership follows the ABI's own division, which is where F45's shape
sentence bends.** F45 says "one move-only RAII class per node kind ...
each owning its handle's lifetime through the ABI's free functions". For
every handle the ABI hands the caller to free — discovery, partition,
space, file, report, assurance, geometry, listing — that is exactly what
was built. But `Machine`, `StorageDevice` and `Medium` are documented by
the ABI as "the session owns this; never free it", and there is no free
function for a destructor to call: a move-only class over them would
invent an ownership the ABI does not have and would make a copy an
error for no reason. They are copyable views instead. F45 was written
before the media-first model landed, and the sentence describes a shape
S2 no longer has; the ruling is that the wrapper follows S2 (which F45
also says: "wraps whatever S2 is when it lands").

**Every accessor on a handle copies its string, and that was not the
first draft.** The draft returned `std::string_view` into the handle's
own memory — faithful to the ABI, zero-copy, and documented as
borrowed. The first example written against it printed
`blank.assurance().evidence()` as garbage, because the temporary
assurance died at the end of the full-expression and the views outlived
it by one line. **That is not a bug a caller can be warned about**: the
temporary-handle expression is precisely what RAII invites, and a
wrapper whose ergonomics produce a dangling read has spent its safety on
performance nobody asked for in a library that reads disks. Handles now
answer `std::string`; the catalogs still answer `std::string_view`,
their strings being static for the life of the release, and `get()`
hands over the raw handle where a caller wants the pointer.

**The one dangle C++ *can* refuse is refused, and the refusal is
checked.** Copying strings closes the common case, but the records
handed out of a listing — a `Layer`, an `Entry`, a `ReportRegion` —
borrow the handle they came from, and `medium.identify().layers()` would
walk views of a handle that died at the semicolon. Every accessor that
answers such a record is deleted on an rvalue, so that line does not
compile and the same line over a named handle does. A refusal is worth
what its check is worth, so the CMake project compiles both forms:
the dangling one must fail and the bound one must succeed. **The
control is not decoration** — the first version of this check compiled
an *executable*, which failed to link for want of the library and
reported the refusal as working; the bound form failing is what
exposed that.

> **Annotated by D54**, which wraps the flux layer too, on the owner's
> direction the same day. The paragraph below stands as the reasoning
> for the boundary it drew; the boundary itself is gone, and the
> wrapper now covers every `remanence_*` function.

**It wraps the storage model, and says so rather than trailing off.**
Every node the storage model has is here — session, machines, devices,
media, discoveries, partitions, volumes, filesystems, files — with the
records they hand back and the inspection report: 334 of the ABI's 470
functions at the time. *[D57 removed the drive-letter composition from
both, so the counts are historical; the rule they were counted to
establish — the header covers the whole ABI, and an unwrapped function
is a defect — is unchanged.]* The 136 left are the flux
presentations (`remanence_flux_*`, `remanence_c1541_*`, `remanence_p64_*`,
`remanence_g64_*`, `remanence_d64_*` and the two medium doors onto them),
which are not storage-model nodes, carry their own presentation ladder,
and are reachable unchanged through `<remanence.h>`, which this header
includes. F45 names the storage model's node kinds and claims no reach
the C ABI lacks, so this is the feature's scope rather than a shortfall
against it — but a later pledge could wrap the flux layer, and it would
need a fresh F-number.

**GoogleTest is declined, which revisits D46 only to record why.** D46
made it a drop-in and expected it here. `FetchContent_Declare` downloads
at configure time, and `cargo test` configures this project on every
run: adopting it would make the default test run need the network, which
is the property D49 had just finished removing ("a fresh clone is
testable immediately"), and would turn an outage into a test failure.
The C tests' own shape — a self-contained program taking a group name,
one named Rust test per group — needs no dependency and reports failures
the same way, so `tests/c/wrapper.cpp` follows it. D46's remark stands as
a statement of availability, not an instruction.

**The `_free` discipline is asserted for the caller who writes no
frees**, which is what D47 predicted this feature would need:
`tests/c/wrapper_leaks.cpp` links the probe build and cycles a session,
its records, a refusal's message and rule, and a handle released back to
C. **Verified by leaking on purpose**, as D47 was: dropping a
`release()`d geometry on the floor reported *5 blocks per cycle over 8
cycles* and named the cycle it was in, while the other two stayed clean.
RAII is where a missing free is least visible — there is no call site to
inspect — so this is the check that makes the header's central claim
falsifiable.

**Name and path, the last thing F45 left open:**
`crates/remanence-ffi/include/remanence.hpp`, beside the C header and
installed from the same directory. `.hpp` rather than `.h` because both
files sit in one directory and the extension is the only thing that
tells a reader which is which. The example is
`examples/identify.cpp` beside `examples/identify.c`.
[Superseded on path by D69 — both directories moved to
`crates/remanence-ffi/c/{include,examples}/`, still beside each other
exactly as this entry describes.]

**Weighed and declined:** wrapping every ABI function including the flux
ladder (it doubles a hand-maintained surface for a layer whose own
presentations are a separate concern, and F45 asks for the storage
model); returning `std::string_view` everywhere with the borrow
documented (above — the documentation was written, and the first
consumer dangled its own output anyway); a `std::expected`-shaped result
type alongside exceptions (C++17 has none, so it would be ours to write
and ours to keep); and generating the header from cbindgen's output
(above).

### D52 — Every member is a default member, because two surfaces were being treated differently for no reason a caller could see

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S1, S2, S3; D44, D51. `DECISIONS.md` was
searched first and returned D44, which recorded the asymmetry this
removes and defended it at the time.

> **Reversed by D56**, which takes both bindings out of
> `default-members` rather than putting one back. The reasoning below
> stands as written and its answer about contributors is still true;
> what it did not weigh is a reader who is not a contributor — a
> distribution packager building the published source, or anyone
> wanting the Rust core alone — for whom the cost this entry accepts
> buys nothing at all.

D51 put the Python suite under `cargo test -p remanence-py`, and the
question that followed was whether it therefore runs "when we run all
tests". It did not. `cargo test` ran the C tests — CMake, MSVC, a
compiled and executed C caller — and none of S3's, because
`remanence-ffi` was a default member and `remanence-py` was not.

**The stated reason was real and is still true**: a C compiler is needed
to *test*, where PyO3 needs a Python interpreter to *build*, so the
Python crate cost more to include. D44 drew the line there. What it did
not weigh is that `default-members` governs building and testing
together, so protecting the build from a Python requirement also
withheld S3's checks from every default run — and a check reached only
by a flag is one that stops being run, which is the objection D42, D43,
D50 and D51 each answered in turn.

**The cost is paid rather than hidden.** `cargo build` and `cargo test`
now need a Python interpreter, on any machine, including one whose owner
is touching only the Rust core. The comment in the workspace
`Cargo.toml` that recorded the old property is replaced by one recording
this, so the next reader meets the decision rather than its residue.

**Who that cost falls on is the reason it is acceptable** (Paul,
2026-08-13, asked directly): *anyone contributing is expected to run the
tests*. A contributor who must run the suite already needs whatever the
suite needs, so a toolchain the *build* alone could have avoided buys
them nothing — it only postpones the requirement to the moment they run
`cargo test`, having already been told to. CONTRIBUTING.md states
`cargo test # the full suite` as the expectation, and after this entry
that line is true rather than approximate.

**What it buys is one command.** `cargo build && cargo test` covers all
three application surfaces: 27 binaries and 600 tests, against 23 and
584 before. No surface's checks are behind a flag, and "run all tests"
means what it says.

**Weighed and declined:** documenting `cargo test --workspace` as the
run-everything command and changing nothing (it works, and it leaves two
surfaces treated differently behind a flag people forget — the shape of
this whole sequence of entries is that such flags do not get typed); and
gating the C tests to match, so a plain run is toolchain-free for both
(consistent in the other direction, and it reverses D44, D45 and D50,
which spent real effort getting those checks to run by default).

**No changelog entry.** How the suite is invoked is not release-facing.

### D51 — The Python suite runs under `cargo test`, staged from the build rather than installed from a wheel

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S3; D48, D50. `DECISIONS.md` was searched first
and returned D48, which wrote the suite and left running it to a person.

D48 delivered a pytest suite and three commands to run it: build a
wheel, install it, run pytest. That is the shape of a check that stops
being run — the objection D42 and D43 answered for the stub and D50 for
the leak probe, arriving a fourth time. `cargo test -p remanence-py` now
runs it.
[Superseded by D65 — `cargo test -p remanence-py` no longer runs it at
all; `just test-py` does, and needs no Rust `#[test]` to exist.]

**A wheel was never the requirement; the layout was.** A wheel is a
`remanence/` package holding the compiled module beside `__init__.py`,
the stub and `py.typed` — and that can be staged from what `cargo build`
already produced, the debug cdylib renamed as the extension plus three
files copied from `python/remanence/`. `PYTHONPATH` finds it and pytest
imports it, so the release compile a wheel needs never happens. This is
the same shape as D50's finding one entry earlier: the artifact a check
needs is not always the artifact that ships.

**What it does not prove is stated rather than left implied.** This
exercises a *debug* build staged by hand, not a release wheel installed
by a packaging tool. `uv build` remains what proves the artifact, and
D48's run from an unpacked sdist remains what proves the suite travels.
The point of this entry is that the suite runs at all, every time,
rather than that it now proves more.

**It stays outside the default `cargo test`, deliberately.**
`remanence-py` is not a workspace default member because pyo3 needs a
Python toolchain to *build*, which the workspace `Cargo.toml` records in
a comment; nothing here changes that, and a plain `cargo test` still
builds none of it. The command this joins is the one AGENTS.md already
names for a Python-surface change.

**A nested cargo build is declined here, where D50 adopted one.** D50
could build a *different* crate's cdylib into a separate target
directory; this would be the crate the test is running from, so the
build would contend for the lock the run already holds however the
target directory is set. The test requires `cargo build -p remanence-py`
first and says so — the same contract the C tests have.

**pytest is found the way mypy is** — `python -m pytest`, then `pytest`,
then `uv run --with pytest` — and its absence fails rather than skips.
The two finders are now one generic helper in `tests/common/mod.rs`
rather than two implementations of the same policy.
[Superseded on the finding method by D63 — there is no search now,
the build having already settled what can import it. The
fail-rather-than-skip policy stands, and the one-helper claim became
true only at D63.]

**Verified by breaking a Python assertion**, which failed the cargo test
with pytest's own output naming the line — so the two are genuinely
wired together rather than merely both green.

### D50 — The leak check runs by default, the probe having never needed to be in the shipped library

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S2; D45, D47. `DECISIONS.md` was searched first
and returned **D47, which made this check opt-in** and named that as its
own weak point. This reverses it.

**D47's constraint was real and is not weakened here.** The probe is a
global allocator and an exported symbol, and S2 is defined as *every*
`remanence_*` symbol `crates/remanence-ffi` exports — so shipping it
would be a surface change bought with a test's convenience. That still
holds; the shipped cdylib is untouched, and `dumpbin /exports` over
`target/debug/remanence_ffi.dll` finds no probe symbol while the probe
build's does.

**What was wrong was the inference, not the constraint.** D47 reasoned
from "the probe must not ship" to "the probe must be opt-in", and the
step between them does not follow. The probe has to be in a library a C
caller can *link*, which is not the same library that ships. The harness
builds one with `--features leak-probe` into `target/leak-probe`, and
the leak binary links that.

**Cargo locks a target directory, not a workspace**, which is what makes
this work at all. D45 declined to run a nested `cargo build` because it
"would contend for the lock the current run already holds" — true of the
same target directory, and false of a different one. That was the second
premise worth testing rather than assuming. Cold, the extra build costs
about twenty seconds; warm, a fifth of a second, which is what makes
running it by default affordable rather than merely possible.

**Both builds carry the same file name, and Windows loads the copy
beside the executable**, so the probe binary gets its own output
directory and its own copy of the library. Without that it would load
the shipped one and fail to find the symbol — a link that succeeds and a
run that does not, which is the confusing shape of failure.

**Verified in both directions.** `remanence_string_free` was made to
`forget` rather than drop, and the check reported *8 blocks over 8
cycles, 1.00 per cycle* on the refusal path while the discovery path
stayed clean; restored, it passes. And the shipped DLL was inspected
directly rather than reasoned about.

**What this closes.** D47 recorded that with the feature off its binary
reported "0 tests", which reads like nothing to check, and called that a
real departure from the fail-rather-than-skip rule D44 and D45 follow.
There is no longer a feature to leave off: the rule holds across every C
test again, and the check that was most likely to quietly stop being run
now runs whenever `cargo test` does.

**Weighed and declined:** making `leak-probe` a default cargo feature
and disabling it at release (one forgotten flag ships an undocumented
symbol, and a surface change by omission is exactly what a governed
surface exists to prevent); naming the symbol outside the `remanence_*`
prefix so it falls outside S2's letter (it would still be an exported
symbol nobody documented, which is the spirit); and a second cdylib that
re-exports the ABI plus the probe (the C tests would then exercise a
library that is not the one shipped).

### D49 — A fresh clone can run the suite, and the rig layout is built rather than downloaded

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S1; D48. `DECISIONS.md` was searched first and
returned D48, whose fixture-free reasoning this extends from the Python
suite to the Rust one.
[Superseded on path by D71 — `freedos_qcow2`, `flux_media`, `hdos_files`,
`identify_hdos_image`, `geometry_fixtures` and the rest of the
`required-features` suites named below moved from `crates/remanence/
tests/` to `integration-tests/rust/tests/`. The `fixtures`/`rigs` split
and the reasoning for it stand exactly as this entry states them —
`rigs` now lives on the crate the moved suite carries it to instead of
on `crates/remanence`, `fixtures` on both.]

D48 found that the shippable Python suite could open no artifact, and
that `new_media` and hand-built structures were the way past it. The same
question turned out to apply to the Rust suite for a different reason: a
fresh clone could not run `cargo test` at all until
`prep_fixtures.py` had downloaded artifacts it needs network and a
`reliquary` alpha to fetch.

**The scale of the problem was smaller than it looked, and the first
count of it was wrong.** Grepping for `fixtures` matched seven files —
five of which say in their own doc comments that they *build their images
by hand and run without fixtures*. The real test is who calls
`ensure_fixture`, and that was six files: four wholly dependent
(`freedos_qcow2`, `flux_media`, `hdos_files`, `identify_hdos_image`) and
two barely so — the drive-letter suite at 6 tests of 15, `geometry` at 2
of 10. Recorded because the wrong number nearly bought a much larger change
than the right one needed.

**The split is cargo's own mechanism.** A `fixtures` feature, and
`required-features` on the dependent targets: cargo does not build a
target whose features are off, so `cargo test` runs everything that
builds its own images and reports nothing misleading, while
`cargo test --features fixtures` runs the rest. `ensure_fixture` already
panicked naming `prep_fixtures.py`, so the guidance was never missing —
it simply fired for the whole suite instead of the part that needed it.

**The two mixed files were split rather than gated**, which kept 17
synthetic tests in the default run that whole-file gating would have
hidden. That cost more than it should have: the section comment marking
the fixture-dependent half of the drive-letter suite did not match the
real boundary — tests below it still used the image builders above it —
and the resolution was to move the whole helper layer into a shared
module. The structure is better than before; it was
reached by trial rather than by reading first.

**And then the rig layout stopped needing a download at all.** The
FreeDOS artifact was there for one shape — two DOS primaries and an
extended chain of two logicals, a richer table than any
single-partition image puts in front of the partition and volume seams —
and that shape is wholly specified. It is now
built: `fat16_volume` writes a labelled FAT16 with files in its root, and
`synthetic_rig_disk` lays out the four volumes with a real EBR chain
(`tests/rig_disk/mod.rs`). All
six tests that needed the qcow2 ran without it.

**The built layout is asserted before anything trusts it.**
`rig_layout.rs` exists so a synthetic fixture that quietly differs from
the artifact it replaces cannot move every test that depends on it onto a
false footing: it checks the two primaries and two logicals appear, that
four volumes compose and read as FAT, that each states the label it was
built with, and that the marker file reads back through the first
primary.

**A detail worth keeping**, because it is the classic way to build a
chain nothing can walk: in an EBR, a **data** entry's start is relative
to its own EBR, while a **link** entry's is relative to the extended
partition's base. The builder says so where it does it.

**The Python suite gained the same thing first, and for the harder
reason.** D48's tests reach what an authored blank can answer and stop
where a *recorded structure* begins, because a partition table and a
filesystem are things found on media. `tests/synthetic.py` supplies
both — an MBR and a FAT12 volume written byte by byte — and the shipped
suite loads the result as a raw image. That reaches the two doors
authorship cannot, and it does so in an artifact a stranger unpacks,
where downloading is not merely inconvenient but forbidden: a distro
packager builds offline. It also buys the thing a downloaded image
cannot give at any price — bending one field and watching the refusal
arrive, which is how the suite now covers a table with no signature and
an entry typed outside the claim.

**What stays behind the feature, and why it should.** `flux_media` needs
a KryoFlux capture, where a real capture *is* the point; `hdos_files` and
`identify_hdos_image` read an authentic HDOS filesystem; `freedos_qcow2`
tests a qcow2 an operating system actually wrote; `geometry_fixtures`
needs a format that declares its own geometry, and that format nested in
an archive. The remaining fixtures are the ones whose authenticity is the
thing under test — which is the right place for that line to fall.

**Weighed and declined:** gating the drive-letter suite and `geometry`
whole (simpler, and it hid 17 tests that needed nothing); `#[ignore]`
instead of a feature (`cargo test` prints "ignored" and nobody reads it);
and synthesising the remaining four (two are not realistically
synthesisable, and for the other two a built artifact would test the
builder rather than the reading).

**No changelog entry.** Test organisation is not release-facing.

### D48 — The sdist carries a pytest suite, and the fixtures it cannot carry decide its shape

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S3; D40. `DECISIONS.md` was searched first and
returned nothing on the Python suite — S3 had no tests of its own at
all, which is the gap this closes as much as the packaging one.

The `release-artifact-contents` standing skill was amended on 2026-08-13
so that **the sdist carries the whole suite** while the wheel carries
none of it, the sdist being conventionally the artifact a stranger can
build *and verify* from. `888c5ba` had excluded tests from both under
the previous reading. This entry brings S3 into line — and finds that
doing so is not a packaging edit but a suite that had to be written.

**pytest, because a packager runs pytest.** S3's only tests were Rust
integration tests (D42, D43) driven by `cargo test`. A distro packager
building the Python sdist has no cargo workspace and would not use it if
they had; the suite that ships has to be Python, run against the
**installed module**. That is also the stronger reading of the same
question: `test_typed_surface.py` compares the stub against what
`import remanence` actually provides, where `stub_matches_module.rs` can
only parse the crate's source.

**The fixtures cannot ship, and that decides what the suite tests.**
Every disk image this project tests against is downloaded by
`test-fixture-prep` and **tracked by git not at all**; they are vintage
third-party distribution media whose copyright status is an open
question in this very record. So the shippable suite has no artifact to
open — and `new_media` is the answer, authorship being the one fact
class that creates a medium whole from what the author states. The suite
makes its own media: coordinates, sector round-trips through a commit,
the direct partition a scheme-less medium bears, the assurance whose
claim is `authored`. What it cannot reach — a real filesystem, a
partition table, the flux ladder — is reached by the Rust suite, which
has the fixtures.

**What stays out of the sdist is the part that tests the repository.**
The Rust integration tests read the crate's own sources and need the
workspace; the mypy fixtures exist to drive them. Both are excluded by
`crates/remanence-py/Cargo.toml`, which is what maturin builds the sdist
from. The core crate keeps `exclude = ["tests/**"]` for the same reason
rather than the old one: those are a Rust crate's tests, they need
fixtures that do not exist in any artifact, and a Python packager can do
nothing with them.

**Verified by unpacking.** The suite was run from the extracted sdist,
outside the repository, and passes — which is the coupling rule the
amended skill states, that a suite in the sdist must be runnable from
the sdist. The check made was self-containment against an installed
wheel rather than a full build from the sdist, which would compile the
Rust; the tests reference no repository path and no fixture, which is
what that rule is protecting.

**Two findings the suite produced on its first run**, recorded because
they are the argument for having written it: `chs-disk` refuses a
partial geometry — every part stated or the declaration addresses
nothing — which no test had covered and which is now
`test_a_partial_geometry_is_refused_because_it_is_whole_or_nothing`; and
a test directory named `typing/` shadows the standard library once
pytest puts its parent on `sys.path`, so the mypy fixtures moved to
`tests/mypy_fixtures/`.

**Weighed and declined:** shipping the mypy check in the suite (it needs
mypy present, which a packager need not have, and it tests the stub
against the source tree rather than the built package); porting the Rust
stub tests to pytest and dropping them (they run without a built module,
which is what makes them useful during development, and D42's reasons
stand); and shipping fixtures so the suite could open a real artifact
(they are not the project's to distribute, which is not a packaging
preference but the open question this record already carries).

### D47 — The `_free` discipline is asserted by counting, because nothing outside the library can see these allocations

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S2; D45, D46; pledged F45. `DECISIONS.md` was
searched first and returned D45, which named leak detection as the thing
a framework would have been for and left it open.

**No leak checker outside this library can see the allocations that
matter.** Everything the ABI hands out is Rust's, made inside the
cdylib — `CString::into_raw` for strings, `Box::into_raw` for handles —
and reclaimed by Rust when the matching `remanence_*_free` runs.
CppUTest's leak detector, and the sanitizers, instrument the *test
binary's* allocator, which these allocations never touch. That is a
statement about where memory comes from, not a preference, and it is
what settles the framework question D45 left open: **CppUTest would have
reported a clean bill of health however badly `_free` leaked.**

**The standard tools do not reach here either, and the reason is the
platform.** Miri is the standard Rust answer and would catch a leaked
`Box::into_raw` — but it is an interpreter and cannot execute a C
caller, so it checks the Rust side only. LeakSanitizer is the standard
answer for a mixed process and *would* see Rust's allocator, but it is
unsupported on Windows, where this project is developed; ASan there
ships without it. Valgrind is Linux and macOS. So the counting allocator
is bespoke because the standard options are absent, not because they
were judged worse. Miri over the Rust-side FFI tests stays worth adding
and is not this entry.

**It counts blocks, not bytes**, because the question is whether every
allocation was given back and a block is what a `_free` returns. The
first cycle is a warm-up whose count is discarded: a library settles
lazily-initialised state on first use, and that is allocation which is
never freed and never should be. A leak is the count rising *per cycle*
after that, which the C caller reports as a rate.

> **Annotated by D50**, which makes this check run by default. The
> constraint below stands — the probe still never ships — but the
> inference from it to "opt-in" did not follow: the probe must be in a
> library a C caller can *link*, which is not the library that ships.

**Opt-in, and the cost of that is stated rather than glossed.** The
probe is a global allocator and an exported symbol; carrying it in a
released artifact would add a `remanence_*` symbol, which is an S2
change. So it lives behind the `leak-probe` feature, is excluded from
the generated header by cbindgen configuration, and the C caller
declares the symbol itself. **This is a real departure from the
fail-rather-than-skip rule** D44 and D45 follow: with the feature off,
the test binary reports "0 tests", which reads like nothing to check.
The rule is kept where it can be — an absent CMake or compiler still
fails — and given up here because the alternative is shipping the probe.
AGENTS.md carries the command so it is a step someone runs.

**Verified by leaking on purpose.** `remanence_string_free` was made to
`forget` rather than drop, and the probe reported *8 blocks over 8
cycles, 1.00 per cycle*, on the refusal path while the discovery path
stayed clean — so it both detects and localises. Sound, it reports zero
live blocks either side of both.

**A cache bug this found, worth recording because it fails
confusingly:** CMake caches what it was last told, so a build directory
configured with the probe on kept trying to link the probe target when
the next run had the feature off — an unresolved symbol failing every C
test for a reason none of them is about. The flag is now stated either
way rather than omitted.

**This serves F45 more than it serves S2 today.** The pledged C++
presentation is a set of move-only RAII classes "each owning its
handle's lifetime through the ABI's free functions" — which is a claim
about exactly what this measures, and which no C++ test framework,
GoogleTest included, has any way to check.

### D46 — CMake drives the C tests, because MSVC's environment is the thing worth outsourcing

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S2; D44, D45; pledged F45. `DECISIONS.md` was
searched first and returned D44 and D45, whose toolchain machinery this
replaces, and D45's declining of CppUTest, which this revisits only far
enough to record that the framework question has moved.

**CMake is adopted for MSVC, and that is the whole argument.** Compiler
discovery in the abstract was never the problem — D44's hand-rolled
search worked. The problem is that `cl.exe` needs the environment
`vcvars64.bat` sets, and locating and sourcing that from a test harness
is *more* bespoke machinery than the gcc search it would replace. CMake
does it, and finds MSVC unaided: configured bare on the development
host it selects the Visual Studio generator and MSVC without being
told. Everything D44 carried goes with it — `MSYS_HOME`, the toolchain
directories, putting the compiler's own directory on `PATH`, and the
rule against ever trying a bare `cl` because it resolves to Watcom's.

**MSVC is the native match, and was tested before being adopted rather
than after.** The cdylib is built by the `x86_64-pc-windows-msvc`
toolchain, so linking a C caller with MSVC links against the import
library the same toolchain produced. All four things compile clean at
`/W4`: the boundary caller, `identify.c`, the self-contained header, and
the header as C++ — and the boundary caller links and passes its 44
checks.

**What is given up is a second compiler's opinion.** MinGW's gcc caught
nothing MSVC does not, on today's sources, but two compilers reading one
header is genuinely more coverage than one. `REMANENCE_CC` /
`REMANENCE_CXX` still override CMake's choice and
`REMANENCE_CMAKE_GENERATOR` the generator, so gcc remains one variable
away; it is no longer the default, and no longer discovered.
[Superseded by D66 — gcc is no longer one variable away for a MinGW
build specifically: that case is refused outright, not matched. The
overrides stay, for an unrelated reason (e.g. `-G Ninja` with `clang-cl`).]

**It is not shorter, and the entry says so.** The shared module is 209
lines against 224. The gain is in what those lines *do* — configure and
build, rather than know where MSYS2 installs, which compiler names to
try, which one to distrust, and which directory a toolchain needs on
`PATH` to load its own runtime.

**GoogleTest becomes a drop-in, deliberately.** F45 pledges an idiomatic
C++ presentation whose deliverable is "a header, its tests, and a C++
example consumer" — move-only RAII types with typed errors, which is
what a C++ framework is for and what CppUTest, an embedded-C framework,
is not. The CMake project declares `C CXX` and already compiles the
header as C++, so adding `FetchContent_Declare(googletest)` when F45
lands needs no new toolchain decision. **The C boundary tests stay C**
regardless: they exist to exercise the ABI as a *C* caller meets it, and
a C++ test of them would be a different claim.

**Weighed and declined:** keeping MinGW as the default and adding CMake
only for its build orchestration (it keeps every line of discovery D44
wrote *and* adds CMake, which is the worst of both); driving MSVC
directly by locating `vswhere` and sourcing `vcvars64.bat` (more
bespoke machinery than the gcc search, for the same result CMake gives);
and adopting GoogleTest now, before F45 exists, to save a later change
(the C tests would become C++ tests of a C surface, which is the one
thing D45 exists to avoid).

**No changelog entry.** Tests and their toolchain are not
release-facing.

### D45 — A C caller crosses the boundary in the test suite, and CppUTest is declined for now rather than on principle

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S2; D44. `DECISIONS.md` was searched first and
returned D44, which closed the *compiling* half of this gap and named
the rest of it.

D44 said compiling was the whole of what was worth checking, and that
was true **of the header and the example**. It left a different gap
standing, which this entry closes: nothing exercised the ABI as a C
caller meets it. The FFI crate's unit tests call the `extern "C"`
functions from Rust — no header is included, no C compiler sees a
declaration, no C calling convention is used, and a `#[repr(C)]` mistake
cannot show.

**`tests/c/abi_boundary.c` is a C program, compiled against the header,
linked against the built library, and run.** It takes a group name so
each group is a named test on the Rust side rather than one pass-or-fail
lump, and it keeps going after a failed check so one run reports
everything wrong with a group. Five groups: the catalogs (the cheapest
thing that can only work if `size_t` and `const char *` cross
correctly), the version and cache bound, a refusal's out-parameter
contract, null handling on accessors documented to answer rather than
dereference, and a real artifact discovered, read and released.

**It needs the built library, and the tests say so rather than
building it.** `cargo test` does not produce a cdylib; `cargo build`
does, and AGENTS.md already orders it first. A nested `cargo build` from
inside a running test would contend for the lock that run already holds,
so the tests refuse with the two commands to run instead of deadlocking
or silently passing.

**Null handling is checked from C because that is where a mistake is a
crash.** An accessor that dereferences a null handle takes the host
process down; asserting the contract from Rust asserts it about Rust.

**The caller is built once for the binary.** Five tests writing one
executable — while another thread is running it — is a file lock on
Windows, not merely wasted work. Found the honest way, by the tests
failing that way first.

**Weighed and declined — CppUTest, and not on the grounds first
given.** The initial objection was that it adds a dependency; that was
overweighted, since D44 had already made a C compiler a requirement and
a submodule is a understood mechanism. The reasons that survive are
narrower: CppUTest is C++ and builds under CMake, so it adds a *second*
build system to orchestrate from the test harness, with a first-run cost
and cache-invalidation logic that the current one-`gcc`-invocation
design does not have; and its distinctive offering beyond a runner —
leak detection over the `_free` discipline — is a question worth asking
on its own terms rather than as a side effect of choosing a framework.
The plain-C harness here is what that question should be measured
against once it exists, which it now does. This is a **not yet**, not a
**no**: if leak checking across ~470 functions is wanted, reopen it.

**Also declined:** mocking (CppUMock or otherwise — these tests exercise
the real library, and a mock of it would test the mock); and asserting
struct layout by hand-computed offsets (the compiler already agrees with
the header by construction, and a wrong `#[repr]` shows as a wrong
*value* in the groups above, which is the failure a caller would meet).

**No changelog entry.** Tests are not release-facing.

### D44 — The C surface compiles under `cargo test`, and compiling is the whole of what is worth checking

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S2; D42, D43. `DECISIONS.md` was searched first
and returned nothing on the C example, gcc, or the toolchain — this is
the first ruling there. It completes the sweep D42 and D43 began: the
Python surface's checks became tests, and the C one was still a step a
person had to remember.

> **Annotated by D45**, which links and runs a C caller after all. The
> ruling below stands as written — it is about the *header and the
> example*, where compiling is indeed the whole check. What D45 adds is
> a different thing this entry did not weigh: exercising the ABI as a C
> caller meets it.

**Compiling is the whole check, and dropping the link step drops the
built cdylib with it.** `include/remanence.h` is generated by cbindgen
from the `extern "C"` signatures, so it *cannot* declare a symbol the
library does not export — the failure linking would catch does not exist
here. What can drift is `examples/identify.c` calling something the
header no longer declares, and that is a compile error. So the tests
compile and do not link, which means they need no built library, no
`target/debug/remanence_ffi.dll`, and no build ordering between the
cdylib and the test.

**Three checks, and the third was an untested claim.** The header
compiles alone (self-contained); the example compiles against it with
`-Wall -Wextra -Werror`; and the header compiles as **C++**, which
`cbindgen.toml` has asserted with `cpp_compat = true` since it was
written and which nothing had ever run a compiler over.

**Discovery is ordered so that a surprising `PATH` cannot mislead it.**
`REMANENCE_CC` / `REMANENCE_CXX` override outright; otherwise
**on Windows** `$MSYS_HOME` (default `C:\msys64`) is searched under
`ucrt64\bin` then `mingw64\bin`, and only then `PATH`. MSYS2 is a
Windows thing, so it is compiled in only there — the names, the search
and the advice a failure gives alike, since a message telling a Linux
developer to install MSYS2 would be worse than no message. Elsewhere
`PATH` is the whole search. A bare `cl` is **never** tried: on
the development host it resolves to Watcom's `cl`, not MSVC's, and a
checker that silently compiled with the wrong compiler would be worse
than one that found none. An override that does not answer `--version`
fails rather than falling back, so a typo in it cannot quietly become a
different toolchain.

**The compiler's own directory goes on `PATH` for the run.** Without it
MSYS2's `g++` exits non-zero and prints **nothing whatever**, which
reads as a compile failure with no diagnostic. The tests put it there
and, if a compiler ever does exit silently, say that is the likely
cause rather than reporting an empty error.

**No compiler is a failure, not a skip**, which is D42's principle and
D43's applied a third time. The consequence is stated rather than
buried: `remanence-ffi` is a default workspace member, so **plain
`cargo test` now needs a C compiler**. That is judged acceptable for a
project whose second application surface *is* a C ABI, and it is a
weaker requirement than the Python one it sits beside — a C compiler is
needed to *test*, where pyo3 needs an interpreter to *build*, which is
why `remanence-py` is out of `default-members` and this is not.

> **Annotated by D52**, which puts `remanence-py` in `default-members`
> after all. The distinction drawn here is still true; what it did not
> weigh is that the same setting governs building and testing, so
> sparing the build a Python requirement also withheld S3's checks from
> every default run.
`REMANENCE_SKIP_CC=1` skips deliberately.
[The escape hatch is removed by D64; the failure stands.]

**Verified by injecting drift.** The example was made to call a name
D39 had renamed away; only the example's test failed, with the header
and C++ checks still passing, and the message says what the state
means — the header regenerates from the Rust, so the example is what
moved. The override, a deliberately wrong override, a wrong
`MSYS_HOME`, and the skip were each exercised too.

**Weighed and declined:** linking and running the example in the test
(it needs a built cdylib, a copy of it beside the executable, and a
fixture to run against — and it tests the library, not the surface
agreement this is about; AGENTS.md keeps it as the manual step it is);
gating the tests behind a cargo feature so plain `cargo test` stays
compiler-free (it makes the check need an unusual flag, which is how a
check stops being run); and `#[ignore]` for the same purpose (`cargo
test` prints "ignored", and nobody reads it).

**No changelog entry.** Tests are not release-facing.

### D43 — The type check joins the name check, and checking the stub *source* beats checking a built wheel

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S3; D40, D42. `DECISIONS.md` was searched first
and returned D42, which named this as the complement it was leaving
manual. Nothing declined it.

D42 automated the *name* check and said plainly what it could not
settle: whether a declared type is right or even usable. This entry
takes the other half. `mypy --strict` now runs in
`crates/remanence-py/tests/stub_typechecks.rs` over two fixtures kept in
the repository.

**Checking the stub source is better than checking a built wheel, not
merely cheaper.** D40 and D42 both assumed the type check needed an
installed module. It does not — mypy resolves `import remanence` to the
stub through `MYPYPATH` — and the source reading is *stronger*: mypy
reports errors **inside** a first-party stub, where a stub reached
through an installed package is followed silently. That is not
theoretical. Pointing mypy at the source immediately found two real
defects the wheel-based run had passed for days: `File.bytes` and
`LocationBytes.bytes` name members that shadow the builtin `bytes`
inside their own class bodies, breaking three return annotations. Both
are fixed here with an explicit `builtins.bytes`.

**The negative fixture is the one that matters.** `accepts.py` — ordinary
consumer code that must check clean — catches a type that is wrong or
unusable. It does **not** catch a stub that has stopped saying anything:
a parameter widened to `object`, a lost `py.typed`, a class the checker
no longer resolves all leave `accepts.py` passing. `rejects.py` is
misuse that must be refused, every line naming the mypy error code it
expects, and the test asserts both that each expected error appears and
that no line fails for a reason nobody asked for — so the fixture can
neither start passing silently nor start failing for the wrong reason.
Injected regressions confirm the split: a wrong declared type fails only
`accepts.py`; a widened parameter and a read-only property made writable
fail only `rejects.py`.

**It runs at Python 3.10**, the minimum `pyproject.toml` claims, so the
stub is verified against the oldest version the distribution promises to
serve rather than whichever happens to be installed.

**An absent mypy fails rather than skips.** This is D42's principle
applied to a tool dependency instead of a parser: a check that quietly
does not run reads exactly like a check that passed. mypy is looked for
as `python -m mypy`, then `mypy` on `PATH`, then
`uv run --with mypy`, which needs no prior install and uses the tool the
project already builds its wheel with. `REMANENCE_SKIP_MYPY=1` skips
deliberately, which is a decision somebody made and can be found.
[The escape hatch is removed by D64; the failure stands. How mypy
is found is superseded by D63.]

**Weighed and declined:** `typing.assert_type` in the accepting fixture
(it arrived in 3.11, and checking at 3.10 is worth more than the nicer
spelling — explicit annotations assert the same thing); asserting
against a built wheel for fidelity (it is the weaker reading, as above,
besides needing a build); and pinning a mypy version (a newer mypy
finding more is the point, and a pin would have to be maintained to keep
that).

**No changelog entry for the tests.** They change nothing a consumer of
S1–S3 meets. The `builtins.bytes` fix is release-facing, but the stub it
corrects is itself unreleased — added under the same `Unreleased`
heading by D40 — and that section is editable until it ships, so the
correction lands inside the entry that introduces the stub rather than
beside it.

### D42 — The stub drift check becomes a test, D40's declination having rested on a false premise

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S3; D40. `DECISIONS.md` was searched first and
returned **D40, which declined exactly this** — so this entry is a
reversal and says why one is warranted rather than treating the earlier
call as absent.

**What D40 got wrong was a premise, not a judgement.** It declined the
test because the check "needs a built wheel and an environment to
install it into, which no test in this workspace has today". That is
true of the check *as D40 had performed it* — importing the built module
and reading `dir()` — and it silently assumed that was the only way to
ask the question. It is not. What `crates/remanence-py/src/lib.rs`
registers with pyo3 **is** the module surface, so comparing the stub
against that source asks the same question of the same norm, and reads
two files to do it. This is the new evidence the record requires before
a declined item is revisited.

**A `tests/` target works in a cdylib-only crate**, which is the second
half of why this is now cheap. The test links nothing — it never
mentions `remanence_py` — so `crate-type = ["cdylib"]` and the
`extension-module` feature both stay exactly as they are, and the
deliberate property recorded in the workspace `Cargo.toml` (plain
`cargo build`/`cargo test` runnable without a Python toolchain) is
untouched: `remanence-py` is still outside `default-members`, and the
test runs under `cargo test -p remanence-py`, which is already the
command AGENTS.md names for a Python-surface change.

**The parsers refuse what they do not understand, and that is the whole
design.** A checker that silently fails to see a construct passes a stub
with a hole in it, which is worse than no checker, because it converts
"unverified" into "verified" without doing the work. So the reader
asserts the shape it depends on — `#[pyclass]` on one line, `#[pymethods]`
and `impl` and `pub struct` at column 0, members indented four — and
fails the test where the file stops matching, demanding the parser be
taught rather than guessing. It also refuses `#[pyo3(name = ...)]`
outright, since that would decouple the Rust identifier from the Python
name this test reads it as, and asserts a floor on the number of classes
found so that a parser that stopped reading cannot look like a surface
that shrank.

**It checks names, not types**, and the entry says so rather than
letting a green test imply more than it establishes. Membership,
registration and constructibility are what a static reading can settle;
whether `rule` is `str | None` is not. The `mypy --strict` pass over a
built wheel remains the way to check that, and AGENTS.md keeps it as the
complement rather than the leftover.

> **Annotated by D43**, which automates that complement — and finds that
> it needs no built wheel either. The clause above stands as a statement
> of what *this* test does and does not establish.

**Verified by injecting drift, not by passing.** A green assertion
proves nothing until it is shown to fail, so each kind was introduced and
caught: a getter added to the module, a property deleted from the stub, a
class invented in the stub, a method renamed on one side only, and a
`#[pyclass]` that `add_class` never registers. Two attributes stay
exempt and are named in the test — `Error.category` and `Error.rule` are
set with `setattr`, so no static reading can see them as members (D41).

**Weighed and declined:** introspecting the built module in-process with
`Python::attach` (the most truthful comparison, and it needs `rlib` in
`crate-type`, `extension-module` moved off the default features, and
libpython linked into a test binary — three changes to how the crate
builds, to check something two files already answer); and adding
`remanence-py` to `default-members` so the test runs under a bare
`cargo test` (it would make every build require a Python toolchain, which
the workspace `Cargo.toml` deliberately avoids and says so in a comment).

**No changelog entry.** A test is not release-facing: it changes nothing
a consumer of S1–S3 meets, and the changelog records what they meet plus
principle armings.

### D41 — The Python exception is `remanence.Error`, and it states its two attributes

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S3. `DECISIONS.md` was searched first and
returned only D39's own note that this was left open.

The last item the naming review reported. `remanence.RemanenceError`
repeated the module in the class, which is the same defect D39 corrected
in `remanence::RemanenceImage` and is corrected the same way: the
exception is now **`remanence.Error`**.

**PEP 8 asks for the `Error` suffix, not for a unique word.** `Error`
satisfies it, and `sqlite3.Error` is the stdlib precedent for exactly
this shape — a module whose one exception type is named for what it is
rather than for the module it already lives in. The counter-examples in
the stdlib (`json.JSONDecodeError`, `subprocess.SubprocessError`) are
modules with *several* exception types that need telling apart; this
module has one, so there is nothing for a qualifier to distinguish it
from.

**The rename is the Rust identifier, not just the registration.**
`create_exception!` takes one name and uses it for both, so the class's
`__name__` moves with the attribute and a traceback reads
`remanence.Error` rather than the old name wearing a new alias. Nothing
in the crate imported `remanence::Error` unqualified, so the core error
type and the binding's exception do not collide.

**`category` and `rule` are written into the stub, which is the second
half of the fix.** Both are set on every instance by the binding, so
both are surface — but they are set with `setattr`, so they are not
class attributes and the stub's first pass (D40) missed them entirely. A
caller could not see in the stub what it is safe to read in an `except`
block. They are now declared, `category: str` and `rule: str | None`,
and the drift check carries a note that these two are instance-only by
construction rather than by omission.

**The C ABI is untouched, and `RemanenceErrorCategory` is not the same
defect.** Every exported C type carries the library prefix because C has
one namespace; that is the convention D39 kept when it made the flux
root `RemanenceFluxImage`. The stutter exists only where a language has
modules, which is Python alone here.

**Weighed and declined:** `RemanenceError` kept as-is on the grounds
that Python libraries commonly stutter (they do, and the project had
just spent D39 deciding it would rather not); registering the existing
type under a second name (an alias is two names for one thing, which is
what pre-1.0 exists to avoid); and a family of exception subclasses per
category (the category is already a stable string attribute, and a type
per category would put the same claim in two places — the objection D40
raised against `Literal` unions, in another form).

**This closes the naming review.** D38, D39, D40 and D41 between them
take every defect it reported; nothing from it is outstanding.

### D40 — S3 gets a hand-written type stub, and the stub is surface rather than documentation

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports** S3. `DECISIONS.md` was searched first and
returned nothing on typing, stubs or PEP 561 — this is the first ruling
in that area.

The naming review that produced D38 and D39 also reported that S3 had no
type stub, which meant every name those two entries corrected was
invisible to a Python consumer's editor and type checker. The stub lands
here.

**It is written by hand, and that is the decision.** Generating stubs
from the pyo3 macros was the obvious alternative and is rejected on the
same ground the C header is *not*: cbindgen derives the header from the
`extern "C"` signatures, which carry the whole contract, whereas a pyo3
class carries its Python types only in `IntoPyObject` impls that a
generator would have to guess through — `Option<T>` reaching Python as
`T | None`, `Py<PyBytes>` as `bytes`, `PathBuf` as an os.PathLike union,
a `#[getter]` as a property, a frozen `get_all` field as a *read-only*
one. A generator that got those wrong would produce a stub that
type-checks a lie, which is worse than no stub. Written by hand, the
stub is a second statement of the surface that can disagree with the
first — and disagreement is detectable, which is the property that
matters.

**The module is the norm and the stub is what moves.** A mismatch is a
bug in the stub, never a licence to change the module to match it. The
obligation is recorded in AGENTS.md beside the binding rule it belongs
to, because nothing regenerates this file and a surface change that
forgets it produces a stub that is confidently wrong.

**Values that cross as stable spellings are typed `str`, not `Literal`
unions.** A format id, a device type, an article and a rule identity are
all enumerated at runtime — `formats()`, `device_slots()`,
`partition_types()`, `dos_assignment_rules()` and their kin exist so a
caller can hold the claim without meeting it first (P3). Freezing those
sets into the stub would put a second, staler claim beside the one the
library answers with, and the two would drift apart silently. The
narrower types are declined for that reason and not for effort.

**A mixed maturin layout is adopted for the marker's sake.** PEP 561
requires `py.typed` inside a package, and maturin's pure-Rust mode
generates the re-export shim with nowhere to put one. Naming a
`python-source` root gives the stub and the marker a home; the generated
`__init__.py` is written out unchanged, so the wheel's shape is what it
already was — `remanence/__init__.py` beside the extension — with two
files added.

**Weighed and declined:** a `Literal` union per stable-spelling family
(above); shipping the stub as a separate `remanence-stubs` distribution
(it decouples the two things that must move together, which is the one
failure this design is trying to make detectable); and asserting the
stub against the module in the test suite (worth doing, and it needs a
built wheel and an environment to install it into, which no test in this
workspace has today — the check is written down in AGENTS.md as a
procedure until something runs it).

> **Annotated by D42**, which reverses the last of those. The premise
> was wrong rather than the judgement: the check needs a built wheel only
> if it compares against a *built module*, and comparing against the
> module's defining source needs neither. The rest of this entry stands.

### D39 — The flux root stops being called an image, and three more names are corrected

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports (none)** — naming again, and the clause is `none`
for the reason D38 gives. D38 named these four and explicitly declined
to rule on them; this entry is that ruling. `DECISIONS.md` was searched
first: D34 and D23 both mention `get_sector`, but on *which types* and
*which handle* own it, never on its spelling, so nothing here reopens a
settled question.

**`RemanenceImage` becomes `FluxImage`, because `image` meant two
things in one C namespace.** The C ABI spelled the `.remanence` root
`remanence_image_*` across 36 functions while `remanence_medium_image_path`,
`remanence_discovery_image_format` and their kin used `image` in the
ordinary disk-image sense. A caller reading `remanence_image_open` had
nothing in the name to tell them it opens a `.remanence` artifact rather
than any image the library reads. The family moves together —
`FluxImage`, `FluxImageReport`, `FluxHole`, `FluxOrbit`,
`FluxWriteReport`, and the internal `FluxImageBuilder` with them — and
the C ABI keeps its library type prefix, so the C spellings are
`RemanenceFluxImage` and `remanence_flux_image_*`. The stutter in
`remanence::RemanenceImage` goes with it.

**The cost is accepted, not overlooked:** the type no longer echoes the
`.remanence` format name, and "flux image" is slightly broader than the
one container it names, P64 also holding flux at rest. Weighed against a
C namespace where one word meant two things, the broader name is the
cheaper defect — and it is broader, not wrong.

**The write-report accessors stop reading as verbs, which the rename
delivers rather than patches.** `remanence_image_write_path(report)`
named the same prefix as the verb `remanence_image_write(image, path)`
and read as "write path". Named after their own type as every sibling
report already is — `remanence_d64_report_*` being the pattern — they
become `remanence_flux_write_report_*`. This was reported as its own
defect and needed no separate rule: name accessors after the type they
read and it does not arise.

**A qualifier the receiver already carries is dropped, and one that
earns its place stays.** `C1541Bitstream::materialize_c1541_bytestream`
and `C1541Bytestream::recognize_c1541_sectors` restated `c1541` to
receivers that are nothing else; they become `materialize_bytestream`
and `recognize_sectors`, which is what the C ABI already spelled them
and what the three surfaces now agree on.
`FluxImage::materialize_c1541_bitstream` **keeps** its qualifier: that
receiver is not a c1541 type, so the word says which family is being
materialized and is doing work.

> **Annotation (D59, 2026-08-16):** this last clause no longer holds.
> F76 took the family out of the rung types, so the verb is general and
> the artifact states which family it holds; the qualifier was dropped
> and the spelling is now `FluxImage::materialize_bitstream`. The rest
> of this entry stands.

**`get_sector`/`put_sector` become `read_sector`/`write_sector`.** Rust
discourages the `get_` prefix (C-GETTER), and the crate already spelled
the same act `C1541Sectors::read_sector`, so the pair was diverging from
its own neighbour. `read`/`write` is the symmetric pair `get`/`put` was
reaching for and matches `read_at`/`write_at` beside it. The addressing
rules, the `GeometryRule` refusals and D34's ruling that block-addressed
types answer no such call are all untouched.

**Weighed and declined:** renaming only the C prefix and leaving Rust
and Python on `RemanenceImage` (cheapest, and it would have manufactured
exactly the three-surface disagreement the `materialize` ruling above
exists to remove); leaving the collision and documenting it as inherent
to a library that shares its name with its native format (defensible,
but it asks every C caller to carry the distinction the name should have
carried); renaming the *other* `image` uses instead (they are correct —
`remanence_medium_image_path` names the medium's type first and reads
unambiguously).

**Still not decided.** The `remanence.RemanenceError` stutter in Python
and the missing `.pyi` stub were both reported and are untouched here.

> **Annotated by D40 and D41**, which take the stub and the stutter
> respectively. Nothing above is overruled: the clause was an accurate
> statement of what this entry left open, and both are now closed.

### D38 — Two surface names are corrected to say what they do: `check_type` and the `_count` pair

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-13. **Supports (none)** — this is a naming choice and nothing
more. No use case turns on the spelling of a verb and no principle
speaks to it; the change is demanded by the languages' own conventions,
which are not this project's vision to amend. The supports clause is
`none` rather than a plausible-sounding P-number, because a citation
that would not survive an audit is worse than an honest absence.

Two names were reported as misleading in an owner-directed review of
all three surfaces. Both are corrected here; both were judged on the
same test — **does the name say what the thing does?** — and nothing
else in the review is decided by this entry.

**`as_type` becomes `check_type`, because `as_` promises a
conversion.** Rust reserves the `as_` prefix for a free borrowed
conversion (C-CONV), so `as_type` reads as a verb that hands something
back. It hands nothing back: it is the check, and D32 already ruled
exactly that — "the check is the whole of it". The old spelling argued
against D32's own ruling every time a caller read it. `check_type`
states the ruling instead of contradicting it, and it no longer sits
oddly beside `filesystem_as`, which *is* a conversion and rightly puts
`as` last. D32's ruling on the **return type** stands untouched; only
the spelling moves.

**`locations()` and `claims()` become `location_count()` and
`claim_count()`, because they answer a number.** Both returned a `u64`
under a plural-noun name. In Rust that is merely wrong; in Python it is
worse, because the bindings expose them as *properties*, so
`bitstream.locations` evaluated to `5` and read as a defect in the
library. The C ABI never had the bug — it spells them
`remanence_c1541_bitstream_location_count` and
`remanence_c1541_sectors_claim_count` — so this is the core and the
Python module being brought to the spelling the C surface already
carried, rather than a new convention being invented. The report
structs keep `locations` and `claims`: those fields hold the
collections, and the plural is correct there.

**Weighed and declined:** `expect_type` for the check (it reads as a
panic in Rust, where `expect` is the panicking unwrap); `len()` for the
counts (it implies the receiver is a collection, and neither is); and
renaming the report fields to match the accessors (they are correctly
named already — the accessors were the ones lying).

**Not decided here.** The same review reported four further naming
defects — the `RemanenceImageWriteReport` accessors that omit `_report_`
in the C ABI, the `image` prefix carrying two meanings there, the
`materialize_c1541_bytestream` qualifier the three surfaces spell
differently, and `Medium::get_sector` against C-GETTER. They are
untouched and unadjudicated; this entry rules on nothing it does not
name.

> **Annotated by D39**, which adjudicates all four. The clause above was
> true when written and is the reason D39 exists; it is superseded only
> in the sense that the four are no longer open.

### D37 — Rulings made delivering the no-cache discovery

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-12. **Supports** S1, S2, S3; in-force P4, P7, P27; D30.

F67's delivery is recorded by the commit; these are the calls made in
its course.

**"Builds no cache" is read as "builds no medium", and recognition is
what a discovery keeps.** The narrow reading — drop the two caches and
keep the medium — would have left the medium's every other part
standing, which is the load's work under a different name. The wide
reading — keep nothing but the claim and re-run the adapter at the load
— would have made the discovery cheap by making it run twice, and D30's
whole point is that it runs once. So the split lands at the seam the
work already has: **recognizing** an artifact (claim the file, ask the
catalog which adapter bears it, run the P28 gate) and **materializing**
a medium from that recognition (the session cache the reads stream
through, the commit buffer the writes land in). A discovery holds the
first; `load_discovery` performs the second over the very claim already
held. Nothing is re-opened, no adapter runs twice, and every fact a
discovery reported before it reports still.

**An archive's index is recognition, not state built ahead of the
question.** Reading a zip's central directory *is* how the artifact is
recognized as a zip, so the catalog belongs on the recognition side of
the seam. What the archive medium adds at the load is its evidence plane
— the artifact's own bytes through a bounded cache — which is exactly
the part that has a bound to declare.

**The bound moves to the load rather than disappearing.** F67 strikes
`discover_media_with_cache`, and the temptation was to leave the
discovery journey with no way to state a bound at all. That would have
cost a delivered capability to make a point: the caller who wanted a
small working set still wants one, and now there is a verb whose job is
to create the thing being bounded. So `load_discovery_with_cache` and
`load_discovery_as_with_cache` land beside the plain doors, matching
`load_media_with_cache` and `new_media_with_cache` exactly.
`add_device_for_with_cache` keeps its bound unchanged — it composes the
discovery *and* the load, and the bound belongs to its load half.

**`Discovery::state()` becomes `recognized()`, and the refusal it feeds
is spelled once over both.** The convenience's "nothing says which drive
wrote it" refusal was built from a `MediumState`; it now has to be built
from a recognition too. Rather than two spellings of one message, the
refusal takes the three facts it names — the artifact, the format, the
recorded types — and both callers hand them over.

**Weighed and declined:** keeping `ResolvedImage` as the shape a load
takes (with the claim and the cache now separated, it held nothing the
`ClaimedSource` and the `ImageSource` do not, and a struct that exists
to be destructured immediately is a seam nobody crosses); and letting a
discovery answer `identify()` lazily off a cache built on first use (it
reinstates the cache this feature removes, one call later, and the read
is bounded evidence either way).

### D36 — Rulings made delivering authored media

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-12. **Supports** S1, S2, S3; the pledged media-first design and
its fact-class and creation-grammar rules; in-force P2, P3, P5, P10,
P13, P14, P27; U32; D34.

F60's delivery is recorded by the commit; these are the calls made in
its course.

**"The blank article kinds and `ChsDisk`" is read as two kinds of
kind, and the enumeration is flat.** F60 names both halves, and they
are genuinely different things: a blank article kind states an
*article* — the manufactured substrate, with nothing recorded on it —
and `ChsDisk` states *coordinates*, which no manufactured article
carries. The reading taken is that the set is one per authorable
article plus the coordinate one, and the blank article kinds are
spelled by their own article ids (`flexible-5.25-soft`,
`flexible-5.25-hard-10`), because that is the whole of what each
declares. `NewMedia` is flat rather than two-level, as `Format` is: a
second enum would name a set with one member outside it.

**An authored disk's article is `authored`, a second member of the
virtual family.** No manufactured article stands behind coordinates a
caller stated, and the candidates all lied: `logical-block-512`
declares "no cylinder, head, track… fact of any kind" and would also
have forced 512-byte sectors on an author who wanted 256, and
inventing a physical article would assert a coating and a form factor
nobody made. The virtual family already exists for exactly this —
independent recorded state with no physical article behind it — and
the one fact it declares is the native vantage, which parts the two
members cleanly: an archive's is a namespace, an authored blank's is a
space. A caller who wants a *manufactured* article authors that
article by name and gets its published facts.

**A third `Claim` class, because a third fact class exists.** The
claim answers whose open a medium's P7 claim is, and an authored
medium was opened by nobody: `library-opened` and `caller-opened` each
assert a file that does not exist. `Claim::Authored` is the honest
third answer, and it travels to S2 and S3 as one — the same shape
`device_type()` answering `None` already has.

**`GeometrySource::Authorship` is a source and does not break the
fact classes.** A geometry is still never *declared onto* a medium
that exists: authorship states coordinates in the same act the medium
is created in, and the created medium's geometry is settled from that
moment and immutable, exactly as a loaded medium's is settled at the
load. The source never appears beside another — there is no artifact
under an authored medium for a second reading to be taken from — so
the settling machinery is bypassed rather than run over one reading:
running it would add an extent-arithmetic reading derived from the
author's own coordinates, which is a source agreeing with itself.
D34's "weighed and declined" pointed here for a caller's own
coordinates, and this is where they enter.

**An authored blank goes in no drive, and `Medium::slot()` became an
`Option` to say so.** Insert is device-type equality and an authored
medium has no device type, so there is nothing to weigh; seating it
anywhere would assert the drive the author deliberately did not. The
pool's admission check had to be re-cut at the same time: it used to
refuse a medium with no slot, which now also describes an authored
one, so the question it asks became the one it always meant — *is
this a reading that could not say what recorded it?* — leaving the
authored medium, which is not missing anything, admitted.

**An authored medium bears the direct partition, and no namespace.**
The walk stays uniform (P19 as amended): every medium is reached
through a partition, so an authored disk bears the direct partition
over the content its coordinates address, addressable and composing no
volume, and a blank article bears an extent-less one. Nothing is
classified to establish it — a blank the author just made is blank,
which is the one case where the content's answer is known without
reading it. The **namespace vantage is refused by name** rather than
opened: recording a layout onto an authored blank is precisely the
authored-to-recorded arc F60 reserves, and offering `filesystem_as`
over a blank would deliver a door that can only ever refuse at the
adapter.

**The commit point stands, with no journal beneath it.** P2 is about
when buffered writes become the medium's state and P9 is about a
*file* being left reconcilable after an interruption. An authored
medium has no file, so it keeps the first and needs none of the
second: `commit` writes the buffered extents through into the
session's own sparse backing and `rollback` discards them. The backing
is the private transient storage the cache already spills to —
unlinked at birth, delete-on-close on Windows — which is what
"session-backed" means and why a 528 MB authored disk costs what was
written to it rather than what it addresses (P27).

**The bound is checked where the write is offered.** A block medium's
format adapter answers for the disk it presents and clamps a
write-through to it; an authored medium has no adapter, so a write
past the author's coordinates would buffer and be dropped at the
commit. The authored space checks its own bound instead, so the
refusal arrives where the caller can act on it.

**Weighed and declined:** deriving the blank article kinds from the
article catalog at run time (the catalog holds articles no author can
make whole — the archive's `virtual`, and `logical-block-512`, which
states no size — so the authorable set is a claim of its own, and P3
wants it enumerated); giving an authored blank the file verbs by
routing FAT recognition over it (it can only refuse until something
records a boot record, and that something is the reserved arc);
`ChsDisk` carrying the four coordinates as loose fields, as the
pledged U32 walk spelled it (F60's own "Needs nothing" clause says the
coordinates an authored blank states are the delivered geometry's own,
and a second spelling of `RecordingGeometry` would be two shapes for
one fact — the walk is updated to the delivered one).

### D35 — Rulings made delivering the collection-sourced load and the flux fold-in

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-12. **Supports** S1, S2, S3; in-force P3, P13, P27, P29, P30;
U23 (pledged), U25, U26, U33; D29, D31.

F59's delivery is recorded by the commit; these are the calls made in
its course.

**The source-gathering verbs claim archive namespaces alone.** A
`File::source` and a `StorageSpace::files` answer from an archive's
namespace — free-standing sources riding the archive's claim, a solid
archive's coded stream decoded once for the whole gathering (P27) —
and a volume-backed filesystem refuses by name, its files being read
through the filesystem that names them. The claim mirrors
`File::discover`'s, and widening it is later work with its own
evidence, not a looser match here.

**A nested archive is refused by name.** A namespace file declared an
archive grammar would need the catalog seam to read through an entry
source rather than a claimed file, which no grammar does yet; the
refusal names the claim ("this release reads an archive from the
caller's own opened file") rather than failing inside a grammar that
was never asked.

**The recognition and capture-inspection reporting fold to
provenance.** The declared load pins recognition to the declared
device's profile and requires the claim borne; which capture head
carries the recording is measured from the claimed locations, a claim
on both heads refusing by name (the family records one surface). The
verdict, the policy and the declared-loss account ride the medium's
assurance evidence (P28's ordered evidence, P29's account), and the
ranked-verdict, plan-preview and capture-inspection *surfaces* stay
out with the question tier, exactly as F59 pledged. The fixture-driven
recognition claims moved to the crate's own test tier with the surface
that carried them.

**The profile's presentation defaults are declarations, stated whole.**
The C1541 profile now declares its channel policy (the family's own
density map, unzoned locations omitted and counted, weak pulses
resolved reproducibly from the profile's stated seed), its codec
policy (landmark framing, unassigned symbols kept as their own bits
and counted), and its sector reading (checksum failures and unpaired
records declared as loss) — P30 reached through the type, every value
travelling into the result as provenance. The non-default choices stay
in the code as the deferred policy-deviation surface D29 records,
constructible by nothing public.

**The family addressing is spelled `Location::track` and nothing
more.** The delivered journeys read whole tracks; half-track
addressing waits for a journey that needs it, and the reports state
half-tracks meanwhile.

**A consequence, stated so nobody meets it as a surprise:** with the
`CaptureSet` root folded and U23's step 4 (one verb taking a
destination format) still unpledged work, no public path produces a
`RemanenceImage` — the `.remanence` root opens existing artifacts and
masters the renditions, and a capture loaded as a medium reaches no
writer. That is the gap D28 already names as what U23 uniquely adds,
not a new removal; it closes when that verb is argued and built.

**Weighed and declined:** keeping `CaptureSet::plan_reconstruction`
public so captures could still reach the renditions (that is the old
root wearing one verb, and F59's substance is that the capture is a
declared collection reading); a public medium-to-image bridge for the
same purpose (an unpledged surface entry, and the save verb U23 is
owed would obsolete it on arrival); spelling the collection formats'
source shape as a second load verb rather than a source conversion
(two verbs would make the shape the caller's problem twice, where one
declared `MediaSource` lets the format's claim refuse the wrong shape
by name); and half-track spelling on `Location` (above).

### D34 — Rulings made delivering the discovered geometry and the recording's coordinates

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-12. **Supports** S1, S2, S3; the pledged media-first design
(fact classes, and the kind-declared actions on the medium); in-force
P2, P3, P4, P6, P10, P14, P27; in-force U4, and pledged U28, U32.

Rulings made in F58's course. The delivery itself is recorded by the
commit; these are the calls made along the way.

**The floppy class is sector-addressed, and the addressing attribute
becomes total.** The delivered `addressing()` answered `Option`, `None`
for every floppy, with a note deferring the question to this feature.
The answer is that it was never a hard-drive fact: a floppy drive steps
to a track and reads records around it, which is exactly what a
cylinder, head and sector name. What the granularity rule keeps *out* of
the type is how many of each, not whether there are any. So the
attribute is total — `sector` or `block` for every device type — and the
cut it makes is the one the sector verbs need: the type declares that
there are coordinates, the medium's evidence says how many. The C and
Python spellings keep their nullability because the archive receiver is
no device type at all.

**An end tuple states nothing on its own, and is solved rather than
read.** The obvious inference — heads are the head number plus one,
sectors per track are the sector number — is wrong twice over: a drive
past what CHS can address writes a saturated tuple whose numbers name no
geometry, and a partition that ends mid-cylinder names a head that is a
floor rather than a count. What makes a tuple evidence is that the same
entry declares the same block a second way, as the last block of its own
LBA extent, so the geometry is whatever puts the one where the other
says it is. Where exactly one geometry within the field widths does
that, it is the reading; where several do, or none, the tuple states
nothing and nothing is inferred from it. Verification fills values under
a reading and never picks one.

**The load's declaration of a raw block size is a source, not an
override.** `Format::Raw` carries the block size because a raw image
records no addressable unit — but a table read in 512-byte blocks states
one too, and this release's MBR reading is written against exactly that.
Ranking the caller's declaration above the evidence would hide a real
contradiction about one disk, so the declaration enters as one reading
among the others and a disagreement reports as `Undetermined` like any
other. This is the fact-class rule holding: the declaration belongs to
the *load*, and what a medium's coordinates are stays discovered.

**The article's addressable unit is not a geometry source.** The
logical-block article declares 512-byte blocks and every hard-drive spec
composes it, so it was available and was deliberately left out: the
article is what the *substrate* is, and a sector size is a fact about
what was recorded on it (D19's boundary, at "recorded"). Reading it here
would also manufacture a conflict with a raw load declaring some other
unit, out of a fact that was never about the recording.

**The extent states the cylinders the recording spans, and the sector
verbs check the content separately.** The strict reading — a cylinder
count only where the track geometry divides the extent exactly — was
weighed and declined: it is the delivered rule for the *filesystem's*
declared geometry, where inventing a number would be a false claim about
a boot record, but here it would leave every image whose size is not a
whole number of cylinders with no coordinates at all, which is most of
them. The extent reading answers the cylinder its last sector falls in,
plus one, and says in its own words how far short of that cylinder the
content stops. The gate that would otherwise be lost is not lost: a
coordinate inside the geometry and past the content is refused by name,
with a different sentence from one outside the geometry altogether.

**A geometry is whole or it is nothing.** Three of the four parts
settled is not a geometry with a hole in it — it addresses nothing — so
the state is `Unstated` and `unsettled()` names the missing parts. That
keeps three answers apart that a partial record would blur: the sources
agreed, the sources disagreed, and nothing spoke. `Unstated` and
`Undetermined` are deliberately two states for the same reason U4 keeps
blank apart from unreadable.

**Establishing a geometry never fails a load.** It runs beside the
partition pool, after it, over the positions the pool already
established — so nothing hunts for a volume — and a source that cannot
be read states nothing rather than refusing. A geometry is evidence
about an artifact, not a condition of opening one, and a degraded
session (P28) that can no longer read a boot record still loads and
still says what it does know.

**Weighed and declined:** a geometry the caller could declare onto a
loaded medium (the design's fact classes forbid it, and F60's authorship
is where a caller's own coordinates enter); ranking the sources so a
disagreement always resolves (it would settle by fiat what the artifact
leaves open, which is the one thing `Undetermined` exists to refuse);
`get_sector` on the block-addressed types under a synthesized geometry
(a `mbr-block-hd` records no cylinder or head, and answering one would
be the library asserting a drive nobody had).

### D33 — Rulings made delivering the device types and the articles they compose

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-12. **Supports** S1, S2, S3; the pledged media-first design and
the pledged P32 with its amendments; in-force P3, P5, P6, P10, P12, P14
(amended here to carry the device-type catalog), P16, P19; in-force U4,
and pledged U23, U25–U28, U32, U34.

Rulings made in F57's course. The delivery itself is recorded by the
commit; these are the calls made along the way.

**The archive receiver is a slot and not a device type, so `DeviceSlot`
is a second enum beside the catalog.** F57 says a device type names the
device a medium's content is *assumed recorded by*, and that archives
were recorded by none — `device_type()` answering `None` is the feature's
own sentence. An `Archive` variant inside `DeviceType` would contradict
it at the first call, since an archive medium would then answer `Some`.
But D27 keeps `arc0` visible in its machine's attachment namespace and
an archive still has to be seated somewhere, so what a device is typed by
is **either** a recording device or the receiver: `DeviceSlot::Recorded(t)
| DeviceSlot::Archive`, with `From<DeviceType>` so the ordinary call
reads `add_device(HardDrive::MbrBlock)`. The receiver's own `device_type()`
answers `None`, which is the same word meaning the same thing on both
sides of the insert check.

**An attachment identity names a place, so it carries the bay and not the
type.** Three hard-drive types take `hdd` and both Heathkit controllers
take `heathfloppy` — the granularity rule cuts the *recording*, and a
machine's bays are not cut the same way — so a slot prefix no longer
resolves to one device. `AttachmentId` became prefix-and-index, the type
moved onto `StorageDevice` where it already belonged, and the lowest free
slot counts by bay: two hard drives of different types cannot both be
`hdd0`, which is a fact about machines rather than about the catalog.
The delivered identity duplicated the device's own family; nothing is
lost by removing the duplicate.

**The interior-name refusal disappears rather than being reworded.** The
delivered catalog classified with interior entries and refused them at
`add_device` (P32's amendment). A two-level enum *is* that hierarchy, and
"some floppy" is no longer a value that can be spelled — F57's "a type
the library does not know fails to compile" covers the vague name as
well as the unknown one. The `is_a` query, the lineage and
`accepted_media` go with it; asking whether a device is a floppy is now
a `match` on the class.

**Where the declared scheme does not check out the answer stays the
direct partition, and D32's reason for that is only half removed.** D32
deferred the refusal to this feature on the ground that nothing then
distinguished a partitioned hard disk from a floppy image. The device
type now distinguishes them, and the floppy class is genuinely exempt
from the table read. But `Format::Raw` is typed to the hard-drive class
by F57's own text, so a bare FAT floppy image arrives declared as a
hard-drive recording — and refusing an unpartitioned one would refuse
every image this release reads that way. The check reads the table where
one is there and composes the direct partition where the content records
none, which is what F56 delivered and what F57 keeps.

**A schemeless medium is still classified, and a table it might hold is
content nothing claims.** Skipping the scheme step entirely would have
cost the floppy class its content outcome — blank, one bare volume, or
content nothing claims — which is evidence about the recording rather
than about a layout, and which is what decides whether the direct
partition composes a volume at all. So `mbr::classify` answers the three
no-scheme answers and never the fourth: a sector 0 carrying a boot
signature on a medium whose device type declares no scheme is reported
as content nothing claims, with a reason of its own saying the table was
not read because nobody declared one.

**A discovery over a format that records several device types asserts
none, and the pool refuses to take it.** The alternative was pooling such
a medium with `device_type()` answering `None` — but `None` means
*recorded by no device*, and a qcow2 was written by some hard drive.
Using it for "we do not know which" would corrupt the one word the model
spends on the honest absence, and the medium would in any case be
seatable in nothing and layoutable under nothing. So the refusal is at
the pool's plain door and at `add_device_for`, naming the types a
declaration may state, and `Discovery::device_type()` answers only where
the format records exactly one.

**The declaration a discovery cannot make is taken by
`load_discovery_as`, and the vantage doors are the precedent.** The
refusal above nearly cost a delivered capability: an artifact *inside an
archive* is reached only through `File::discover`, so a member no adapter
identifies — a KryoFlux stream, which is bytes to every enrolled
adapter — would have become unloadable, there being no file for
`load_media` to take. The library already has the shape for this:
`filesystem()` opens where the declared type determines a namespace and
`filesystem_as(id)` takes the caller's reading where nothing does. So
`load_discovery` is the plain door and `load_discovery_as(discovery,
device)` the declared one, the second checking the type against what the
recognizing format records. The claim is held across both, so the nested
journey keeps its one open. F67 is where discovery's shape is next
argued.

**`Discovery::default_device` collapses into `device_type` because the
two facts became one.** The delivered surface distinguished "the family
the format declares" from "the medium's own type", the medium having
none to carry. A medium now carries the device type, and where a format
records exactly one there is nothing left to distinguish: what the format
declares *is* what the medium will carry. `accepting_devices()` remains
the other question — where could this go — and `device_types()` is the
adapter's list, which is what the refusals name.

**The identity crosses C as its stable spelling rather than as an
integer constant.** F57 says "integer constants in C, enums in Python".
The catalog ships as strings on both, because every other enumerated
claim this ABI carries already does — formats, partition schemes,
partition types, drive profiles, the families this replaces — and the
generated header is derived from the Rust signatures, so one catalog
crossing as an integer would be the only one a C caller has to hold a
second table for. The stable spelling *is* the cross-language identity
the surface is built on, and P5's "same semantics" is served by using
it. Python takes the same spelling for the same reason; a Python enum
over it remains available later without moving anything.

**Three enumerated types are declared by no format in this release, and
that is the catalog working.** `HardDrive::Gpt` is enumerated because the
scheme is part of the hard-drive spec and GPT implies block addressing by
its own definition; no adapter records it, because none reads a GPT, so
declaring it is a named refusal rather than a silent reading of the wrong
table. `FloppyDrive::HeathH37` and `FloppyDrive::Sector` are the same
shape: named by the granularity rule, reachable when a format records
them. The catalog is the claim; what a format admits is a separate
declaration, and F57 asks for exactly that gap.

**Weighed and declined:** an `Archive` variant in `DeviceType` (above —
it makes the model's own sentence false); keeping the attachment identity
type-bearing by giving each device type its own slot prefix (`mbrsector0`
beside `mbrblock0` is a bay no machine has); refusing a declared scheme
that does not check out (above — it refuses every bare FAT image);
skipping content classification for schemeless media (above — it costs
the floppy class its volume); pooling a device-typeless medium
(above — it spends `None` on two meanings); and leaving the refusal
without the `_as` door (above — it makes an archived raw member
unloadable, there being no file to declare over).

### D32 — Rulings made delivering the partition pool and the vantage doors

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-10. **Supports** S1, S2, S3; the pledged media-first design; the
P19 amendment; in-force P3, P4, P10, P16, P17, P18, P21, P27; U4.

Rulings made in F56's course. The delivery itself is recorded by the
commit; these are the calls made along the way, and the first is the one
a later reader most needs.

*Annotation (D33, 2026-08-12): F57 has landed. The scheme moved from the
media type to the device spec as the first ruling below anticipated; the
refusal the second ruling deferred is **not** reinstated, and D33 records
why.*

**The pool populates under the medium's kind, because the device spec it
is owed does not exist yet.** F56 says the pool populates "under the
device spec, kind-determined for every type — the hard-drive class by its
spec's scheme, checked at load". Device specs are F57's, and F56 needs
nothing from F57 — so what names the scheme today is the only kind a
medium carries today: its media type. A space-native medium is laid out
under MBR and the table is checked at the load; a namespace-native one
bears the direct partition with no extent. **What was removed is the
probe, not the check**: the delivered partition catalog ranked layout
adapters against a device and fell through to a bare volume, which is a
reading being picked, and one specified scheme checked against the
content is not. When F57 lands, the scheme moves from the media type to
the device spec and nothing above it moves.

**Where the specified scheme does not check out, the answer is the
direct partition rather than a refusal.** "Checked at load" reads as a
refusal, and a refusal is wrong here for a reason F57 will remove: with
no device spec there is nothing distinguishing a partitioned hard disk
from a floppy image, so refusing a medium whose sector 0 is a boot record
would refuse every partitionless disk this release reads. The scheme
adapter's three no-scheme answers — a filesystem boot record, a blank
disk, content nothing claims — are what they always were, and each of
them now composes the direct partition instead of nothing.

**The pledged tree's one `Partition` is two Rust types, and Rust is the
reason.** The design draws the facts and both doors on one node, and
`partitions()` handing out several door-bearing nodes at once cannot be
written: a door composes a space over the medium, so each node would hold
a mutable borrow of the same medium. So the pool answers with
**`Partition`**, a borrow-free record carrying every fact the scheme
declared, and `partition(n)` answers with **`PartitionView`**, the borrow
that holds a partition and its medium at once. That is the split
`StorageDevice`/`DeviceView` already is, one
tier down, rather than a new shape. *[The entry also cited
`Machine`/`MachineView` as the same pair; D58 withdrew those, and the
device pair carries the point unchanged.]*

**Opening a door spends the view, which is the identity rule carried by
the type.** F56 says both doors hand out *the one* `StorageSpace` the
partition composes. Handing out `&mut` to a space the view held would
have said it too, and it would have made the view a second place a
composed space lives. Consuming instead makes the rule unforgeable: the
node comes back once, through whichever door was asked, carrying whatever
vantages the partition has — so which door was opened changes nothing
about what comes back, which is the identity rule stated exactly.
`Partition::is_addressable` and `bears_namespace` are the non-consuming
predicates for a caller who wants to ask before spending it.

**The direct partition is ordinal 0 and a scheme's own numbering starts
at 1.** MBR numbers its entries from one, so zero is the library's to
spend, and spending it there means the two never collide and the walk is
uniform. A medium recording a scheme bears no direct partition, and a
medium recording none bears exactly it.

**The direct partition never appears in the inspection report, and the
evidence answer is unchanged.** The pledged ledger says the evidence
answer (`partition_scheme: None`) stands while the navigation answer
gains the declared synthetic member, and the code says it the same way: a
composition act is provenance, so `DiskReport` derives its regions from
the scheme's entries alone and a medium recording no scheme still reports
none. `DiskReport` is now computed from the pool rather than being what
navigation goes through, which is the whole of its demotion — every fact
it reports is the fact it reported before.

**U4's identity clause survives the move of the file verbs, and was not
amended.** The in-force entry says an identity "names exactly the same
region, volume, or filesystem in every file verb that it named in this
report". The file verbs are now reached by the scheme's own ordinal, and
the identity travels with them: `StorageSpace::volume_id` answers the
same value the report issued for that partition's composition, and
`Partition::volume_id` answers it beside the ordinal. The identity is
still opaque, still the library's, still stable across opens, and still
never built by a caller from a number or a position — so the claim holds
in substance, and the ordinal is the schema adapter's own fact rather
than a second identity (P16 puts it there, and U4 already declares it
load-bearing where it says a refused entry keeps its place).

**A namespace declaration is a stable spelling, not a Rust enum.** The
partition *type* is enumerated because a scheme's type values are what a
declaration is checked against, and `PartitionType` is that set. The
namespace declaration is not the same kind of thing: it names the adapter
that will read it, and adapters are already named by stable spellings
everywhere they are reached — `"hdos"`, `"cpm"`, `CBM_DOS`, the FAT
kinds. So `filesystem_as` takes the spelling and refuses one outside the
claim by name (P3), which is what `Format::from_id` and
`media_profile::by_id` already do at their own boundaries. The claim is
four: `"fat"`, `"hdos"`, `"cpm"`, `"cbmdos"` — and `"cpm"` still refuses
at the open, recognition and reading being separate claims.

**`as_type` answers `Result<()>`, because the check is the whole of it.**
The verb exists so a caller can state their reading and be refused by
name where the recorded byte does not bear it; it settles nothing about
what the partition then hands out, since the namespace vantage opens
under the type the *scheme* declared. A verb whose value is its refusal
is unusual and is what this one is.

> **Annotated by D38** on the spelling only. The verb is now
> `check_type`, `as_` having promised a conversion this verb never
> performed. The ruling above is unaffected and is in fact what D38
> argues from: the check is still the whole of it, and the new spelling
> says so where the old one worked against it.

**The resolver's medium-namespace bound dissolves into the adapter's
own.** The 8 MiB bound existed because a resolver *searched* a medium's
own content for a namespace and a search needs one (P27). Nothing
searches now: a declaration names its adapter, and the adapter says how
much it will take whole — which is where the number came from in the
first place, and it stays there. `filesystem_catalog::recognize`,
`CatalogRecognition` and `SpaceRule::SeveralCandidates` go with the
search, having nothing left to tie or to break a tie between; the
catalog's probes stay where they were always used, identifying an
artifact's layers.

**Corrected in passing:** root ARCHITECTURE.md's S1 inventory still named
`Archive` and `ArchiveEntry`, which left all three surfaces when the
archive became a medium. The line was being edited for the partition
vocabulary anyway.

**Weighed and declined:** verifying every declared namespace at the load
so the doors could be lookups over verified readings as well as declared
ones (it makes a load read a boot record per partition, and it puts the
recognizing seam's refusal somewhere a caller cannot reach it — the
delivered shape has the door answer from the declaration and the space
carry the verified state, which is D25's ruling that a refused
recognition answers with its own refusal, kept); making the direct
partition addressable only where a volume composed (it would have made
the one member that is *defined* as the whole content refuse to address
the whole content on a blank disk); giving `partitions()` door-bearing
nodes by putting the medium behind a shared cell (interior mutability to
buy one call site is a shape this crate has nowhere else); enumerating
the namespace declaration as a Rust type beside `PartitionType` (above);
and amending U4 (above — its claim holds, and an amendment written to
excuse a change that did not break the claim is worse than no amendment).

### D31 — The declared format set enumerates what a medium *is*, so `p64` waits for the flux fold-in

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-10. **Supports** S1, S2, S3; in-force P3, P13; F53's own pledge,
F59.

F53 lists the format ids its declared reading claims as "`zip`, `7z`,
`h8d`, `qcow2`, `vdi`, `raw`, `p64`". Six of the seven are delivered
with it; **`Format::P64` is not, and moves to F59**, which is the
feature that folds the standalone `CaptureSet` and `P64Image` roots into
the model and already says "`Format::P64` loads the served form straight
in".

The reason is what a format id *does* here. A declaration names the
adapter that checks it and, through that adapter, what the medium turns
out to be — and this release's media are the block family and the
archive. A P64's own adapter declares flux (P13), so `Format::P64` would
have to answer with a flux medium: a media profile no flux artifact
carries yet, an insert check with nothing to check, and content verbs
with nothing behind them. Delivering that is F59's substance, and doing
it under F53's name would have been the fold-in wearing another
feature's number.

**Nothing is dropped by the move.** F53's number retires with its
delivery, so the id would have left no trace; F59's entry now carries it,
and the refusal a caller meets meanwhile is the enumerated one P3
requires — `Format::from_id("p64")` names what this release claims rather
than accepting a spelling that leads nowhere. The flux family stays
reached through its own types, exactly as before.

**Weighed and declined:** shipping `Format::P64` as a variant that
always refuses (a surface entry that never works is worse than an absent
one, and P3's enumerated-claim discipline is precisely against it); and
building the flux medium inside F53 (that is F59, and the sprint bound
bites at the pledge rather than at delivery).

### D30 — The discovery surface is reinstated: discovery is not a duplicate of loading

**Decided** Paul Galbraith, 2026-08-10. **Supports** S1, S2, S3; in-force
P3, P4, P7, P27; U-numbers none — the demand is a caller's, and the
question tier's own argument is where a use case for it belongs.

The media-first design demoted `discover_media`, its cache sibling, the
consumable `Discovery`, `load_discovery`, `add_device_for` and the
image-format `default_device` declaration out of S1–S3, on the reading
that the ask-first journey duplicated what a declared `load_media`
already does. **That reading was wrong, and the ruling is reversed.**
The two verbs answer different questions: loading says *make this a
medium under a format I name*, and discovery says *what is this?* — on
no handle at all, with nothing configured and nothing created. A caller
who does not yet know what an artifact is has no format to declare, and
telling them to guess one so the refusal can teach them the answer is
the ask-first journey wearing a worse shape.

**What makes it not a duplicate is now a stated constraint rather than
an observation: discovery holds the claim and builds no cache.** It
opens the artifact, takes the P7 claim, probes for the type, and stops —
no media state, no session cache, no spilled backing. The `Discovery`
stays consumable, so a load takes the open handle out of it: nothing
runs twice and no window opens between the question and the load. The
cache bound is the *load's* declaration and has no meaning at discovery,
so the delivered `discover_media_with_cache` and the bound travelling
into the device with a discovery go — the delivered surface materializes
today, and closing that gap is F67 rather than something this ruling
performs. *(Delivered: F67 landed the constraint and the bound moved to
`load_discovery`; D37 records the rulings.)*

Three places carried the demotion and all three are corrected: F55 is
struck, the pledged media-first design's "the question tier is demoted,
not deferred" section is replaced by this ruling, and
[proposed/design/question-tier.md](proposed/design/question-tier.md)
stops describing itself as the demoted successor of a delivered surface.

**What is *not* reinstated is everything that tier still proposes.**
Ranked verdicts, policy templates, and gated derivation chains were
never delivered and stay in `proposed/`, to be argued as one thing. This
ruling reverses a removal; it pledges no extension, and the delivered
surface keeps the shape it has until one is argued.

**Weighed and declined:** leaving the demotion standing and letting the
question tier restore the surface when it is argued (the surface is
delivered and working, and removing it to re-add it later costs every
consumer a migration for a decision already known to rest on a false
premise); and reinstating it as delivered, cache and all (that is the
duplication complaint's one true grain — a discovery that materializes a
medium *is* doing the load's work, and the constraint above is what
keeps the two verbs distinct).

### D29 — What the swept flux-layer design deferred, kept where a design cannot go

**Decided** Paul Galbraith, 2026-08-10. **Supports** in-force P22, P29,
P30; U25, U26.

The remanence flux layer's design served F63 through F66, all now
delivered, so it is swept with the last handle that carried it — a
design is guidance toward work not yet done, and what was done is the
code. Its body described delivered surfaces and goes with it. **Its
deferrals do not**: a deferral is the reason a choice was *not* made,
which outlives the design and belongs here.

Four stand, none of them blocked, none of them pledged:

- **The divergence sidecar** — the reconstruction's account as its own
  text artifact beside the image. The account rides the in-memory report
  until a journey needs the file.
- **Flip-side pooling and the flippy transform's fitted origin** — the
  pipeline's seams admit a second capture group, and the work arrives
  when a flippy fixture does. The repository holds one disk captured
  twice in opposite directions, which is the evidence that would drive
  it.
- **Sector-anchored angle merging and checksum-selected arcs** — the
  anchoring licence was written into the delivered reduction; the
  machinery lands when fixtures demand it.
- **The unguided orchestration** — survey, recognise, rebuild both
  orientations without the caller naming a side. It belongs beside the
  question tier's argument rather than ahead of it.

A fifth is **overtaken rather than deferred**: the served projection as
a general verb (remanence image → flux medium). It now has two callers
— the p64 rendition and the presentation ladder's image entry — and is
still crate-private and still not a general verb, which remains the
right shape until something outside the crate needs one.

**Weighed and declined:** keeping the design file on the strength of its
deferral list alone (planning holds no delivered surface, and the list
is four bullets that fit here); and promoting the four to features
(none is argued yet, and pledging is what argument earns).

### D28 — U23 is withdrawn from the in-force list: its journey runs, but not in the shape it is owed

**Decided** Paul Galbraith, 2026-08-10. **Supports** U23, U25, U26;
in-force P13, P29, P30; F59.

U23 asked how a user accomplishes what it claims, and the answer was
that they do — through a surface that exists for captures alone. The
entry is therefore **moved from root [USE-CASES.md](../USE-CASES.md) to
[pledged/USE-CASES.md](pledged/USE-CASES.md)**, which is a withdrawal
rather than a delivery: it will return to the root list when the shape
below is built, keeping its number.

**What the journey should be**, in four steps, fixing the shape and not
the spelling:

1. `load_media("abc.7z")`, which answers with an **archive medium**,
   because an archive is a medium and its content is a namespace.
2. Take that namespace's files as a collection and `load_media` them —
   the second act, materializing the archive's contents into a floppy
   image through the same verb every other medium arrives through.
3. Get a disk back, reached the way every other medium is reached.
4. Save it as a P64 by naming the destination format.

**Step 1 is built. Steps 2, 3 and 4 are not.** `load_media` takes one
path and no collection; there is no disk kind a capture loads into, the
capture set being its own root outside the device model; and every write
verb the library has is format-specific and hangs on the root that
produced it. Steps 2 and 3 are the media-first fold F59 already pledges,
and are what U25 and U26 narrate one link earlier. Step 4 — **one verb
taking a destination format, rather than one verb per format** — is
pledged nowhere and is what U23 uniquely adds past them.

**Withdrawal was the honest move, not a rewrite in place.** The root
list is an implementation claim and a divergence from it is a bug, so an
entry whose journey the code performs *differently* cannot stay there by
having its prose adjusted to match what was built. That would make the
list describe the code instead of the code answering to the list, which
is the whole of what arming a use case means.

**Two clauses of the withdrawn entry do not survive into the pledge.**
Its claim that the reduction's every input is a named policy input the
caller states was never wholly true — the source-position-to-half-track
map has no policy field and is the profile's declaration (P30) — and the
media-first shape does not want it to be: U25 runs the reduction under
the profile's declared defaults, the caller growing their declaration
only where a family convention cannot decide. What the entry demands
past that stands unchanged: both accounts read before the write, loss in
the source's own terms, provenance that does not overstate itself,
determinism, and refusals that name the rule they broke.

**Consequence for F65.** The gap-first reconstruction's pledge ends
"the selected-observation reduction it succeeds retires with its
delivery", and that retirement was blocked while U23 was armed, because
the armed entry named the mastering profile as an owner. The block is
now lifted in kind but not in fact: pledged U23's *body* still names it,
so the retirement waits on U23 being rewritten around the four steps
above — where the reduction's policy is the profile's, not the
caller's — rather than on nothing at all.

**Step 2 does not fold into step 1, and the reason is structural.**
Step 1's call is already spoken for: it answers with the archive,
because an archive is a medium in its own right. Making the same verb
sometimes answer with the disk inside instead would give one call two
answers chosen by inspecting content, which is the discovery the
declared tier exists to keep out. Materializing a floppy out of an
archive's contents is a second act because it is one, and the caller
taking it is the caller declaring what they have.

**Weighed and declined:** amending U23 in place to describe the built
surface (see above — it inverts what the root list is for); leaving it
armed and treating the mismatch as prose to be clarified (the journey
differs in its shape, not its wording, and no clarification reaches
that); splitting step 4 out as a use case of its own (it has no journey
without steps 2 and 3, and a use case that cannot be walked is not one);
and folding step 2 into step 1 (above — the verb is already spoken for,
and overloading it buys one call at the cost of the rule that the caller
declares and the library checks).

### D27 — Rulings made delivering the uniform archive open

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; the U2 amendment, the P14
amendment, the P19 amendment; in-force P7, P12, P13, P19, P27.

Rulings made folding the archive journey into the storage model. The
delivery itself is recorded by the commit; these are the calls made in
its course, including the two the feature existed to settle.

**The archive slot is visible in its machine's attachment namespace.**
The alternative — a virtual slot kept behind the report — was weighed
and rejected for D23's reason one tier down: it would make the archive
the one device kind a caller cannot see, paid for at every seam that
lists devices, and it buys nothing. What the restriction would have been
written for is already handled by the receiver's own answer: it records
no device type, so anything reasoning by family passes it over without a
rule about archives. `arc0` is an ordinary attachment identity.

**The backing relationship is settled by what the child holds, not by an
outliving rule.** A stored entry is source-backed through the archive's
own claim and a coded one is session-backed in private session storage
(P27) — the delivered split, unchanged. What this feature adds is that
the child *holds* its backing: the claim or the spool is refcounted into
the medium loaded from it, so ejecting the archive, or removing its
device altogether, takes nothing away from a disk already loaded. The
draft's "that machine must outlive the child" is therefore not a rule
the code needs, and stating it would have described a constraint the
implementation does not have.

**The two vantages are two states, not one state with empty fields.**
P14's "families own their representation" is applied at the state tier:
a space-native medium and a namespace-native one are separate kinds
behind one `MediumState`, and every verb that addresses a space passes
through one accessor that refuses on the other **by name** — naming the
vantage, not failing further in. That is what made the archive medium
additive rather than a rewrite of the block state, and it is why an
archive reports no phantom volume (D26) without anything having to
suppress one.

**A path names a file.** The `archive[/entry]` syntax is gone from the
medium journey: an entry is reached through the namespace its archive
bears and loaded from the file view that names it, which is the same
journey every other medium takes. Two consequences were accepted
deliberately. Loading a one-entry archive no longer silently opens the
entry — the old convenience guessed, and the namespace asks instead. And
the ambiguity refusal for a many-membered archive is gone with the guess
that needed it.

**The capture-set adapter keeps `captures.7z/subtree`.** That spelling
names a subtree of *members* read as one logical artifact — one disk per
stream per head per step position — not one medium named inside an
archive, and the flux family reaches it through its own type as P13
requires. It is not the syntax this feature retired.

**The file-view load lands as `File::discover`, and the claim it mints
from is bounded.** D24 deferred the third load form to whichever feature
minted the view; this is it. It answers with the delivered consumable
`Discovery`, so the nested artifact travels through exactly the path
`load_discovery` already served — no second load form, and the claim is
the archive's own, held continuously from naming the entry to loading
it (P7). The claim is stated as an **archive entry**: a file on a
volume-backed filesystem is refused by name, because backing a medium
from a cluster chain is new capability rather than a spelling, and P3
would rather refuse than half-answer.

**In-force P19's serialized-artifact provider form dissolves**, which is
what D25 deferred to here. A medium may bear its namespace directly —
an archive, a flat catalog on an unpartitioned disk — its grammar being
a P12 adapter at that seam. *[The clause that followed, holding the
composer's three constraints in P19 pending P35, is overtaken by D57:
the composer and P35 are both struck, and P19 states the question as
outside the claim.]*

**Weighed and declined:** recognizing an archive by its leading bytes
rather than by the extension its grammar answers to (a ZIP's signature
sits behind whatever stub precedes it, so signature recognition would
refuse the self-extracting archives the catalog reads today); giving
`identify` an archive-medium *media* layer beside the grammar layer (the
layer kind for a medium is spelled `physical-media`, and a virtual
medium has no physical anything — the media type is answered where it
belongs, on the handle); and keeping `Archive` as a read-only listing
beside the medium (two ways to walk one namespace is the second
interface the one-node claim exists to refuse).

### D26 — Volume and filesystem are two traits on one object, not two types

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; P17, P18, P19.

A ruling on the shape F48 delivered, made while weighing whether the
filesystem node could be embedded in the volume as the medium was
embedded in the device.

**The type merge was refused first, and for a good reason.** Embedding
`Filesystem` into `Volume` fails the test the device/medium merge passed.
That merge worked because no caller ever holds a medium outside a device;
this one breaks because two of the three providers have no volume at all —
an archive's content is a namespace with no space beneath it, and a
machine's composed namespace is assembled over several filesystems. A
merged type would have had to invent a phantom volume for each, which is
the invention refused when a zip's byte extent was ruled *encoding* rather
than a model space. The reverse merge fails too: absorbing volume into
filesystem makes swap and unformatted space unrepresentable, and P19's
honest absence is the 0 in the 0..1.

**Traits dissolve what the type merge could not.** One object,
`StorageSpace`, implementing addressable I/O, namespace I/O, or both,
carries every case without a phantom in either direction — and it is the
rule already applied one level up, where a device's vantages are
capability traits a family implements as it claims rather than a
hierarchy. What the prose asserted — one node, two vantages — the type
system now carries, and the 0..1 becomes trait presence with F48's
delivered `no-namespace` refusal already fitting.

**The capability, not the tidiness, is why it is a feature.** Addressed
reads today are whole-medium only; a volume's boot sector, its
unallocated extents, and the bytes behind a listed file all require
computing offsets against the medium by hand. The addressable trait
closes that on the object that already hands over the files.

**An earlier flag was withdrawn.** It had been recorded that F48 should
have spelled selection `device.filesystem(volume_id)`, on the ground that
the handle rule makes volumes values rather than handles. That reasoning
assumed a single device: a volume spanning partitions across several
devices is not a selector on any one of them, so it needs a handle. F48's
shape was right, and the gap it leaves is scope rather than handle-ness.

**The machine is the scope for anything spanning devices, and that stays
unpledged.** A composition is reached from the smallest scope that can
compose it — a namespace on one device's medium from that device, a
volume spanning that device's partitions from the device, a volume
spanning devices from the machine. *[D57 struck P35, so the
namespace-composed-over-several-filesystems case named here no longer
exists; the scope rule stands for whatever future composition does.]*
The rule is recorded here because it settles where future compositions hang;
the surface is not pledged, because multi-device volume composition is
not claimed (P17 defers it, U14 is proposed), and a machine-level
enumeration added today would flatten over devices without delivering a
capability — surface ahead of demand.

**Two boundary readings settled in passing.** A multi-partition volume
with no filesystem is ordinary and real — an LVM logical volume formatted
as swap, a spanned volume never formatted, raw database extents across
several disks — so the 0..1 needs no special case at the composed level.
A LaserDisc's analog program is **not** such a case: it is not a volume
at all, frames and time codes addressing program content rather than
storage. The test is whether it is an addressable space of the kind a
filesystem could occupy, which swap is and an analog program is not. One
disc can be both, since LV-ROM digital data carried in a program-channel
mapping is a genuine addressed extent that may bear a filesystem
(proposed F22). The 0 in 0..1 is for a space that could bear a namespace
and does not — never for content that was never a space.

**Weighed and declined:** merging the types in either direction (above);
naming the object `Volume` and accepting the stretch (an archive's
namespace typed as a volume undoes the strictness the vocabulary rulings
bought); leaving it as two types with the hop (the prose would keep
claiming one node while the surface showed two, and the addressable
capability would still be missing); and pledging the machine-scope
surface now (above).

**Folded into:** F52 in [pledged/FEATURES.md](pledged/FEATURES.md) and
the storage model design.

### D25 — The namespace node lands whole; the P19 amendment lands as far as the code honors it

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; the U2 amendment, the P19
amendment, P35; in-force P10, P18, P19, P23, P27.

Rulings made delivering the `Filesystem` node and the container
retirement it pays. The delivery itself is recorded by the commit; these
are the calls made in its course, and the first is the one a later
reader most needs.

*[Overtaken in part by D57: the composer half of the amendment is moot,
its subject having left the claim with P35. The archive half landed at
D27 and the reading below stands for it.]*

**The P19 amendment could not fully arm, and landed in the part that
could.** The amendment says P19 keeps the convergence claim and loses two
things: the "serialized-container adapter" provider form, because an
archive is a medium whose grammar is a P12 adapter at the namespace seam;
and the namespace-mapping composer, which moves to P35. Neither loss is
available yet. An archive is *not* a medium until the uniform archive
open lands, so deleting that provider form now would unbind the journey
the code actually takes today; and P35 is pledged, not armed, so moving
the composer's three constraints out of P19 would delete from the
in-force list a rule the code implements and honors, leaving it bound by
nothing. **The root lists are implementation claims**, so what landed is
what the code honors: the retitle, the retired word purged from P19 and
from P23's active-layer row, and the amendment's positive claim — one
file-access interface however reached, with file access living on one
node and nowhere else. The rest is F49's, which is why that feature now
says so. The pledged scope-of-claim amendment (the coverage account) is
untouched and unclaimed: the delivered node produces no account, and
nothing here says it does.

**"Container" was retired into three different words, because it was
doing three different jobs.** At the P19 seam it becomes **namespace**,
which is the vocabulary ruling's own word and the one the node is named
for. In an identification it becomes a **layer** of the artifact's
nesting — `Layer`, `LayerKind`, `Identification.layers`,
`remanence_layer_*` — with the doc comments saying in as many words that
this is a different axis from P13's authoritative layer and P23's active
layer, because two disjoint enumerations sharing a word is the ambiguity
the retirement exists to end. On a region role it becomes **structure**,
an extended partition being a structural region. And on
`Error::InvalidImage` the `container` field becomes `format`, which is
what it always held: the seam a refusal is attributed to. What survives
is the word where it is somebody else's — an *image container format* is
the industry's term for qcow2, VDI and P64 — and the surviving uses were
audited one by one rather than swept.

**A `Volume` handle exists, and it is still not a thing to hold.** The
storage model rules volumes values, passed as selectors and never held;
the feature's own spelling is `device.volume(id).filesystem()`. Both are
satisfied by a borrowed selector: `Volume` carries the identity the
report issued and the extent, borrows the device it came from, and cannot
outlive it. It accepts no ordinal, because no format defines one.

**An entry declares what the node's vocabulary has no field for.** In-force
U2 claims the real names, sizes, **dates and flags** of an HDOS catalog,
and the node's common `Entry` names only the first two. Rather than keep a
second file type beside it — which is the "one file-access interface"
claim abandoned at the first filesystem that records more than three
facts — an entry carries `EntryFact`s in the recognizing filesystem's own
spelling and order: HDOS declares its catalog date, its flag letters, its
sector count and the raw values behind the readings. This is the flux
layer's two-outcome rule at the node's surface, and it is why the
standalone HDOS reader could be deleted rather than kept.

**"And nowhere else" reached the free functions too.** `list_hdos_files`,
`read_hdos_file` and `HdosFile` took a byte slice and belonged to no
node; keeping them would have left a second way to walk a namespace
outside the type that claims to be the only one. They are deleted from
all three surfaces and the reader is private behind the node.

**The recognizing adapter opens what it recognized.** The resolver needs
to reach a namespace a medium bears directly, and the obvious route —
read the filesystem id the catalog returns and `match` on it — is the
string-named rule in orchestration that P12 and P18 keep out. So the
catalog's adapters gained an `open`, whose default is a refusal naming a
namespace this release recognizes and does not read; CP/M is that case
today. **The lookup is bounded** by the byte count the HDOS reader
already declared, said once for the seam: a medium composing no volume
and larger than it is a named absence rather than a full scan of a
gigabyte (P27).

**A refused recognition answers with its own refusal.** Where a volume
composed and its filesystem seam attempted a recognition and refused, the
node hands that seam's error back — category and rule intact — instead of
a coarser "bears no filesystem" of its own. The seam that owns the
refusal already carries what explains it (P4, P10), and replacing it
would tell a caller less than the inspection report already holds.

**Weighed and declined:** landing the P19 amendment whole and arming a
narrowed P35 in the same act (P35's own claim is the machine namespace,
which nothing builds yet, so arming it would assert a node that does not
exist — and narrowing a principle at the moment of arming is a bigger
ruling than this feature's course); leaving the composer's clauses in P19
with a note that P35 will take them (a pointer to unbuilt work inside an
in-force principle is planning prose in the one place the project keeps
free of it); keeping `HdosFile` beside `Entry` so U2's dates and flags
had a typed home (two entry types is two interfaces, and the declared-fact
route was already the project's answer for a fact with no named field);
naming the identification's records `Recognition` (taken by the drive-profile
seam) or leaving them `Container` for F49 to rename (F48 names the C symbols
explicitly, and renaming the C mirror while the Rust type kept the retired
word would split one vocabulary across two surfaces); and giving the
resolver a whole-medium scan with no bound, which is the P27 violation the
HDOS reader's existing bound was written to prevent.

**Reopens if:** nothing. D57 struck P35 and the composer both, so the
first ruling's subject is gone; what survives here is the "container"
vocabulary split and the one-node file access, which stand.

### D24 — The file-view load waits for the node that mints the view

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; in-force P7, P19.
*[Its P19-amendment and P35 citations are spent: D57 struck both.]*

F51 said `load_media` accepts "a path, a file view, or a discovery", and
two of the three landed with it. A **file view** is not something this
release has: the `Filesystem` node that mints one belongs to F48 and the
recursion journey that reaches an artifact through it belongs to F49, so
delivering the third form inside F51 would have meant minting another
feature's node to have an argument type for it.

Nothing a caller was promised is missing meanwhile. The path form
already carries the nested artifact — `archive/entry` resolves under the
same claim and loads into a device of its own — so what is deferred is
the *typed* spelling of a journey that works, and it lands with the type
rather than ahead of it.

This is recorded because F51's number retires with its delivery: without
an entry the deferral would leave no trace at all, and the next reader of
F48 or F49 would have nothing telling them the form is theirs to finish.

*Annotation (D35, 2026-08-12): the deferral is spent. F59 delivered the
typed spelling — a `File` from another medium's namespace, and a
collection of them, as `load_media` source shapes — landing with the
type exactly as this entry said it would.*

### D23 — Rulings made pledging the storage model

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; P7, P14, P19, P27, P32.
*[Its P35 citation is spent: D57 struck that principle.]*

The promotion itself is recorded by the commit that moved the documents,
not here. These are the rulings made in its course, which the moved text
settles only by being written that way.

**The pledge is scoped to the families claimed today.** The model
describes shapes other families would take, and those illustrations
pledge nothing: optical and tape media (proposed P24, P26) and volumes
composed across several regions (P17's deferred future) stay proposed,
and the design document says so in its own scope paragraph. The trim was
not cosmetic — a pledged item resting on a proposed one is pledged too
early, so the media-kind table, the two trees, and the spine's volume
line were narrowed until nothing pledged depended on anything proposed.

**F47 was split at the pledge and its number retired.** The sprint bound
bites at the pledge, and one feature carrying the two-act access path,
concrete device families, discovery, format-declared defaults, and the
one-step convenience was two. It became **F50** (the two acts and the
lineage-bearing family catalog) and **F51** (discovery, the declared
default, and the convenience over it), F51 needing F50. README's rule
for features governs: a split retires the parent's number and issues
fresh ones.

**The CHS/LBA clause was wrong and is struck.** The draft read "CHS and
LBA hard drives are separate partitionable *families*", which
contradicts the already-pledged P32 amendment: a device declares an
**addressing nature** when it is created, and that amendment
*deliberately declines* to confine a family to one nature, because a
hard drive answers both depending on the command issued. The owner's
own words had been "separate partitionable **devices**" — the pledged
mechanism exactly — and the drafting drifted. Both the new amendment and
the design document now defer to the pledged one instead of restating a
rival rule.

**One storage handle, two model nodes.** The device/medium split was
argued three times and survives as *model*, because U23 and D19's three
facts and U24's flippy each need two nodes. It does not
survive as two *handles*: a caller never holds a medium outside a device
— discovery returns a discovery, every load goes into a device, a child
artifact gets its own device — so `Disk` merges into `StorageDevice`,
which homes the media state of whatever occupies it. The facts stay
attributed on the one handle, which is what keeps D19's pair sayable:
the medium states an index hole, the drive states no sensor for it.
`get_sector`-on-device-or-medium, an open question at the time, dissolves
rather than being answered — evidence the seam was an artifact of two
handles rather than of the model.

**A session holds machines; a machine holds devices.** Pledged P32 made
the session the device set. Nesting broke that: an archive on the host
was never part of the machine whose disk it contains, so reconstructing
that machine from `games.zip/boot.h8d` wants an archive device in one
machine and a drive in another. Inserting the machine lets each device
set hold only its own machine's configuration, while the session keeps
the meaning the principles already give it (P7 claims, P27 budget and
private storage) and owns every machine's lifetime, so a stored archive
entry may back a drive elsewhere in the same session without a lifetime
question. P32's "nothing groups sessions into a machine" is
untouched: the containment runs the other way.

*[Overtaken by D58, which withdrew the machine tier: the session is
the device scope again, so there is no anonymous machine to be the same
kind of thing as a named one. What survives is the ruling the paragraph
was written to protect — devices are added to a scope that already
exists, never to one conjured per call, so the unanswerable "which
device?" that killed the media-first one-step does not arise.]*

**Every verb a named machine answers, the anonymous one answers too.**
Restricting it was weighed and rejected: it buys
nothing, and it would make the anonymous machine the one that behaves
unlike every other. A
caller who adds two unrelated floppies gets a
deterministic answer stating exactly what produced it — surprising to a
naive caller, perhaps, and never dishonest. The archive case such a
restriction would have been written for is handled one level down by
family: an archive device has no partitions or volumes, so an assignment
rule never reaches it, and no machine-level rule was ever doing that
work. Uniformity is the other half — a restriction would have made the
anonymous machine behave unlike every other, a special case paid for at
each seam that touches machines. What survives as description rather
than rule is the usage: **the anonymous machine is where artifacts are
opened, a named machine where one is reconstructed.**

**Further conveniences are deliberately not pledged.** The explicit walk
is what the model owes past the anonymous machine, which is a structural
rule rather than a shortcut. The room is real — a default machine for a
single-machine session, a filesystem straight from a session — and each
is its own later proposal, weighed as the machine-level one-step was:
admissible where it declares, refused where it would guess. The
media-first machine-level spelling is dropped rather than kept, since
with one storage handle it would return the same device its device-first
twin does.

**Weighed and declined:** pledging the model whole, illustrations
included (it would have made pledged text rest on proposed principles);
keeping `Disk` as a `Medium` type beside the device (no journey produces
one, and the delegation it would require is the merge with extra
ceremony); renaming `Session` to `Machine` as the earlier draft had it
(the nesting case needs both words, and both already carry their meaning
in P7 and P27); loading a nested entry into a named machine
that also holds the host's archive (it would put a host-side wrapper in
an emulated machine's configuration and hand anything reading that set a slot to
letter); an anonymous machine forbidden to compose a namespace
(provenance already states what a mapping was derived from, and the
restriction would have made one machine behave unlike the rest); and
giving the anonymous machine a reserved identity rather than a null one
(a name nobody chose, citable in provenance as though it had been
declared).

**Reopens if:** a claimed family needs a medium handle no device holds —
the mastering path is the candidate, since a `MasteredMedium` is a medium
in no device today, and this ruling deliberately leaves the flux handles
where they are.

**Annotated on delivery (F53, 2026-08-10): the "one storage handle"
ruling above is reversed, as the media-first storage model's ledger said
it would be.** The medium is now the pool-owned handle a caller holds and
every content verb answers on; the device slims to a slot, its family and
a link, with `insert`/`eject` the one edge between configuration and
state. D23's actual worry — lifetime questions from media held outside
the session — is answered structurally by the pool rather than by
refusing to hand a medium out. The rest of this entry stands: the machine
tier, the anonymous machine, and the reasons the device/medium split
survives as *model* are untouched, and `Disk`'s merge into
`StorageDevice` was the step that made this one sayable.

### D22 — P27 splits: the resource rule keeps the title, thread invisibility becomes P34

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** P2, P23, P27, P34.

The last of the three principles D20 reported as resisting compression,
and the same test D21 applied: two rules under one heading that fail
independently are two principles.

**The two rules, and the test.** The resource rule — sized by the
operation, the bounded working set, source-backed and session-backed
state, the residency classes, streaming everywhere — makes the testable
claim *peak memory bounded independently of source size*. The
concurrency rule — threads may predict, prefetch, and offload under four
invisibility rules — makes the testable claim *results, evidence, and
refusals identical at any thread count, including none*. A whole image
loaded resident with zero threads violates the first and honors the
second; a failed speculative read that reports its error violates the
second while memory stays bounded. Neither implies the other.

**The second rule is a determinism claim, not a memory claim.** It is
the rule any future concurrency must obey — parallel decode, deriving
layer extents ahead of demand — and P23's cache tie already cited it as
a distinct thing ("under P27's speculation rules"). Keeping it inside a
principle titled "memory holds a bounded working set" made the
determinism obligation citable only through a resource principle whose
title says nothing about it.

**P27 keeps the resource rule and its number; the concurrency rule is
P34.** The title match settled the direction: "sessions stream; memory
holds a bounded working set" describes the resource rule exactly. Of the
84 code citations of P27, nearly all are resource-half; the concurrency
sites are concentrated — the offload worker and speculative install in
`cache.rs`, the predictive reader in `source.rs` and `disk.rs` — and
were widened to P34 in the same change. The budget stays P27's, and
P34's demand rule spends it: a cross-reference, exactly as P23's caches
sit under P27's budget.

**Corrected in passing:** D21 stated that three P23 citations sat "in
released CHANGELOG entries". Every P-number citation in the changelog
sits under `Unreleased` today — the released headings hold none — so
that leg of D21's numbering argument was factually wrong. The ruling
stands on its other legs (77 citations across 21 files, the code's
citations being the state half), and per this record's own rule the D21
entry keeps its spelling; this note is the discovery.

**Weighed and declined:** leaving P27 whole (defensible as "the threads
exist only to spend the budget", but that reading makes the
determinism claim a sub-clause of a resource principle, and the two
claims are independently violable and independently testable); the
subsection compromise — one number, two subheadings — (cheaper, but
leaves two independently violable claims citable only as one number,
which is the ambiguity the P-sequence exists to prevent).

**Landed as:** P27 515 → ~370 words with a two-line pointer to P34; new
P34 at ~200 words in root [ARCHITECTURE.md](../ARCHITECTURE.md); four
comment citations widened in `cache.rs`, `source.rs`, and `disk.rs`; a
CHANGELOG entry under Unreleased beside the P23 one. SEQUENCES advances
P to 35 and D to 23. No S1–S3 surface is touched: the split renumbers
rules without changing what any of them claims.

**Reopens if:** either half is found to have lost a binding clause — the
clause returns, since this ruling moved rules without changing them.

### D21 — P23 splits into what an active layer is and how it changes; generate-flux was P29 all along

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** P13, P22, P23, P27, P29, P30, P33.

The first of the three principles D20 reported as resisting compression.
P23 was 2101 words before that pass and 1147 after, which is what a
principle looks like when it is two.

**Two rules were sharing a heading, and they fail independently.** What an
active layer *is* — one per independently mutable instance, the closed
six-member vocabulary, what cannot be one, one layer per instance in a
nested graph — is violated by two mutable copies of one state, or by a
session active at a representation outside the vocabulary. How an instance
*moves* — the ladder, the initial choice, materializing downward,
atomicity, no return — is violated by lowering an LBA device, by a partial
descent, or by the layer rising when a lower presentation closes. Neither
failure implies the other, which is the test that made this a split rather
than a trim.

**P23 keeps the state half and its number; the transition half is P33.**
The direction was settled by evidence, not preference. P23 has 77
citations across 21 files, and the code's are almost entirely the state
half — "the layer active for this composition" in `report.rs`, in the
generated C header and its Rust origin, and the module headers of
`hardware_bitstream.rs` and `encoded_bytestream.rs`. Three of the
citations are in released CHANGELOG.md entries, which AGENTS.md forbids
editing. Retiring P23 and issuing two fresh numbers — the rule README
states for *features*, whose handles evaporate on delivery — would have
orphaned all of it, and vision handles are permanent for exactly this
reason. Only `c1541_presentation.rs` and its integration test needed
their citations widened, and both were already citing P30 beside P23.

**Generate-flux was P29 restated, and D14 had already said so.** P23's
four generate-flux bullets map one-to-one onto P29: "ambiguity remains
ambiguity unless an explicit deterministic policy" and "a missing or
contradictory rule refuses" are both P29's *a reduction that no policy
names is a refusal, not a default*; "only detail absent from the source is
synthesized, with its provenance retained" is *the result is derived and
says so*; and "every known timing preserved at its known fidelity" is the
declared-loss account. D14 had already ruled that mastering's destination
"may be a new artifact or an active layer inside the session… Only the
destination differs; the inputs, the plan, and the declared-loss account
are the same." So the bullets were deleted and P33 cites P29 instead.

**P29 widened to match what it was already governing.** Its opening said
mastering derives "a new artifact"; it now derives a new *representation*,
with the destination named as the only thing that varies. "The loss is
declared before the write" becomes "before anything is produced", and the
interruption clause reads "a complete destination or none", because an
in-session destination is written to no file. No requirement of P29
changed — the principle simply stopped describing only half of its own
scope.

**Why this was invisible to the D20 pass.** The restatement was wearing
bullets. D20's rule catches a principle that re-explains a neighbour in
prose; it did not catch one that restates a neighbour's requirements as an
apparently operational checklist. That is the shape to look for in the two
principles still outstanding.

**Weighed and declined:** retiring P23 and issuing P33 and P34 to the two
halves (orphans 77 citations, three of them in immutable released
changelog text, to buy a symmetry nothing needs); giving the transition
half to P13 (P13 governs what the *artifact* records and can persist,
P33 what a *session* currently carries — the same distinction P23's own
authoritative-versus-active paragraph draws, and collapsing it here would
undo that); folding the transition half entirely into P29 rather than
issuing P33 (mastering governs the honesty of a descent, but not the
ladder, the initial-layer choice, the atomicity of rebinding, or the
one-way rule, none of which are reductions); and keeping the generate-flux
bullets under P33 as operational detail (that is the duplication D20 just
ruled against, and D14 had already collapsed the distinction they rested
on).

**Landed as:** P23 1147 → 698 words, new P33 at 482, P29 442 → 482, in
root [ARCHITECTURE.md](../ARCHITECTURE.md); citations widened in
`c1541_presentation.rs` and its integration test; a CHANGELOG entry under
Unreleased, because P-numbers appear in released entries and a reader
reconciling them needs to know P23 narrowed. SEQUENCES advances P to 34
and D to 22. No S1–S3 surface is touched: the split renumbers rules
without changing what any of them claims, and the C header's own P23
citation stays correct.

**Also corrected in passing:** `c1541_presentation.rs` described a
container as holding a medium "at rest", a term D2 retired from
library-side prose. The line was being edited for its citation anyway.

**Reopens if:** P23 at 698 words is found to be two rules again — the
vocabulary table is a fifth of it, and the remainder is one claim.

### D20 — A principle in force states the rule; the argument lives here

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** (none) — a form ruling about how the in-force
list is written. No numbered vision entry demands it, and it changes no
claim any of them makes.

**The observation that prompted it.** The nine principles written first
average 127 words. Everything from P10 onward averages 540, and P23 had
reached 2101 — an entire treatise per rule. The list had stopped being a
list of rules and become an essay collection, which makes it unreadable
as the thing it exists to be: the place a triage decision looks up what
binds.

**The rule adopted.** A principle in force states its claim, what it
binds, and **at most one line of why**. That is not an aesthetic
preference; it is the shape the first nine already had, and P5 is the
proof that a real principle survives it in 38 words.

**Three things a principle no longer carries, each because it has a home
that will not drift:**

- **The argument that settled it** — this file. Most of what was removed
  was already *here*, in duplicate: P30's "an angle, never a byte" was
  D12's sentence verbatim, and P23's capture/medium reasoning was D14's.
  A second copy in the norm is not a safeguard, it is a second thing to
  keep in step. Where the argument is worth finding, the principle now
  cites the D-number.
- **The enumerated sets a claim ranges over** — the code, which is the
  norm ([ARCHITECTURE.md](../ARCHITECTURE.md) "The application
  surfaces" says so). P10 listed its ten error categories and the DOS
  8.3 rule identities; P28 listed its conditions and which
  interpretation the gate is armed for; P14 listed the enrolled media
  types. Each principle keeps the rule that the set is enumerated, owned
  by its seam, and part of that seam's surface. What it drops is the
  transcript, which could only ever be a stale copy of an enum.
- **A restatement of a neighbouring principle** — a cross-reference.
  P23 re-explained P13, P22 and P27 before reaching its own subject.

**What was deliberately not cut.** Every normative clause, including the
ones wearing rhetorical clothes. The one class nearly lost on the first
pass was the **surface limit** buried at the end of each "Knock-on
requirements" section — "creates no public evidence iterator", "the flux
floor is not a public interface", "adds no multi-device opening". Those
sections were otherwise cross-references, and the limits were restored to
the body of P21, P22, P29 and P30 once the review caught them. A negative
claim about the surface is as binding as a positive one and is easier to
delete by accident.

**Planning prose is untouched, deliberately.** Under `planning/`,
precision and accuracy outrank brevity: an argument takes the length the
argument takes, and this file is the clearest case. The rule bites only
where a principle is *in force*, because that is where prose stops being
an argument and becomes the thing the code is measured against.

**Three principles resisted compression, and were reported rather than
re-cut.** They are the finding, not a failure of the pass, and each is a
candidate split rather than a candidate trim:

- **P23 (2101 → 1147 words)** reads as three rules sharing a heading: one
  active layer per independently mutable instance and the vocabulary it
  ranges over; how the initial layer is chosen and how a transition
  between layers behaves (generate-flux and its atomicity); and the
  layer/cache tie, which was a restatement of P27 and has now been folded
  into P27 where it belongs. The first two are separable and were not
  separated here.
- **P19 (805 → 543)** carries the file-container seam and, bolted to it,
  the namespace-mapping composer with its own three constraints. The
  composer derives a mapping where a system persisted none, which is a
  different act from exposing a namespace.
- **P27 (602 → 515)** carries a resource rule and a concurrency rule. The
  four rules that keep threading observationally invisible are a
  self-contained claim about behavior, not about size.

**Weighed and declined:** splitting those three in the same act (the
splits are principle amendments in their own right and each deserves its
own argument, which is exactly what this pass is trying to stop being
smuggled into an edit); a companion `RATIONALE.md` beside the principles
(a third home for prose, competing with both the norm and this file, and
D11 already refused that shape for delivered designs); keeping the
enumerated sets with a "may be stale" caveat (a caveat on a copy is an
admission the copy should not exist); and compressing the pledged and
proposed principle drafts in the same pass (they are planning prose,
which the ruling above deliberately exempts — they take this shape when
they arm, not before).

**Landed as:** root [ARCHITECTURE.md](../ARCHITECTURE.md), 1172 → 839
lines, with the rule itself stated under "The architectural principles"
so the next entry is written to it. No S1–S3 surface is touched and no
principle changed what it claims, so no code, binding, test, or changelog
entry moves with it.

**Reopens if:** a principle is found to have lost a binding clause — in
which case the clause returns to the principle, since this ruling removed
argument, transcript and restatement only.

### D19 — A media profile holds what the article is, and nothing that was recorded on it

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** S1, S2, S3; P3, P12, P14, P22, P30, P32.

Rulings made in P14's course, which its own text settles only in
principle.

**The boundary is drawn at "recorded".** P14 says a profile holds
"passive compatibility facts", and the contested reading is what that
excludes. It excludes everything a recording put on the medium: density,
encoding, track and sector counts, and the geometry an image format
declares all stayed where they were, and what moved into the catalog is
the article — form factor, coercivity, track density, hole topology, and
the sense of the write-protect mechanism. The test applied was whether
the fact is true of a blank disk in its sleeve.

**How many surfaces the medium certifies is not declared at all**, and
this is the sharpest case of that test. It is a genuine passive fact, and
nothing the library ever holds establishes it: a capture records what one
head saw, an image records what was written, and neither says whether the
physical disk was sold certified for one side or two. Declaring it from
the drive's recorded-surface count would be the drive's fact wearing the
medium's name. So the medium is silent, and P30's `Surfaces { recorded }`
and the image format's `sides` continue to answer the question they
actually answer.

**"Hard disk" and "floppy" leave the medium's vocabulary.** They were the
image-format descriptor's `media_kind` string, and central identification
code `match`ed on them to choose a display name — a string-named rule in
orchestration, which is exactly what P12 keeps out of it. A virtual
disk's medium is logical-block media; *hard disk* is the device family a
session's slot carries under P32, and the P32 amendment's rule that a
device's addressing nature never reaches the media instance is the same
rule one step over. The public spelling follows: `media_kind` becomes
`media_type` and carries an enrolled identity rather than a word.

**A drive profile declares the medium its family is served**, which is
not the violation it can look like. P14 forbids a *media profile* from
containing hardware behavior; a drive declaring which article it accepts
is a compatibility fact of the family, the same class as its rotation or
its density map, and the catalog entry it points at knows nothing about
the drive. It is also the only honest source for a mastered medium's
media type: a capture does not record what disk it was, so the name comes
from the family's declaration with provenance, as every other P29 policy
input does.

**The seam is crate-private**, as the drive-profile seam beside it is. A
media type reaches a caller as the name a medium answers with — in an
identification's physical-media layer and in the layered report's device
record — and the facts stay behind that name until something outside the
library needs one. Nothing about P14 requires a public flexible-media
fact today, and publishing one would fix a schema on the strength of two
enrolled entries.

**Weighed and declined:** one universal media schema with per-family
fields left empty (P14 refuses it in as many words, and the two claimed
families share no fact — a coercivity is meaningless for a logical-block
medium and a block size for a disk); leaving the block family's medium
unnamed, as `media_kind: None` left a raw image (a medium that cannot say
what it is makes P14 conditional, and block-active state *is*
logical-block media — that much the authoritative layer establishes);
letting the caller declare a medium's type at attach (nothing in the
delivered stack needs it, and a caller-asserted fact would have to travel
as provenance rather than as a declaration, which is a P29-shaped
surface built ahead of a demand); enrolling 8-inch and 3.5-inch entries
to prove the family generalizes (unused declarations no test measures,
and the 8-inch write-protect sense is inverted from the 5.25-inch one,
which is exactly the kind of fact to declare when a format needs it and
not before); and holding the media type only on the image-format
descriptor rather than on the medium (a medium is state *between* image
formats and drives, so reaching through the format to ask what the
medium is inverts the direction the principle draws).

**Reopens if:** a claimed family needs a fact the delivered schema cannot
hold, or a caller is found needing a medium's facts rather than its name.

### D18 — A VDI parent is searched for by identity, because the format records no path

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** U6, S1; P3, P4, P6, P7.

A scope call made in F41's course, which its own text could not carry:
**the VDI format records the parent's identity and no path at all.** F41
was written as "names its parent by the parent's own identity rather than
by path alone", which reads as a path plus a check. There is no path. The
producing hypervisor resolves the identity through its own machine
registry — an XML document outside this library's claim, and one this
library will not acquire a reader for to open a disk image. So the choice
is not *how to check a resolved path* but *how to resolve at all*, and the
delivered answer is a **search by identity**.

**The search is bounded and named**: the directory holding the child, then
the directory above it — the layout this format's tooling produces, where
differencing images sit in a subdirectory of their own and the base image
stays in the folder above. In each, the file *named* for the identity is
nominated first, in both spellings the tooling writes it with, because
that is how a differencing image is named. Failing a nomination, the VDI
files beside it are examined and the one declaring the identity is the
parent.

**Nomination is checked, not trusted**, which is where F41's sentence
lands intact: a nominated file whose identity does not match is a refusal
rather than a fallback to searching, so a substitute standing where the
parent should be is never silently read in its place. Two matches in one
directory is a contradiction and refuses; none anywhere is the
missing-parent refusal, naming the identity looked for and every candidate
it could not examine (P4).

**A candidate that cannot be examined is not a failure of the open.** A
scanned file another process holds against the P7 claim, or one that is
not a VDI of the claimed major version, is recorded and passed over rather
than failing the chain — it was never established to be in the chain. A
*nominated* file is different: it is the parent by name, so contention on
it fails the open as P7 requires. Without that split, one unrelated locked
image in a directory would refuse an open that has nothing to do with it.

**Identity also replaces the path-visited cycle check** qcow2 uses. The
members' declared identities are what the chain carries, so a cycle is an
identity already in the chain — which catches an image naming itself as
squarely as it catches two naming each other, without canonicalizing a
path to find out.

**What this costs, stated rather than hidden:** a differencing image whose
parent sits outside those two directories does not open, and says so by
name. That is the missing-parent refusal F41 already enumerates, and the
alternative — widening the search until it finds something — is the
substitute this decision exists to refuse.

**Weighed and declined:** resolving only by the nominated name and never
searching (deterministic, and it cannot find a base image, which is named
by a person and not after its identity — it would have shipped a feature
that resolves snapshot-over-snapshot and fails the common case); reading
the producing hypervisor's machine registry to get a real path (a second
format, an XML reader in a crate that is deliberately dependency-free, and
a machine-configuration document this library has no claim over); taking
the parent's path from the caller through a new surface verb (it is
defensible, and it contradicts F41's "the top image opens" — the caller
would have to hold what the format was supposed to say); searching
recursively from the child (unbounded, and every directory added makes an
accidental identity collision likelier); and checking the parent's
modification stamp beside its identity, which the format also records
(it detects a parent changed since the branch, and F41 enumerates neither
it nor a refusal for it — a claim to widen deliberately, with the evidence
of a real chain it would have rejected, rather than in passing).

**Reopens if:** a VDI is found that records a parent path after all, or
the search is measured refusing a layout the format's own tooling
produces.

### D17 — A design document's purpose ends at delivery

**Decided** Paul Galbraith, 2026-08-04. **Supports** (none) — a records
ruling; no numbered vision entry demands it.

D11 held that a companion design survives the feature that carried it.
That is true of the *design* — the shape survives, embodied in the code,
which is why a handle can evaporate without losing anything. What it
authorized was the retention of a *document*, and that does not follow.
Its design-retention holding is overruled; the rest of D11 stands.

D11's positive argument — that the code implements the contract but is
not a readable statement of what a future provider must satisfy — names
a real need and puts it in the wrong place. Prose a future implementer
must satisfy is a normative specification, or it is a principle. What
the retention cost: nine further designs restated themselves as
permanent residents on a reason D11 never gave, that no S1–S3
specification has shipped, and one delivered design's caller-flow
example went on naming an entry point a later feature had retired.

**Weighed and declined:** narrowing D11 to its stated case, keeping a
design that touches no surface and states a contract for a future
provider (that need is a specification or a principle, and admitting
the category at all is what the nine claimed); an archive location for
delivered designs (a third mechanism, for content the code already
holds authoritatively); and maintaining each delivered design against
the code it describes (it sets prose to compete with the norm and
schedules the drift rather than ending it).

**Reopens if:** a delivered design is found to carry something that is
neither the code, a principle, nor a decision — that would name a gap
in those three rather than a reason to keep the document.

### D16 — NIB enters at the flux medium, with synthetic timings, to keep one ladder

**Decided** Paul Galbraith, 2026-08-03. **Supports** P13, P22, P23, P29, P31.

**NIB and NBZ enter the flux family, materializing into a flux medium whose
pulse timings are synthesized.** A pulse's position is computed from a bit
index and a declared cell width; nothing about it is recorded evidence, and the
flux medium's model already refuses to call it so, since every pulse names what
put it there. In-force P22 governs the rest unchanged — synthetic provenance is
retained, and protection, weak regions and timing evidence the source never
stored cannot be reproduced from it.

**What settles the rung is a characteristic, not a convenience.** The flux
layer's defining trait is that a rotational recording's start and stop are not
crisp — a disk has no natural beginning, its origin is given rather than found,
and the delivered medium already carries an origin statement saying which rule
located its circle, with the C1541 defaulting to the longest gap because that
drive never observes an index. One rung up the circle is crisp: a bitstream has
a definite cell count per revolution and a G64 writes each track's length down.
A NIB has the flux trait and not the bitstream one — a fixed window longer than
a revolution, overlapping itself, wrap nowhere recorded — so that is where it
enters, and the synthesized timings are the price of the placement rather than
the case for it.

**Corollary: manufactured transitions carry jitter, at half the family's
admissible reading deviation.** No drive writes at the tick, so pulses are not
placed at exact multiples of a declared cell; each is drawn seeded and recorded
as every other draw in this family is. The amount is derived from the profile's
existing reading band rather than declared as a second number, which says the
writing drive sat comfortably inside its own family's tolerance — and, more
usefully, makes a property checkable: every synthesized transition stays well
within the band that classifies it, so recovering a bitstream from a synthesized
medium returns exactly the bits that were synthesized. A round trip that could
lose a bit would make this whole placement unsafe.

Two constraints keep the factor honest. **Jitter is drawn on the interval, not
the absolute position** — two independently jittered positions put twice the
deviation into the interval between them, landing on the band edge and
misclassifying. And **the circle closes exactly**: jitter redistributes within a
revolution and never changes its total, so the wrap stays the one the reduction
declared rather than the sum of a random walk. Spindle speed variation is a
third thing, correlated across a revolution where these are per-transition, and
is left to its own declaration rather than folded in.

**The reason the placement was wanted is the hierarchy, and it is the owner's
call over an argued objection.** One entry point below one ladder keeps the model solid: everything
above the medium is then the ordinary route every other flux source takes —
read channel, bitstream, codec — instead of a second adapter shape entering
partway up. The artifact needs materialization either way, so materializing it
one rung lower costs a synthesis that is declared and buys a path that already
exists.

**What was weighed against it, and lost:** that a NIB records bits rather than
timings, so entering at flux asserts a content the file never held (D15, now
annotated). The objection is answered rather than dismissed — the timings are
declared synthetic at every pulse, so no claim of recorded evidence is made —
and the residue is accepted deliberately: the read channel will recover bits
from timings computed from bits, and the loop-point analysis a NIB needs does
not disappear by moving rungs, it moves with it. Both are the price of the
single ladder.

**G64 does not move with it.** It records its track lengths and positions, so it
is servable at the hardware bitstream as it stands, and the pledged P23
amendment already names it there as an image whose authoritative and initial
active layer are hardware bitstream. What sends NIB down is that it must be
materialized regardless; an artifact that need not be keeps its own rung.

**Read-only here is a capability, not a property of the rung.** No flux artifact
receives a write and no writable flux composition is claimed today, which is
what makes this placement simple — but that is each adapter's enumerated claim
under P3 and P13, and the project's current scope. In-force P22 continues to
say that a low-level composition claiming the physical path holds flux as
durable mutable state which receives modeled writes; it constrains work not yet
done, exactly as it did when it was armed. Nothing in this entry narrows it, and
a later write path — a modified medium encoded to a new artifact — needs no
amendment to arrive.

### D15 — A capture-form artifact is sorted by servability, not writability

> **Partly overruled by D16**, which moves NIB and NBZ into the flux family
> with synthesized timings. The first ruling below — that a capture-form
> artifact is not placed at a rung whose content it does not record — no longer
> binds for that class; the entry rung is a family's declared convention, and
> the honesty it protected is carried instead by declaring the synthesis at
> every pulse. **The second ruling stands unchanged**: servability, not
> writability, is what sorts the two modalities. Kept as written, per this
> record's rule that an entry only partly overruled is annotated rather than
> rewritten.

**Decided** Paul Galbraith, 2026-08-03. **Supports** P13, P22, P23, P27, P29,
P31.

Two rulings made while pledging P31, neither of which that principle's own
text would otherwise carry.

**NIB stays at the hardware bitstream and is not moved down to flux.** It was
weighed: the format records no track length, so a reader must analyse the
stream before it can serve a circle, and needing analysis before use reads
like raw evidence. It is not. P13's authoritative layer states what an
artifact actually records, and a NIB records bits a drive's read channel had
already recovered — placing it at flux would assert transition timings the
file has never held, which is the false provenance claim the P23 amendment
refuses from the other direction when it rules that generate-flux is
generate-medium. **Needing a reduction is what the modality is, and says
nothing about the content.** Where a composition genuinely wants a flux floor
beneath a NIB, that is the ordinary generate-flux transition, carrying
synthetic provenance and unable to reproduce evidence the source never stored.

**Writability sorts nothing, and the first cut at this used it.** The
distinction was initially drawn as G64 writable against NIB read-only, which
is wrong twice over: no artifact in this family is a writable backing, P64 and
G64 included, because writes land in the active layer and an artifact appears
only by an explicit encode building a new file. The axis is **servability** —
whether a session can truthfully serve one location by key from the file as it
stands, under P27's source-backed residence. That puts P64 and G64 on one side
and a stream set and a NIB on the other, which is the line that was actually
meant.

**Weighed and declined:** a new active-layer row for capture-form artifacts
(they carry no session's mutable truth at any rung, exactly as flux capture
carries none, so a row would have to be a row nothing is ever active at);
amending in-force P22's two-model clause to cover every rung (the clause is
scoped to the flux family where the models were found, and it is true as
written — generalizing the shape does not require rewriting the place it was
discovered).

### D14 — The flux family holds two models, and only the medium is ever active

**Decided** Paul Galbraith, 2026-08-03. **Supports** P13, P22, P23, P27, P29,
P30.

Rulings made while pledging the flux capture / flux medium split.

**One word was doing two jobs, and P22 already said so.** It reads that a
capture adapter may preserve several revolutions "while a normalized media
model may define one circular revolution" — two models, one name. They are
now **flux capture** and **flux medium**, and the boundary between them is a
test rather than a taxonomy: **disagreement across observations is a capture
fact, and strength is a medium fact.** A capture records that three passes
differed; a medium records that a pulse is weak; the conversion is a P29
reduction performed by neither model unasked.

**The medium is not a tidier capture.** What it adds — the rotational frame,
the family's addressing, the reference clock, the strength vocabulary, and
which surface is the disk — is absent from the flux and declared by a P30
profile. The measurement that settled it: the fixture was captured at 359.8
RPM on a 360 RPM instrument, and nothing in the flux knows a 1541 spins at
300. The medium is where declared knowledge and recorded evidence combine.

**Flux capture takes no active-layer row, for a concrete reason.** A drive
writing to a capture would have to choose which of several disagreeing
observations to overwrite, and no answer to that is better than another. It
stays authoritative image state under P13, read by inspection and by
mastering. P23's rule is scoped to independently mutable instances, and a
capture set opened to be inspected and mastered is not one.

**Capture becomes medium by mastering, not by lowering**, with the same
declared inputs whether the destination is a new artifact or an in-session
active layer. That supplies the mechanism the pledged P15 clause assumes
when it says a drive's floor may be "timed flux for a P64 or a raw capture":
a capture becomes a floor by being mastered under declared policy, never by
a normalization nobody named. For the same reason **generate-flux is
generate-medium** — fabricating instrument evidence from sectors would be a
false provenance claim in the clause most concerned with honest provenance.

**F30 is renamed, not split.** Its content was already entirely the capture
model, so nothing of it becomes the medium and its handle survives; the
medium takes F37. README's split rule reaches a feature cut into pieces, not
one whose subject is renamed.

**The promotion was compressed with the retarget.** Renaming pledged F30 and
retargeting pledged F33 and F34 cannot be done while the vocabulary they
would use exists only in `proposed/`, since a pledged item resting on a
proposed one is pledged too early. The amendments were therefore promoted in
the same act rather than the retarget being deferred.

**Weighed and declined:** one `FluxLayer` carrying both models behind a mode
discriminant — D9 already declined a kitchen-sink union record at this exact
layer, and this is that shape again; giving flux capture an active-layer row
of its own (no coherent write destination, and it would license a writable
capture-editing session nothing claims); keeping "flux" for the capture and
naming only the medium, which was rejected because P23's row already
*described* the medium, so renaming the row was both the smaller edit and
the truer one; splitting F30 into two fresh handles; and treating the medium
as a derived cache over the capture, which fails P27's own definition — a
derived cache is a clean-only accelerator regenerable from the layer below,
and a medium cannot be regenerated from a capture without the policy that
produced it.

**Folded into:** P22 and the P23 amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); pledged F30 (renamed),
F31, F33, F34, F36 and the new F37 in
[pledged/FEATURES.md](pledged/FEATURES.md); proposed F32 and its design; the
annotation on D8.

### D13 — The capture's two head designators are the disk's sides, not two capture channels

**Decided** Paul Galbraith, 2026-08-03. **Supports** U23, P29, P30.

A factual correction to pledged text, and a scope call that follows from it.

**The fixture was misread.** The `.0.raw` / `.1.raw` suffix on a Pinball
Construction Set stream is the KryoFlux head designator: the two are the
disk's two **sides**, not two passes over one surface. Side 1 is the
unrecorded back of a single-sided disk, measured as noise on every position
sampled — roughly 49,000 transitions per revolution with the count varying by
hundreds between passes, against side 0's tracks reproducing transition for
transition.

**Confirmed from the flip.** The source archive holds a second capture of the
same disk turned over, and it inverts exactly: there, head 1 reproduces
transition for transition and head 0 is the noise. The recorded surface
follows the flip to whichever head faces it, so the disk carries exactly one
recorded surface, established from both orientations. It is not a flippy.

**"Capture-channel identity" was never a second concept.** F31 already owned
"track and side identity", so the clause is struck rather than renamed.

**Side selection stays a policy input but stops being a judgment.** F33's
first input read as choosing which of two beliefs about one surface to
trust, and weighed accordingly. It is not that: P30's `Surfaces` declares
how many surfaces a family records and how a captured side maps onto one, so
for a 1541 the answer is declared, and a captured side the mapping does not
cover is refused. This is why the correction is not cosmetic — the input
that looked like the reduction's hardest call is answered by declaration,
and the reductions that actually carry risk are the timebase projection,
half-track admission, and the partial revolution outside the destination's
one rotation.

**The fixture is one capture, both heads.** It holds all 84 step positions
from each head in a single archive, named for the disk, which is the artifact
a real capture produces: a single-sided disk read in a two-head drive yields
both heads, and the operator archives the lot. Splitting the heads into two
archives would have pre-answered the question the library exists to answer.
Members carry the `.0.raw` / `.1.raw` designator rather than having it
stripped: a stream declares no track or side in its own out-of-band data, so
a member's name is the only place its position exists, and a fixture renamed
out of the convention would admit a grammar no real capture has.

**Weighed and declined:** leaving the vocabulary and adding a note (the
misreading had already been weighed into a pledged policy input, which is
exactly the damage a note does not undo); renaming "channel" to "side"
mechanically without revisiting F33's input (it would have preserved the
weighing that was the actual error); and recording this as an open question
rather than a decision, which would leave pledged text stating something
measured to be false.

**Folded into:** U23 in [pledged/USE-CASES.md](pledged/USE-CASES.md); F31 and
F33 in [pledged/FEATURES.md](pledged/FEATURES.md);
[AGENTS.md](../AGENTS.md); `../test-fixture-prep/prep_fixtures.py`;
`../test-fixture-prep/test-rigs/README.md`;
`crates/remanence/tests/sevenzip_catalog.rs`;
`crates/remanence/Cargo.toml` and the fixtures directory's `.gitignore`.

### D12 — Drive profiles own the knowledge a capture does not contain, and recognize structure without reading content

**Decided** Paul Galbraith, 2026-08-03. **Supports** P4, P12, P22, P23, P29,
P30.

Rulings made while pledging P30 and F36.

**The seam earns a principle.** P22 and P23 both rest on a "media profile"
and a "hardware profile" — the authority that says whether a drive observes
a selected revolution or a seeded variation, and the authority that makes a
downward synthesis honest — and neither names an owner. Knowledge assumed by
two principles and held by none is exactly the gap D8 found in P13 and
closed with P29, and the same reason applies here: a rule that binds every
future drive family does not belong in the design document of one mastering
profile. P30 states it.

**Recognition stops at structure.** A profile may read flux interval lengths
and the patterns they form; it may not resolve a bit value, assemble a byte,
name a sector, or validate a checksum. The test is what leaves the probe:
**an angle, never a byte.** This admits the landmark that makes recognition
work — a GCR sync is ten or more consecutive `1` bits, so in the interval
domain it is a run of minimum-length intervals, locatable without a clock,
without the encoding table, and without knowing what it introduces — while
refusing the ascent that would make every recognition depend on a
clock-recovery model.

**Discovery proposes; it never decides silently.** Verdicts are ranked,
carry P4 evidence, and may be pinned or overridden; a capture no profile
claims is a named refusal, and a lone enrolled profile never wins by being
alone. This does not weaken P29, whose policy inputs were always "supplied
by the caller **or declared by the profile**": recognition supplies
declarations with provenance, and a profile that cannot state a reduction
still refuses.

**The ruling was made against measurement.** Probing the prepared capture
set recovered all four 1541 speed zones at their documented track boundaries
with their documented sector counts, from interval statistics alone, with no
decoding — which is what established that the boundary above is a real place
to stand rather than a hopeful one. The same run also showed the cost of the
weaker alternative: a confidence figure without evidence hid a defect in the
probe's own cell estimate for one track, and only the evidence beside it
made the defect visible rather than reportable as a finding about the disk.

**Weighed and declined:** folding recognition into F33's design document
(D8's precedent — a design authorizes one feature, and this binds every
family); requiring the caller to declare the family in every case (the
evidence discriminates decisively, and a forced declaration puts an
unevidenced assertion into the plan's provenance); letting the probe ascend
to the hardware bitstream and recognize a family by decoding its sectors
(collapses the boundary between what a medium is and what a drive makes of
it, contradicts D8, and would make recognition depend on F32, which is only
proposed); a bare confidence scalar without the observations behind it
(P4 forbids it, and the measurement above showed why); and treating a
profile as a P12 image-format adapter (it owns no container grammar and
recognizes recorded state rather than an encoding).

**Folded into:** P30 in [pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md);
pledged F36; the annotation on D8.

### D11 — A design outlives the feature that carried it

**Decided** Paul Galbraith, 2026-08-03. **Supports** (none) — a records
ruling; no numbered vision entry demands it.

Two delivered features are struck from the pledged list, their handles
retired: the archive-catalog foundation and the file-container presentation
contract. The pledged list states that everything in it is owed, so a
delivered entry left standing makes it overstate the project's debt.

The archive-catalog entry was never struck on delivery because it was
**pledged two minutes after the code landed** — written retrospectively into
the owed list — so no delivery moment ever arrived at which the evaporate
rule applied. That is a defect in the record, not a change to the rule,
which has stood since the initial import. The lesson is the ordering, not
the rule: an entry describing work already done does not belong in a list of
what is owed.

[Overruled by D17: a design document's purpose ends at delivery, and it is swept with the feature whose handle evaporates.]

**A companion design does not evaporate with its feature.** README's sweep
covers a design whose *proposal dies*, and its one-way move out of
`planning/` covers a document describing a *delivered application surface*.
Neither reaches a design for delivered work that touches no surface, and the
file-container contract is exactly that: the code implements it, but the
code is not a readable statement of what a future provider must satisfy.
Deleting it would destroy the contract's only prose to satisfy a rule
written for a different case. It stays, restated as delivered, and a design
whose feature is struck is re-headed rather than swept.

**Weighed and declined:** sweeping the design with its feature on the
strict reading that a design serves one feature and dies with it (it would
leave the conformance rules discoverable only by reading the module);
moving it out of `planning/` under the delivered-surface rule (it describes
no surface — the feature's own scope was `Touches: none`); and leaving both
entries in place until a later cleanup, which is what let the first one
persist.

**Folded into:** [pledged/FEATURES.md](pledged/FEATURES.md).

### D10 — The truth is the lowest materialized layer; file container is an interface, not a layer

**Decided** Paul Galbraith, 2026-08-03. **Supports** P19, P23, P25.

**The rule, in the owner's words:** the lowest durable layer the session has
materialized is the source of truth. A file-container view has real
utility — display, envisioning structure, and the account of what an
interpretation claims — but it is not the truth. And there is **no container
layer above these systems at all**: a ZIP grammar, a FAT volume, a Commodore
directory each already hold their own structure and simply *present* a
file-container view of it.

In-force P23 already carries the first half for disks: the initial active
layer is "the least physically expressive durable media layer which
faithfully serves every presentation requested". This states it generally,
past disks to serialized containers. P23 needs no amendment — it already
separates the P19 interface from the active layer, and a ZIP's active
named-entry state is owned by its grammar.

The second half is a correction of this project's own drafting rather than
of P19: in-force P19 was always written as a **seam** whose adapters *expose*
a view and whose results *present* an interface. The word "layer" entered
through the F35 drafts and nowhere else. What F35 delivers is therefore the
interface providers present through and the vocabulary they answer in.

Four consequences fold into the pledged P19 scope-of-claim amendment and the F35 design.
**No materialized model**, so a provider answers about the directory it was
asked about instead of building an item pool for fifty thousand files, and
identity is the provider's own rather than an index into a pool that no
longer exists. **Nothing to invalidate**, so a floor that moves needs no
regeneration protocol. **One hook, not two concepts**: a footprint and a
content source were the same fact about different floors. **Coverage
everywhere**, since every presentation has a floor — a self-extractor stub is
an opaque region exactly as a protection track is, which overrules D9's
clause to the contrary.

**Weighed and declined:** a materialized model as the active layer for
serialized containers (it made ZIP and media structurally different for no
gain, and its footprints would go stale the moment a composition descended to
flux); a materialized model as a generated view above the floor (it kept an
invalidation protocol and a read-whole for no benefit the interface does not
already give); declaring a file container never active at all, which would
have contradicted in-force P23 and left a writable ZIP's pre-commit truth
unowned; and treating an archive's unaccounted bytes as adapter evidence
rather than opaque regions, which duplicated one concept in two
vocabularies.

**Folded into:** the pledged P19 scope-of-claim amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); the annotation on D9;
pledged F35 and its companion design.

### D9 — The file-container model's scope calls

**Decided** Paul Galbraith, 2026-08-03. **Supports** P19, P23.

Rulings made while pledging the file-container model foundation (F35) and
the P19 scope amendment.

**The unclaimed remainder is an "opaque region."** Opaque *to this
interpretation* — no implication that it is garbage, free, or unclaimed by
every layer; in the protection case it is load-bearing content, and over
flux it is angular track regions rather than bytes. The proposed U8 already
uses the phrase.

**An opaque region is an item, never an entry.** In-force P19's refusal to
manufacture pseudo-files stands untouched: the namespace lists only what the
source names, and the opaque remainder is itemized without a name,
reachable through the coverage account rather than by path.

[Overruled in part by D17: there is no design-level home for a delivered
feature's contract. The metadata contract lives in the code implementing
it; the principle-level half of this split stands.]

**The scope clause is principle-level; the metadata contract is
design-level.** The coverage obligation amends P19, while the superset
metadata contract stays in the companion design — the same split the flux
foundation made between the P22/P23 amendments and its design document.

**Coverage exists only over a materialized sub-layer.** A serialized
container's unaccounted source bytes (a self-extractor stub, padding) are
the adapter's evidence, not opaque regions; there is no layer beneath the
active file container for a footprint to address.

> **Overruled by D10** on this clause alone: a serialized container's own
> named-entry state is a floor like any other, so its unaccounted bytes are
> opaque regions and it carries an account. Every other ruling in this entry
> stands.

**Deleted-but-present entries are accounted, not itemized.** A scratched
CBM entry or FAT `0xE5` slot is part of the namespace structures' footprint;
itemizing it would be a recovery claim nothing pledges.

**v1 claims one content stream per file item.** Alternate data streams and
forks enter by the superset contract's additive named-home route or are
refused by name.

**Weighed and declined:** "blob" and other byte-shaped terms (wrong over
flux, and they imply extractability the view may not claim); "unclaimed
extent" (reads as nobody's when the truth is not-this-view's); "remnant"
(suggests leftover-from-deletion; protection tracks are deliberate); a
kitchen-sink union record with every metadata field optional (rejected once
already at the flux layer; the two-outcome rule is reused instead);
itemizing deleted entries (a recovery claim in disguise).

**Folded into:** the pledged P19 scope-of-claim amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); pledged F35 and its
companion design.

### D8 — Mastering a capture to P64 stops at flux, and gets its own principle

**Decided** Paul Galbraith, 2026-08-03. **Supports** U23, P29.

Two scope calls made while pledging U23.

**It stops at flux.** Converting a KryoFlux capture to P64 descends no
further than the flux layer: no hardware bitstream is materialized, no GCR
codec runs, no sector or filesystem interpretation is attempted. Both
endpoints are flux-shaped, so the intervening layers would be built only to
be discarded. Proposed F32 is therefore *not* a dependency of U23 and stays
in `proposed/`, which also keeps U23's pledge from resting on something only
proposed.

> **Annotated by D12**, which narrows nothing. Locating a synchronization
> landmark as a run of minimum-length flux intervals is not "a GCR codec
> running": no clock is recovered, no symbol is resolved, and what leaves
> the probe is an angle rather than a byte. The clause stands as written,
> and D12 states the boundary that keeps it checkable.
>
> **Annotated by D14** on the spelling only. The journey now stops at the
> **flux medium**, one rung above where this entry could name at the time,
> because the flux layer it spoke of has since been split in two. The
> ruling is unaffected: both endpoints remain the same shape, no hardware
> bitstream is materialized, no GCR codec runs, and F32 is still not a
> dependency of U23.

**And it earns a principle.** P13 already licenses the act — choosing another
authoritative layer is an explicit conversion creating a new image and naming
its loss — but names no owner for the reduction policy and no mechanism for
"naming the loss". Reading that into P13 would have made the strongest clause
in the conversion story an inference. P29 states it instead: declared policy
inputs, two owners, plan before write, derived provenance, reproducibility.

**Weighed and declined:** requiring F32 so a mastered image could be verified
by decoding it to sectors (verification is round trip through the P64
adapter's own decode, which tests the claim actually made); folding the
mastering rules into F33's design document alone (a design authorizes one
feature, and this rule binds every future destination format).

### D7 — The library names no consuming project

**Decided** Paul Galbraith, 2026-08-01. **Supports** (none) — a naming
ruling; no numbered vision entry demands it.

Documentation follows the dependency direction the code does: a consumer
may name the libraries it builds on, and this library names none of the
projects that build on it. In-force U3 and U4 named the consuming
application outright, inherited from the demand they were dictated from.
Both are reworded to the caller's voice — every claim, contract and symbol
unchanged — under authority compression. The rule's home is AGENTS.md,
"The library does not name its consumers"; it reaches every library-side
document, not only the ones a registry publishes — this record included,
where D2's weighed alternative is reworded to the caller's voice. A name
that survives sits inside the fixture-tooling permission, which runs the
other way: the project may name what it builds on.

**Weighed and declined:** keeping the name in the use cases on the grounds
that they are the owner's demand narrative and a real name is more concrete
than "my automation layer" — that concreteness is exactly what goes stale
inside a published artifact, and the use cases are the first library-side
document a newcomer reads.

**Folded into:** root [USE-CASES.md](../USE-CASES.md) (U3's title and
opening; U4's opening); [AGENTS.md](../AGENTS.md); D2's
weighed alternative; `crates/remanence/src/model/disk/mod.rs` and
`crates/remanence/src/filesystem/fat.rs` doc comments.

### D6 — Device identity is assigned, not requested

**Decided** Paul Galbraith, 2026-07-31. **Supports** P21.

D5 still defers multi-device topology, volumes spanning devices, and
cross-source transactions. Its refusal of preparatory identity was too
broad: a library-assigned, composition-scoped identity adds useful internal
structure without adding a caller-supplied datum. It gives identity no
global meaning and revives none of the machinery D5 deferred; P21 carries
the rule.

**Partially overrules:** D5's rejection of topology-ready identities. The
new evidence is that automatic identity and caller-authored topology have
different interface costs.

### D5 — Multi-device topology is deferred until a use demands it

**Decided** Paul Galbraith, 2026-07-31. **Supports** P17.

> **Partly overruled by D6:** the refusal of automatic device identity no
> longer binds; the deferral of multi-device topology and volumes stands.

The proposed P20 is withdrawn. Multi-device volumes are extremely unlikely
to enter Remanence, and the concrete cost of adding them later is an
ordinary refactor: qualify disk-local identities, supply several devices to
volume composition, and add cross-source write coordination if writing is
claimed. That does not justify making source, device, attachment, and
multi-parent provenance part of F19 or the architecture now. P20's number
is retired and will not be reused.

P17 remains the independent volume-composition seam. It supports current
whole-medium, partition-backed, and region-composed volumes without
promising or preparing for a volume spread across devices. If that use ever
becomes real, it receives its own proposal and surface design. Existing
disk-local identifiers retain their existing scope; no present interface
claims they are globally unique.

**Weighed and declined:** building topology-ready identities and
multi-parent provenance into F19; a multiple-source open with manual
`hdd0`/`hdd1` assignment [no longer declined: pledged P32 admits a session
device set with `hdd0`-style attachment identities, which in-force P21
routed to its own proposal rather than refusing; the deferrals of
volumes spanning devices and of cross-source transactions stand]; a
principle governing cross-file transactions before any multi-device write
use exists.

**Folded into:** proposed P17; the F19 design; withdrawal of proposed P20.

### D4 — "At rest" leaves the library's vocabulary; the surface is the `Disk` stack

**Decided** Paul Galbraith, 2026-07-30. **Supports** (none) — a
vocabulary ruling; no numbered vision entry demands it.

The term "at rest" is retired from library-side prose and comments.
It borrowed its meaning from the consumer's frame — a disk not held
by a running machine — a contrast this library cannot represent (it
has no concept of a machine); inside the library it distinguished
nothing, since every operation here works on an image as a file;
and it collides with the security-jargon sense of "data at rest".
The geometry/volumes/files read-write stack is named by its own
API: **the `Disk` surface** (in prose, the disk stack). Use cases
keep the consumer's voice, but "a stopped machine's" already
carries the whole meaning, so U3 and U4 drop the term too — a
wording-only amendment, landed under authority compression: no
claim, contract, or symbol changed, and no public symbol ever
carried the term.

**Weighed and declined:** keeping "at rest" as an established
project word (it was established by inheritance from the consumer's
design vocabulary, not by a decision here); "offline" (relative to
the same machine concept the library lacks).

**Folded into:** the U3/U4 rewording in root
[USE-CASES.md](../USE-CASES.md); root
[ARCHITECTURE.md](../ARCHITECTURE.md) "The system"; README.md;
AGENTS.md; doc comments in the three crates (the C header
regenerates from them); `tests/at_rest.rs` renamed `tests/disk.rs`;
the test-rigs prose; the drafts under `proposed/`.

### D3 — One upstream version; packaging versions derive; repacks are post-releases

**Decided** Paul Galbraith, 2026-07-30. **Supports** (none) — release
machinery; no numbered vision entry demands it.

The workspace SemVer is the sole upstream version. The PyPI version
is derived from it by maturin (`0.0.1-alpha.1` → `0.0.1a1`), never
hand-written. Repackaging an unchanged upstream — the distro-revision
case — is spelled as a PEP 440 post-release by appending `.post.N` to
the Python packaging crate's own Cargo version (`0.0.1a1.post1`);
whether a repack is warranted is the releaser's judgment, and only
the spelling is mechanized.

**Weighed and declined:** PEP 440 local versions (`+r1`, the true
distro-revision analog — PyPI rejects them on upload); a static
hand-maintained pyproject version (drifts from the lib; replaced by
derivation); bumping the upstream version for packaging-only changes
(misstates the library). PEP 440's discouragement of post-releases
on pre-releases was seen and consciously overridden — the
distro-revision model is the point.

**Folded into:** AGENTS.md "Versioning and releases";
`crates/remanence-py/pyproject.toml` (dynamic version).

### D2 — The commit point is an in-memory overlay, not qcow2 internal snapshots

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-07-30. **Supports** P2, U3.

P2's commit point is implemented as an in-memory write overlay over
the virtual disk: every write buffers, reads see the buffered state,
`commit` writes through and flushes, `rollback` discards. The drafted
alternative — reproducing a caller's qcow2-internal-snapshot
protocol natively (the feature drafted as F4) — was superseded before
it was pledged.

**Weighed and declined:** internal snapshots as the commit point.
The overlay is uniform across raw and qcow2 images where snapshots
exist only for qcow2; it means **nothing whatever touches the host
file before commit** (stronger than snapshot-then-write under P6);
and it removes the snapshot-table machinery from the write claim
entirely — the write path refuses images carrying internal snapshots,
keeping the all-refcounts-are-one invariant checkable.

**Folded into:** root [ARCHITECTURE.md](../ARCHITECTURE.md) P2's
in-force text; `crates/remanence/src/io/cache.rs` (the overlay) and
`crates/remanence/src/model/disk/commit.rs` (commit/rollback).

### D1 — The HDOS fixture images leave git and every published artifact

**Decided** Paul Galbraith, 2026-07-30. **Supports** (none) — no
numbered vision entry exists yet to demand it; the demand is the
licensing policy in [AGENTS.md](../AGENTS.md): the project must own
every line it ships, and the vintage HDOS distribution images are
not the project's to distribute — or at least that is not certain,
which is the same bar.

The fixture images (then under `crates/remanence/tests/fixtures/`,
relocated by the 2026-08-19 amendment below) are
excluded from **everything the project distributes or records**:
Python sdists and wheels, cargo packages, and the git repository
itself — history was rewritten to expunge them before any remote
existed, and the directory is ignored. They remain local-only test
data. Implemented as `package.exclude` on the core crate (governing
maturin sdists and `cargo publish` alike), the `.gitignore` entry,
and the history rewrite.

**Amended** Paul Galbraith, 2026-07-31. The exclusion was a whole
directory, which cost the project a fixtures directory it could use
at all. It is now **per file**: the fixtures directory
holds checked-in fixtures the project owns, and the third-party and
generated material sits beside them, named file by file in that
directory's own `.gitignore` — the ignore rule lives with the files
it governs, so adding a fixture is a local act. Nothing about what
D1 refuses to distribute changes; only the granularity does, and
`package.exclude` mirrors the same names.

**And the material is fetched, not carried.**
`../test-fixture-prep/prep_fixtures.py` downloads the HDOS 1.0 distribution
zip from `https://sebhc.github.io/sebhc/software/HDOS/HDOS_1-0.zip`
under a pinned SHA-256, extracting only the image the tests read;
the FreeDOS LiveCD downloads through the rig blueprint's own
reliquary media spec, likewise pinned, into
`../test-fixture-prep/test-rigs/cache/media` (git-ignored, outside the
crate). The FreeDOS qcow2 the rig builds lands in the fixtures
directory as a generated artifact. So a fresh checkout carries none
of it and can obtain all of it, which closes the accepted cost this
decision took on — the repair T5 tracked, struck with this change.

**Amended** Paul Galbraith, 2026-08-19. The material moves out of the
core crate to `../integration-tests/`, at the repository root:
`fixtures/` for what a test opens, `downloads/` for the sources a
fixture is repackaged out of (previously
`../test-fixture-prep/downloads/`). Three surfaces read the same
files — the Rust suites, the C/C++ CTest suite, and the prep script
that writes them — so filing them under one crate asserted an
ownership none of the three could honour, and D69 had just settled
that `crates/*/tests` holds Rust tests and nothing else. **What this
strengthens is the exclusion itself**: `package.exclude` was the only
thing keeping the fixtures out of a cargo package and a maturin sdist,
a rule that had to stay correct as fixtures were added. They now sit
outside every crate directory, so cargo cannot see them to package
them — structure rather than a rule, the same substitution D69 made
for the pytest suite. Nothing D1 refuses to distribute changes; only
where it lives and what enforces it. The reliquary media cache stays
at `../test-fixture-prep/test-rigs/cache/media`: that layout is
reliquary's own, and keying it to the rig is what lets a rebuild reuse
the ~0.5 GB download.

**Weighed and declined:** publishing the wheel without an sdist
(with no public repository, GPL object code would ship with no
corresponding source at all); annotating the fixtures in REUSE and
shipping them (the project cannot convey rights it does not hold);
keeping them in git as local-only history (any future push would
distribute the blobs); folding `downloads/` into `fixtures/` outright
(the KryoFlux source archive differs from the fixture it yields by a
` (1of2)` suffix alone, and nothing reads it).

**Folded into:** `../integration-tests/fixtures/.gitignore`,
`../integration-tests/downloads/.gitignore`, root `.gitignore`,
`crates/remanence/Cargo.toml`, `../test-fixture-prep/prep_fixtures.py`,
`../REUSE.toml`, AGENTS.md "Prior art and provenance notes".

## Retired decisions

Overruled or no longer relevant, kept intact for the record. A
retired decision binds nothing.

### D55 — U22 rested on a false premise about DOS, and is struck rather than amended

*Retired by D57, which withdrew guest volume mapping and drive lettering
from the claim altogether. Everything below adjudicated how the DOS
derivation should read its own inputs; there is no derivation left for
it to bind. Kept for the FreeDOS reading it records — `DLASORT` is
patched into `KERNEL.SYS` by `SYS CONFIG` and is no `CONFIG.SYS`
directive, contrary to two secondary sources — which cost real work to
establish and would cost it again.*

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-14. **Supports** S1, S2, S3; P3, P4, P19, P32. Retires **T7**,
whose number evaporates with it, and amends in-force P19 and pledged
U16. `DECISIONS.md` was searched first and returned D23, which settled
the machine/letters relationship and was annotated on F53's delivery;
nothing there adjudicated letter *detection*, and nothing forecloses it.

**The premise.** In-force U22 held that "DOS persists nothing, so the
mapping is a *rule* applied to machine facts", and split itself from
U13/U16 on exactly that: those journeys read a persisted mapping, this
one could not. Its exclusion list refused "inferring one from a
`CONFIG.SYS` the images may not even hold", and its composer therefore
asked the caller to assert the DOS variant, the `LASTDRIVE` ceiling, and
every resident condition.

**The premise is false, and its falsity is not a detail.** DOS persists
no drive-letter *map* — nothing records "C: was this volume" — but it
persists every input the map was derived from: the kernel files that say
which DOS is installed, and the startup files that say what it was told.
`CONFIG.SYS` is to DOS what the registry is to Windows. A composer asking
its caller for a fact recorded on the disk in front of it contradicted
U22's own second constraint, that evidence outranks a rule; the seam had
been reading past its own evidence and calling the result an assertion.

**Struck, not amended.** Once the premise goes, nothing holds U22 apart
from pledged U16, which already describes this journey for Windows and
Unix — seat the disks, inspect, detect installation candidates, compose
the namespace the installed system's own configuration establishes. The
DOS case is that journey with a DOS adapter, not a second journey beside
it. And two of U22's three sections were already duplicated into in-force
entries: its label policy is U4's ("the label is one whole answer,
decided where the format is known"), its 8.3 name rules are U3's. Only
the letters were uniquely U22's, and they relocate. Striking loses no
in-force claim; amending would have left a third journey asserting what
the other two read.

**U-22 retires and is never reissued.** The four in-force cross-references
to it now name the capability rather than the handle, because the journey
they point at is pledged and an in-force document citing a pledged one
cites upward.

**P19 is amended rather than left to drift.** Its namespace-mapping
paragraph said the composer derives from "the machine facts its caller
asserts" and "opens nothing"; both are now false. The amended text says
what the caller states is the *machine*, that every input to the rule is
read from the media it holds, and — added as its own clause — that
evidence outranks a rule *including the rule's own inputs*, so a future
composer asking for a persisted fact is violating the principle rather
than serving it.

**One assertion survives, and it moved.** Which device the firmware
booted is set by a stopped machine's host and recorded on no disk, so it
is a property of the machine model (`declare_boot_device`) rather than an
argument to a composer, and a report marks it configuration rather than
evidence. Pledged U16 is softened to match: it had forbidden the active
flag and attachment order from selecting a candidate at all, which was
written for two Windows installs and is wrong for an era whose boot order
is simple enough to state as a rule. Applying a named rule is not
guessing; what stays excluded is reaching for a tie-breaker no rule
authorized.

**FreeDOS is claimed, from its own source rather than from write-ups.**
Two secondary sources described `DLASORT` as a `CONFIG.SYS` directive;
the kernel's own directive table has no such entry, and `SYS CONFIG`
patches it into `KERNEL.SYS`. The implementation that had been written
against those write-ups would never have fired. What ships instead reads
nothing for it and records that it did not, the kernel's documented
default being the order applied.

**Weighed and declined:** amending U22 in place (it would have kept a
third journey beside U13/U16 with no premise left to justify the split);
keeping `DosMachine` beside the machine reading for facts a session
cannot hold (the gap was `Format::Raw` refusing a floppy, which is fixed
where it was rather than worked around); reading `DLASORT` out of
`KERNEL.SYS` (its layout is version-specific and the default is near
universal, so the honest answer is a stated omission rather than a
fragile read); and inferring a raw floppy's article from image size (a
1.44M and a 720K image are both bytes, and deriving the article from the
length is the guess this project refuses — the declared device carries
it instead).

**Reopens if:** a claimed DOS is found whose letter order its own
installation does not determine, or a second operating-system family is
given a recognition seam, at which point whether `dos_install.rs`
generalizes or is joined by a sibling is a live question.
