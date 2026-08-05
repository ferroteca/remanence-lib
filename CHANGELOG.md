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

- **A medium now says what it is, from a catalog of media types.** Every
  medium the library holds names one immutable entry carrying that
  article's passive compatibility facts — a media profile — and the
  facts are family-specific by construction rather than one schema with
  most of its fields empty. Two families are claimed: **flexible
  magnetic** media, whose facts are its form factor, coercivity, track
  density, sectoring and hole topology, and which way its write-protect
  mechanism reads; and **logical-block** media, whose only compatibility
  fact is the addressable unit, a geometry-opaque medium having no
  other. The enrolled types are the soft-sectored and ten-sector
  hard-sectored 5.25-inch disks and the 512-byte logical-block medium,
  and a media type outside the catalog is refused by name (P3).

  **What the medium is, what was recorded on it, and what a drive does
  to it are three facts with three homes.** A hard-sectored disk's ten
  sector holes are the medium's own division of a revolution; the ten
  records the H8D format declares to a track are the recording, and the
  two are checked against each other rather than one being derived from
  the other. A disk carries an index hole whatever drive it is served
  in, and whether a drive observes one is that drive's declaration —
  which is why a 1541 medium states the hole and the 1541 drive profile
  states no sensor. The catalog holds no recognition, no grammar and no
  behavior at all, which is what lets it be declarative.

  A caller meets this as a named media type where a free-form word used
  to sit. `DiskLayout.media_kind` — `"floppy"`, `"hard_disk"`, or
  nothing at all — is now `DiskLayout.media_type`, always present and
  always an enrolled identity, and the physical-media container layer of
  an identification carries that identity and the catalog's own name for
  it. The layered report's `DeviceInfo` gains `media_type` beside
  `image_format`, so an inspection says which article is attached as well
  as which format loaded it. In C,
  `remanence_container_disk_media_kind` becomes
  `remanence_container_disk_media_type` and
  `remanence_report_device_media_type` joins it; in Python,
  `DiskLayout.media_type` and `DeviceInfo.media_type`. "Hard disk" is
  gone from the medium's vocabulary deliberately: that is the device
  family a session's slot carries, not a fact about the medium in it.

  **P14 is armed**: it moves from the pledged list to the in-force
  architectural principles, where a divergence from it is a bug.

- **A VDI differencing image is a first-class disk.** The top image opens
  and the whole chain composes as one (U6), exactly as a qcow2 with a
  backing file already did: a block the top image never allocated reads
  through to its parent, a block it holds as discarded reads as zeroes
  and masks the parent, and writes allocate copy-on-write into the top
  image only. A parent is claimed immutable for the session's life (P7),
  is never modified and is never flattened into the child, so after
  commit the relationship stands and the delivering hypervisor's own
  tooling reads the changed guest bytes.

  **The parent is found by identity, not by a path, because the format
  records no path.** A VDI names its parent by the parent's own
  identity; the library searches for the image declaring that identity
  beside the child and in the directory above it, and the file *named*
  for the identity — how the format's own tooling names a differencing
  image — is nominated first. That nomination is checked rather than
  trusted: a file standing where the parent should be whose identity
  does not match is refused by name, never read as a substitute. It is
  the one place this format hands the library evidence a backing path
  alone cannot give.

  Every refusal a qcow2 chain already named is named here too, at the
  open and never as a partial interpretation (P3, P6): a missing parent
  naming the identity it looked for, a cycle — caught by identity, so an
  image naming itself is caught as squarely as two naming each other —
  a chain past the sixteen files this release claims, and a parent whose
  own version or image type falls outside the claim, which refuses in
  its own name. The `differencing` image type joins `dynamically
  allocated` and `fixed` in the enumerated claim, leaving `undo` as the
  one type refused by name.

  Identification is deliberately untouched: a differencing VDI
  identifies as the VDI container it is (U5), with its type and the
  parent it declares among the evidence (P4). No S1, S2 or S3 symbol
  changed — a composed chain is the same `Disk` presenting the same
  `DiskFormat::Vdi { major, minor }`, and what changed is what the three
  surfaces now open rather than refuse.

