# AGENTS.md — repository guidance

This is the canonical, agent-agnostic guidance for working on remanence-lib.
Human usage documentation belongs in [README.md](README.md); keep this file
focused on repository structure, engineering constraints, verification, and
maintenance context.

## Project state and layout

remanence-lib is a reusable disk image analysis library, ported to Rust
from an earlier implementation lineage. The Rust code here is the
authoritative implementation; callers consume it through the Rust API, C
ABI, or Python module.

- `crates/remanence/` — the core library, laid out under `src/` in
  **eight groups plus the root**. The grouping is the architecture made
  physical: each group's `mod.rs` states its own seam and the principles
  that govern it, and `lib.rs` declares the groups and nothing else. The
  paragraphs below are the map from a principle to a path.

  **The root** holds what every group stands on and what belongs to no
  group. `error.rs` owns the error taxonomy (`Error`, three diagnostic
  variants, the stable `ErrorCategory` set, and the rule identity beside
  it — a value the seam owning the broken rule spells, never a second
  global set; display messages remain human diagnostics); `evidence.rs`
  the vocabulary a derived fact carries its basis in — declared facts,
  issues, provenance and the declared-loss account; `checksum.rs` the
  small checks several formats share.

  **`codec/`** — the compression the library owns (P1), self-contained
  by construction: `inflate.rs` the streaming RFC 1951 DEFLATE decoder,
  `deflate.rs` its encoder counterpart (deterministic within this
  implementation; cross-implementation byte identity deliberately
  unclaimed), and `lzma.rs` the streaming LZMA/LZMA2 decoders. Every one
  of them decodes through its own LZ window into private session
  storage, so nothing is resident whole.

  **`io/`** — bytes, claims, and the bound over them; everything above
  reads and writes through here. `handle.rs` is the **caller-owned
  claim** (P7): what a handed-over `std::fs::File` affords, asked by a
  zero-length write that changes nothing, and the name recovered from it
  for **location only**, under an identity check — a nameless handle
  refusing the commit journal's *beside* and a backing parent's *next
  door* by name and serving everything else. `device.rs` is the
  block-device seam, the P7 claims (the library's own declared-intent
  open, the discovery ladder for identification sessions, and the
  `Claim` class that says whose open a medium's is — the library's, the
  caller's, or nobody's, an authored medium having been opened by no
  one), and the host-write capture a durable commit stages into.
  `cache.rs` is the session cache — the P2 commit buffer and the bounded
  working set (P27): unaltered extents evict and re-read from the image,
  altered extents spill to private session storage, and the bound is
  declared at open with a stated default. `journal.rs` is the durable
  commit's intent record (P9), and `source.rs` resolves a file named by
  path, or one entry named through the file view that reaches it, under
  the archive's own claim.

  **`model/`** — the session model: the pools, the node a caller holds,
  and the three fact classes that fill them.

  `machine.rs` is the session and its two pools (P32) — the session
  being the claim and cache scope, owning the **media pool** (state) and
  the machines (configuration) independently of each other, a machine
  being one device set with its own attachment identities and attachment
  order, and the anonymous machine being the one whose identity is null;
  **every pool runs the same three verbs — create, look up, release** —
  where a lookup (`machine`, `device`, `medium`) answers with an
  `Option` and nothing is manufactured to report absence, there is no
  `require_*` form at all (a caller who wants a demand writes it, where
  they know what the absence means), creation still refuses by name
  (duplicate identity, taken slot, empty identity), and the removals are
  all spelled `release_*`: `release_machine` cascades through the
  configuration below it, `release_device` ejects first, and
  `release_media` severs its own link then ends the claim. `load_media`
  and `load_discovery`/`load_discovery_as` — the plain door and the
  declared one, the second taking the device type a format recording
  several leaves to the caller — fill the media pool, as does
  `new_media`, the authorship door where the caller has no artifact at
  all, and `MachineView`/`DeviceView` are the borrows that hold a node
  and the pool at once, since linking is the one act that crosses.

  `storage_device.rs` is the **slot**: its attachment identity (`hdd0`),
  the acts that fill it (`add_device` on the machine view, then
  `insert`/`eject`, with an empty device first-class configuration, a
  medium in the wrong drive refused naming both sides, and **eject
  severing only** so the claim and buffered writes survive pooled), and
  the one convenience over discovery that composes the acts
  (`add_device_for`, adding a device of the format-declared default
  family and refusing by name where a format declares none) — **and
  nothing else: every content verb lives on the medium**, file access
  included, because a device holding a partitionable medium and bearing
  `get_file` would be a category error in the type rather than a refusal
  waiting to happen.

  `media.rs` is the **media pool and the medium a caller holds** — the
  declared `Format` set with each format's declared source shape, the
  `MediaSource` conversions (the caller's opened file, a collection of
  them, a `FileSource` from an archive medium's namespace, a collection
  of those), the pool identity, and every content verb (identify,
  inspect, read_at, the space and namespace doors, the discovered
  geometry and the `read_sector`/`write_sector` pair that addresses in it,
  the argument-free `bitstream`/`bytestream` pair a flux medium answers,
  commit and rollback), a medium being created by a declared reading
  **or by its author** (`authored_as` saying which) and destroyed only
  by `release_media`; `disk/` is the private state a medium homes, in
  five files. `disk/mod.rs` holds `DiskFormat` and `MediumState`, the
  enum over the four families and the dispatch every verb above enters
  through — **families own their representation** (P14) at the state
  tier, so asking an archive to inspect partitions is a category error
  answered by name rather than a hole to fall into.
  `disk/state.rs` is `MediaState` itself: the two planes one P7 claim
  serves (F43) — the raw bytes and the disk a format adapter presents
  above them — with `Composed` and `Window` as those planes' `Device`
  faces and `assess` the narrow P28 gate that settles which of the two
  an open gets. `disk/recognition.rs` is what a load recognized before
  it becomes state, which is what a discovery holds and a load consumes
  (see `discovery.rs` below). `disk/files.rs` is the namespace verbs
  and the two guards every write passes first, and `disk/commit.rs` the
  durable commit (P9) with the crash harness that proves it. Because
  one type's verbs live in four files, `MediaState`'s fields and a
  handful of its methods are `pub(super)` — visible inside `disk/` and
  nowhere else. `disk/fixtures.rs` is `#[cfg(test)]` only: the image
  builders the commit and end-to-end tests share.

  The three fact classes meet here. **Discovery reads**: `discovery.rs`
  is the first-class `discover_media`, on no handle at all — the claim,
  the identification, the exact article, the devices derived from the
  catalog's own declarations as accepting it, and the device the format
  records where it records one — answered as a consumable handle a load
  takes the claim out of, so nothing runs twice and no window opens
  between the question and the load. **It holds the claim and builds no
  cache**: no medium, no session cache, no spilled backing, the bound
  being the load's own declaration and stated at `load_discovery`, where
  the medium comes into existence; what a discovery holds instead is the
  `MediumRecognition` beneath it — the claimed source, the adapter that
  claimed it, and what the assurance gate settled — which the load turns
  into state over the very claim already held.

  **Declaration configures**: `device_type.rs` is the P14 recording
  seam — the **device-type catalog** in its two levels (the class —
  floppy, hard-drive and optical, with tape reserved — then the
  concrete type), one spec shape per class and one instance per
  concrete type, the granularity rule that cuts it, the article each
  type composes, the flux path it claims, the partition scheme the
  hard-drive specs carry, and the **addressing** every type declares —
  `sector` or `block`, which is the type's half of the sector verbs and
  the medium's discovered geometry the other — beside `DeviceSlot`,
  which is a device type or the archive receiver, the receiver being no
  recording device at all. `media_profile.rs` is the P14 substrate
  seam — the passive compatibility facts of an **article**,
  family-specific by construction (flexible magnetic, logical-block and
  optical are claimed, with no fact in common — a coercivity, a block
  size and a track pitch in nanometres each mean nothing to the other
  two), and the declarative article
  catalog they are enrolled in, which holds no recognition, no grammar
  and no behavior; every medium the library holds names one entry, a
  block medium from the image-format adapter that loaded its state, a
  flux medium from the drive profile's declaration of what its family is
  served, and an authored one from the kind its author declared — the
  virtual family holding two entries for the two things nobody
  manufactured, the archive whose native vantage is a namespace and the
  **authored** article whose vantage is a space. The optical entry is
  the pressed 120 mm disc, and it **declares the disc rather than
  anything recorded on it**: sessions, tracks, gaps, audio and
  subchannels are a recording's facts and belong to the optical state
  model, which is proposed and unclaimed. Nothing reads ISO 9660
  either, and the gap is visible rather than quiet: the schemeless
  content classifier reads sector 0, ISO 9660's first descriptor is at
  sector 16 behind a normally-zero system area, so a data disc inspects
  as `blank` and `session_devices.rs` asserts that it does.

  **Authorship creates media whole**: `authored.rs` is the third fact
  class, where discovery reads and declaration configures — the
  enumerated `NewMedia` kinds (the blank article kinds, each spelled by
  the article it makes with nothing recorded on it, and
  `ChsDisk { geometry }`, whose facts *are* coordinates), the check the
  author's statement passes at the one moment authorship offers, the
  provenance it becomes, and the session-backed sparse blank the content
  lives on — P2's commit point over it with no journal beneath, because
  no file changes for an interruption to leave half-written, and no
  device assumed, the authored-to-recorded arc that would bind one being
  reserved.

  What an open established travels with the medium. `assurance.rs` is
  the P28 gate — the outcome one open established, the enumerated
  condition set a withheld operation names as its rule, the ordered
  evidence, the exact readable extents, and the effective access mode,
  with the read bound carried to where the reads happen. `geometry.rs`
  is the discovered-geometry seam — the recording's own coordinates as
  *evidence*: the enumerated sources (the format's declaration or a raw
  load's declared block size, a FAT boot record's recorded track
  geometry, the partition table's end tuples solved against the extent
  the same entry declares, and extent arithmetic for the cylinder
  count), each reading kept with where it was taken, what they settle
  between them, and **`Undetermined` where two of them disagree** — both
  readings standing, neither preferred — beside `Unstated`, which is the
  different fact that nothing spoke at all; the coordinate arithmetic
  and the `GeometryRule` set the sector verbs refuse by are here too,
  beside `Authorship` — the one source that is no reading of an
  artifact, belonging to the one medium that has none. Geometry is
  established at the load beside the partition pool, or stated by the
  author in the act that creates the medium, and never declared onto a
  medium that exists; which types *have* coordinates is the device
  type's own `addressing` declaration, and how many of each is this.
  `session.rs` is the layered identification model — the layers of an
  artifact's nesting, reached through the medium — `report.rs` the
  layered inspection report a medium's records are returned in (device,
  content outcome, partition schema, regions, volumes, filesystems,
  joined by opaque layout-derived identities), and `volume.rs` the P17
  composition seam.

  **`image/`** — block image formats, implementations at representation
  seams (P12). `adapters.rs` is the catalog and the wiring: the
  executable image-format adapters, probe aggregation,
  authoritative/active layer vocabulary, device identity, the built-in
  image catalog, and each format's **recorded device types** — the
  recording-side fact an article cannot hold: one means the format
  carries the type bare, several mean the load declares which, and none
  is an archive grammar. `qcow2.rs` is the native qcow2 v2/v3 driver (P8
  version gate first, run for every member of a backing chain; chains
  compose for reading and allocate writes into the top image only, with
  each backing file claimed immutable; write path refuses snapshots and
  non-16-bit refcounts by name). `vdi.rs` is the native VDI driver (P8
  version gate first, then the enumerated image-type claim — dynamically
  allocated, fixed and differencing, with undo refused by name; the
  block map stays in the file and is read where it is needed, so the
  driver holds no mutable state and a block a dynamic image never
  allocated is allocated on the write path alone; a differencing chain
  composes for reading and takes writes copy-on-write into the top image
  only, and because the format records the parent's identity and no
  path, the parent is **searched for by identity** — beside the child,
  then the directory above it — with the identity checking the file the
  search found rather than a path being trusted). The flux family's
  formats are deliberately **not** here: block and flux are disjoint
  (P13), so a flux artifact is reached through its own type rather than
  by opening a byte-addressed device.

  **`partition/`** — `mod.rs` is the partition-layout catalog and the
  vantage doors (P16, P17, P19), with `mbr.rs` the one scheme reader
  beneath it: partition discovery with pinned types.

  **`filesystem/`** — the P19 seam, whose head is four files.
  `filesystem/mod.rs` is the shared vocabulary every layer here speaks:
  the one `Entry` set with the facts a filesystem declares in its own
  spelling, the `Catalog` trait a flat on-medium namespace is reached
  by, and the enumerated `SpaceRule` set its refusals name.
  `filesystem/space.rs` is the node itself — the public `StorageSpace`
  carrying **two vantage traits on one object**, addressable I/O within
  its own extent and namespace I/O over the files it names, so that a
  FAT volume has both, a volume bearing no filesystem has only the
  first, and a medium's own namespace only the second (the 0..1 as trait
  presence rather than prose) — beside the `File` view and the resolver
  that walks device → volume → namespace, every seam having one
  supported answer and refusing naming the candidates where it does not.
  **The file verbs live there and on nothing else**, including for a
  namespace no device composed, where the node is the same one with its
  device and its extent absent rather than a second type carrying the
  same verbs.

  Beneath the node, `filesystem/contract.rs` is what an adapter presents
  *through*: a `RecordedName` keeping the bytes as written beside the
  encoding claimed for them, an `ItemRef` so several names may reach one
  item, a `SizeClaim` recording what the size is a claim *about*, a
  `ContentSource` that stays a bounded descriptor rather than bytes, and
  a `ForeignRecord` keeping whole what this layer cannot name — with
  `FilesystemView` the trait and its refusals attributed to the provider
  that presents rather than to the seam. `filesystem/coverage.rs` is the
  account over a floor, **total because the remainder is derived** rather
  than declared, so a provider cannot leave a hole by forgetting to
  mention one; overlapping claims are refused naming both sides, a claim
  past the floor is refused in the floor's own units, and an opaque
  region is accounted and never named. `filesystem/fixtures.rs` is
  `#[cfg(test)]` only: the one synthetic provider both test modules
  drive, so they cannot drift onto two providers that disagree.
  `space.rs` carries no unit tests of its own — `StorageSpace` is
  exercised end to end from `tests/`, over real volumes rather than a
  synthetic one.

  `catalog.rs` holds the streamed filesystem adapters and catalog for
  the namespaces a medium bears directly (crate-private, reached through
  the device's `identify` and through the resolver — the adapter that
  recognized a namespace being the one that opens it, so nothing
  branches on a filesystem identifier). Beneath it sits one module per
  filesystem: `fat.rs` FAT12/16 volume read/write, with `dos_name.rs`
  owning every 8.3 name decision it makes — reading a stored name,
  matching one without regard to case, storing a caller's, and the
  seven-rule set a refusal names; `hdos.rs` the HDOS directory
  lister and file extractor with the `Catalog` adapter over it, private behind the
  namespace node; and `cbm_dos.rs` the P18
  adapter at the top of the flux ladder — the directory CBM DOS wrote,
  read through a `BlockSource` and nothing else, so the filesystem never
  learns what it is standing on: the BAM header as the space's label,
  the directory chain in its own order, PETSCII names read beside the
  sixteen bytes as recorded, the CBM facts as declared entry facts, and
  byte sizes established by walking each chain — with the recorded block
  count kept where a block that never came back stops the walk, so one
  unreadable sector qualifies its own entry instead of taking the
  listing down.

  **`archive/`** — `mod.rs` is the archive **medium** and the catalog
  seam beneath it: the `ArchiveCatalog` trait and the enrollment each
  grammar is reached by, the `ArchiveMedium` an archive-family device
  holds, and the namespace it presents through the same `Catalog` seam a
  flat on-medium catalog does. `zip.rs` is the self-contained ZIP
  central-directory catalog and `sevenzip.rs` the 7z header reader —
  archives are read in place by positioned reads, and a coded entry
  decodes through `codec/`'s decompressors into private session storage,
  never resident whole; the 7z claim is a single-coder folder using
  Copy, LZMA, or LZMA2, and everything outside it refuses by name.

  **`flux/`** — the flux family (P22), and the largest group: magnetic
  recording descended to timed flux transitions, and the ladder that
  reads back up from them. Its dependence on the rest of the crate is
  deliberately thin — `error`, `evidence`, `io`'s bytes and claims,
  `codec`'s DEFLATE pair, and the two named crossings noted below.

  **The family holds two models.** `capture/` is the private
  flux-capture model, in four files. `capture/records.rs` is what a
  capture is made of — exact timebases, the markers and foreign records
  a source wrote in the order it wrote them, and the `TrackKey` and
  fractional `SourcePosition` that address a location in the source's
  own terms, unrounded. `capture/wire.rs` is the byte grammar those
  records are written in: varints and length-prefixed text underneath,
  and above them the two chunk forms — transitions delta-coded,
  markers at absolute positions — each decode refusing rather than
  partly believing a chunk, and each split deterministic so a section
  key means one thing forever. `capture/sections.rs` is the
  section-addressable backing: records streamed to a `ByteSink`, an
  ordered index behind them, and a reader that walks to the one section
  asked for and reads nothing else (P27) — with `SectionAddress` the
  seam that lets one backing serve both of the family's models, which
  do not address alike. `capture/mod.rs` is the model those three
  compose: locations, capture runs, circular observations, and the
  builder and metadata codec over them. Only `records`' three key
  fields are `pub(super)`, read by the backing to write a section key
  and by nothing else. Above it all sits `kryoflux.rs`, the KryoFlux
  capture-set adapter: the member grammar and its completeness, the stream
  grammar, and the assembler that reads one disk out of a declared
  collection of members — a stream per head per drive-step position —
  for the collection-sourced load. `medium.rs` is the second model, what
  a drive would read rather than what an instrument recorded — one
  circular pulse stream per family-addressed location, an exact
  rotational frame, per-pulse strength, and the medium-level facts
  beside them — always derived and never constructible without the
  policy that produced it, over the same backing keyed by the family's
  own addressing. `analysis.rs` is the gap-first reconstruction's
  numeric core over plain arrays — the cell lattice from a comb
  periodogram with per-context peak-shift medians and the alternation
  parity, the gap correspondence's resynchronising walk, the gap-first
  integration whose closure solves the cell exactly, the coherence rule,
  the fat-track comparison, and the orbit clock — floats measure,
  integers state.

  `drive_profile/` is the P30 seam, in four files with the dependency
  running one way. `drive_profile/mod.rs` is the **declaration
  vocabulary** — the types a family's facts are stated in (stepping,
  rotation, surfaces, encoding shape, density map, the read-channel and
  group-code declarations, the record grammar) and the enrollment list.
  It holds no entry and no behaviour, which is why it carries no tests
  of its own: what there is to test is the entry filled into it.
  `drive_profile/entries.rs` holds the enrolled families — today the
  C1541 alone — and **each entry is declared whole in one place**, its
  recognition half beside its materialization half, because these are
  facts about the same drive and splitting them across two places is
  how two features come to hold different answers about one of them.
  `drive_profile/intervals.rs` is the measurement: the cell derived as
  the rational the interval population is self-consistent with, and
  every interval classified into a declared multiple by exact integer
  arithmetic — recognition stops at structure, so what leaves it is a
  count, a density, an angle, a location and an absence, never a
  resolved bit or an assembled byte. `drive_profile/verdict.rs` is what
  that measurement is reported as: `probe` over one profile,
  `recognize`/`recognize_as` over the enrollment, and `recognition` as
  the whole act — ranked, carrying the observations that produced them
  (P4), and refusing by name where no profile claims a capture, a lone
  enrolled entry never winning by being alone. Every rung reads its
  rules through this seam, which is why the rungs take no policy
  arguments.

  `bitstream.rs` and `bytestream.rs` are the two P23 layers above the
  medium — circular track-relative clocked bit state, every bit saying
  whether it was recorded or resolved by a declared rule, and the byte
  sequence a declared group code makes of it, which assigns no header,
  sector or file to any of them. Both are family-agnostic, which is why
  they sit here rather than under `c1541/`.

  `c1541/presentation.rs` is the family's read channel and GCR codec
  above both: the declared policies of each transition — the profile's
  own declarations, read argument-free through the type (P30), with the
  deviation surfaces deferred (D29) — the clocking, the framing, the
  `Location`-addressed framed-byte reads, and the account of what each
  layer does not carry from the one below. **The ladder's two entry
  points are a flux medium and a remanence image**: the medium a flux
  load pools answers `bitstream()`/`bytestream()` directly, and an image
  carries no clock, so its entry stands on the served projection of it
  rather than on the image directly.

  `c1541/sectors.rs` is the rung above them — the **seam where the
  bytestream's silence ends**, and it ends by a new layer stating what
  it derives: the family's declared record grammar (which byte opens
  each block, how long it is, where the header states track, sector and
  disk identity, which bytes each checksum covers) recognized over the
  bytestream's own runs, one record per framing the layer below
  declared, pairing grammatical rather than metric, every claim carrying
  both checksums stated beside computed, and reads by the recording's
  own (track, sector) refusing by name — its own `SectorRule` set —
  where nothing states an address, where no claim of one reads, or where
  several readable claims disagree. It is derived rather than a seventh
  active layer, and its payloads stream to private session storage as
  they are recognized; **the filesystem door is on it** — `filesystem()`
  answering the same `StorageSpace` a device resolves to, because the
  file verbs live on the namespace and on nothing else, and a space
  presented over a layer no device composed carries the namespace
  vantage alone. Its `impl BlockSource` is one of the group's two
  crossings back into the core.

  `c1541/renditions.rs` masters the d64, g64 and p64 renditions off the
  remanence image — each claimed twice, as a `describe_` that computes
  everything and writes nothing and a `write_` that does both, and each
  stating what its destination did not carry (P29): clocking, the
  crate-private GCR group code and sector reading (analysis machinery,
  deliberately **not** the F61 sector surface), the CBM DOS 683-block
  grid with the error map as the d64's declared-loss account made flesh,
  the `GCR-1541` grammar, and the served projection into the delivered
  P64 encode path.

  `remanence/` is the family's **physical stratum** — the disk's own
  magnetization, below every clock and every code. `image.rs` is the
  public `FluxImage` root, which answers the image's *shape* and
  nothing below it (form factor, the angular unit, holes, surfaces, and
  per orbit its radius and counts), over the crate-private model it is a
  face of: form factor, holes as angular data, and per surface the
  orbits, each an ordered circular array of packed 32-bit points
  (28-bit angle high, magnetization and widths flags low) held as
  cache-backed chunks over the family's section-addressable backing,
  with the model's own invariants (alternation, the rewrite splice, one
  radius one recording), the refinement where only ignorance may be
  overwritten, and the reversal where spans reverse rather than points.
  `format.rs` is the `.remanence` artifact, claimed in both directions
  and carrying the root's own `open`/`open_with_cache`/`write` — a flux
  artifact is reached through its own type rather than a device, as the
  capture set and the P64 image are, block and flux being disjoint
  families (P13) — magic, binary sentinel, version gate, then one
  zlib-framed DEFLATE payload of varint angle deltas with the
  magnetization byte elided wherever alternation derives it, read and
  written through `codec/` (the P29 account empty because the artifact
  is the model's own). `reconstruction.rs` is the P29 reduction from an
  opened capture to a remanence image under a declared policy: every
  revolution of every location aligned and integrated, recordings
  measured by the count-spread discriminator or declared, the fat track
  merged under measured agreement, the plan/execute split and the
  declared-loss account, survey facts riding provenance with their basis
  stated — and **it answers with the image itself**, the same root a
  `.remanence` artifact opens to, rather than a second root beside it,
  the account of how it came to be belonging to the plan that computed
  it. Its 7z gather is the group's other crossing into the core.

  `p64.rs` is the P64 image-format adapter, claimed in both directions —
  the container grammar and its own range coder, the version gate and
  the structural refusals, decode of a stored medium into the
  flux-medium layer (the served form `Format::P64` loads straight in),
  and encode of a projected image into a new artifact under a claim
  stated before the file exists. It sits outside `image/adapters.rs`'s
  catalog deliberately: that catalog's adapters open a byte-addressed
  device, and block and flux are disjoint families (P13), so a flux load
  builds a flux medium rather than opening a block device.

  `load.rs` is the group's **one seam into the session model**: the two
  declared flux loads — a KryoFlux collection checked whole (member
  grammar, completeness, stream grammar, the declared device's profile
  claim) then reduced under the profile's declared `Materialization`
  defaults, and a P64 decoded straight in — the verdicts, policy and
  declared-loss account riding the medium as provenance, and the
  presentation ladder materialized once on demand under the profile's
  declarations. Everything else in `flux/` reaches the core only for
  bytes, a claim and a cache bound, which is what keeps the group
  separable.

  Unit tests live in their modules; integration tests in `tests/` —
  synthetic FAT/MBR/qcow2/VDI images built in-test, including the
  truncated floppy the degraded reading is stated over, plus the
  fixture-driven HDOS tests. A test that names its own path in a string
  (the commit crash harness in `model/disk/fixtures.rs` re-invokes the
  test binary by name, and that name carries the module path) must be
  updated when its module moves.
- `crates/remanence-ffi/` — the C ABI (`remanence_*` symbols): opaque handles,
  accessor functions, borrowed strings owned by their handle. `build.rs`
  regenerates `include/remanence.h` with cbindgen on every build; the
  header is generated output, never edited by hand.
  `examples/identify.c` is the example C consumer and doubles as the ABI
  smoke test (build instructions in its header comment).

  `include/remanence.hpp` is the **idiomatic C++ presentation of that
  same ABI** (D53) — header-only, C++17, no compiled artifact of its own,
  and **written by hand**, which is the one thing that makes it
  different from the header beside it: nothing regenerates it, so it can
  fall behind. It is no fourth surface — S2 is the norm and this derives
  from it, claiming no reach the C ABI lacks. RAII classes over the
  handles the ABI hands you to free, copyable views over the ones the
  session owns, refusals as `remanence::Error` carrying the delivered
  category and rule identity, and every handle accessor answering an
  owned `std::string` rather than a view into memory a temporary handle
  took with it. **It covers every `remanence_*` function** — the storage
  model and the flux ladder both — so one that is not wrapped is a
  defect rather than a boundary (D54). `examples/identify.cpp` is its
  example consumer, beside the C one.
- `crates/remanence-py/` — the Python module (PyO3, abi3, Python ≥ 3.10),
  excluded from default workspace members so plain `cargo build`/
  `cargo test` never needs a Python toolchain. Distribution artifacts
  are built with **uv** (`uv build crates/remanence-py` → sdist + abi3
  wheel in its `dist/`), which drives the maturin build backend in an
  isolated environment; publishing is `uv publish` and is owner-gated.
  **The Python package claims Windows only** (the tested host; the
  classifiers state it) — keep POSIX paths correct but never state or
  imply support the project has not tested. **And it claims the native
  CPython, not MSYS2's**, which `build.rs` enforces by refusing a MinGW
  interpreter before anything compiles. pyo3 names a MinGW Python's
  import library `libpython3` rather than `python3`, which puts it
  outside the subset `raw-dylib` linking covers, so the module links
  `libpython3.dll` — a DLL that exists only inside MSYS2. Nothing
  downstream noticed: 0.0.1a4 built, published, and failed at `import`
  for every consumer of it. `uv build` supplies its own native
  interpreter and never reaches the refusal; a `cargo build` from an
  MSYS2 shell does, and the message names the remedy —
  `export PYO3_PYTHON=$(uv python find)`. The Python suite
  (`tests/python_suite.rs`) shares the same concern from the other
  side: if `pytest` is not installed for the interpreter on `PATH`, it
  falls back to `uv run --with pytest`, which by default picks an
  interpreter of `uv`'s own choosing — possibly not the one the module
  was built against, which is the same class of mismatch showing up as
  a bare `DLL load failed` on import. `common::python::pytest` pins
  that fallback to `REMANENCE_BUILD_INTERPRETER` (the interpreter
  `build.rs` recorded) so it can't drift. `uv build
  crates/remanence-py` produces the sdist and abi3 wheel in its
  `dist/`; publishing is `uv publish`.  `test-fixture-prep/` is a
  separate uv project with its own `pyproject.toml` and lock file,
  carrying only the fixture-preparation dependency group.
- [CHANGELOG.md](CHANGELOG.md) records release-facing changes; the rules
  it follows are in "Versioning and releases" below.
- `planning/README.md` is the map of the maintainer-facing planning
  machinery, and the place to start. `planning/SURFACES.md` is the
  surface-change rule; the application surface inventory it scopes over is
  S-numbered in root [ARCHITECTURE.md](ARCHITECTURE.md) "The application
  surfaces", where the housekeeping lookup answers by checklist.
  `planning/DECISIONS.md` is the adjudication record — **search it before
  a governed act** (drafting a proposal, pledging one, changing a norm)
  and report what you found, including nothing. `planning/SEQUENCES.md`
  is the handle ledger; advance it in the same edit that issues a
  handle. `planning/TASKS.md` is the pre-approved task queue: **agents
  do not add tasks on their own initiative, and ask before editing that
  file at all**; anyone may pick up what is already there.
- **The vision is in force.** Use cases U1–U6, U25, U26, U32, U33 and U34
  (root [USE-CASES.md](USE-CASES.md)) and architectural principles
  (root [ARCHITECTURE.md](ARCHITECTURE.md)) are armed: every entry is
  met or honored by the code today, and a divergence is a bug. Triage
  cites them by number; the surface-change rule in
  [planning/SURFACES.md](planning/SURFACES.md) is fully operable.
- **There is no roadmap**, and no issue tracker yet — until one exists the
  task lane has no proposed state at all (see `planning/TASKS.md`).

## Required invariants

### Pre-release: no backward compatibility

remanence-lib is pre-1.0 and maintains no backward compatibility: when a
surface changes, change it coherently and completely — every binding,
document, example, and test moved to the new shape, the old one deleted.
No aliasing, no deprecated shims. Compatibility guarantees are defined no
earlier than 1.0.

### Surface changes are vetted

The Rust crate API, the C ABI, and the Python module are the application
surfaces (S1–S3, root [ARCHITECTURE.md](ARCHITECTURE.md)). Any decision
that changes one follows
[planning/SURFACES.md](planning/SURFACES.md). The in-force use cases and
principles supply the authority that its triage requires; surface-changing
proposals still follow that process rather than being self-approved.

### The bindings track the core in the same change

A public-surface change in `crates/remanence` lands with its C ABI and
Python reflections in the same change, never deferred: the cbindgen header
regenerates on build (commit the result), and `remanence-py` mirrors the
public surface explicitly. The example consumers and tests move in the same
change.

**The C++ header is derived from S2 and moves with it** (D53).
`crates/remanence-ffi/include/remanence.hpp` is hand-maintained, so a
`remanence_*` function added, renamed or retired moves it in that same
change — and a disagreement between the two is a defect in the wrapper,
the ABI being the norm. It is not a surface of its own: it claims no
capability the C ABI lacks, and the S-numbers are unchanged. What
catches a lapse is `cargo test`, which compiles the header standalone,
compiles `examples/identify.cpp` against it, and runs a C++ caller
through it (below). **Coverage is total and is meant to stay that way**
(D54): every `remanence_*` function is wrapped, so a new one that is not
is a defect rather than a scoping choice, and the answer to "is this
covered?" is a lookup rather than a judgement.

**S3 has a pytest suite, and it is what the sdist ships** (D48). It runs
under the ordinary `cargo build && cargo test` (D51, D52) — no wheel, no
install, and no flag: every workspace member is a default member, so all
three surfaces' checks run together.

The suite needs the compiled module, which `cargo test` does not build
but `cargo build` does. `python_suite.rs` then stages a `remanence/` package from what
that produced — the cdylib renamed as the extension, beside
`__init__.py`, the stub and `py.typed` — and points `PYTHONPATH` at it.
That exercises a **debug** build staged by hand; `uv build` is still
what proves the artifact, and the sdist run below is what proves the
suite travels.

It opens **no disk image**, deliberately: every fixture this project
tests against is third-party media it does not distribute and git does
not track, so the shippable suite makes its own through
`Session.new_media`. What needs a real artifact — filesystems, partition
tables, the flux ladder — is the Rust suite's job, which has the
fixtures. The Rust integration tests in `crates/remanence-py/tests/*.rs`
test the stub against *this repository's sources*, so they stay out of
the sdist along with the mypy fixtures they drive; `Cargo.toml`'s
`exclude` is what maturin reads for that.

**The Python type stub is part of that surface, and nothing regenerates
it.** `crates/remanence-py/python/remanence/__init__.pyi` is written by
hand and states S3 in full — every class, property and verb the module
registers, exactly once. A name added, renamed or removed in
`crates/remanence-py/src/lib.rs` moves the stub in the same change; a
stub that disagrees with the module is a bug in the stub, the module
being the norm. The `py.typed` marker beside it is what makes a type
checker honour either (PEP 561), and both reach the wheel through
`python-source` in `pyproject.toml`.

### The core stays dependency-free at runtime

`crates/remanence` has no runtime dependencies, deliberately — its ZIP
and 7z readers and its DEFLATE and LZMA/LZMA2 decompressors are its own.
That is a property the
licensing tiers below make load-bearing, not just tidiness. Discuss before
adding any dependency anywhere in the workspace; for the core the answer
is expected to stay no.

### The library does not name its consumers

Documentation follows the dependency direction that the code does: a
consumer may name the libraries it builds on, and this library names none
of the projects that build on it. Not in the use cases, the principles, a
planning document, the README, the changelog, this file, a doc comment, a
test, or a commit message — not as an example, not as a "used by" credit,
not as a note about where removed functionality went. Work that sits
outside this library is **the caller's**, said that way, and generic
placeholders carry any example that needs one.

The reason is not tidiness. A downstream name here implies a relationship
a reusable library should not have, and it goes stale silently inside a
published artifact — a consumer's rename leaves the falsehood shipped.

One thing is not a violation of it. The fixture-preparation tooling under
`test-fixture-prep/` *depends on* a named tool the way any dependency is named —
the permitted direction — which reaches that tooling, the metadata pinning
it, and the prose documenting them, and nothing else. Being nameable there
licenses nothing elsewhere, `planning/DECISIONS.md` included.

## Licensing

The project is **GPL-3.0-only** and follows REUSE conventions. The name
**Remanence** is reserved to Paul Galbraith under [TRADEMARKS.md](TRADEMARKS.md) — a reservation GPL section 7(e)
expressly permits; do not weaken or contradict that policy in docs or packaging metadata.

Every new file authored for the project needs:

```text
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
```

Use the appropriate comment syntax for the file type. Files that cannot or
should not carry headers must be covered by `REUSE.toml`.

The licence text is present three times, each copy answering a different
requirement, and **the three must not be allowed to drift**:

- `LICENSE` at the root — what GitHub reads to identify the licence.
- `LICENSES/GPL-3.0-only.txt` — where the REUSE specification requires
  it. Byte-identical to the root copy.
- `crates/remanence-py/LICENSE` — what the wheel carries, because
  `license-files` in pyproject resolves relative to pyproject's own
  directory and cannot reach the root. Same text, CRLF line endings.

Nothing checks that they agree, so a change to one is a change to all
three. `uvx --from "reuse[charset-normalizer]" reuse lint` is the
whole-repository check; it must report compliance before a release.

### The relicensing reservation, and what it constrains

Paul holds copyright in the whole work and **reserves the right to
relicense the project on any terms**. Nothing is planned; the reservation
exists so the option is not lost by default. Two consequences bind
everything below, and neither is negotiable at the level of an individual
change:

- **The project must own every line it ships.** Relicensing is only
  available to a party holding rights in the whole work, and enforcing
  copyleft requires standing that only an owner has. One file the project
  cannot account for forecloses both, permanently and silently.
- **Assignability, not licence compatibility, is the test for incoming
  code.** GPL-compatible is not good enough. Code the project cannot
  acquire *title* to cannot enter, whatever its licence.

**Vet against a commercial dual licence, and say only "relicensing" out
loud.** What the project *states* — in README.md, CONTRIBUTING.md, and
CLA.md — is that relicensing is reserved and nothing is planned, which is
true and is all the disclosure the reservation needs. What the project
*vets against* is the strictest realistic outcome, which is a commercial
dual licence, because vetting to a weaker bar would forfeit the reserved
option invisibly. The question to ask of any external source is **"could
this ship inside a proprietary product?"** — never "is this
GPL-compatible?"

Contributions are accepted only under the copyright assignment in
[CLA.md](CLA.md). Once assigned, a contributor's files carry Paul's
copyright notice, because he is then the actual owner — the REUSE record
states ownership, not authorship, and authorship credit lives in the git
history. Keep the human submission terms in
[CONTRIBUTING.md](CONTRIBUTING.md) synchronized with this policy.

**Never merge third-party source.** Not permissively licensed source, not
public-domain-looking snippets, not vendored files. The contributor cannot
assign what they do not own, and neither can the project. Third-party code
enters as a declared dependency or not at all.

### Dependency licence tiers

Every dependency that reaches a **distributed artifact** — the published
crate a consumer compiles in, the staticlib/cdylib, the Python wheel —
sorts into exactly one tier, drawn against the commercial-dual-licence bar
above rather than against GPL compatibility. Verify a new dependency's
whole transitive closure, not just the package named.

| Tier | What qualifies | Standing |
|---|---|---|
| **1 — Sublicensable** | MIT, BSD-2/3-Clause, Apache-2.0, ISC, Zlib, Unicode | Freely dependable. Attribution obligations carry into any redistribution. |
| **2 — Arm's length only** | LGPL as a separately installed, replaceable library; GPL invoked as a separate process | Permitted, never combined. Static linking — Cargo's default — is **not** arm's length, which makes this tier nearly unreachable for Rust dependencies. |
| **3 — Refused** | Any GPL/AGPL code that would be linked, imported, or copied in | Never. Compatible with the GPL arm and fatal to the reservation. |

Build-time and development dependencies are out of scope — they are not
distributed. cbindgen (MPL-2.0) is build-time only. The current
distributed closure: the core crate has **zero** dependencies;
`remanence-py` adds pyo3 and its closure (MIT/Apache-2.0 — tier 1),
compiled into the wheel.

### Prior art and provenance notes

- `crates/remanence/src/codec/inflate.rs` follows the structure of Mark Adler's
  "puff" reference DEFLATE implementation, as did the C++ file it ports
  (the C++ described puff as public-domain; puff ships in zlib's contrib
  under the zlib licence — tier 1 either way). The Rust is an original
  implementation of RFC 1951 following that published structure, written
  from the project's own C++ lineage, not from puff.c. Keep the
  attribution comment in the file.
- `crates/remanence/src/codec/lzma.rs` is an original Rust implementation of
  the LZMA and LZMA2 decoders, written from the format description Igor
  Pavlov published with the LZMA SDK (which he placed in the public
  domain — tier 1). The probability model, range coder, and chunk
  grammar are what the published description specifies; no SDK source
  was copied or ported. `crates/remanence/src/archive/sevenzip.rs` reads the 7z
  container from the same published description. Keep the attribution
  comments in both files.
- The remanence image model, the `.remanence` grammar, the gap-first
  reconstruction, and the C64 renditions
  (`flux/remanence/image.rs`, `flux/remanence/format.rs`,
  `flux/analysis.rs`, `flux/remanence/reconstruction.rs`,
  `flux/c1541/renditions.rs`) are ported from
  the owner's own private, unpublished flux-capture research
  implementation — owner-authored throughout, so title to every line
  is already the project's. `codec/deflate.rs` is an original RFC 1951/1950
  encoder written for this port. **Nothing in the suite compares against
  that implementation's own rendered output.** It did once — the port was
  validated against its d64, g64 and `.remanence` renderings of the same
  capture, which was the right check while porting and the wrong one to
  leave standing. Those artifacts were reduced from a pooled pair of
  captures this repository does not hold, so equality was never possible
  and the thresholds were chosen to fit rather than derived from
  anything; a prior program by the same author in the same lineage is no
  independent oracle in any case; and the comparisons skipped silently
  when the untracked artifacts were absent, so a suite that had stopped
  checking still reported green. The flux tests now assert what the
  formats and the model fix.
- `crates/remanence/tests/fixtures/` holds the test fixtures.
  `test-fixture-prep/prep_fixtures.py` (run with `uv run --directory
  test-fixture-prep`; testing-prep/test-rigs/README.md) prepares them: it downloads the
  sha256-pinned HDOS 1.0 distribution zip straight into
  `tests/fixtures/` (a multi-image zip, test material in its own
  right), extracts only the one disk image the tests read beside it
  plus a generated single-image zip; downloads the sha256-pinned
  Pinball Construction Set KryoFlux source archive into
  `test-fixture-prep/downloads/` and packages disk one's whole capture —
  all 84 step positions from both heads — into one local 7z fixture,
  which is the artifact a real capture produces when a single-sided
  disk is read in a two-head drive. Members keep the `.0.raw` /
  `.1.raw` head designator: a KryoFlux stream records no track or side
  in its own out-of-band data, so the member name is the only place a
  capture's position exists, and stripping it would admit a grammar no
  real capture has. Head 0 carries the disk and head 1 is the
  unrecorded back, which reads as noise — telling them apart is the
  library's job, not the fixture's. And it builds the FreeDOS qcow2
  rig artifact there. The FreeDOS LiveCD downloads through
  reliquary's own media mechanism into
  `test-fixture-prep/test-rigs/cache/media`; downloads that are not
  fixtures at all belong in `test-fixture-prep/downloads/`. Downloaded,
  extracted, and generated fixture files are never tracked: the
  fixtures directory's own `.gitignore` names each one, and
  `package.exclude` keeps them out of published artifacts.
  Checked-in fixtures may sit alongside them. Unit tests expect
  required fixtures to be present and fail with diagnostic
  instructions to run the prep script when missing.

## Versioning and releases

The **workspace SemVer is the single upstream version** —
`workspace.package.version`, inherited by every crate (the value in the
root `Cargo.toml`, never restated elsewhere in prose). Pre-releases follow
SemVer's ladder (`-alpha.N` →
`-beta.N` → `-rc.N` → bare); nothing below `-alpha.1` is ever
published to a registry — unpublished git is the dev channel.

The **PyPI version is derived, never hand-written**: pyproject
declares `dynamic = ["version"]` and maturin converts the Cargo
version to PEP 440 (`0.0.1-alpha.1` → `0.0.1a1`). Do not put a static
version back in pyproject.

**Repackaging an unchanged upstream** (distro-style revision — the
wheel changed, the library did not) is spelled as a PEP 440
post-release: give `crates/remanence-py` its own Cargo version with
`.post.N` appended to the workspace version (e.g.
`0.0.1-alpha.1.post.1` → PyPI `0.0.1a1.post1`), and return it to
`version.workspace = true` at the next upstream bump. **The decision
that a repack is warranted is the releaser's judgment — only the
spelling is mechanized.** PEP 440 discourages post-releases of
pre-releases; the distro-revision model is chosen deliberately over
that advice (D3), and PyPI's local-version syntax — the truer
analog — is rejected by the index outright.

### The changelog

[CHANGELOG.md](CHANGELOG.md) records **release-facing** changes — what a
consumer of S1–S3 meets, plus a principle arming, which is a claim about
the code. Version headings are the workspace SemVer, matching the tags.
Planning moves are not release-facing: proposing, pledging, and drafting
leave their record in the commit that made them.

**The changelog is history, not documentation.** Everything under a
released version heading records what was true at release and stays as
released — superseded wording, renamed concepts, and stale paths included.
Corrections go in a new entry under `Unreleased`, never as an edit to
released text; the unreleased section is freely editable until it ships.
The one exception is removing legally problematic content: redact
minimally, and record the redaction as an entry of its own.

It names no consuming project, like every other library-side document
("The library does not name its consumers", above).

## Required checks

```bash
cargo build                 # the core alone, and nothing but rustc is needed
cargo test
cargo build --workspace     # every surface; regenerates include/remanence.h
cargo test --workspace      # needs CMake, a C++ compiler, Python and uv
cargo fmt --all -- --check
git diff --check
```

**All six are required, and the first pair is not a subset anyone may
stop at.** Only `crates/remanence` is a default member, so the bare
commands build and test the Rust core and nothing else — which is what
lets a reader of the core, or a packager, work without acquiring two
further toolchains. A contributor is not that reader: the `--workspace`
pair is what checks S2 and S3, and it is also what regenerates
`crates/remanence-ffi/include/remanence.h`, since the build script that
writes it runs only when its own crate is built.

**The formatting check is here because nothing was asking.** It went
unrun long enough for 21 files to drift, and the drift was invisible:
every one of them compiled, passed, and read fine. `rust-toolchain.toml`
is what makes the check meaningful rather than a second opinion —
rustfmt's output is not stable across releases, so an unpinned tree gets
a different answer per host and the check becomes noise to ignore.
Bumping the pin therefore means running `cargo fmt` and committing what
it rewrites as its own change, never mixed into a commit that means
something.

**`cargo test` needs no downloaded fixture** (D49). Everything the
default run touches builds its own images, so a fresh clone is testable
immediately — and that is a claim about the *whole* default run, the
unit tests included. Seven tests inside `crates/remanence/src/` open the
KryoFlux capture instead of building anything: the reduction and the
renditions over the pinball disk, and the `fixture_tests` module in
`flux/drive_profile/verdict.rs`, whose claims sit in the source because
F59 folded the verb they exercised into the declared load and they reach
`pub(crate)` helpers to do their work. They carry
`#[cfg(feature = "fixtures")]` for the same reason the suites below
carry `required-features` — the feature is what marks the tier, not the
directory — and they cost the default run nothing, having been most of
its wall clock. Nine suites open such an artifact too — seven in the
core crate, and `cpp_flux.rs` and `c_abi_rig.rs` in the FFI crate (D54):

```bash
cargo test --features fixtures                    # what was downloaded
cargo test --features rigs                        # what reliquary built
cargo test -p remanence-ffi --features fixtures   # the C++ flux walk
cargo test -p remanence-ffi --features rigs       # the C ABI over the rig
```

**Two features, because what it costs to obtain them differs in kind.**
`fixtures` names what the project acquires from elsewhere, downloaded
against pinned SHA-256s; `rigs` names the one artifact it generates,
`freedos-parttest.qcow2`, which reliquary produces by booting a machine
and installing FreeDOS onto a disk — needing that toolchain, an
emulator, and as long as the install takes. Someone holding the
downloads should not have to own the rig toolchain to run everything the
downloads reach, and the gap widens with the operating system being
installed.

**`ensure_fixture` is gated on those features too**, which is what stops
the declaration drifting from the fact. `media_sources` and
`sevenzip_catalog` reached for the HDOS zip and the KryoFlux capture
while declaring neither, so they passed on machines that had run the
prep and would have panicked on a clone — invisible precisely where it
mattered. A target that calls the helper without declaring a feature now
fails to compile, in the default run, on the first machine to build it.

**The guard only reaches targets that call the helper**, which is how
the same bug survived once more in the FFI crate. `c_abi_boundary.rs`
checked `freedos-parttest.qcow2` with its own `Path::exists`, so nothing
made it declare `rigs`, and `cargo test --workspace` — a required check
— wanted the generated rig on every machine. It passed on every host
that had run the prep and failed on the first fresh checkout to try it,
which was a release build. The test is now
`c_abi_rig.rs` behind the feature. When a test reaches an artifact
without going through `ensure_fixture`, the declaration is the only
thing standing, so write it first.

Cargo does not build a target whose required features are off, so the
default run reports nothing misleading about them. What stays there are
the readings whose *authenticity* is the point — a KryoFlux capture, an
authentic HDOS filesystem, a qcow2 an operating system wrote, and a
format that declares its own geometry. Anything whose shape is wholly
specified belongs in the default run instead: `tests/rig_disk/mod.rs`
builds MBR tables, EBR chains and FAT12/FAT16 volumes, and
`tests/rig_layout.rs` is what asserts a built disk is the shape it
claims before other tests trust it.

**Three tiers, and only the first ships.** In-source `#[cfg(test)]`
tests are the unit tier and travel inside the published crate, so they
must need nothing but rustc — which is what the gating above is for.
`tests/*.rs` is the integration tier, and `exclude = ["tests/**"]` keeps
it out of both published crates: what it checks is *this repository's*
claims rather than a consumer's build, and it cannot run there anyway —
the C surface's suites need CMake and a compiler, and the leak probe
builds through a cargo workspace two directories up that an extracted
crate does not have. Anything reaching a downloaded or generated
artifact is gated behind `fixtures` whichever tier it sits in. So a
packager who extracts either crate and runs `cargo test` gets the unit
tier, passing, with no network, no fixture and no second toolchain —
which is the run a distribution build actually performs. `examples/`
stays in the FFI crate deliberately: `identify.c` and `identify.cpp` are
the best documentation a C caller gets, and cargo ships them as plain
files rather than as targets.

When the C ABI changed, rebuild and commit the regenerated header.
`cargo test` **compiles** the C surface for you (D44, D53): that the C
header stands alone, that `examples/identify.c` still compiles against
it, that the header is valid C++, which `cpp_compat = true` claims and
nothing tested before, and — since the C++ presentation landed — that
`include/remanence.hpp` stands alone and `examples/identify.cpp`
compiles against it. For the C header, compiling rather than linking is
enough: generated from the `extern "C"` signatures, it cannot declare a
symbol the library lacks. **The C++ header is the one that can**, being
hand-written, which is why it is compiled and then run through (below).

**CMake builds all of it, with MSVC** (D46). CMake is here for one
reason: `cl.exe` needs the environment `vcvars64.bat` sets, and locating
and sourcing that from a test harness is more bespoke machinery than the
compiler search it replaces. CMake does it and finds MSVC unaided, which
is the native match — the cdylib is MSVC-built, so a C caller links the
import library the same toolchain produced. `crates/remanence-ffi/tests/c/CMakeLists.txt`
is the build; the Rust tests drive it and it is not meant to be
configured by hand.

**The rule underneath that is the match, not MSVC.** A C caller is built
by whichever toolchain built the library it links, and on a `windows-gnu`
rustc that is not MSVC: cargo writes `libremanence_ffi.dll.a`, which
`cl.exe` cannot link and whose C runtime is not MSVC's, so the harness
names gcc and a generator that can drive it (Ninja, or `mingw32-make`)
rather than letting CMake pick MSVC and fail at the link. Nothing is
inferred from the host: the harness reads the file names out of cargo's
own `--message-format=json` report of the build it just ran, which is
also what stops an artifact left behind by a *previous* toolchain being
linked — cargo does not delete a file it has stopped writing, and a
search of `target/<profile>` for a plausible name would find it.

Overrides, all optional: `REMANENCE_CC` / `REMANENCE_CXX` set CMake's
compilers and win over the choice above, so another compiler is still
one variable away), and `REMANENCE_CMAKE_GENERATOR` sets the generator.
**No CMake or no compiler is a test failure, not a skip** —
`REMANENCE_SKIP_CC=1` skips deliberately, and plain `cargo test` needs
both because of it.

`cargo test` also **runs** a C caller against the built library
(D45): `tests/c/abi_boundary.c` is compiled against the header, linked,
and executed, one group per Rust test — the catalogs, the version, a
refusal's out-parameters, null handling, and a real artifact discovered
and released. This is the only thing that crosses the boundary as a C
caller meets it; the FFI crate's unit tests call the same functions from
Rust, where no header, no C compiler and no C calling convention are
involved.

`cargo test` **runs a C++ caller through the wrapper** as well (D53):
`tests/c/wrapper.cpp`, one group per Rust test, checking what only the
C++ surface can be wrong about — a refusal arriving as
`remanence::Error` with the delivered category, an owned handle freeing
itself, a moved-from wrapper freeing nothing, an honest absence staying
an empty `std::optional`. Every group but one authors its own medium or
lays the remanence format's own worked example on disk, so it needs no
fixture; `a_real_artifact_reports_and_reads` walks the qcow2 rig because
a layered report and a namespace above it are answers only a recording
has.

**One C++ suite is fixture-gated, and only one** (D54):
`cpp_flux.rs` walks a real KryoFlux capture up the whole flux ladder,
because the sector layer needs a recording that frames records and the
worked example is deliberately two points on one orbit. It sits behind a
`fixtures` feature on `remanence-ffi` mirroring the core crate's, and
takes about two and a quarter minutes:

```bash
cargo test -p remanence-ffi --features fixtures
```

**And it checks one refusal at compile time** (D53): the wrapper deletes
every borrowed-record accessor on a temporary, so
`medium.identify().layers()` must *not* compile while the same walk over
a named handle must. CMake compiles both at configure time and fails if
either answers wrongly. If you add an accessor that hands back a record
borrowed from its handle, give it the `const&` / `const&& = delete` pair
the others have.

**Those need the built library, and `cargo test` does not produce a
cdylib — `cargo build` does.** Running the two in the order above
satisfies it. When the library is missing the tests say so and say what
to run; they do not build it themselves, because a nested `cargo` would
contend for the lock the running test already holds.

**The `_free` discipline is checked too, and it runs by default**
(D47, D50). Every handle and string the ABI hands out is allocated by
Rust inside the cdylib, so no leak checker outside it — CppUTest's, or a
sanitizer's — can see them; the library counts its own live allocations
instead and exports the count, and a C caller reads it either side of
repeated create/release cycles.

**The probe never ships**, being a global allocator and an exported
symbol — and an extra `remanence_*` symbol is an S2 change. It does not
have to: the harness builds a second copy of the library with
`--features leak-probe` into `target/leak-probe`, and the leak binary
links *that*, leaving `target/<profile>` exactly as it was. Cargo locks
a target directory rather than a workspace, so that build runs while
`cargo test` holds the other lock. Cold it adds about twenty seconds;
warm, a fifth of a second.

**The same probe answers for the C++ wrapper** (D53), and that is where
it earns the most: `tests/c/wrapper_leaks.cpp` cycles a session, its
records, a refusal's message and rule, and a handle released back to C —
a caller who writes no `_free` cannot see a missing one either, there
being no call site to inspect. It authors its own medium and needs no
fixture.

If you change what the ABI hands out or who frees it, or what a C++
destructor discharges, these are the tests that notice. Nothing needs
enabling.

Running the example against a real image is still by hand, below; that is
the part neither compiling nor the boundary tests stands in for. When the Python surface changed, **move the type stub with it** (above);
the suite itself needs no separate command; for release artifacts, `uv build crates/remanence-py`
produces the sdist and abi3 wheel.

The stub is checked by `cargo test`, in three tests that
between them cover both halves of what it claims (D42, D43). None of
them needs a built wheel or an installed module. Run them after any
Python-surface change and believe them: the module is the norm, so what
they report is a fix to the stub.

- **Names** — `stub_matches_module.rs` compares what `src/lib.rs`
  registers with pyo3 against what the stub declares, in both
  directions, and names the class and member on any disagreement.
- **Types** — `stub_typechecks.rs` runs `mypy --strict` over
  `tests/typing/accepts.py`, ordinary consumer code that must check
  clean, at Python 3.10 (the minimum `pyproject.toml` claims).
- **Refusals** — the same file runs mypy over `tests/typing/rejects.py`,
  which must *fail*, each line naming the error code it expects. This is
  the one that catches a stub degraded to `Any`: a widened parameter or
  a lost `py.typed` still lets `accepts.py` pass.

mypy is found through `python -m mypy`, `mypy` on `PATH`, or
`uv run --with mypy` — the last needs no prior install, uv already being
how the wheel is built. If none is reachable the tests **fail** rather
than skip, because a check that quietly does not run reads exactly like
one that passed; `REMANENCE_SKIP_MYPY=1` skips them deliberately.

### Running the C example on this host

`cargo test` already compiles the example (above). This links and runs
it, which is the part a compile cannot stand in for: it exercises the
ABI end to end against a real artifact.

The C++ example is the same journey through `remanence.hpp`, and builds
the same way with `g++ -std=c++17` over `examples/identify.cpp`. It
takes `<path> [device-type]`, `--author [kind]` and `--devices`, and is
worth running beside the C one for the comparison the two make: the same
work, with the frees and the out-parameters gone.

This recipe uses MSYS2's ucrt64 gcc, which is no longer what the tests
use (D46 moved those to CMake and MSVC) but still works and is kept
deliberately: it is the second compiler's opinion the switch otherwise
gave up. **Put `C:\msys64\ucrt64\bin` on `PATH` first** — without it the
toolchain cannot load its own runtime DLLs, and `g++` in particular
fails with no diagnostic at all. From PowerShell:

```powershell
$env:PATH = "C:\msys64\ucrt64\bin;$env:PATH"
gcc crates/remanence-ffi/examples/identify.c target/debug/remanence_ffi.dll `
    -I crates/remanence-ffi/include -o "$env:TEMP\identify.exe"
```

Then run it beside a copy of `target/debug/remanence_ffi.dll`, against
both a plain image and one inside an archive — the archive path is a
distinct composition, not the same code with a longer path. The example
takes the device type as an optional second argument
(`identify <path> h17`); given one it opens the artifact itself
and declares its format and that device — whoever opens owns the lock —
and given none it asks the artifact instead, through the convenience over
discovery, so a format recording several device types — a raw image, a
qcow2 — refuses there and names the types to pass. Its report carries
the medium's discovered geometry with every reading that produced it,
and reads cylinder 0, head 0, sector 1 in the recording's own
coordinates wherever the evidence settled them.
`identify --author [kind]` authors a blank medium instead of opening
one — the third fact class, where the caller has no artifact — listing
the kinds this release makes, showing the author's own facts as the
medium's provenance and geometry, writing and committing a boot sector
in the authored coordinates, and meeting the refusal that no drive
takes it. `identify --list <archive>` walks an archive's
namespace, `identify --discover <path>` reports what an artifact is
without loading it, `identify --remanence <path> [write-to]` reads a
`.remanence` artifact through its own type — there is no device to
load a flux artifact into — writes it back where a destination is
given, and describes the three C64 renditions without writing them,
`identify --renditions <path> <stem>` writes all three beside each
other with their accounts, and `identify --devices`
lists the claimed devices.

**Without that `PATH` entry gcc exits 1 and prints nothing at all**: it
is gcc's own runtime DLLs failing to resolve, so the compiler never
starts and there are no diagnostics to read. The silence looks like a
broken example rather than a broken invocation, which is exactly how it
wastes time.
