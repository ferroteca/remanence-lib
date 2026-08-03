<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Changelog

All notable changes to remanence-lib are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions here are the workspace SemVer, which is the project's single
upstream version; the PyPI version derives from it (`0.0.1-alpha.1` →
`0.0.1a1`) and is never written by hand. Pre-1.0 the project promises no
backward compatibility: a surface change lands complete across the Rust
crate, the C ABI, and the Python module, and the old shape is deleted
rather than bridged. Read every entry below in that light.

## Unreleased

### Added

- **A capture is mastered into a 1541 flux medium, on all three
  surfaces.** `CaptureSet::plan_c1541_mastering` computes the whole
  reduction and writes nothing; `MasteringPlan::execute` produces the
  medium. Reflected as
  `remanence_capture_set_plan_c1541_mastering` with its accessors, and
  as the Python `plan_c1541_mastering` / `MasteringPlan.execute`.
  **Every reduction is a named policy input** — which captured side
  supplies the family's one recorded surface, which observation of a
  location is used, what to do with a location whose content its
  neighbour also holds, what happens when two transitions land on one
  cycle of the destination frame, how the evidence becomes pulse
  strength, and where the circle begins — each supplied by the caller or
  declared by the profile, each reported in the plan, and each carried
  into the result as provenance. A reduction no input names is a
  refusal, not a default: the prepared capture holds locations whose
  content their neighbour also holds, and the profile refuses them until
  the caller declares which they are, because flux alone cannot tell a
  head reading its neighbour from an instrument that did not move. The
  loss is declared before the medium exists and in the source's own
  terms — the unselected side, the unselected observations, the flux
  outside the bounded revolutions, the marker channels a 1541 never
  observes, the capture's metadata, its foreign records and its transfer
  results, and every transition the destination frame could not express
  apart. A count is not an account, so each entry says what was lost.
  The projection is exact rational arithmetic against both declared
  bases, and the circle begins at the track's own seam rather than at
  the capture's index, a 1541 drive having no index sensor at all. The
  same sources and policy produce the same plan.
- **A capture is recognized as a drive family's, on all three surfaces.**
  `CaptureSet::recognize` consults every enrolled drive profile and ranks
  what claims the capture; `recognize_as` pins one whether or not it
  would have won, and what the caller pinned travels into the result.
  Reflected as `remanence_capture_set_recognize` with its accessors, and
  as the Python `recognize` / `recognize_as` returning `Recognition`.
  A capture no profile claims is a named refusal, and a lone enrolled
  profile never wins by being the only one. The verdict carries the
  observations that produced its confidence rather than the figure
  alone: per zone, how many of its declared locations were recovered and
  what each holds; per source position, the derived cell projected onto
  the family's nominal rotation, the record count, the bit spacing
  between records and how far it departs from repeating, the seam that
  departure locates as an angle, how many observations agreed, and the
  adjacent position holding the same content where one does — reported,
  never resolved, because flux alone cannot tell a head reading its
  neighbour from an instrument that did not move. Recognition stops at
  structure: it reads interval lengths and the patterns they form, and
  resolves no bit, assembles no byte, names no sector and validates no
  checksum. The Commodore 1541 is the first and only enrolled family,
  declared from its published conventions — two drive steps to a track,
  300 RPM against a 16 MHz reference, and the four documented speed
  zones with their track boundaries and sector counts. Probing the
  prepared capture set recovers all four of those zones at their
  documented boundaries with their documented sector counts; the
  half-step positions, the unrecorded surface and the positions past the
  last declared zone are each refused by the rule they broke.
- **A KryoFlux capture set is opened as one capture, on all three
  surfaces.** `CaptureSet` reads a capture of a floppy disk — one stream
  file per head per drive-step position, archived together — out of a
  catalog subtree, and `inspect` reports it as the adapter recognized it:
  every member with its catalog identity, its exact drive-step position
  and head, the transfer read out of it, and the circular observations
  that transfer's index records bracket. Reflected as
  `remanence_capture_set_open` and its accessors, and as the Python
  `CaptureSet` class with its report types. The flux recorded before a
  transfer's first index and after its last is retained rather than
  consumed by the bounding; the transport's own control records and its
  declared transfer result stay beside the run as provenance or as a
  recorded issue; and a record the grammar has no home for is kept
  verbatim rather than dropped. The two heads stay two locations —
  nothing merges them into an ideal disk, chooses a cleanest pass, or
  averages a timing — and no medium, bitstream, sector, or file is
  materialized. What the set admits is an enumerated claim: members of
  one capture, named `<capture><SS>.<H>.raw`, complete across every step
  position and head. An absent, duplicate, contradictory, or unrelated
  member refuses the whole set by name, with the catalog evidence that
  refused it, rather than leaving a member to be read as a disk of its
  own. The capture is timed against the device's exact sample clock,
  which the stream's own rounded declaration is checked against and never
  replaced by. Members are decoded once into private session storage and
  addressed a bounded section at a time, so peak memory follows the
  declared cache bound rather than the capture's size.