- **A 1541 now reads the disk, not just the recording.** A mastered flux
  medium — or the one a P64 container holds at rest — materializes the
  family's **hardware bitstream** under declared mechanics and
  read-channel rules, and that bitstream materializes the family's
  **encoded bytestream** under its declared group code:
  `MasteredMedium::materialize_c1541_bitstream` and
  `P64Image::materialize_c1541_bitstream` in Rust, then
  `C1541Bitstream::materialize_c1541_bytestream`, with
  `remanence_mastered_medium_materialize_c1541_bitstream`,
  `remanence_p64_image_materialize_c1541_bitstream` and
  `remanence_c1541_bitstream_materialize_bytestream` in C, and the same
  three verbs on `MasteredMedium`, `P64Image` and `C1541Bitstream` in
  Python. Each layer reports what it holds — per location the cell and
  its zone, the bit counts, the framing landmarks, the byte counts — and
  what it does not carry from the layer below.

  **Every rule either transition applies is the drive profile's**, and
  the profile gains the half that owns them: the read channel's resync
  behavior and the window a transition is admitted by, and the family's
  group code, which is the published sixteen-symbol table its bytes are
  recorded as. Nothing derives either from what it is reading. The
  window is why this is a read channel rather than a comparison: at half
  a cell the channel locks onto the recording's own phase, so a disk
  written a little fast holds a few more bits than the nominal rate
  states rather than being read as though it did not.

  **Every bit says how it came to be.** A pulse the medium states reads
  the same every time yields a recorded bit; one it states does not is
  resolved by a declared rule — flatly, or reproducibly from a seed —
  and which rule resolved it travels with it. A location the family's
  density map does not cover is refused or omitted by declaration, never
  clocked at a neighbouring zone's rate. A group holding a pattern the
  family's table does not assign keeps its own bits and is counted, or
  refuses; there is no nearest entry.

  **Neither layer assigns anything.** No byte is a header, a data field,
  a sector or a file. The codec locates the family's framing landmark
  because byte framing has to begin where the family says it does, and
  having located one it claims nothing about what follows it. There is
  no way back down: returning to a medium is a separate, explicit
  mastering operation, and this release performs none. Both layers live
  in private session storage under a declared bound and are never held
  whole (P27); the bits and bytes themselves stay behind the surface, as
  the medium's pulses already do.

  **The upper half of the P23 amendment is armed**: the durable
  active-layer vocabulary gains hardware bitstream and encoded
  bytestream between flux medium and CHS, with the magnetic ladder
  stated in full, and it moves from the pledged list to the in-force
  architectural principles. P30's enumeration of what a drive profile
  owns takes the read channel and the group code beside it.

- **A deficient image is no longer all-or-nothing.** Every open now states
  what it established about the evidence beneath it, and it states it
  before anything is read: `Disk::assurance` in Rust,
  `remanence_disk_assurance` and the `remanence_assurance_*` accessors in
  C, `Disk.assurance` in Python. A verified open says so and keeps every
  authority it declared. A raw image whose FAT12/FAT16 boot record
  declares more bytes than the source holds opens **degraded** instead:
  the declaration, the observed size, the first byte that is not there,
  the exact extent that reads, and the effective access mode all come
  back as ordered evidence, with the stable condition `source-truncated`
  beside them.

  A degraded session is read-only for its whole life, and that is
  evidence-driven rather than declared: a write-intent open reports the
  effective read-only mode and every mutation — write, ranged write,
  resize, mkdir, and commit — is refused carrying the condition as its
  rule identity. Reads answer for what is wholly present: a directory
  lists, a file inside the readable extent is copied out unchanged, and a
  file whose cluster chain runs into the missing tail is refused whole,
  by name, with its range — never clipped, zero-filled, or served in the
  part that happens to be there. The ranged read form is refused for the
  same file for the same reason, because an entry is extracted whole or
  not at all. Where the shortfall leaves no safe bound to state — a boot
  record declaring two different total-sector counts — the medium is
  refused at the open under the condition `evidence-conflict`, rather
  than read in part.

  The gate is deliberately narrow and its scope is a claim like any
  other: it is armed for a raw image whose leading sector is a FAT
  boot record, the composition where a filesystem's own declaration
  bounds the whole disk. A container format answers for its own declared
  size at its version gate, so no automatic degradation rule is claimed
  for qcow2, VDI, an archive, or a partition schema; and a failure of the
  library machinery around an interpretation — a claim, the session
  cache, private session storage, the commit journal, host I/O — remains
  an immediate failure that is never re-described as imperfect media
  evidence.

  The error taxonomy gains one category for this, `unavailable`: the
  artifact does not hold what was asked for, as distinct from `io`, where
  the host failed to deliver bytes that exist. The C enumerator for `io`
  moves accordingly, which the generated header carries.

  **P28 is armed**: it moves from the pledged list to the in-force
  architectural principles, where a divergence from it is a bug.

