# Integration tests

The suites that need a downloaded or generated artifact, the script
that prepares those artifacts, and the Taskfile that drives both. None
of it is reached by `cargo build`/`cargo test` in any form — the
integration crate is not a default workspace member, because every
target in it needs a fixture a fresh clone does not have.

```
integration-tests/
├── Taskfile.yml       the tasks below — run `task` from this directory
├── prep_fixtures.py   prepares every fixture (PEP 723: `uv run` provisions it)
├── fixtures/          downloaded archives, extracted images, the rig-built qcow2
├── downloads/         archives nothing reads directly; a fixture is repackaged out of each
├── rust/              the `remanence-integration-tests` crate: Rust suites over the fixtures
└── python/            the fixture-gated pytest suite over the Python bindings
```

`fixtures/` and `downloads/` hold nothing tracked but their `.gitignore`,
which is the list of what the prep script lays down there. The FreeDOS
rig itself — the reliquary blueprint and install script that build
`fixtures/freedos-parttest.qcow2` — lives in
[`../test-fixture-prep/test-rigs/`](../test-fixture-prep/test-rigs/README.md).

## Running

[Task](https://taskfile.dev) searches upward from the current directory
and this is the repository's only Taskfile, so run the tasks from here,
or from the root as `task -d integration-tests …`. Every task's working
directory is this one either way. `task` alone lists them.

```bash
cd integration-tests
task integration-test      # every suite: test-rust, test-ffi, test-python
```

`integration-test` runs the three suites in that order, every one of
them even after a failure so a single pass reports everything, and
exits non-zero naming the suites that failed. The suites run on their
own too:

```bash
task test-rust             # rust/: both feature tiers, preparing fixtures first
task test-ffi              # the C/C++ suite, built via CMake and run with CTest
task test-python           # python/: the fixture-gated pytest suite
task test-py               # the bindings' own fixture-free pytest and mypy checks
```

`test-rust` and `test-python` depend on `prep-fixtures`, so they need
nothing prepared beforehand; the prep is idempotent, and a machine that
already has everything pays only the existence checks. Extra arguments
pass through — to `cargo test` from `test-rust`, replacing its default
of both tiers; to `ctest` from `test-ffi`; to `pytest` from
`test-python`:

```bash
task test-rust -- --features fixtures      # the half that needs no reliquary install
task test-rust -- --features rigs          # the half over what reliquary built
task test-ffi -- -LE "rigs|fixtures"       # the CTest set that needs no fixture
```

The Rust suites are gated on two features of the integration crate,
mirroring the core crate's own: `fixtures` for everything downloaded
and extracted, `rigs` for `freedos-parttest.qcow2`, which reliquary
builds by installing FreeDOS into a QEMU machine. A target that
declares neither builds nothing — `ensure_fixture` in
`rust/tests/common/mod.rs` refuses to compile without the declaration,
so a suite cannot reach for a fixture without saying so.

## Preparing and reclaiming

```bash
task prep-fixtures         # prepare everything ahead of time, or rebuild after a clean
```

`prep_fixtures.py` downloads each archive sha256-pinned, extracts or
repackages the images the suites read, and builds the rig artifact —
skipping whatever is already on disk, so the way to force a rebuild of
one fixture is to delete that file. The rig build downloads a ~0.5 GB
LiveCD into the rig's media cache and takes a few minutes; it always
starts from a fresh machine, destroying one an interrupted build left
behind, because the install script partitions a disk it takes to be
blank. It needs QEMU installed; see the rig README for what the
install script depends on and how it breaks.

Going the other way, each reclaim is one directory or tier, and `clean`
is all three:

```bash
task destroy-rigs          # every rig machine, and the rig's media cache
task clean-fixtures        # fixtures/: everything its .gitignore lists; checked-in fixtures stay
task clean-downloads       # downloads/
task clean                 # destroy-rigs, then clean-fixtures, then clean-downloads
```

`destroy-rigs` goes through reliquary's own API, so its machine locks
and phases are honoured (a guest that powered itself off still reads as
running until a stop reconciles it); the fixture a rig built is left in
place, since that is the deliverable — `clean-fixtures` is what removes
it. Nothing is rebuilt until the next `prep-fixtures`, or a suite that
depends on it.

The fixtures and the rig's media cache are per machine: they are never
tracked, and a failure seen on one machine may be over an artifact
another machine does not have. A Rust suite that fails over
`freedos-parttest.qcow2` names the private copy it read and leaves it in
place, so the bytes that failed can be examined before anything
rebuilds them.