- **7z archives are read, and archives are listable, on all three
  surfaces.** `Archive` opens an archive under the deny-write claim and
  reports its entries in the archive's own order, reading the archive's
  index and never its entry data; `Session::open` accepts
  `archive.7z[/entry]` beside `archive.zip[/entry]`. Reflected as
  `remanence_archive_open` and its accessors, and as the Python `Archive`
  class with `ArchiveEntry`. The 7z reader is the library's own — signature
  and header grammar, coded headers, solid folders, per-member CRCs — with
  self-contained LZMA and LZMA2 decompressors beside the existing DEFLATE
  one, so no external program is behind any claim. What it reads is an
  enumerated claim: a single-coder folder using Copy, LZMA, or LZMA2.
  A filter chain, an unimplemented coder, encryption, an external header,
  or an anti-file is refused by name, never delegated or approximated.
  A member of a solid folder decodes only as far as that member's last
  byte, into private session storage, never the folder whole.
- **A declared session cache bound, on all three surfaces.** One bound per
  session governs reads, uncommitted writes, and each commit's capture,
  rounded up to whole 64 KiB extents with one extent as the floor —
  narrowing the working set, never refusing the work.
  `Session::open_with_cache`, `Disk::open_with_cache` and
  `DEFAULT_CACHE_BYTES`; `remanence_session_open_with_cache`,
  `remanence_disk_open_with_cache` and `remanence_default_cache_bytes`; a
  `cache_bytes` keyword on both Python constructors and the matching module
  constant.
- **Bounded session reads.** `Session::size_bytes()` and
  `Session::read_at()`, with `remanence_session_size_bytes` /
  `remanence_session_read_at` and the Python `size_bytes` / `read_at`,
  replacing the whole-image byte accessor.
- **Streamed file read and write beside the whole-file verbs.**
  `read_file_at`, `resize_file` and `write_file_at` walk only the clusters
  covering the span; `resize_file` preserves kept bytes, releases surplus
  clusters, and zeroes growth including the stale tail of a partial last
  cluster. Reflected as `remanence_disk_read_file_at`,
  `remanence_disk_resize_file`, `remanence_disk_write_file_at`, and in
  Python as the `pread`/`pwrite`/`truncate` idiom. A span past the file's
  size is a refusal, never a silent clamp. The whole-file `read_file` and
  `write_file` remain as the conveniences.

### Changed

- **Archive grammars sit behind a common catalog seam.** The ZIP reader
  became the ZIP catalog adapter beside the new 7z one, both reached by
  enrollment on the extensions they claim. An archive path with no entry
  still resolves when the archive holds exactly one file, in any grammar,
  and the refusal when it holds several now names the archive rather than
  a `.zip` suffix.

- **Image formats are executable modules behind role-specific built-in
  catalogs.** H8D and qcow2 image adapters, ZIP serialized-container
  handling, MBR partition-layout discovery, and HDOS/CP/M filesystem
  adapters own their recognition, evidence, validation, and behavior.
  Catalogs select only a unique strongest match; ties remain unknown with
  competing evidence, and recognized-invalid inputs keep their refusal.
  Each loaded disk carries its authoritative layer, active durable layer,
  derivation provenance, and a composition-scoped device identity.

- **Sessions stream, and memory holds a bounded working set** (P27, armed
  with this release). No representation is loaded whole as a design
  assumption: identification probes read the evidence their claims name;
  ZIP entries are read in place when stored and decoded once into private
  session storage when deflated; reads and uncommitted writes pass through
  a bounded session cache whose clean extents evict and re-read while
  altered extents spill to private session storage, never to the image; and
  the commit pipeline captures and journals through bounded buffers. Peak
  memory is bounded independently of source size, and behavior is identical
  at every size.
- **Reads may prefetch and the cache may offload, using threads.** A
  predictive reader fills ahead of a sequential access pattern and the
  session cache pre-spills altered extents under pressure, with the
  standard library's threads alone. Speculation produces only clean state,
  never gaps the truth, spends the declared budget behind demand, and fails
  silently — results, evidence, and refusals are identical with any number
  of threads, including none. No public surface changed.
- **One Python toolchain for the whole repository.** The root
  `pyproject.toml` is a virtual uv workspace whose sole member is
  `crates/remanence-py`, and it carries the test-fixture preparation
  dependency group, so one uv install serves building, publishing, and
  fixture prep.

### Removed

- **The caller-authored format registry and definition language.**
  `FormatRegistry`, `ContainerFormat`, `FilesystemFormat`,
  `DiskImage`, the default definition constants and parser helpers,
  `Session::open_with_registry` / `Session::registry`, their Python
  reflections, and the built-in definition files are gone. Formats are
  implemented modules; there is no compatibility parser or deprecated shim.

- `Session::bytes()`, `remanence_session_bytes`, and the Python `bytes`
  property, which required the whole image to be resident. Use the bounded
  `read_at` accessors above.

## 0.0.1-alpha.2 - 2026-07-31