- **The VDI container is an ordinary image format.** A VirtualBox disk
  image attaches, identifies, inspects, and reads and writes its files
  exactly as a raw or qcow2 image does, through the same session, the
  same evidence model and the same commit point, because the adapter is
  the only thing that knows it is a VDI. `DiskFormat` gains
  `Vdi { major, minor }`; the C ABI gains `REMANENCE_DISK_FORMAT_VDI`
  with `remanence_disk_vdi_version_major` and
  `remanence_disk_vdi_version_minor` beside it, and Python's
  `Disk.format` answers `"vdi"` with a `(major, minor)` pair on
  `Disk.vdi_version`.

  The claim is stated and everything outside it is refused by name. The
  declared version is validated before any other field is trusted (P8) —
  major version 1, minor 0 or 1, the two shapes of the same header — and
  the image type is enumerated after it (P3): the dynamically allocated
  and fixed types are read and written, and undo and differencing name
  themselves rather than being attempted, as do per-block extra data, an
  image flag the release does not model, and a block size past the
  claimed ceiling. A block map entry marking a block unallocated, or
  allocated and then discarded, reads as the zeroes the format says it
  holds, and is never confused with an allocated block whose contents
  happen to be zero.

  Writing follows the delivered disk stack unchanged: reads never alter
  the image, writes buffer to the session cache under its declared bound,
  and commit is the single durable moment with the recovery journal
  beneath it (P2, P9, P27). Allocating a block into a dynamically
  allocated image happens inside that commit and never during a read, and
  the fault-injection harness now proves reconciliation for a VDI beside
  the raw, qcow2 and backing-chain shapes it already covered. The block
  map itself stays in the file and is read where it is needed, so the
  driver holds no mapping of its own for a failed commit to put back.

- **A stopped DOS machine's drive letters are now the library's answer.**
  `DosMachine` takes the machine facts a caller asserts — which medium
  occupies which floppy slot, which disks are attached in what order,
  whether a CD-ROM is present and where its resident driver was declared
  to be — plus the inspection reports the caller already holds, applies
  one named assignment rule, and answers with the volume each letter
  names. It opens no artifact and composes no file container over the
  result: the letter is what a consumer shows a user, and the volume
  identity is what it passes back into a file verb. `remanence_dos_*`
  and `remanence_drive_map_*` are the C ABI's, and `DosMachine`,
  `DriveMap`, `DriveMapping` and `dos_assignment_rules()` Python's.

  The assignment rule is the substance, and it is an enumerated claim
  (P3). Two variants are claimed by name — `ms-dos-4` and `ms-dos-5` —
  differing in exactly the place DOS variants differ: what becomes of a
  primary DOS partition past the first on one disk, which `ms-dos-5`
  letters after every logical drive and `ms-dos-4` letters not at all.
  Where the caller states the variant its rule settles the map; where it
  states none, both are applied and a letter they disagree on comes back
  undetermined with each rule's answer in the reason, never averaged into
  a mapping that is neither variant's. A DOS outside the claim —
  2.x through 3.3 — is refused by name rather than served by the nearest
  rule.

  What no claimed rule models is undetermined rather than approximated: a
  declared `LASTDRIVE` ceiling unsettles the letters above it, and
  `SUBST`, `JOIN`, `ASSIGN`, a resident block-device driver or a network
  redirector unsettles every letter, each saying which condition did it. A
  partition type no claimed variant letters takes none, an
  LBA-addressed extended container's logical drives take none, an
  undeclared CD-ROM takes none, and a declared CD-ROM letter that the
  rule also assigns is a refusal rather than a silent winner. A
  single-floppy machine still has two floppy letters, the second being
  DOS's phantom drive rather than a second volume. The asserted facts and
  the applied rules travel with the answer as provenance, which the map
  states is **not** evidence: nothing in it was read off a disk (P4, P19).

  P19 is amended and in force with it: a namespace composer may now
  *derive* a mapping and not only consume one, under the three constraints
  above. The composer reads its machine facts from the caller, because a
  session's device set holds only the block family and cannot express a
  floppy slot, a CD-ROM drive, or DOS attachment order.

