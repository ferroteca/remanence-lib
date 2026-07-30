# remanence-lib

[![License](https://img.shields.io/badge/license-GPL--3.0--only-blue.svg)](LICENSE)

Core disk image analysis library for **Remanence Workbench**, in Rust. A
`Session` opens a disk image — raw, or inside a `.zip` archive — and
identifies its container layers: the archive wrapper, the image format, the
physical media geometry, and the probable filesystem, each with a confidence
and the evidence behind it. Detection is driven by plain-text container and
filesystem format definitions, so new formats can be described without code.
An HDOS directory lister reads the file catalog out of Heathkit `.h8d`
images.

The library is dependency-free at runtime, including its own ZIP
central-directory reader and RFC 1951 (DEFLATE) decompressor.

## Layout

```
crates/
  remanence/        # the core library (pure Rust, no runtime dependencies)
  remanence-ffi/    # C ABI: staticlib + cdylib, cbindgen header for C and C++
  remanence-py/     # Python module (PyO3), built with maturin
```

The C++ front-ends (CLI and GTK4 GUI) live in the separate Remanence
Workbench project and consume this library through the C ABI.

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
from that `dist/`, and is owner-gated.

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
let files = remanence::list_hdos_files(session.bytes())?;
```

```python
import remanence
session = remanence.Session("HDOS_1-0.zip")
for c in session.identify().containers:
    print(c.kind, c.id, c.confidence)
for f in session.list_hdos_files():
    print(f.display_name, f.size_sectors, f.modified_date_string)
```

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
