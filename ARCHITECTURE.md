# ARCHITECTURE

The whole-system view, the application surface inventory, and the
architectural principles. This document describes the project **as it
exists today**; vision that has not arrived yet lives under
[planning/](planning/README.md).

## The system

One core, two bindings:

- **`crates/remanence`** — the analysis library, pure Rust, zero runtime
  dependencies. Everything the project knows lives here: the
  format-definition parser and registry, container/filesystem detection,
  the session and identification model, the HDOS directory lister and
  file extractor, the self-contained ZIP/DEFLATE reader that lets
  `Session::open` reach inside archives, and the disk stack —
  the declared-intent deny-write claim, the native qcow2 v2/v3 driver
  with read composition and top-image copy-on-write through backing
  chains, MBR partition discovery, FAT12/FAT16 volume read/write, and the
  commit-point session cache that keeps every write bufferable and
  revocable until committed — reads stream through a bounded working
  set, and altered extents hold in memory or spill to private session
  storage, never the image.
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
  and `HdosFile`, `Error`/`ErrorCategory`/`Result`, and the embedded
  default format definitions. Defined by the crate's `pub` items; `cargo
  doc` output is a representation of it.
- **S2 — The C ABI.** Every `remanence_*` symbol exported by
  `crates/remanence-ffi`, with the generated `include/remanence.h` as its
  consumer-facing representation. Covers naming, ownership rules (who
  frees what), null/out-of-range behavior, and enum values — an ABI
  change is a surface change even when no Rust type changed.
- **S3 — The Python module.** The `remanence` module registered by
  `crates/remanence-py`: its classes, properties, functions, exception
  type and category attribute, and module constants.
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

> **Status: in force.** Every principle on this list is honored by the
> code as it exists today, and **a divergence between a principle here
> and the code is a bug** — not unbuilt work, a defect to fix. Numbers
> come from the one global P-sequence and are never reused.

### P1 — Self-contained format implementations

Every format the library claims, it implements itself — from published
format documentation, in the library, with no external tool, helper
process, or runtime dependency behind any claim. A ZIP is read by our
reader, a DEFLATE stream by our decompressor, a qcow2 by our driver —
never by shelling out. This is what makes the library embeddable from C
and Python without an environment around it.

### P2 — Reading is harmless

Opening, identifying, listing, and extracting never mutate an image —
not a byte. Write access is a separate, explicit request, and every
write path offers a commit point that can be rolled back until it is
committed: altered data stays in the session's cache — in memory or
spilled to private session storage, never the image — and nothing
reaches the file before the commit. An archivist's tool that damages
what it examines has failed at the door.

### P3 — Claims are enumerated and refusals fail closed

What the library recognizes is a named, enumerated claim — formats,
versions, feature subsets — and anything outside the claim is a named
refusal, never a guess, a silent skip, or an untested approximation. A
partition type we cannot read is refused rather than skipped, because
skipping renumbers every volume after it; a qcow2 feature bit we do not
honor names itself in the error.

### P4 — Identification carries its evidence

No verdict without the observations that produced it. Every
identification names its evidence in human-readable terms, and
confidence is bounded and comparable. "h8d, confidence 100" is not an
answer; "matched expected size of 102400 bytes; matched file extension
'.h8d'" is.

### P5 — One semantic surface, three presentations

Every core capability is reachable from Rust, from C, and from Python,
with the same semantics, and a change to the surface lands on all three
presentations in the same change — never deferred. No capability is
binding-private.

### P6 — Unexpected means stop: fail immediately, write nothing, say why

When the library meets a situation it does not expect — a structure
that contradicts itself, a value no claim covers, a state an operation
cannot account for — it **fails immediately**: it writes nothing, and
it gives a clear indication of the reason. No partial update, no
best-effort continuation, no repair attempted on the caller's behalf,
and no error that names a symptom when the cause is known. Two
consequences make the rule operative: surprises are sought before
mutation begins (a mutating operation validates everything it can up
front), and the reason is a diagnostic — what was expected, what was
found, where. P2's commit point is the backstop, not the excuse:
roll-back exists for the interruptions the world inflicts, never as
license to start writing before the checks are done.

### P7 — The file must never change under our feet

The library cannot support a file changing underneath it while it
works — not while writing, not while merely reading. **Denying write
permission to every other process is mandatory in all scenarios**, from
the moment a file is opened, and a file for which that denial cannot be
obtained is not opened at all: fail fast, with the reason named. A disk
image held open for writing by a running VM is the designed refusal.
On the disk stack the caller declares the session's mode at open —
read, or write — and the mode report echoes the declaration. A
writable open that cannot secure its own write access fails at the
open, never by silent fallback, and **a writable session admits no
observers**: its claim excludes every other read or write for the
session's whole life. A read open takes no stronger access than it
needs and keeps admitting other readers, every remanence write action
refused by name. An identification session, which only reads, still
takes the strongest access the file grants — read/write preferred,
read-only otherwise — with writes denied to others either way. The
claim covers every file of a backing chain, consistently: the top
image is claimed per the declared intent, and every backing file is
claimed immutable through this access — writes denied to others, the
library's own access read-only. Contention anywhere in the chain is
an immediate, named failure, never a hidden wait. The
claim is held from open until the session or disk is completely done:
no claim-on-modify, no release-on-save. On Windows the mapping is
native and kernel-enforced (share modes: a writable disk session
shares nothing; every other open shares reads only); on POSIX the
advisory lock is the claim — shared for a disk read open, exclusive
otherwise — binding cooperating processes and asserted as protocol
against the rest.

### P8 — Versioned formats are supported by explicit version, or refused

Where a container format or filesystem declares its version — a version
field, a feature bitmap, anything the format provides for saying "this
is newer than you know" — the library validates it against the versions
it explicitly claims, **before touching anything else**, and a version
or feature bit beyond the claim fails immediately, naming what it found
and what it supports. Read and write alike. Support for a new version
is a deliberate release: understand what changed, implement it, widen
the stated claim, publish. Where the version is not stamped but
versions are known to exist, the library determines the version by
every available means, declares its ceiling all the same, and fails
fast above it — an undeterminable version on a format known to have
them is itself a named refusal. Where a format genuinely carries no
versioning, the claim is structural and P3 governs: FAT width is
decided by cluster count because the format says so, and FAT32 is
refused by name, never guessed at.

### P9 — Interruption never invents a third state

P2 makes commit the only moment the image changes; this principle
armors that moment. An interruption at any point during commit — a
killed process, lost power — leaves state the next open reconciles
**before exposing the disk**, and after reconciliation the image is
wholly the old state or wholly the committed new state, never a partial
third state.

The durable undo journal beneath the overlay is private transient
state: no user-owned file, no cleanup verb, no contract about its shape
or location. A fault-injection harness terminates a separate process
after each durability boundary in commit and proves reconciliation for
raw, standalone qcow2, and backing-chain images; in-process rollback
tests are not evidence for this principle.

### P10 — Every refusal is machine-addressable

A refusal's human diagnostic (P6) is not its interface. Every error
carries, beside its message, a stable machine-readable category
from one enumerated set — the same category in Rust, in C, and in
Python (P5) — so an embedder maps behavior without parsing text no
release promises to keep. The category set is itself part of the
surface: adding a category is a surface change; rewording a message
never is.

The initial set, covering every refusal the library makes today:
`locked`, `invalid-image`, `unsupported`, `read-only`, `not-found`,
`not-directory`, `is-directory`, `no-space`, `io`.

### P11 — Portable Rust comes first

Remanence is written as portable Rust, not as a Windows implementation
with incidental reach elsewhere. Core behavior avoids host-specific
assumptions unless the operating system forces them, and any necessary
platform-specific behavior is isolated behind a small internal boundary.
Public semantics stay the same across platforms; where they cannot, the
difference is a named refusal rather than a silent divergence.

Windows is the directly tested and wheeled platform today. Linux, macOS,
and BSD-family systems are expected to remain buildable from source as a
soft portability obligation, and may become directly tested and wheeled
platforms when repeatable CI or trusted native builders are added. A
support claim names the host tuple it covers rather than letting an
operating-system name imply every architecture that operating system can
run.


### P27 — Sessions stream; memory holds a bounded working set

Remanence is sized by the operation, never by the artifact. A source may
be a floppy image of a few hundred kilobytes or a virtual disk of a
hundred-plus gigabytes; the same open, identify, read, and write journeys
serve both, so no representation — a source's encoding, the session's
durable state, a derived view, or the uncommitted write set — is ever
loaded whole as a design assumption. An operation may visit bytes in
proportion to its task; it may hold only a bounded working set. A whole
layer may be held only when its format bounds it beneath the working set;
every other path streams, and a format that resists streaming is
materialized to private session storage, never to memory.

Every session's durable state has one backing. It is **source-backed**
when bounded random access is served directly from the source encoding —
a raw image by identity, qcow2 through its allocation structures — and
reads stream from the source on demand through the session cache. It is
**session-backed** when it cannot be — a decoded representation whose
encoding permits only sequential access, such as a DEFLATE-compressed
archive entry the session must address randomly — and is then produced
once by a streamed transform into private session storage and served
from there through the same cache.

Caching is per modeled durable layer, under one declared session budget.
The active state's cache carries the session's mutable truth in two
residency classes: **clean state is always evictable** — droppable and
re-read from its backing at will, sound because the P7 claim pins the
source, so a small image simply becomes fully resident while a huge one
converges on the operation's locality — and **dirty state is never
dropped**: alteration is tracked at extent granularity, uncommitted
changes hold in memory within the bound and spill to private session
storage beyond it (P2), eviction moves them, only rollback discards
them, and commit projects them. A derived view's cache, where a session
models one, is an accelerator holding only clean state: its writes
complete into the layer below in the same act or alter nothing, a lower
write invalidates the overlapping derived extents above it, and eviction
regenerates from below.

The library may use threads to predict, prefetch, and offload —
speculatively reading ahead of an access pattern, deriving ahead of
demand, spilling ahead of pressure — with the standard library's threads
alone. Four rules keep the concurrency observationally invisible:
speculation produces only clean state; offload never gaps the truth (an
altered extent leaves memory only once its spill write has completed,
and every act that consumes the altered set joins the offloads in
flight); the work spends the declared budget with demand outranking
prediction; and speculation is silent — a failed speculative read caches
nothing and reports nothing, so results, evidence, and refusals are
identical with any number of threads, including none.

Commit, materialization, and recovery stream like everything else
through bounded buffers; identification probes read the bounded evidence
their claims name; private session storage takes the shape P9 gave the
journal — no user-owned file, no cleanup verb, discardable after
interruption — and the bound and its read-ahead are declared session
configuration with a stated default, never discovered behavior. Public
presentations carry the same rule: an operation whose result is
proportional to source content offers a bounded or streamed form in
Rust, C, and Python alike (P5), with whole-value conveniences beside it,
never as the only route. This principle constrains resources, not
semantics: behavior is identical at every source size, and peak memory
bounded independently of source size is the testable claim this entry
makes.
