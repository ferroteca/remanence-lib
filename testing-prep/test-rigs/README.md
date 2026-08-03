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

## Running prep_fixtures.py

Through uv, from the repo root (reliquary is pinned in the root
`pyproject.toml`'s `testing-prep` dependency group):

```bash
uv run --group testing-prep testing-prep/prep_fixtures.py
```

Or activate the uv-managed `.venv` once and run directly, as before:

```bash
uv sync --group testing-prep
.venv\Scripts\Activate.ps1
python testing-prep/prep_fixtures.py
```

The prep script drives reliquary through its **Python API**. The
LiveCD zip downloads through the blueprint's own media spec
(sha256-pinned), into a media cache the script pins to
`testing-prep/test-rigs/cache/media` — inside the rig tree so the
~0.5 GB download survives `cargo clean` and machine rebuilds, and
git-ignored there. The install script runs, and the machine's `hdd0`
image is harvested into
`crates/remanence/tests/fixtures/freedos-parttest.qcow2`. Subsequent
runs reuse the existing artifact; **delete the file to force a
rebuild**. Unit tests run with `cargo test -p remanence` and expect
this fixture to be present in `tests/fixtures/`.

(The same script also prepares the HDOS fixtures: the distribution
zip downloads sha256-pinned straight into `tests/fixtures/`, and only
the one disk image the tests read extracts beside it. It also downloads
the Pinball Construction Set KryoFlux source archive into
`testing-prep/downloads/`, then packages only disk one — all 84 step
positions from both heads — into a single 7z fixture. `.0.raw` and
`.1.raw` are the KryoFlux head designator, not two passes over one
surface, and members keep those suffixes because a stream records its
position nowhere but its name.)

## Prerequisites — the tests fail naming the gap, they do not skip

- **Python ≥ 3.12** (reliquary's floor); the root `pyproject.toml`
  pins `requires-python` accordingly for the `testing-prep` group.
- **QEMU** installed where reliquary can discover it, per reliquary's
  docs: a standard install location is sufficient, and PATH also
  works.
- Windows is the tested host, as everywhere in this project.

**This directory is a reliquary home.** That is why `blueprints/`
and `scripts/` sit where they do: the prep script assigns the home
and reliquary derives the rest — `cache/` from the home, then
`media/` and `machines/` from that cache. Everything regenerable
lands under `cache/` (git-ignored): the downloaded LiveCD in
`media/`, the machine materializations in `machines/`.

**A successful build gives that cache back, in two stages.** As each
rig machine finishes it is destroyed and the media cache is *pruned*
— the safe reclaim, which drops only what falls outside the
remaining attachment closure, so anything a machine still to run
would attach survives. Once the last rig is done the cache is
*cleaned* outright. For this rig the prune reclaims the LiveCD zip
(its extracted ISO is already cached, so the container is a husk)
and the final clean takes the ISO too — together roughly 0.8 GB that
would otherwise sit in this tree.

A **failed** build reclaims nothing: the machine, its `screenshots/`
and every cached payload stay exactly as they were, because that is
the whole diagnostic. The cost of reclaiming is that the next build
re-downloads the LiveCD, which only happens when the fixture itself
is deleted.

Environment knobs:

- `REMANENCE_TEST_RIG_CACHE_DIR` — the regenerable cache root;
  defaults to the derived `cache/` beside this file. Point it
  elsewhere to keep the ~0.5 GB download and the machine images off
  this tree. The home is deliberately not overridable — it holds the
  checked-in blueprints and scripts, so moving it would only point a
  run at files that are not there.

To build with unpublished reliquary changes, override the pin with a
local editable checkout for the run:
`uv run --with-editable D:\Projects\reliquary --group testing-prep testing-prep/prep_fixtures.py`.

First build notes: the LiveCD zip (~0.5 GB) downloads into
`testing-prep/test-rigs/cache/media/`, and the install run can take
tens of minutes.
After a failed build the machine is still there to inspect;
`uv run rlq destroy-machine --machine remanence-parttest-<n>
--home-dir testing-prep/test-rigs` (`uv run rlq list-machines
--home-dir …` names them) resets it, and
`uv run rlq clean-media --home-dir …` gives back the LiveCD. A machine
whose guest powered itself off still reads as `running` until
`rlq stop-machine` reconciles it.

## What the script depends on, and how it breaks

The install script is converged against the FreeDOS 1.4 LiveCD
pinned in the blueprint, and it is pinned to that LiveCD's
*observable behaviour* — so a version bump is what would break it.
The three dependencies worth knowing before touching either:

- **The boot-menu item is `Use FreeDOS 1.4 in Live Environment
  mode`.** There is no "Return to DOS"; that item is also the menu's
  own default after roughly 48 seconds, so selecting it only makes
  the run deterministic and quick.
- **Drive letters move across the partitioning reboot.** Before it,
  the CD-ROM is `D:` and `C:` is a RAM drive; after it the four
  volumes take `C:`–`F:` and the CD-ROM becomes `G:`. DOS orders
  them primaries-first, then logicals, then remaining primaries, so
  `C:`/`F:` are the primaries and `D:`/`E:` the logicals. No phase
  may assume the shell's own drive.
- **FDISK is silent on success**, so each partitioning step is
  followed by an echoed sentinel the script waits on with a
  start-anchored regex. `FORMAT` wants the word `YES` typed out.

A failure names the phase and the screen text it wanted, and drops a
screenshot in the machine's `screenshots/` directory —
`rlq screen --machine remanence-parttest-<n>` shows the live screen
for the rest.
