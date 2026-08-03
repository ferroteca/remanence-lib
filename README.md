# remanence-lib

[![License](https://img.shields.io/badge/license-GPL--3.0--only-blue.svg)](LICENSE)

A self-contained disk image analysis library in Rust. A `Session` opens a
disk image — raw, or an entry inside a `.zip` or `.7z` archive — and
identifies its container
layers: the archive wrapper, image format, physical media geometry, and
probable filesystem, each with comparable confidence and human-readable
evidence. Executable, role-specific adapters recognize and validate formats;
ambiguous strongest matches remain unknown rather than being resolved by
catalog order. An HDOS directory lister reads the file catalog out of
Heathkit `.h8d` images.

An `Archive` lists what a supported archive holds, reading its index and
never its entry data. Each grammar sits behind its own catalog adapter,
and a member is produced bounded: an entry stored uncompressed is read in
place from the archive, and a coded entry decodes once into private
session storage — one member of a solid 7z folder without materializing
the rest.

A `CaptureSet` opens a KryoFlux capture of a floppy disk — one stream
file per head per drive-step position, archived together — as the one
logical capture it is, rather than as a hundred and sixty-eight
unrelated members. It reads each stream's flux, its asynchronous index
records, its transport control records and its transfer result, keeps
the flux recorded before the first index and after the last, and bounds
the circular observations the indices bracket. The two heads stay two
locations: nothing merges them into an ideal disk, chooses a cleanest
pass, or averages a timing. An incomplete, duplicate, contradictory, or
unrelated member refuses the whole set by name, with the catalog
evidence that refused it. The decoded capture lives in private session
storage and is addressed a bounded section at a time, so a forty-megabyte
capture opens under whatever working set the caller declared.

An opened capture can then be recognized: every enrolled drive profile
is consulted and what claims the capture is ranked, never resolved by
catalog order. A profile is where a family's recording conventions are
declared — the knowledge a capture does not contain — and the probe
reads only interval lengths and the patterns they form, resolving no
bit, assembling no byte, naming no sector and validating no checksum.
What comes back is a bounded confidence and the observations that
produced it: which of the family's zones were recovered and what each
location holds, the derived cell against what the zone claims, the seam
located as an angle, and a named reason for every position not claimed.

A recognized capture can then be mastered: the reduction to one
circular, half-track-addressed flux medium resolves in two stages, a
plan that computes everything and writes nothing and an execution that
produces the medium. Every reduction is a declared policy input, and one
the policy does not name is a refusal rather than a default — so a
location whose content its neighbour also holds stops the plan until the
caller says which it is. The plan carries the complete account of what
the destination will not carry, in the source's own terms and before
anything exists to carry it.

The library is dependency-free at runtime, including its own ZIP
central-directory reader, 7z header reader, RFC 1951 (DEFLATE) and
LZMA/LZMA2 decompressors, and native qcow2 v2/v3 driver.

Beyond identification, the `Disk` surface opens a raw or qcow2
disk image with a declared intent: a read session denies writes to
every other process while admitting other readers; a writable session
admits no observers at all; and an image whose claim cannot be secured —
one held by a running VM, say — is refused outright at the open. It
reports the disk's MBR partitions and FAT12/FAT16 volumes as they
actually are, gives each reported volume an opaque stable identifier,
and uses that identifier to work with files — list, stat, read, write
(overwriting in place), and create directories with their missing
parents — under a commit point: nothing touches the image until
`commit`, and `rollback` discards everything. A qcow2 whose content
lives partly in a backing chain — raw or qcow2 members, relative
paths resolved from the image that names them — opens as one composed
disk, every backing member claimed immutable for the session's life.
Writes allocate copy-on-write into the top image only and preserve the
backing relationship; a missing member, a cycle, or a chain past the
claimed depth is refused by name.
The in-force vision — what the library is for (U-numbers) and the rules
it holds itself to (P-numbers) — is in [USE-CASES.md](USE-CASES.md) and
[ARCHITECTURE.md](ARCHITECTURE.md).

## Layout

```
crates/
  remanence/        # the core library (pure Rust, no runtime dependencies)
  remanence-ffi/    # C ABI: staticlib + cdylib, cbindgen header for C and C++
  remanence-py/     # Python module (PyO3), built with maturin
```

## Building

```bash
cargo build            # core + C FFI (generates crates/remanence-ffi/include/remanence.h)
cargo test             # the full test suite
```

The Python module is excluded from default builds so a Python toolchain is
never required for library work:

```bash
cargo build -p remanence-py    # needs Python >= 3.10
# or, for distributable artifacts (sdist + abi3 wheel into crates/remanence-py/dist/):
uv build crates/remanence-py
```

uv drives the maturin build backend in an isolated environment, so no
tooling beyond uv itself needs installing. Publishing is `uv publish`
from that `dist/`, and is owner-gated. **The Python package is tested
on Windows only, for now**, and its packaging classifiers say so; the
POSIX code paths exist and should stay correct, but they are
unexercised and unclaimed.

An example C consumer is at
[crates/remanence-ffi/examples/identify.c](crates/remanence-ffi/examples/identify.c),
with build instructions in its header comment.

## Using the library

```rust
let session = remanence::Session::open("disk.h8d")?;
let identification = session.identify();
for container in &identification.containers {
    println!("{:?} {} ({}%)", container.kind, container.id, container.confidence);
}
let files = session.list_hdos_files()?;

let archive = remanence::Archive::open("captures.7z")?;
for entry in archive.entries() {
    println!("{} ({} bytes)", entry.name, entry.uncompressed_size);
}
let member = remanence::Session::open("captures.7z/track00.raw")?;

let capture = remanence::CaptureSet::open("captures.7z")?;
for member in &capture.inspect().members {
    let run = &member.runs[0];
    println!(
        "step {} head {:?}: {} transitions, {} revolutions",
        member.position.numerator,
        member.head,
        run.transitions,
        run.observations.len()
    );
}
```

```python
import remanence
session = remanence.Session("HDOS_1-0.zip/HDOS_1-0_Issue_#50-00-00_890-1.h8d")
for c in session.identify().containers:
    print(c.kind, c.id, c.confidence)
for f in session.list_hdos_files():
    print(f.display_name, f.size_sectors, f.modified_date_string)

with remanence.Archive("captures.7z") as archive:
    for entry in archive.entries:
        print(entry.name, entry.uncompressed_size)

with remanence.CaptureSet("captures.7z") as capture:
    for member in capture.inspect().members:
        run = member.runs[0]
        print(member.position.numerator, member.head, run.transitions,
              len(run.observations))

    verdict = capture.recognize().verdicts[0]
    print(verdict.profile_name, verdict.confidence)
    for line in verdict.evidence:
        print(" ", line)

    plan = capture.plan_c1541_mastering(remanence.MasteringPolicy(
        side=0, observation_ordinal=0, duplicate="omit",
        projection="declare-loss", pulse_strength="declared",
        strength_state=2, origin="declared", seed=0x0123456789abcdef))
    for loss in plan.report().declared_loss:
        print(loss.code, loss.count, loss.detail)
    medium = plan.execute()
```

## Changes

Release-facing changes are recorded in [CHANGELOG.md](CHANGELOG.md).
Pre-1.0 the project makes no compatibility promise: a surface change lands
across the Rust, C, and Python presentations together and the old shape is
deleted, so read the changelog before upgrading.

## Planning and governance

Maintainer-facing planning lives under [planning/](planning/README.md); the
map there explains how ideas enter and how decisions are recorded.
Repository guidance for agents and contributors is in
[AGENTS.md](AGENTS.md).

## License

GPL-3.0-only. See [LICENSE](LICENSE).

remanence-lib is copyleft. You may run, study, modify, and redistribute it
freely; any work you distribute that incorporates it must also be
GPL-3.0-only. It cannot be taken into a proprietary product.

Paul Galbraith holds copyright in the project and **reserves the right to
relicense it**, on any terms, at any time. No relicensing is planned or in
preparation — the reservation exists so the option is not lost by default,
not because it is about to be used. It takes nothing back from what has
already been released: every version published under the GPL stays under
the GPL, permanently. Contributions are accepted under a copyright
assignment that keeps the reservation intact; see
[CONTRIBUTING.md](CONTRIBUTING.md).

The name **Remanence** is owned by Paul Galbraith and is not licensed
for use by forks or redistributions. See [TRADEMARKS.md](TRADEMARKS.md).
