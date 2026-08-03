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