### Added

- **Declared access intent at open.** `Disk::open` takes an access intent
  and the mode report echoes the declaration. A writable open that cannot
  secure its claim fails at the open, naming the reason, and a writable
  session admits no observers for its whole life; a read open denies writes
  to others while continuing to admit readers.
- **Machine-addressable refusals.** Every error carries a stable category
  from one enumerated set — the same category in Rust, C, and Python — so
  an embedder maps behavior without parsing diagnostic text.
- **The complete partition and volume report.** Blank is an answer: an
  all-zero sector 0 reports a blank disk rather than an error, and nonblank
  content that is neither a partition table nor a recognized volume is
  refused as invalid by name. Every declared partition row is reported with
  its kind, its pinned type name where the type is inside the claim, and a
  structured issue where it is not — a row outside the claim or one whose
  volume cannot be read keeps its number, so the volumes behind it never
  renumber. Chain faults attach to the extended container row and stop the
  walk instead of failing the disk. A volume's cylinders are derived only
  where the boot record's stated track geometry divides the total sector
  count exactly, and are otherwise absent rather than invented.
- **Stable volume identifiers.** Opaque identifiers issued by the report,
  accepted by every file verb, with a missing identity refused by name.
- **`stat`, in-place overwrite, and recursive directory creation.** One
  path answers with its entry or with an absence distinguished from
  failure; a write replaces an existing file's contents, shorter or longer,
  releasing and reclaiming clusters with both FAT copies kept in step; a
  directory creation creates missing parents and succeeds when the
  directory already exists.
- **qcow2 backing chains, read and written.** Reads compose through the
  chain — unallocated clusters falling through, v3 zero clusters masking
  the backing, compressed clusters decompressed wherever they sit, a short
  backing reading zero past its end — to a claimed depth of 16 files with
  cycle detection, every member gated by its version and features and
  claimed immutable for the session's life. Writes allocate copy-on-write
  into the top image only; a backing file is never modified and the chain
  is never flattened.
- **Durable commit, and proof that interruption invents no third state.**
  Host-level writes stage in a capture of the top image and a sealed undo
  journal is armed beside it before the first byte moves; the next open
  reconciles before exposing the disk, leaving the image wholly old or
  wholly new. A fault-injection harness terminates a subprocess after each
  durability boundary and verifies recovery for raw, standalone qcow2, and
  backing-chain images.
- **Portable Rust as a stated rule.** Host-specific behavior is isolated
  behind a small internal boundary, and public semantics stay the same
  across platforms or name their difference as a refusal.

### Changed

- **C ABI symbols renamed `Rmn*` → `Remanence*`** across enums, structs,
  and functions, aligning the ABI with the Rust names it reflects.
- **"At rest" left the library's vocabulary.** The read/write stack is
  named by its own API — the `Disk` surface, in prose the disk stack. The
  term borrowed a consuming application's frame, distinguished nothing
  inside this library, and collided with the security sense of "data at
  rest". No symbol carried it.

### Removed

- The access-mode fallback ladder on the disk stack: intent is declared at
  open and never silently downgraded. The identification session keeps its
  ladder, which only ever reads.
- The one-argument Python `Disk(path)` spelling; `writable` is required and
  keyword-only.

## 0.0.1-alpha.1 - 2026-07-30

The first published version: the Rust port of the core library, and the
disk stack on top of it.

### Added

- **The core library.** Format-definition registry and parser, container
  and filesystem detection, the session identification model with layered
  evidence, the HDOS directory lister and file extractor, and a
  self-contained ZIP reader and RFC 1951 inflate implementation — so an
  archive is read, and a DEFLATE stream decompressed, by this library
  rather than by anything shelled out to. The core has no runtime
  dependencies.
- **The disk stack.** A native qcow2 v2/v3 driver validating its version
  and feature bits before anything else and decompressing clusters through
  the crate's own inflate; a deny-write claim taken at every open, with a
  writable open failing fast when another process holds write access; a
  commit point at which nothing has touched the host file until it is
  committed, and which rolls back cleanly until then; an MBR partition walk
  with pinned types; FAT12/FAT16 volume read and write; and the public
  `Disk` API over all of it.
- **Three presentations of one semantic surface.** The C ABI
  (`crates/remanence-ffi`) with its cbindgen-generated header and an
  example C consumer, and the Python module (`crates/remanence-py`, PyO3,
  abi3, Python ≥ 3.10) mirroring the public surface. The Python package
  claims Windows only — the platform the project tests.
- **uv as the Python build and publish frontend**, driving the maturin
  backend in an isolated environment.

### Changed

- Python may no longer construct the data-model types directly. They are
  library-produced values returned to callers, and constructing one by hand
  could only misrepresent an image.

### Removed

- The vintage HDOS distribution images left the repository and every
  published artifact. They are third-party material the project cannot
  establish title to, so it does not distribute them; the test-fixture
  preparation script fetches them under a pinned hash instead, and tests
  that need them say so by name when they are absent.
