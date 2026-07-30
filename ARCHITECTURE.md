# ARCHITECTURE

The whole-system view, and the application surface inventory. This document
describes the project **as it exists today**; vision that has not arrived
yet lives under [planning/](planning/README.md).

## The system

One core, two bindings:

- **`crates/remanence`** — the analysis library, pure Rust, zero runtime
  dependencies. Everything the project knows lives here: the
  format-definition parser and registry, container/filesystem detection,
  the session and identification model, the HDOS directory lister, and the
  self-contained ZIP/DEFLATE reader that lets `Session::open` reach inside
  archives.
- **`crates/remanence-ffi`** — a C ABI over the core: opaque handles,
  accessor functions, borrowed strings owned by their handle. The header
  `include/remanence.h` is generated from the Rust signatures by cbindgen
  at build time; the Rust `extern "C"` items are the definition and the
  header is a first-class representation of them, not a rival.
- **`crates/remanence-py`** — a Python module over the core (PyO3), a
  deliberate mirror of the Rust public surface in Python idiom.

The bindings contain no analysis logic; a behavior lives in the core or it
does not exist. The C++ Remanence Workbench front-ends consume the C ABI
from their own repository.

## The application surfaces

The surfaces through which the world drives or reads this project,
enumerated here in one place so downstream rules answer "does this touch an
application surface?" by lookup, not judgement. Numbers are permanent and
never reused.

- **S1 — The Rust crate API.** The public surface of `crates/remanence`:
  `Session`, `Identification` and the container/layout types,
  `FormatRegistry` and the format types, `DiskImage`, `list_hdos_files`
  and `HdosFile`, `Error`/`Result`, and the embedded default format
  definitions. Defined by the crate's `pub` items; `cargo doc` output is a
  representation of it.
- **S2 — The C ABI.** Every `rmn_*` symbol exported by
  `crates/remanence-ffi`, with the generated `include/remanence.h` as its
  consumer-facing representation. Covers naming, ownership rules (who
  frees what), null/out-of-range behavior, and enum values — an ABI
  change is a surface change even when no Rust type changed.
- **S3 — The Python module.** The `remanence` module registered by
  `crates/remanence-py`: its classes, properties, functions, exception
  type, and module constants.
- **S4 — The format-definition text format.** The `[section]` /
  `key = value` dialect parsed by `FormatRegistry` — section kinds,
  known keys and their types, list syntax, comment and attribute
  handling — including the built-in starter definitions under
  `crates/remanence/formats/`. Users author files in this dialect, so its
  grammar and semantics are a world-facing contract.

**Norms today are the code.** No prose specification has been written for
any surface yet; the defining code (and for S2, the generated header) is
the authority, which relocates vetting onto review of changes to it. Prose
norms are future work the owner may pledge; when one lands, it becomes the
single norm for its surface and this section names it.

## The architectural principles

None are armed yet. The P-numbered list is the owner's to dictate, and an
in-force entry asserts the code honors it today — so this list stays empty
rather than carrying placeholders. Drafts will appear under
`planning/proposed/ARCHITECTURE.md` when dictated.