### Changed

- **Devices are added and media are loaded, as two acts, and a device
  family is a concrete drive.** One-act `attach` is gone. A machine takes
  a device — `machine.add_device(family)`, or `add_device_at` for a
  chosen slot — and answers with the device; the device takes a medium —
  `device.load_media(path, intent)` — and answers with nothing to hold,
  because the device is the one storage handle. `device.eject()` takes
  the medium out and leaves the device where it was, and
  `remove_device` retires the slot. `Session::attach`, `attach_at`,
  `attach_with_cache`, `attach_at_with_cache` and `detach` are deleted
  along with their `Machine` counterparts, with no shim; in C,
  `remanence_{session,machine}_{attach,attach_at,detach}` become
  `_add_device`, `_add_device_at` and `_remove_device`, beside new
  `remanence_device_load_media`, `remanence_device_eject`,
  `remanence_device_attachment`, `remanence_device_family` and
  `remanence_device_is_occupied`; in Python, `Session.add_device` and
  `Machine.add_device` (with a `slot=` keyword), `remove_device`, and
  `StorageDevice.load_media` and `.eject`.

  **An empty device is first-class configuration**, which is what the
  split buys: the drive U22 letters whether or not a disk is in it now
  exists in the model, "insert the disk" no longer hangs off the disk,
  and a handle survives eject and reload while every view taken through
  it stops answering when the medium leaves.

  **The device-family catalog gains its lineage.** `DeviceFamily` stops
  being a one-variant enum and becomes an entry in a declarative catalog
  beside the media-type and drive-profile catalogs: each entry states
  what it is a kind of, and a concrete entry declares its slot prefix,
  the media types it accepts, and the drive profile it claims as its
  flux path (P22). Six are enrolled — `storage-device`, `floppy-drive`
  and `cbm-floppy-drive` classify; `commodore-1541` (`cbmfloppy0`),
  `heathkit-h17` (`heathfloppy0`) and `hard-disk` (`hdd0`) instantiate.
  **Interior names classify and instantiate nothing**: a device added as
  "some floppy" would declare no media a load could be checked against
  and no drive a machine ever had, so it is refused by name (P3). A
  family's stable spelling and its slot prefix are separate namespaces —
  `commodore-1541` is the family, `cbmfloppy0` the slot. In C the
  catalog reads through `remanence_device_family_*`; in Python through
  `device_families()`.

  **A medium belonging in another drive is refused naming both sides**,
  which is the check a concrete family exists to make possible (P14).
  An `.h8d` holds ten-sector hard-sectored 5.25-inch media, so it loads
  into a Heathkit H-17 and a hard disk refuses it, naming what the
  medium is and what the family is served. A flux artifact is refused by
  every device and says where it is read instead.

  The example C consumer takes the family as an optional second argument
  and lists the claimed families with `--families`.

- **The DOS drive-letter composer reads a machine's own device set.**
  `Machine::compose_dos_letters(rule, conditions)` — in C
  `remanence_machine_compose_dos_letters`, in Python
  `Machine.compose_dos_letters` — derives the mapping from the machine's
  devices in the order they were added, which is P32's other half: the
  attachment order is now an explicit machine fact rather than something
  only a caller could assert. Families no claimed rule letters are
  passed over **by family** — an attached `cbmfloppy0` legitimately
  receives no DOS letter — and the mapping's provenance names the
  machine, the fixed disks lettered in attachment order, the devices
  holding no medium, and the devices passed over. `DosMachine` and its
  assertions are unchanged and remain the only way to state a PC floppy
  slot or a CD-ROM drive, neither of which this release claims a device
  family for.

