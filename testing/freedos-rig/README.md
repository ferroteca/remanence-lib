# The FreeDOS qcow2 integration rig

Reliquary builds this rig's test artifact: a FreeDOS 1.4 system
installed into a qcow2 whose disk carries **multiple primary
partitions and an extended chain of logical drives**, so the disk
stack (U3/U4) is exercised against a QEMU-authored image with real
allocation patterns and a real installer's FAT volumes — not only the
synthetic images the unit tests build.

Only this rig's **blueprint and scripts** are checked in; they are
authored here and wholly owned. The built image is a **local-only
artifact** (the fixtures directory is never tracked — see D1 in
[planning/DECISIONS.md](../../planning/DECISIONS.md)).

## One command

```bash
cargo test -p remanence --test freedos_qcow2 -- --ignored
```

The suite does everything itself: when
`crates/remanence/tests/fixtures/freedos-parttest.qcow2` is missing,
it provisions reliquary through
`uv tool run --from reliquary rlq` (install-if-missing into uv's
cached tool environment; uv fetches a Python when none is present),
drives it against the checked-in blueprint (creating the machine,
fetching the pinned LiveCD, running the install script), harvests the
machine's `hdd0` image into that path, and then runs the assertions
against private copies of it. Subsequent runs reuse the cached
artifact; **delete the file to force a rebuild**.

## Prerequisites — the tests fail naming the gap, they do not skip

- **uv** on PATH (<https://docs.astral.sh/uv/>) — Python and
  reliquary are provisioned through it automatically.
- **QEMU** installed where reliquary can discover it, per reliquary's
  docs: a standard install location is sufficient, and PATH also
  works.
- Windows is the tested host, as everywhere in this project.

Environment knobs:

- `REMANENCE_RIG_RELIQUARY` — the package spec uv runs `rlq` from;
  defaults to `reliquary` (PyPI). Point it at a local checkout
  (e.g. `D:\Projects\reliquary`) to build with unpublished reliquary
  changes.
- `REMANENCE_RIG_HOME` — the reliquary home the build uses; defaults
  to `target/freedos-rig-home`. Point it at an existing reliquary
  home to share the media cache.

First build notes: the LiveCD zip (~0.5 GB) downloads into the rig
home's media cache, and the install run can take tens of minutes.
The built machine is left in the rig home for inspection;
`uv tool run --from reliquary rlq destroy --blueprint
remanence-parttest --home-dir <home>` resets it.

## Status

The install script is a **first draft**: the FDISK/FORMAT sequences
and exact prompt texts need live convergence against the LiveCD in
reliquary's driving loop before the rig produces its first artifact
(T6 in [planning/TASKS.md](../../planning/TASKS.md)).