- **A session holds machines; a machine holds devices; and `Disk` merges
  into `StorageDevice`.** Two structural changes to the delivered types,
  no behavior change at all.

  A **machine** arrives beneath the session as the device set: it owns
  the attachment identities and the attachment order, and it carries an
  identity of its own. The session keeps its name and the meaning the
  principles already give it — the P7 claims, the P27 cache budget and
  private session storage — and owns every machine's lifetime. Machines
  in a session do not know about each other, so two of them may each
  hold an `hdd0` and neither can reach the other's. **The session's
  anonymous machine is the one whose identity is null**, every session
  has exactly one of it, and it behaves as any other machine in every
  respect; the session's own device verbs land there, which is why
  nothing a caller did before changes meaning. New surface: `Machine`,
  `Session::add_machine` (a duplicate identity refused by name, the
  empty one refused as the anonymous machine's), `machines`, `machine`,
  `require_machine`, `anonymous`; in C `remanence_session_add_machine`,
  `remanence_session_machine`, `remanence_session_machine_count`,
  `remanence_session_machine_identity` and the
  `remanence_machine_*` family, with the anonymous machine reached by
  passing a null identity; in Python `Machine`, `Session.add_machine`,
  `Session.machines` and `Session.machine`.

  **`Disk` merges into `StorageDevice` rather than being renamed.** A
  caller never holds a medium outside a device, so the delivered
  two-type shape — `session.medium(attachment) -> &mut Disk` — becomes
  one handle carrying both nodes' data: the slot-side facts
  (`attachment`, `family`, `is_occupied`) and the content-side facts
  (`identify`, `inspect`, `assurance`, `mode`, `format`, the file verbs,
  `commit`, `rollback`) on the same object, with every content verb
  refusing by name while the slot is empty. The medium survives as a
  model node and as data — its media type and profile are undisturbed —
  not as a type: `Disk` is gone from the public surface, and
  `Session::medium` with it. Callers reach the handle through the
  already-delivered `require_device`. In C, `RemanenceDisk` becomes
  `RemanenceDevice`, every `remanence_disk_*` symbol becomes
  `remanence_device_*`, `remanence_session_medium` becomes
  `remanence_session_device` (beside `remanence_machine_device`), and
  `remanence_disk_report_free` joins the report family it belongs to as
  `remanence_report_free`. In Python, the `Disk` class becomes
  `StorageDevice` and `Session.medium` becomes `Session.device`.

  In prose the geometry/volumes/files read-write stack is the **device
  stack**, following its API as D4 named it.

- **P27 splits: the resource rule keeps the title, thread invisibility
  becomes P34.** No rule changed and no surface moved — the two halves
  fail independently, so they are two principles. P27 keeps what its
  title describes: sessions stream, memory holds a bounded working set,
  and peak memory bounded independently of source size is its testable
  claim. New **P34** takes the four rules that keep threads
  undetectable — clean-only speculation, no-gap offload, demand over
  prediction, silent speculation — with its own testable claim: results,
  evidence, and refusals identical at any thread count, including none.
  The budget P34's threads spend remains P27's (D22).
- **P23 splits: what an active layer is stays P23, how it changes becomes
  P33.** No rule changed and no surface moved — the two halves fail
  independently, so they are two principles. P23 keeps the closed
  active-layer vocabulary, the one-per-independently-mutable-instance
  rule, and what cannot be an active layer; new **P33** takes the
  family-owned ladder, the least-physically-expressive initial choice,
  the requested and atomic descent, and the rule that a layer never rises
  again. P23 keeps its number because the citations in this changelog and
  in the C header are its state half. P29 widens in the same act:
  materializing a layer downward *is* a mastering act whose destination
  is an active layer rather than an artifact, so mastering now derives a
  new representation rather than a new artifact, and generate-flux's
  requirements are P29's rather than a second copy of them (D14, D21).
  `Error::rule()` returns a stable machine-readable identity where the
  refusal came from an enumerated set of rules a format, namespace, or
  grammar defines, and `None` where no such set applies — which is the
  ordinary case rather than an omission. The category still says how a
  caller should behave and is unchanged; the rule identity says which
  rule, and never substitutes for it. Rule sets belong to the seam that
  defines them rather than to the error type, which is why the identity
  is a value the seam spells and not a second library-wide enumeration:
  widening the small cross-cutting category set one entry per format rule
  would have dissolved the mapping it exists to provide (P10). Every
  fallible C ABI call takes a third optional output, `error_rule_out`,
  null where no rule applies and freed with `remanence_string_free` like
  the message; `RemanenceError` in Python gains a `rule` attribute beside
  `category`.
- **The DOS 8.3 namespace is the file-access seam's own rule**, and its
  seven rules are the first set to populate that field. A read matches
  without regard to case and returns the name as stored, so a caller can
  show a user what the directory actually holds; a write takes the name
  the caller has and stores the DOS one, uppercasing and padding at the
  seam rather than leaving a caller to perform the library's rule in the
  one place it cannot be checked against the format. A name outside the
  namespace is refused with its rule named — `empty-base`,
  `base-too-long`, `extension-too-long`, `separator`,
  `excluded-character`, `reserved-device-name`, `surrounding-space` —
  read back in Rust through `DosNameRule::from_identity`. Nothing is
  truncated, transliterated, or repaired to fit (P6). Two of those rules
  were not enforced at all before: `CON`, `PRN`, `AUX`, `NUL`,
  `COM1`–`COM9` and `LPT1`–`LPT9` are refused with or without an
  extension, being names DOS resolves ahead of any file on a volume, and
  a name ending in the separator is refused rather than silently stored
  without it. The rest were enforced already and refused with one
  undifferentiated diagnostic, which is what left a consumer
  reimplementing the set to say which rule was broken.
- **A recognized FAT volume's label is one complete answer.**
  `FilesystemInfo.label` is a `VolumeLabel` rather than a bare string:
  `name` is the label or `None` for a volume that has none, `answered_by`
  names the source that decided, and `readings` carries what each source
  held. FAT records a label in two places — the boot record's field and
  the root directory's volume-ID entry — and a volume may carry either,
  both, or disagreeing values, so the filesystem adapter holds that policy
  and states it (P18): the root-directory entry is the label DOS itself
  displays and answers wherever it exists, the boot-record field answers
  where it does not, and `NO NAME` at either source is the format's own
  spelling of unlabeled. That comparison now happens once, where the
  format is known, instead of in every consumer that displays a drive.
  Both readings stay beside the answer as evidence (P4), so a caller that
  needs a particular structure's bytes has them without opening a sector.
  Nothing else may become a label: not a directory name, not the
  filesystem kind, not a file inside the volume, and not the image's own
  filename.
- **The boot record's label field is only a field where the format says
  it is.** It belongs to the extended boot record and is read only under
  signature `0x29`, the form that carries one. Where that signature is
  absent the reading is *no such field* — a third state distinct from a
  field that is present and blank — and the shorter `0x28` form stops at
  the volume serial, so reading the label offset there would manufacture a
  label out of whatever bytes happen to sit at it.
- **`FilesystemInfo.label` is `None` only where recognition was refused**,
  which is the absence of a *filesystem* rather than of a label. Reflected
  on the C ABI as `remanence_report_filesystem_label_answered`,
  `remanence_report_filesystem_label_answered_by`,
  `remanence_report_filesystem_label_reading_count`,
  `remanence_report_filesystem_label_reading_source`,
  `remanence_report_filesystem_label_reading_present` and
  `remanence_report_filesystem_label_reading_stored` beside the existing
  `remanence_report_filesystem_label`, and in Python as the `VolumeLabel`
  and `LabelReading` classes.

### Added

- **A session holds storage devices, and a medium is reached through
  one.** `Session` is the machine scope: it holds a dynamic set of
  family-typed `StorageDevice`s, each a durable slot distinct from
  whatever medium occupies it. `Session::attach` takes the lowest free
  slot in the medium's family and returns the **attachment identity** it
  took — `hdd0`, `hdd1` — while `attach_at` lets a caller choose the
  slot. A caller chooses the slot, never the name. Attachment identities
  are deliberately caller-facing and predictable, which is the opposite
  of the opaque region, volume and filesystem identities an inspection
  report issues: a device is machine configuration the caller supplied,
  not evidence read off a disk (P21 already distinguishes the two).
  Reflected as `remanence_session_*` on the C ABI, with
  `remanence_session_medium` returning a **borrowed** medium view the
  session owns, and as the Python `Session` class with `attach`,
  `attach_at`, `detach`, `devices` and `medium`.
- **Attach and detach are machine-down operations**, so a slot freed by
  detaching is reused by a later same-family attach. That is safe
  because nothing live refers to the old occupant, and it is not the
  renumbering the layered report refuses for evidence-bearing lists.
  Each attached medium holds its own claim for exactly as long as it is
  attached; detaching releases it.
- **A storage-device family is an enumerated claim.** Only the block
  family is claimed, so `hdd0` is real and `floppy0` is refused by name
  rather than guessed at.

### Fixed

- **A flux container no longer attaches to a block device and reads as
  raw.** The image catalog opens anything it cannot identify at the raw
  adapter, and a P64 is deliberately outside that catalog because block
  and flux are disjoint families — so a P64 attached happily and was
  read as raw, declaring the block layer authoritative when its own
  adapter declares flux. In-force P13 forbids that, and a device now
  refuses a medium outside its family by name. The check reaches every
  foreign family this release can recognize; formats it has no
  recognizer for are unidentified rather than misfiled, and still open
  at the raw fallback as they always have.

### Removed

- **`Disk::open` and `remanence_disk_open`.** A medium is reachable only
  through the device holding it, because a medium opened beside the
  session would belong to no machine. The Python `Disk` constructor goes
  with them: a `Disk` now arrives from `Session.medium(...)` and cannot
  be constructed directly. `Disk::open_with_cache`'s declared cache
  bound survives as `Session::attach_with_cache` and
  `attach_at_with_cache`.

### Changed

- **One claim, one medium surface: `Session` is merged into `Disk`.** The
  library had two unrelated top-level types over the same file — a
  session that identified and read bytes, a disk that inspected and
  performed file verbs — and they could never both be used on one image,
  because each took its own P7 claim on it. Two ways in that
  structurally exclude each other is a defect in the surface, so `Disk`
  is now the one way in: `identify`, `read_at`, `image_size_bytes`,
  `image_path`, `list_hdos_files` and `read_hdos_file` join it, and
  `Session` is deleted rather than bridged. A medium's two planes — its
  own bytes, and the disk a format adapter presents above them — are
  different layers (P13) and both are served from the single claim.
  Reflected as `remanence_disk_*` replacing every `remanence_session_*`
  symbol, and as the Python `Disk` absorbing the `Session` class.
- **An image inside an archive is now a disk, not merely something
  identifiable.** The disk verbs could not reach into a `.zip` or `.7z`
  because the adapter open seam took a whole claimed file; it now takes a
  claimed *range*, so an entry stored uncompressed is opened in place at
  its offset inside the claimed archive, and a compressed one inside the
  spool it decodes to. `Disk::inspect` and the volume-scoped file verbs
  work on archived images as a result.
- **A write open on an archive entry is refused by name.** Gaining the
  disk verbs over an archive entry does not confer writes: a write would
  have to be encoded back into the archive's own grammar, which no
  adapter claims (P13), so the open states that rather than degrading.
- **Access intent is declared on every open, never laddered.** The
  identification path used to fall back quietly to read-only when it
  could not take write access, while the disk path refused by name. One
  surface cannot hold both rules and in-force P7 forbids obtaining a
  claim by silent fallback, so the refusal is what survives.
- **`Disk::size` and `Disk::image_size_bytes` are now distinct by
  name.** One is the presented disk's size, the other the image's own;
  for a qcow2 they differ, and holding both planes on one type made the
  old shared spelling a trap.

### Removed

- **`Session::mark_modified_for_test` and its bindings.** It existed
  because an identification session had no real writes to report. The
  merged surface has them, so `Identification.modified` reads the actual
  session cache and the test-only hook is gone.

### Added

- **One deep inspection of a disk, layered rather than flattened, on all
  three surfaces.** `Disk::inspect` returns a report whose records keep
  the seams apart: the block-active device, what the device's leading
  structure turned out to be, any recognized partition schema, every
  region that schema declares, every volume actually composed, and every
  filesystem recognition attempted on one. Reflected as
  `remanence_disk_inspect` with an owned `RemanenceDiskReport` handle and
  its indexed accessors, and as the Python `Disk.inspect` returning a
  `DiskReport`. **What the disk turned out to be is stated, not
  inferred**: `content` is exactly one of blank, a recognized schema, a
  direct unpartitioned volume, or non-blank content no adapter claims,
  so no caller reconstructs that judgement from lists that are each
  empty for more than one reason. **Every declared region is reported
  twice over** — the type value exactly as the schema records it, and a
  reading of what that value declares, present whether or not this
  release reads the type, so a refusal is quotable without a consumer
  keeping a second partition-type table. The reading describes the
  declaration and never the content. **Region, volume, and filesystem
  identities are opaque and derived from the layout's structure**, so an
  unchanged single-disk layout names the same objects on a later open,
  and no relationship is traversed by a string or an array position.
  **A failure at one seam neither erases nor renumbers what another owns**:
  a region whose type is refused keeps its place, and a volume whose
  filesystem could not be recognized stays a volume with the refusal
  recorded at the filesystem seam. Composed-volume count and
  host-readable filesystem-volume count are separately available for
  that reason. Scope is what is already claimed: raw and qcow2, MBR
  including extended and logical entries, a partitionless direct volume,
  and FAT12/FAT16.

### Removed

- **The `DiskGeometry` snapshot and `Disk::geometry` are gone**, with
  the flattened partition and volume records that made them up, on all
  three surfaces at once: `RemanenceDiskGeometry`,
  `remanence_disk_geometry`, and every `remanence_geometry_*` accessor
  are removed from the C ABI, and `DiskGeometry`, `PartitionInfo` and
  `GeometryVolume` from the Python module. No alias, no flattened view
  of the old model, no deprecation window — the layered report replaces
  it whole. Volume-scoped file verbs no longer take a caller-parsed
  string like `"partition:1"`: they take the opaque volume identity the
  inspection report issued, which is the only way to name a volume now.
  A caller that built those strings reads the identity out of
  `Disk::inspect` instead. What the geometry surface reported about a
  region's placement in its schema — primary slot or extended-chain
  entry — is carried on the region record, alongside the separate
  question of whether the schema declares that region as data or as
  structure.

### Changed

- **Non-blank content no adapter claims is an outcome of `inspect`
  rather than a refusal.** A disk in no format this release knows is a
  fact about the disk, so the layered report states it and carries the
  evidence. Identification is unchanged and still refuses it by name; an
  image that cannot be *read* still fails everywhere.
- `VolumeInfo` in the Rust crate and the Python module now names the
  volume record of the layered report, the FAT-shaped record it replaced
  having been removed with the rest of the geometry surface.

- **A mastered medium is saved as a P64, and a P64 is opened, on all
  three surfaces.** `MasteredMedium::describe_p64` computes what the
  container will and will not carry and writes nothing;
  `MasteredMedium::write_p64` produces the artifact; `P64Image::open`
  reads one back. Reflected as
  `remanence_mastered_medium_describe_p64` /
  `remanence_mastered_medium_write_p64` / `remanence_p64_image_open`
  with their accessors, and as the Python `describe_p64` / `write_p64` /
  `P64Image`. **The container grammar is the adapter's own claim** —
  signature, version, flags, integrity fields, chunk vocabulary,
  half-track addressing, and the width and meaning of a stored pulse's
  position and strength, enumerated in the module from the published
  format description, along with the format's own adaptive range coder.
  The version is validated before anything else is touched, and a
  version, reserved flag bit, or chunk signature past the claim is
  refused by name. Every chunk is checked against its own stored
  checksum and all of them against the header's, so a file that did not
  arrive as it was written is a refusal rather than a plausible medium.
  Both directions declare their loss before they act: a written
  container carries no policy, no per-half-track provenance, no located
  origin, no seam, and no statement that the medium was derived at all,
  and each of those is named and counted first. A medium the claim
  cannot encode — another family's addressing, another frame, a position
  the container cannot address, a strength outside the family's declared
  vocabulary — is refused rather than approximated into it. **An
  existing destination is a named refusal, never an overwrite**: the
  artifact is built beside its destination under this library's own
  claim and moved into place whole, so an interruption leaves the
  destination absent rather than half a file. Encoding is deterministic,
  so the same medium is the same bytes; conformance is a same-layer
  round trip, the written artifact reopening through the adapter's own
  decode as the same half-tracks, at the same angles, with the same
  strengths.
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

- **The durable active-layer vocabulary names the flux medium, and a
  capture is never active** (the delivered half of the P23 amendment, armed
  with this release). P23's `flux` row is renamed **flux medium** with its
  description unchanged, and flux capture takes no row at all: a capture is
  an authoritative image layer read by inspection and by mastering, and it
  never carries a session's mutable truth, because a drive writing to a
  capture would have to choose which of several disagreeing observations to
  overwrite. A capture becomes a medium by mastering under declared policy,
  never by lowering, and the generate-flux transition below CHS synthesizes
  a medium and never a capture. No code changed: the flux stack already
  behaved this way, and the vocabulary had lagged in-force P22, which names
  both models.
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
