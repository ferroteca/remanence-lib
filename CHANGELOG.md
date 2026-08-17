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

- **The storage model has an optical drive.** `DeviceType` gains a third class,
  `Optical`, with `OpticalDrive::CdRom` (`cdrom`) in it: a device
  configured like every other one, taking the bay `cdrom0`, served the
  pressed 120 mm disc, and addressing what it reads by logical block.
  The article catalog gains that disc as `optical-120-pressed` in an
  optical family of its own, whose facts are the ones a blank disc in
  its sleeve carries — its size, the spiral it was manufactured to, the
  wavelength it is read at, and that nothing can write it.

  **The disc is loaded as bytes through a declared block size**, which
  is what an ISO-like artifact is: the raw reading now records the
  CD-ROM drive alongside the two MBR hard drives and the sector floppy,
  so `Format::Raw { device: OpticalDrive::CdRom.into(), block_bytes:
  2048 }` pools a medium the drive takes and every other drive refuses
  by name. An optical spec declares no partition scheme, so the medium
  bears the direct partition, as a schemeless floppy's does.

  **Reading ISO 9660 is a separate claim and is not in this release**,
  and the gap has a visible shape rather than a quiet one: the
  schemeless content classifier reads sector 0, and ISO 9660 puts its
  first descriptor at sector 16 behind a system area that is normally
  zero — so a data disc carrying a whole filesystem inspects as
  `blank`. That is what an ISO 9660 recognition would have to fix, and
  the suite asserts the current answer so that landing one is a
  visible change rather than a silent one.

  **What the disc is *not* is the optical state model.** Sessions,
  tracks, gaps, audio and subchannels are a recording's facts; nothing
  here claims them, no optical active layer exists, and the article
  declares none of them.

- **ImageDisk (`.imd`) artifacts are read** (F68). `Format::Imd { device }`
  loads one: the header and its comment, every track's encoding mode and
  data rate, its sector-id map and optional cylinder and head maps, and
  all nine sector-record types the format defines.

  **Sectors are presented in the order the recording numbers them, and
  that is a ruling rather than a convenience** (D60). An ImageDisk track
  stores its sectors in the physical order they were recorded and states
  their ids separately; a raw dump of the same disk is already in id
  order. Resolving that belongs to the image format, because the id map
  is in the file and nowhere else — and nothing is resolved that the
  format does not state, so a raw dump's ordering remains a declaration
  some layer above makes.

  The Heath CP/M disks are what settled it: the hard-sectored raw dumps
  need a four-way skew declared in their CP/M layout, and the
  soft-sectored ImageDisk images of the *same release* need none, the
  interleave having been written into the sector numbering. Both now read,
  each under its own declared block.

  **A recording whose tracks differ declares no geometry.** That is the
  ordinary CP/M and DOS floppy, whose track 0 is not like the rest, and it
  has no single coordinate system; its bytes and every filesystem above
  them read, while `read_sector` refuses through the geometry seam's
  existing rule rather than addressing it by coordinates it does not have.
  Giving such a recording per-track coordinates is separate work.

  **A sector the artifact records as unrecovered is not zeroes**: a read
  touching one is refused with its range. Deleted-address marks, data
  errors and compressed encoding are counted into the load's account,
  which now also carries whatever the imaging tool wrote in the header.

  Read only; writing ImageDisk is refused by name.

- **CP/M 2.2 volumes read, against a layout the caller declares.** A new
  namespace reader takes CP/M's directory grammar — 32-byte entries, user
  numbers, extents joined across `EX`/`S2`, record counts, attribute bits
  carried in the high bits of the name, and allocation pointers one byte
  or two according to the block count.

  **The layout is declared because nothing on the disk can be looked up
  for it.** A CP/M volume records no structure saying where its directory
  is, how large an allocation block is, or how its sectors are ordered:
  that is the disk parameter block, and it is a structure in the BIOS
  rather than in the filesystem. Two different blocks read the same bytes
  as two different directories, both self-consistent, so a reader that
  guessed would be undetectably wrong rather than occasionally wrong.
  `"cpm"` therefore recognizes and refuses at the open, naming the layouts
  a caller may declare instead.

  That is not the same as the information being absent. A bootable CP/M
  disk carries its BIOS in the reserved tracks, so the block is often
  physically present — the Heath distribution disks each carry theirs, and
  the enrolled layouts were checked against them field by field. What is
  missing is a reliable way to *find* it: no fixed offset, no identifying
  form, inside 8080 code, and only on disks that happen to be bootable.
  Searching for a plausible fifteen bytes finds several candidates on
  these very artifacts, of which one describes the volume it sits on.

  Two layouts are enrolled, both named for the medium rather than the
  release: `"cpm-heath-h17"` for the hard-sectored 5.25-inch disk and
  `"cpm-heath-soft"` for the soft-sectored one. The release is not in
  either name because it turned out not to matter — CP/M 2.2.02 and
  2.2.03 write the same block on the same drive — while one release
  recorded two ways needs two. Their volume parameters are identical and
  they differ in exactly one field, the sector translation, which is the
  one fact the two recordings differ in.

  Every value was derived from the distribution disks and confirmed by
  reading their files back, rather than taken from a published table, and
  the reader's account says so — including that the block count is the
  artifacts' upper bound rather than a stated figure.

  **The sector map is part of the declaration.** The H-17 layout skews
  four ways, and a wrong map is the quiet failure: the directory still
  lists, because its first sector is where every candidate map agrees, and
  only the file contents come back interleaved. The artifact is put into
  logical record order once, at the open, and the account states that it
  was.

- **The HDOS reader states the initializer version byte its label
  carries**, as evidence rather than as a switch. It is reported and not
  interpreted: no mapping from that byte onto an HDOS release number is
  claimed here, because the HDOS 1.0 distribution disk carries `0x00`,
  which rules out the packed-decimal reading its other values invite.

### Removed

- **The machine tier is withdrawn; a session holds its devices
  directly.** With the drive-letter composer gone, nothing in the
  library read a device set as a set or read attachment order at all,
  and the tier's remaining justification — artifact nesting, a host's
  archive in one machine and the disk inside it in another — is a
  journey this release does not walk. Building against it now would fix
  the seam's shape before the demand that has to shape it, so the code
  stops anticipating it and the tier goes back to being an argument
  rather than a promise.

  **Nothing a single-machine caller wrote has changed.** `add_device`,
  `add_device_at`, `add_device_for`, `device`, `devices`, `attachments`
  and `release_device` were already on `Session`, delegating to the
  anonymous machine; they are the implementation now. Gone from the Rust
  crate: `Machine`, `MachineView`, `Session::add_machine`,
  `Session::machine`, `Session::machines`, `Session::release_machine`,
  `Session::anonymous` and `anonymous_mut`. Gone from the C ABI: the
  `RemanenceMachine` handle with `remanence_session_add_machine`,
  `remanence_session_machine`, `remanence_session_machine_count`,
  `remanence_session_machine_identity`,
  `remanence_session_release_machine`, `remanence_machine_identity`,
  `remanence_machine_add_device`, `remanence_machine_add_device_at`,
  `remanence_machine_add_device_for`, `remanence_machine_release_device`,
  `remanence_machine_device`, `remanence_machine_device_count` and
  `remanence_machine_device_attachment`, with the C++ header's `Machine`
  class moving with them. Gone from Python: `Machine`,
  `Session.add_machine`, `Session.machine`, `Session.machines` and
  `Session.release_machine`.

- **Guest volume mapping and drive lettering leave the claim.** This
  library reads what is *on* a disk — its partition schema, its volumes,
  their filesystems and the files in them — and no longer derives or
  reports what an operating system running above them called any of it.
  A drive letter, a mount point, a volume-GUID path: each is a fact
  about a guest's own configuration, one seam above the storage here,
  and a consumer that wants one holds the volume identity the inspection
  report issues and maps it in its own terms.

  **The question is outside the claim rather than answered
  undetermined.** Nothing reports a letter it could not settle, because
  reporting that a letter *exists* and could not be settled is itself a
  claim about a guest. Per-disk inspection, volume composition,
  filesystem recognition and every file verb are untouched, as are
  sessions, machines, devices, attachment order and the insert/eject
  edge; what a machine no longer does is read itself.

  Gone from the Rust crate: `MachineReport`, `MachineDisk`,
  `MachineVolume`, `BootOutcome`, `BootCandidate`,
  `MachineView::inspect`, `MachineView::declare_boot_device` and
  `clear_boot_device`, `Machine::boot_device`, `DosAssignmentRule`,
  `DriveMapping`, `LetterOutcome`, `ResidentCondition`,
  `DosInstallation`, `DosKernel`, `DosVersion`, `VersionReading`,
  `VersionSource` and `InstallRule`. Gone from the C ABI:
  `remanence_machine_inspect`, `remanence_machine_declare_boot_device`,
  `remanence_machine_clear_boot_device`, every
  `remanence_machine_report_*` accessor, `remanence_dos_rule_count`,
  `remanence_dos_rule_name`, `remanence_dos_rule_reading` and
  `remanence_dos_condition_is_claimed`, with the C++ header's
  `MachineReport`, `DriveMapEntry`, `MachineVolume`, `MachineDisk`,
  `LetterOutcome`, `BootOutcome` and `dos_rules()` moving with it. Gone
  from Python: `MachineReport`, `MachineDisk`, `MachineVolume`,
  `DriveMapping`, `DosAssignmentRule`, `dos_assignment_rules()`,
  `Machine.inspect()`, `Machine.declare_boot_device()` and
  `Machine.clear_boot_device()`.

  An **empty drive stays first-class configuration** on its own
  account — the machine held the drive whether or not a disk was in
  it — rather than because a letter reached it.

### Changed

- **PC-DOS 1.x volumes read, from the release that wrote no parameter
  block.** A 1.x boot sector has code where a later one puts its BPB, so
  the layout was never on the disk in a form a reader could look up — it
  was in the operating system, selected by the media descriptor, the
  first byte of the first FAT. A boot sector is now asked whether it
  states a parameter block at all, and where it does not the descriptor
  declares the layout: `0xfe` the single-sided 160 KB format, `0xff` the
  double-sided 320 KB one.

  **One byte is not enough on its own, and the extent is what checks it.**
  A descriptor matching a disk that does not hold the extent it declares
  is refused by name rather than read, and a descriptor this release
  declares no layout for is refused naming the ones it does.

  The single-sided entry is confirmed against the IBM PC-DOS 1.00
  distribution disk: 320 sectors, root directory at sector three, one
  sector to a cluster, and its forty files read back at their recorded
  lengths. The double-sided entry is declared and unconfirmed, and the
  code says which is which.

- **FM and MFM recordings are read** (F78). CRC-16/CCITT joins CRC-32 in
  `checksum.rs`, and a new IBM family carries a recording from cells to
  the sectors it states: the encodings' clock rules, the address marks
  whose deliberate clock violations are the only thing that can say a
  field begins, the addresses and both checksums stated beside computed,
  and the deleted-data mark carried as the fact the recording states
  rather than a judgement about whether a sector counts.

  Two Heath families are enrolled with it (F77): the **H-17-1**, single
  surface at 48 tracks to the inch, and the **H-17-4**, two at 96. No
  `.mfm` container is read yet, so nothing loads into these profiles from
  an artifact; what is delivered is the channel, the framing, the records
  and both declarations.

- **Step pitch is a pair of pitches rather than a count.** How many steps
  a drive takes per track is not a fact about the drive — it is the ratio
  between the pitch the mechanism steps at and the pitch the recording
  was laid down at, and a bare count answers for exactly one pairing. A
  profile now declares both, and the 1541's documented two steps derive
  from 96 over 48 rather than being asserted. A mechanism coarser than
  its recording answers zero steps, so every location refuses rather than
  reading its neighbour.

- **A drive profile's group-code declarations moved to the family that
  has them.** The shared `Presentation` carried a symbol table, a record
  grammar and their policies while the 1541 was the only family in it; an
  FM or MFM family has no symbol table at all and could only have filled
  them in with values that meant nothing. They are the 1541's own now,
  and the shared struct holds what every family has.

- **The flux presentation rungs no longer name their family** (F76). The
  bitstream and bytestream a flux medium answers were spelled
  `C1541Bitstream` and `C1541Bytestream`, which made the 1541 part of
  the type of every rung and left a medium of any other family with
  nothing to answer with. They are now `Bitstream` and `Bytestream`, and
  the seam a caller reaches is one seam whatever disk they loaded.

  **What differs between families is behind the rung, not in it.**
  Clocking pulses into bit cells is one phase-locked channel that reads
  every number it uses off the family's profile, so it moved out of the
  1541's module and takes the profile as an argument; resolving those
  bits into bytes differs in kind between families — a group code with a
  symbol table here, an address mark elsewhere — so a profile now
  carries its own transition as behavior. Enrolling a family enrols its
  codec, and nothing central branches on which family arrived.

  In C, `RemanenceC1541Bitstream` and `RemanenceC1541Bytestream` become
  `RemanenceBitstream` and `RemanenceBytestream`, and the
  `remanence_c1541_bitstream_*` and `remanence_c1541_bytestream_*`
  functions become `remanence_bitstream_*` and `remanence_bytestream_*`.
  In Python, the classes are `Bitstream` and `Bytestream`. The sector
  rung is untouched: `C1541Sectors` is the 1541's own record grammar and
  keeps its name, and it now refuses by name a bytestream resolved for
  another family.

- **`FluxImage::materialize_c1541_bitstream` loses its qualifier** and is
  now `materialize_bitstream`, in all three surfaces
  (`remanence_flux_image_materialize_bitstream` in C). The family is
  read off the artifact — an image states which family it holds — and an
  image whose family no enrolled profile declares is refused naming it,
  rather than being clocked by whichever channel was nearest. This
  overturns one clause of D39, which kept the qualifier on the ground
  that the receiver was no c1541 type and the word therefore said which
  family was meant; with the rungs family-neutral the word would instead
  be claiming something the call does not. Recorded as D59.

- The device class a slot answers — `class`, `device_class` — now also
  answers `optical`.

## 0.0.1-alpha.5 - 2026-08-15
## 0.0.1-alpha.4 - 2026-08-15

### Changed

- **A machine reads its own DOS, and the caller stops asserting one.**
  Building a machine, adding drives and loading media into them is now
  the whole of what a caller states. `MachineView::inspect()` answers
  with the machine: every device and what the medium in it turned out to
  be, which device booted, the operating system installed on the volume
  it booted, and the drive letters that system gave.

  **The facts that used to be asserted are read**, because DOS persists
  every one of them. The kernel files in a root directory say which DOS
  it is — `IO.SYS` with `MSDOS.SYS` for MS-DOS, `IBMBIO.COM` with
  `IBMDOS.COM` for PC DOS, `KERNEL.SYS` for FreeDOS, the *set* being the
  evidence rather than any one file. The version is settled the way
  geometry is: from ordered sources, each kept with where it was taken,
  `Undetermined` where two disagree and `Unstated` where none spoke.
  `CONFIG.SYS` and `AUTOEXEC.BAT` — or `FDCONFIG.SYS` and `FDAUTO.BAT`,
  which FreeDOS prefers — declare `LASTDRIVE`, the block-device drivers,
  the network redirectors, and `SUBST`, `JOIN` and `ASSIGN`. An
  `MSCDEX /L:` line places an optical drive exactly, rather than leaving
  every letter in doubt.

  **One fact stays the caller's, because no disk holds it**: which
  device the firmware booted. `MachineView::declare_boot_device` is
  where a stopped machine's host states that its BIOS booted something
  other than the default, and a report marks it as configuration rather
  than as evidence. Declaring nothing leaves the era's firmware order to
  settle it — the first attached bootable device — with the partition
  table's own boot flag settling a tie inside one disk.

  **FreeDOS is claimed** and letters as MS-DOS 5 and 6 do, by its
  kernel's documented default. Its order is a setting patched into
  `KERNEL.SYS` by `SYS CONFIG` rather than declared in any configuration
  file, so this release does not read it and a recognition says so
  rather than leaving the gap silent.

  **Removed with the assertions**: `DosMachine` and its `assert_floppy`,
  `assert_fixed_disk`, `assert_cdrom` and `declare` verbs; `DriveMap`;
  `MachineDevice`; `MachineView::compose_dos_letters`; the
  `LetterOutcome::DeclaredDevice` outcome, replaced by `OpticalDrive`;
  and in the C ABI the eight `remanence_dos_machine_*` functions,
  `remanence_machine_compose_dos_letters` and the sixteen
  `remanence_drive_map_*` accessors. In Python, `DosMachine` and
  `DriveMap` go the same way. What replaces them is
  `remanence_machine_inspect` with the `remanence_machine_report_*`
  accessors, `remanence::MachineReport` in C++, and `Machine.inspect()`
  in Python, each answering the same reading.

  The claimed assignment rules stay an enumerated claim (P3) — a DOS
  outside MS-DOS 4.0 through 6.22 is refused by name rather than served
  by the nearest rule — and what a report applied is named in its
  provenance, which remains provenance and not evidence.

- **Drive letters go to the bootable partition, not the first one.**
  Both claimed rules letter a disk's *bootable* primary DOS partition
  ahead of the rest, falling back to the first row only where the table
  flags none. This is a correctness fix independent of the reshaping
  above: on a disk whose active partition is not its first primary, the
  previous behaviour handed `C:` to a volume DOS never gave it to, and
  said nothing about having done so. `RegionInfo` gained
  `declared_active` — the boot flag exactly as the table records it — so
  the evidence the rule now turns on is visible in the report as well.

- **A raw image may be declared as any device it could have been
  recorded by, not only a hard drive.** `Format::Raw` carries a
  `DeviceType`, so a floppy image loads into a floppy drive and a
  machine's `A:` and `B:` are drives it holds rather than facts asserted
  beside it. Bytes record no ecosystem, which is why the raw reading's
  device list reaches across families while the container formats' do
  not.

  **A block medium's article now follows its declared device**, falling
  back to the format's own where nothing declares one. Every existing
  pairing is unchanged — each claimed hard drive is served the same
  logical-block article the block formats declare — and what it fixes is
  a floppy image being enrolled as a logical-block medium and then
  refused by the drive it was loaded for. `From<HardDrive>` and
  `From<FloppyDrive>` conversions into `DeviceType` land beside the ones
  `DeviceSlot` already had.

### Added

- **C++ consumers get an idiomatic header, derived from the C ABI.**
  `crates/remanence-ffi/include/remanence.hpp` is header-only and
  C++17: RAII classes over the handles the ABI hands you to free, views
  over the ones the session owns and documents as never-free, scoped
  enumerations whose constants *are* the C ones, and refusals as
  `remanence::Error` — a `std::runtime_error` carrying the delivered
  category and, where an enumerated rule set owns one, the rule
  identity. An honest absence comes back as an empty `std::optional`
  rather than an exception, and every accessor on a handle answers an
  owned `std::string`, so nothing dangles when the handle was a
  temporary.

  **It is not a fourth surface and adds no reach.** The C ABI remains
  the norm, this derives from it exactly as the generated C header
  derives from the Rust, and it moves with the ABI in the same change.

  **It covers the whole ABI**: the storage model — sessions, machines,
  devices, media, discoveries, partitions, volumes, filesystems, files,
  the inspection report and the DOS drive-letter composition — and the
  flux ladder beside it, the remanence image with its bitstream,
  bytestream, recognized sectors and the d64, g64 and p64 renditions,
  each carrying the account of what it did not carry. A `remanence_*`
  function that is not wrapped is a defect rather than a boundary.
  `examples/identify.cpp` is the example consumer beside the C one, and
  the suite compiles the header standalone, runs a C++ caller through
  it, and counts what its destructors give back (D53, D54).

- **The sdist carries a pytest suite; the wheel still carries none of
  it.** An sdist is conventionally the artifact a stranger can build
  *and verify* from — distro packagers run the upstream suite at
  package-build time, on platforms and Pythons this project never tests
  against — so `remanence-0.0.1a3.tar.gz` now ships
  `crates/remanence-py/tests/`, runnable with `pip install
  remanence[test] && pytest`. The wheel's contents are unchanged: a
  consumer has no use for a suite in their `site-packages`.

  The suite opens no disk image. Every fixture this project tests
  against is third-party media it does not distribute, so the shippable
  tests make their own media through `Session.new_media` — coordinates,
  sector round-trips through a commit, the direct partition, the
  assurance whose claim is `authored` — beside the catalogs, the refusal
  contract, and the type stub checked against the installed module. What
  stays out is the part that tests the repository rather than the
  package: the Rust integration tests and the mypy fixtures they drive
  (D48).

- **The Python module ships a type stub and a `py.typed` marker.**
  `remanence/__init__.pyi` states S3 in full — every class, property and
  verb, with `Option<T>` as `T | None`, byte payloads as `bytes`, paths
  as `str | os.PathLike[str]`, and frozen fields as read-only
  properties — so an editor completes the surface and `mypy --strict`
  checks it. Both files reach the wheel and the sdist; the wheel's shape
  is otherwise unchanged.

  The stub is written by hand and is **surface, not documentation**: a
  name added, renamed or removed in the module moves it in the same
  change, and a disagreement is a bug in the stub, the module being the
  norm. Stable spellings — format ids, device types, articles, rule
  identities — are typed `str` rather than `Literal` unions, because the
  claim is enumerated at runtime by `formats()`, `device_slots()` and
  their kin, and a frozen copy in the stub would drift from it (D40).

### Changed

- **The Python exception is `remanence.Error`, not
  `remanence.RemanenceError`.** The old name repeated the module in the
  class; PEP 8 asks for the `Error` suffix rather than a unique word,
  and `sqlite3.Error` is the stdlib precedent for a module with one
  exception type. The rename moves the class's `__name__` too, so a
  traceback reads `remanence.Error` rather than the old name behind an
  alias. Its two attributes are unchanged and are now declared in the
  stub: `category: str`, always set, and `rule: str | None`, naming
  which rule of an enumerated set the input broke. The C ABI's
  `RemanenceErrorCategory` is **not** affected — every exported C type
  carries the library prefix, C having one namespace (D41).

- **`as_type` is now `check_type`.** The `as_` prefix promises a
  conversion in Rust (C-CONV), and this verb performs none: it states
  the caller's reading of a partition's type, checks it against the
  recorded byte, and answers `Result<()>` — the refusal is its whole
  value. The new spelling says that; the old one argued against it. The
  behaviour is unchanged, including the direct partition's refusal by
  name. Rust `Partition::check_type` and `PartitionView::check_type`,
  C `remanence_partition_check_type`, Python `Partition.check_type`
  (D38).

- **The count accessors on the c1541 rungs are spelled `_count`.**
  `C1541Bitstream::locations`, `C1541Bytestream::locations`,
  `C1541Sectors::locations` and `C1541Sectors::claims` each returned a
  `u64` under a plural-noun name. They are now `location_count` and
  `claim_count`. This bit hardest in Python, where the bindings expose
  them as properties: `bitstream.locations` evaluated to a number and
  read as a library defect. The C ABI already spelled them
  `remanence_c1541_bitstream_location_count` and
  `remanence_c1541_sectors_claim_count` and **is unchanged** — the core
  and the Python module move to the spelling C already carried. The
  `BitstreamReport`, `BytestreamReport` and `SectorReport` fields keep
  `locations` and `claims`: those hold the collections, where the plural
  is correct (D38).

- **The `.remanence` flux root is `FluxImage`, not `RemanenceImage`.**
  In the C ABI `remanence_image_*` named this root across 36 functions
  while `remanence_medium_image_path`, `remanence_discovery_image_format`
  and their kin used `image` in the ordinary disk-image sense, so one
  word meant two things in one namespace. The family moves together:
  Rust and Python `FluxImage`, `FluxImageReport`, `FluxHole`,
  `FluxOrbit`, `FluxWriteReport`; C `RemanenceFluxImage`,
  `RemanenceFluxHole`, `RemanenceFluxOrbit`, `RemanenceFluxWriteReport`
  and the `remanence_flux_image_*` prefix, the library type prefix being
  kept as C requires. `remanence::RemanenceImage` no longer stutters
  (D39).

- **The flux write-report accessors are named after their own type.**
  `remanence_image_write_path(report)` shared a prefix with the verb
  `remanence_image_write(image, path)` and read as "write path". They
  are now `remanence_flux_write_report_*`, matching
  `remanence_d64_report_*` and every other report in the C ABI. C only
  (D39).

- **A `c1541` qualifier the receiver already carries is dropped.**
  `C1541Bitstream::materialize_c1541_bytestream` becomes
  `materialize_bytestream` and `C1541Bytestream::recognize_c1541_sectors`
  becomes `recognize_sectors` — which is what the C ABI already spelled
  them, so the three surfaces now agree.
  `FluxImage::materialize_c1541_bitstream` **keeps** its qualifier: that
  receiver is not a c1541 type, so the word says which family is being
  materialized (D39).

- **`get_sector`/`put_sector` are `read_sector`/`write_sector`.** Rust
  discourages the `get_` prefix (C-GETTER), and the crate already spelled
  the same act `C1541Sectors::read_sector`. C
  `remanence_medium_read_sector` and `remanence_medium_write_sector`,
  Python `Medium.read_sector` and `Medium.write_sector`. The addressing
  rules and the `GeometryRule` refusals are unchanged (D39).

## 0.0.1-alpha.3 - 2026-08-12

### Added

- **Authored media: `new_media` creates blank media whole, and
  authorship is the third fact class.** Evidence is discovered onto
  media and declarations are configured onto machines;
  `Session::new_media(kind)` is neither. It takes one enumerated
  `NewMedia` kind and the facts that declaration states become the
  medium's *original* facts — carried from creation as its
  `assurance()` provenance and, where the kind states coordinates, as
  its `geometry()`, whose one reading is the new
  `GeometrySource::Authorship`. Nothing is read, probed or opened,
  there being no artifact.

  The kinds are an enumerated claim like every other creation grammar
  here (P3). The **blank article kinds** —
  `NewMedia::Flexible525Soft`, `NewMedia::Flexible525HardTen`, each
  spelled by the article it makes — create that manufactured substrate
  with nothing recorded on it: the article is the whole of what they
  state, so they state no coordinates and bear no content.
  `NewMedia::ChsDisk { geometry }` is the kind whose facts *are*
  coordinates; its article is the new **`authored`** entry, a second
  member of the virtual family whose native vantage is a space where
  the archive's is a namespace — nobody manufactured either. A geometry
  with a zero anywhere in it, or one addressing more bytes than a
  medium could hold, is refused when it is stated, which is the one
  moment authorship offers to check it.

  **An authored blank assumes no device**: `device_type()` answers
  `None`, as an archive's does and for the same reason, so no drive
  takes one and `insert` refuses by name. `Medium::authored_as()` says
  which kind made it, or `None` for a medium loaded from an artifact.
  It bears the direct partition over its own content — addressable, no
  namespace, nothing classified to establish it — and the namespace
  vantage refuses toward the **authored-to-recorded arc**, which stays
  reserved. It is **session-backed** until an explicit encode gives it
  an artifact: the content lives on a sparse blank in private session
  storage within the declared cache bound (P27), so a 528 MB authored
  disk costs what was written to it, and `commit()`/`rollback()` are
  the ordinary commit point over it (P2) with no recovery journal
  beneath — no file changes for an interruption to leave half-written.
  `Claim` gains a third class, `authored`, because nobody opened
  anything.

  S2 mirrors it: `remanence_session_new_media` with the
  `remanence_new_media_count`/`_id`/`_name`/`_article`/`_takes_geometry`
  catalog and `REMANENCE_CLAIM_AUTHORED`; `identify --author [kind]` is
  the example consumer's walk. S3 mirrors it: `Session.new_media(kind,
  cylinders=…, heads=…, sectors_per_track=…, sector_bytes=…)`,
  `Medium.authored_as`, and the module-level `new_media_kinds()`.

- **`load_media` gains its source shapes, and a KryoFlux capture loads
  as a medium.** The one load verb now reads four declared source
  shapes (`MediaSource`, arrived at by plain conversion): the caller's
  own opened `std::fs::File`, a collection of them, a `FileSource`
  taken from an archive medium's namespace, or a collection of those —
  and **a format declares which shape it reads**, refusing the other by
  name. `File::source()` takes one namespace file as a load's source
  and `StorageSpace::files(path)` gathers every file under a path, each
  free-standing and riding the archive's claim, a solid 7z's coded
  stream decoded once for the whole gathering (P27). In Python,
  `load_media` accepts the same four shapes; in C,
  `remanence_session_load_media_collection` takes an array of OS
  handles and `remanence_file_source`/`remanence_space_files` with the
  `remanence_session_load_media_source`/`_sources` pair carry the
  namespace shapes.

  **`Format::KryoFlux { device }` (id `kryoflux`) is the first
  collection-sourced format.** The member grammar, the set's
  completeness, every stream's own grammar and the declared device's
  profile claim are checked whole — which capture head carries the
  recording is measured, the unrecorded back reading as noise — then
  the gap-first reduction runs under the profile's declared
  `Materialization` defaults, and what pools is a `Commodore1541`
  medium with the verdicts, the policy and the declared-loss account
  riding its assurance as provenance (P28, P29). **`Format::P64` (id
  `p64`) loads the served form straight in** — the one format id F53's
  declared set did not carry (D31), because a P64 answers with a flux
  medium and that medium is now an ordinary pooled one: it bears the
  direct partition, seats in a `Commodore1541` drive, and outlives
  whatever archive its members were gathered from.

- **The flux presentation is argument-free: the type carries the
  channel and the codec (P30 reached through the type).**
  `Medium::bitstream()` and `Medium::bytestream()` answer on a flux
  medium — materialized once under the profile's declared channel and
  codec policies, the same state answering every call — and refuse by
  name where the device type's profile bears no flux (P13). The C1541
  profile now declares its whole presentation policy: the family's own
  density map, unzoned locations omitted and counted, weak pulses
  resolved reproducibly from the profile's stated seed, landmark
  framing, unassigned symbols kept as their own bits, and checksum
  failures and unpaired records declared as loss. The rungs above
  follow: `RemanenceImage::materialize_c1541_bitstream(cache_bytes)`,
  `C1541Bitstream::materialize_c1541_bytestream(cache_bytes)` and
  `C1541Bytestream::recognize_c1541_sectors(cache_bytes)` take a P27
  bound and no policy, and the policy types
  (`ReadChannelPolicy`, `GcrCodecPolicy`, `SectorPolicy` and their
  enums) leave all three surfaces — the deviation surfaces stay
  deferred (D29). A flux medium's `filesystem_as("cbmdos")` opens the
  directory through the medium's own direct partition, materializing
  the ladder beneath it.

- **`C1541Bytestream::location(Location::track(n))` reads the framed
  bytes of one location** — `LocationBytes::read_at`, the first byte
  being the first *framed* byte because nothing before sync is a byte
  at all; a track the stream does not hold is absent rather than blank,
  and a byte the family's table does not assign is refused rather than
  invented. `Entry::fact(key)` answers one declared fact by the
  recognizing filesystem's own key.

- **Discovered geometry: the recording's own coordinates, read as
  evidence and never declared.** `Medium::geometry()` answers a
  `Geometry` — what the sources beneath the medium stated about its
  coordinates, established when the medium was loaded and evidence from
  then on. The sources are enumerated (`GeometrySource`): the image
  format's own declaration where it makes one (`h8d` records 40
  cylinders of one side at ten 256-byte sectors) or the block size a raw
  load declared, a FAT boot record's recorded sectors-per-track, heads
  and bytes-per-sector, the partition table's addressing unit and its
  **end tuples** where one solves against the extent the same entry
  declares, and arithmetic over the content's own extent for the
  cylinder count. Every `GeometryReading` keeps the parts its source
  actually states, where in the artifact it was taken, and what it says
  in its own terms (P4).

  **Sources that disagree settle nothing.** Where two readings state
  different values for one part of the coordinates, the state is
  `Undetermined`, both readings stand, and `conflicts()` names what
  disagrees with what — nothing ranks sources or breaks ties. Where
  nothing states one at all the state is `Unstated`, kept distinct
  because "no source spoke" and "the sources contradict each other" are
  different facts about an artifact; `unsettled()` names which parts are
  missing either way. A geometry is whole or it is nothing: a record
  with holes in it would address nothing.

  **`Medium::get_sector` and `Medium::put_sector`** address in what that
  established — cylinders and heads numbering from zero and **sectors
  from one**, which is the recording's convention rather than this
  library's — on the device types whose `addressing()` says `sector`. A
  write buffers until `commit()` like every other write (P2). Everything
  else refuses by name, its own `GeometryRule` set saying which:
  `not-sector-addressed` for a block-addressed drive or a medium no
  device recorded, `geometry-unstated` and `geometry-undetermined` for
  the two evidence states with no coordinates to offer,
  `outside-geometry` for a coordinate the geometry does not cover — or
  one it covers and the content does not hold — and `partial-sector` for
  a buffer that is not one whole sector.

  In C: `remanence_medium_geometry` and `remanence_geometry_free`, with
  `remanence_geometry_state`, `_coordinates`, `_conflict_count`/
  `_conflict`, `_unsettled_count`/`_unsettled`, `_reading_count` and the
  `remanence_geometry_reading_*` readers (source, at, detail, cylinders,
  heads, sectors per track, sector bytes), plus
  `remanence_geometry_source_count`/`_source_name` and
  `remanence_medium_get_sector`/`_put_sector`. In Python: the
  `Medium.geometry` property answering a `Geometry` of `GeometryReading`
  records, `Medium.get_sector(cylinder, head, sector)` answering
  `bytes`, `Medium.put_sector(…, data)`, and `geometry_sources()`.

- **Device types: one identity per medium naming the device its content
  was recorded by.** The catalog is enumerated in two levels — the
  **class** (`DeviceType::Floppy`, `DeviceType::HardDrive`, with optical
  and tape reserved for the coming families), then the **concrete type**
  within it: `FloppyDrive::Commodore1541`, `FloppyDrive::HeathH17`,
  `FloppyDrive::HeathH37`, `FloppyDrive::Sector`, and
  `HardDrive::MbrSector`, `HardDrive::MbrBlock`, `HardDrive::Gpt`. A type
  the library does not know **fails to compile**; the display strings
  (`c1541`, `mbr-block-hd`) survive in provenance, refusals, and the C
  and Python spellings, where the identity crosses as text. `Medium`
  answers `device_type()` as an `Option`, and `None` is the honest
  answer rather than a gap: an archive was recorded by no device.

  **The granularity rule cuts the catalog**: a device type is the
  coarsest name fixing the whole addressing surface and recording
  discipline without per-media parameters. What the device fixes lives in
  the type — the hard-drive specs carry the **partition scheme itself**,
  and `HardDrive::Gpt` is block-addressed by GPT's own definition — and
  what varies disk to disk lives on the medium, which is why the generic
  sector floppy declares no geometry. Each spec **composes** an article
  and restates none of its facts, so D19's three homes hold: the
  substrate in the article catalog, the recording here, the drive's
  behavior in the P30 profile. A type's definition has one home — one
  spec shape per class, one instance per concrete type — and the
  enumeration is the instantiation.

  In C: `remanence_device_slot_count` and the `remanence_device_slot_*`
  readers (id, name, provenance, class, article, prefix, flux path,
  scheme, addressing). In Python: `device_slots()` answering `DeviceSlot`
  records — whose class is spelled `device_class`, `class` being a
  Python keyword and unreachable as an attribute — and
  `Medium.device_type`.

- **The load declaration carries the device.** A format that records one
  device type carries it bare — `Format::H8d` **is** a Heathkit H-17
  recording — and one that records many takes the caller's declaration,
  the field typed by the class its adapter records: `Format::Qcow2 {
  device: HardDrive }`, `Format::Vdi { device: HardDrive }`, and
  `Format::Raw { device: HardDrive, block_bytes }`, which carries the
  block size too because a raw image records no addressable unit of its
  own. **A flux capture of a hard drive fails to compile**, and a
  pairing no adapter declares within the class — a `gpt-hd` today, which
  no adapter in this release reads — is a named refusal at the load
  rather than a silent reading of the wrong table.

  `Format::claimed()` is the catalog behind that: each entry states the
  device types its adapter records and whether its declaration carries a
  block size, and `Format::declared(id, device, block_bytes)` is the
  text-boundary constructor the C and Python surfaces use, refusing each
  half by name on its own terms. In C, `remanence_session_load_media`
  takes the device type and the block size beside the format, with
  `remanence_format_device_count`/`_device` and
  `remanence_format_takes_block_bytes` enumerating what each accepts; in
  Python, `Session.load_media(source, format, device=…,
  block_bytes=…)`, with `formats()` carrying the same four facts.

- **The partition pool: every medium bears a partition, and its content
  is reached through one.** `Medium::partition(ordinal)` is the borrow
  that holds a partition and its medium at once, `Medium::partitions()`
  hands over the whole pool as values, and `Medium::partition_scheme()`
  names the scheme it was populated under (P16, P19). The ordinals are
  the scheme's own — MBR entry 1 is `1` — so a partition carrying a
  refusal keeps its number and nothing behind it renumbers (U4). **The
  pool is established when the medium is loaded** — by `load_media`,
  `load_discovery` and `add_device_for`, under the medium's own kind
  rather than by probing for a layout — and is evidence from then on,
  immutable for the session's life. Nothing re-reads a table behind a
  caller's back, and nothing adds a partition to a medium that was
  loaded without one: the create and release slots a partition editor
  would need are deliberately unfilled.

  **A `Partition` is a value, not a handle.** It holds nothing and
  outlives nothing, and it carries what the scheme declared beside what
  the library composed over it: the type value exactly as recorded and,
  next to it, a reading of what that value *declares* — the reading
  describes the declaration and never the content, so a partition
  recording `0x07` is explained as NTFS or exFAT without thereby being
  asserted to hold either — the boot flag as `active()`, the placement
  in the scheme's own vocabulary (`"primary"` for one of MBR's four
  slots, `"logical"` for an entry on the extended chain), the role,
  which is a different axis from placement and does not follow from it,
  the extent as `start_bytes` and `length_bytes`, whether this release
  reads the declared type at all, whether each vantage opens, the
  structured `issue()` that keeps an unreadable partition in the pool
  rather than dropping it out of the account, and the `evidence()` the
  scheme's adapter read to declare it (P4).

  **Ordinal 0 is the direct partition**, which the library composes
  rather than reads. A scheme numbers its own entries from one, so zero
  is the library's to spend, and it is spent where a medium records no
  scheme at all — a filesystem boot record where a table would be, a
  blank disk, and content nothing claims being three different answers
  about the same absence. It declares no type, so a reading made of it
  is refused by name rather than checked against nothing, and its
  account is `provenance()` and not `evidence()`: a composition act is
  stated as one, synthetic and said to be, never offered as something
  the medium said. Over a medium whose native vantage is a namespace it
  is **extent-less**, holding no start and no length, because nothing
  composed a position for it to be within.

  **`PartitionView` carries the two vantage doors, and both hand out the
  same node** (D26). `volume()` answers where the partition composes an
  addressable extent and `filesystem()` where its declared type
  determines a namespace; the `StorageSpace` either door hands over
  carries whichever vantages that partition has, so which door was
  opened changes nothing about what comes back and the choice is only
  which question is being asked. Both are `Option` because both are
  lookups — the extent and the declared namespace were settled when the
  pool was established — and opening one spends the view, which is that
  identity rule carried by the type.

  **`as_type` is the caller's reading and the library's check** (P3). A
  declaration names one entry of the new `PartitionType` set — a DOS
  data partition (`0x01`, `0x04`, `0x06`, `0x0e`) or a DOS extended
  partition (`0x05`, `0x0f`) — and the byte the scheme recorded is
  weighed against the values that reading covers, a disagreement being
  refused naming both sides. Where no partition type determines a
  namespace, `filesystem_as` is the same discipline one vantage further
  in: it claims exactly `"fat"`, `"hdos"`, `"cpm"` and `"cbmdos"`,
  refuses any other spelling naming what it does read, and **runs P18's
  recognizer to verify the declaration rather than to pick one** — the
  adapter the declaration names is the adapter that reads it, and
  content that cannot bear the declaration is refused by that adapter,
  by name. `"cpm"` still refuses at the open as recognized-and-not-read,
  recognition and reading being separate claims.

  Everything behind a door is specified and nothing is probed for, and
  three enumerated sets carry the specification across the surfaces:
  `PartitionScheme` (`mbr`), `PartitionType` (`dos-primary`,
  `dos-extended`, each answering the type values its reading covers),
  and `PartitionRule`, the seam's own refusal set (P10) —
  `partition-type-disagrees`, `no-declared-type`, `unclaimed-namespace`
  and `partition-no-extent`.

  In C: `remanence_medium_partition_scheme`,
  `remanence_medium_partition_count` and
  `remanence_medium_partition_ordinal` over the pool;
  `remanence_medium_partition`, answering a handle the caller ends with
  `remanence_partition_free`; the `remanence_partition_*` readers over
  the record, its evidence and the direct partition's provenance
  included; `remanence_partition_as_type`; `remanence_partition_volume`,
  `remanence_partition_filesystem` and
  `remanence_partition_filesystem_as`, each answering the same
  `RemanenceSpace` the positioned-read and `remanence_filesystem_*`
  verbs already take, and null where the vantage does not open; and the
  catalogs `remanence_partition_scheme_count`/`_id`/`_name` and
  `remanence_partition_type_count`/`_id`/`_name`. In Python: a
  `Partition` class carrying the record and the three doors,
  `Medium.partition(ordinal)`, `Medium.partitions`,
  `Medium.partition_scheme`, and the `partition_schemes()` and
  `partition_types()` catalog functions.

### Changed

- **Discovery holds the claim and builds no cache.** `discover_media`
  opens the artifact, takes the P7 claim, probes for the type, and
  stops: no medium, no session cache, no spilled backing. It used to
  open a whole medium under a declared bound, which is a load's work
  done before anyone asked for one — and doing it made the ask-first
  journey a duplicate of the declared one. It is not: loading says
  *make this a medium under a format I name*, discovery says *what is
  this?*, and this constraint is what keeps them distinct (D30).
  Everything a `Discovery` reports still answers — the article, the
  presented size, the format, the recorded and accepting devices, the
  effective mode, the assurance, and `identify()` — because the probe
  reads the bounded evidence its claims name (P27), as identification
  always has. The discovery stays **consumable**: the load takes the
  claim out of it and builds the medium over that very claim, so
  nothing is re-opened, no adapter runs twice, and no window opens
  between the question and the load.

- **The cache bound moves from the discovery to the load.** A bound is
  a declaration about state that exists (P27), so it is stated where
  the medium comes into existence. In Rust:
  `Session::load_discovery_with_cache(discovery, bytes)` and
  `Session::load_discovery_as_with_cache(discovery, device, bytes)`
  join the two plain doors. In C:
  `remanence_session_load_discovery_with_cache` and
  `remanence_session_load_discovery_as_with_cache`. In Python:
  `Session.load_discovery(discovery, cache_bytes=…)` and
  `Session.load_discovery_as(discovery, device, cache_bytes=…)`.
  `add_device_for_with_cache` is unchanged and keeps its bound — it
  composes the discovery *and* the load, and the bound belongs to its
  load half.

### Removed

- **`discover_media_with_cache` and the bound travelling into the
  device with a discovery.** Gone from all three surfaces:
  `remanence::discover_media_with_cache`,
  `remanence_discover_media_with_cache`, and the `cache_bytes` keyword
  on Python's `remanence.discover_media`. A verb that creates nothing
  has nothing to bound; the bound is declared at the load, above.

- **The standalone `CaptureSet` and `P64Image` roots are folded into
  the model.** Gone from all three surfaces: `CaptureSet` and its
  report types (`CaptureSetReport`, `CaptureSetMember`,
  `CaptureRunReport`, `ObservationReport`, `CaptureIssue`,
  `TimeBaseReport`, `StepPosition`), `P64Image` (the root —
  `P64Report` and `P64HalfTrack` stay, answered by the renditions),
  the recognition reporting (`Recognition`, `ProfileVerdict`,
  `LocationVerdict`, `ZoneClaim`), and the reconstruction surface
  (`ReconstructionPlan`, `ReconstructionPolicy`,
  `ReconstructionReport`, `ReconstructedOrbit`, `RecordingSelection`).
  A capture and a P64 are reached through `load_media` like every
  other medium; the recognition runs inside the declared load, pinned
  to the declared device's profile, and its verdict rides the medium
  as provenance. Capture-inspection reporting and plan preview stay
  out, with the question tier. One consequence is stated in D35 so
  nobody meets it as a surprise: until U23's destination-format save
  verb lands, no public path produces a `RemanenceImage` from a
  capture — the `.remanence` root opens existing artifacts and masters
  the renditions, and a capture loaded as a medium reaches no writer.
  The example's `identify --reconstruct` mode goes with the surface
  that carried it.

- **The medium's resolver and selector retire: `Medium::filesystem()`
  and `Medium::volume(id)`.** One resolved to a namespace wherever
  exactly one candidate stood beneath the medium; the other selected
  among the volumes an inspection had already issued identities for.
  Neither is renamed and neither is bridged, because what replaces them
  is a different shape rather than a different spelling: **uniformity of
  the walk replaces resolve-without-selecting.** Resolving bought the
  simple medium one step at the price of two shapes for one library — a
  path where the layers were named and a path where they were skipped —
  so a caller who wrote against the short path rewrote it on meeting a
  disk that turned out to be partitioned. Now the partition is named in
  both, and the step it costs the simple medium is the step every other
  caller was already taking.

  **`SpaceRule::SeveralCandidates` goes with them**, and `SpaceRule::ALL`
  is five long. Its whole subject was a resolution that found more than
  one candidate and would not choose between them; nothing resolves
  among candidates any more, so the rule had nothing left to refuse. The
  identity `"several-candidates"` reaches no surface: it cannot come
  back through C's `error_rule_out` or through a Python exception's
  `rule`.

  In C: `remanence_medium_filesystem` and `remanence_medium_volume` are
  gone. In Python: `Medium.filesystem()` and `Medium.volume()` are gone.
  Nothing is bridged and nothing is aliased — each journey they served
  is written through the pool instead: ordinal 0 with a declared reading
  where a medium records no scheme, and the scheme's own ordinal where
  one does.

### Changed

- **Every device type declares how it addresses its recording.**
  `DeviceType::addressing()` answers `"sector"` or `"block"` rather than
  `Option`: the floppy class was left unanswered when the attribute
  landed, and it is `sector` — a floppy drive steps to a track and reads
  records around it, whatever geometry the disk in it was recorded
  under. It is the type's half of the sector verbs and the medium's
  discovered geometry is the other: the type declares *that* there are
  coordinates, the evidence says how many of each. The C and Python
  slot catalogs are unchanged in shape (`remanence_device_slot_addressing`
  and `DeviceSlot.addressing` are null only for the archive receiver,
  which is no device type at all).

- **In-force P14 is amended: the recording side is the device type.**
  The principle gains the device-type catalog and its granularity rule
  beside the article it already carried, and the media-type vocabulary
  is superseded throughout: what a medium **is** is now `article()` —
  `flexible-5.25-soft`, `flexible-5.25-hard-10`, `logical-block-512`,
  `virtual` — and what **recorded** it is `device_type()`. The archive's
  article is renamed `virtual` from `archive`, which is what it always
  was: a substrate with no physical article behind it.

  In C: `remanence_medium_article` and `remanence_medium_device_type`
  replace `remanence_medium_media_type`; `remanence_discovery_article`,
  `_article_name` and `_device_type` replace the media-type pair and
  `_default_device`; `remanence_layer_disk_article` and
  `remanence_report_device_article` replace their `_media_type`
  spellings, with `remanence_report_device_type` beside the second. In
  Python: `Medium.article`, `Medium.device_type`, `Discovery.article`,
  `Discovery.article_name`, `Discovery.device_type`, and the `article`
  and `device_type` fields of `DiskLayout` and `DeviceInfo`.

- **The device-family catalog is replaced by the device type, and the
  lineage goes with it.** A device is now typed by a `DeviceSlot`: a
  recording device of one concrete type, or the **archive receiver**,
  which is no device type at all — an archive was recorded by no device,
  so there is no recording for a type to name, and `arc0` stays an
  ordinary attachment identity (D27). `add_device` takes that, and
  **there is no interior name left to refuse**: "some floppy" is not a
  value, because the catalog is an enumerated two-level type and a name
  the library does not know fails to compile. `StorageDevice::slot()`
  and `device_type()` answer what a device is;
  `DeviceFamily`, its lineage query and its `accepted_media` are gone.

  **An attachment identity now names a place rather than a recording.**
  Several device types share one bay — three hard-drive types take
  `hdd`, both Heathkit controllers take `heathfloppy` — so
  `AttachmentId` carries the prefix and the index, `AttachmentId::prefix()`
  replaces `family()`, and the lowest free slot counts by bay: two hard
  drives of different types cannot both be `hdd0`. In C,
  `remanence_device_slot` and `remanence_device_type` replace
  `remanence_device_family`; in Python, `StorageDevice.slot`,
  `.device_type` and `.slot_prefix` replace `.family`.

- **Insert is device-type equality, naming both sides.** A medium
  carries the device its content was recorded by and a slot is typed by
  the device that fills it, so **a 1541 refuses an H-37 disk it could
  physically hold but never serve** — a check the article alone cannot
  make, both being the same soft-sectored 5.25-inch disk. That is the
  rule the recording being a fact of its own (D19) exists to support;
  the refusal names what recorded the medium and what the slot takes.

- **The partition pool populates under the device type's spec.** F56
  landed the pool under the medium's kind because the device spec it was
  owed did not exist yet (D32); it exists now. A medium recorded by a
  hard-drive type has that spec's declared scheme checked against its
  content, and the **schemeless types — the floppy class, and the
  archive whose vantage is a namespace — bear the direct partition with
  no step at all**, the table never read because no spec declared one.
  A sector 0 that looks like a table on a floppy is content nothing
  claims, and says so, rather than being read as a layout nobody
  declared. Where a declared scheme does not check out the answer stays
  the direct partition rather than a refusal, which is D32's ruling
  kept: an unpartitioned hard-drive recording is an ordinary disk this
  release reads, and refusing it would refuse every bare FAT image.

- **A discovery over a format that records several device types asserts
  none, and the load takes the caller's declaration instead.**
  `Discovery::device_type()` answers where the recognizing format records
  exactly one — an H8D says Heathkit H-17 — and `None` where it records
  several, because nothing in a qcow2 says which hard drive wrote it and
  `None` means *recorded by no device* rather than *unknown*.
  `Discovery::device_types()` is the list a declaration may name, and
  `Discovery::accepting_devices()` is the other question — every device
  served the article — replacing `accepting_families()`.

  **`Session::load_discovery_as(discovery, device)` is the declared
  door**, and `load_discovery` is the plain one: the same pair the
  vantage doors already make, opening where the evidence determines the
  answer and taking the caller's reading where it does not. The plain
  door refuses a device-typeless discovery by name and points at the
  `_as` form; the `_as` form checks the declaration against what the
  recognizing format records, so a raw member of an archive may be
  declared a hard-drive recording and never a 1541's. That keeps the
  nested journey whole — an archived artifact no adapter identifies is
  still reachable, under one claim held from the question to the load —
  while no medium is ever pooled that could be neither seated nor laid
  out (P3). `add_device_for` has nothing to declare with and refuses,
  naming the types. In C:
  `remanence_session_load_discovery_as`, consuming the discovery exactly
  as the plain door does. In Python: `Session.load_discovery_as`.

- **In-force P19 is amended: the walk is uniform.** "When every
  applicable seam has one supported result, composition is transparent:
  a simple legacy floppy image resolves to its filesystem without asking
  the caller to select the intervening layers." becomes *every medium
  bears a partition its content is reached through, so one path serves
  whatever a medium turns out to be: a medium recording no partition
  scheme bears the library's own composition of the whole content,
  declared as such, and a caller who knows nothing about partitions
  still takes the step every other caller takes.*

  Transparency and uniformity answer the same complaint and a surface
  can hold only one of them. Transparency paid in shapes: the number of
  steps varied with what the medium turned out to be, which is exactly
  the fact a caller opening an unknown artifact does not yet have.
  Uniformity pays one step and charges it to everyone equally — the
  floppy image's caller names ordinal 0 and, where no type determines a
  namespace, declares one, which is a line of code for the account of
  what is being read through. A medium recording no scheme costs its
  caller nothing the partitioned medium's caller does not also pay, and
  nothing is guessed to keep a path short.

- **`DiskReport` demotes to a view derived from the partition pool.**
  Every fact it reports is unchanged — the regions in the scheme's own
  order with their type values, readings, placements, roles, extents and
  refusals; the volumes with the identities they issue and the regions
  they stand on; the filesystems recognized on them; and the content
  answer for a medium that records no schema — but it now reads those
  facts off the pool the load established rather than parsing a table of
  its own. One medium cannot carry two accounts that disagree when it
  carries one account.

  **The direct partition never appears as a region.** A composition act
  is provenance and never evidence, so a medium recording no scheme
  still reports zero regions, exactly as before, and the evidence answer
  is untouched: `partition_scheme` is still `None` there (U4). A report
  listing the library's own composition among the regions would be the
  library quoting itself back as something the disk said. In C every
  `remanence_report_*` accessor answers as it did, and in Python so does
  every attribute of `DiskReport`.

- **The CBM DOS door is rehomed onto the partition.**
  `C1541Sectors::filesystem()` becomes `C1541Sectors::partition()`, and
  the namespace is declared through it —
  `sectors.partition().filesystem_as("cbmdos")`. The door moves and
  nothing beneath it is rewritten: the same adapter reads the same BAM
  header, the same directory chain and the same entries, and carries the
  same label and the same evidence out through `StorageSpace`. What
  changes is where the door hangs. A recording records no partition
  scheme, so it bears the direct partition like every other medium, and
  that partition is the one composition a layer no medium composed can
  bear — extent-less, because the recording addresses its own blocks and
  nothing composed a position for them to sit within. The sector layer
  still carries no file verbs of its own: it may be asked what it
  composes, and may not be told to act as a namespace it is not (P19).

  In C: `remanence_c1541_sectors_filesystem` becomes
  `remanence_c1541_sectors_partition`, whose partition takes
  `remanence_partition_filesystem_as` with `"cbmdos"`. In Python:
  `C1541Sectors.filesystem()` becomes `C1541Sectors.partition()`,
  answering a `Partition` whose `filesystem_as("cbmdos")` answers the
  `StorageSpace`.

- **`StorageSpace::volume_id()` answers `None` where the report composed
  no volume.** It is still the identity the inspection report issued,
  opaque and stable across opens of an unchanged layout (P21, U4), and
  it is now an honest absence where there was none to issue — a
  recording's record layer, an archive's content and a blank disk's
  direct partition among them — rather than an identity manufactured so
  the field could be filled. **The file verbs key on the partition's own
  extent** rather than on a volume identity: a space reads and writes
  through what composed it, not through a name looked up afterwards,
  which is why an addressable space may still carry no volume identity
  at all. In C, `remanence_volume_id` answers 0 there, with
  `remanence_volume_is_addressable` as the separate vantage question; in
  Python, `StorageSpace.volume_id` is `None` there.

### Changed

- **Lookups answer with absence, and the lifecycle is create, lookup,
  release.** Every in-memory lookup in the storage model — `machine`,
  `device`, `medium`, and their `_mut` forms — answers with an `Option`:
  a question about what a session holds has an honest negative answer,
  and nothing is manufactured to report it. **The `require_*` forms are
  gone** — `Session::require_machine`, `Session::require_device`,
  `MachineView::require_device`, `MachineView::into_required_device` and
  `DeviceView::require_medium` — because a demand belongs where the
  caller knows what an absence means; the code that wanted one now
  writes it. Creation refusals are untouched: a duplicate machine
  identity, a slot already taken and the empty identity are still
  refused by name, being the world saying no rather than a lookup
  finding nothing.

  **The removal verbs unify as `release_*`.** `MachineView::remove_device`
  and `Session::remove_device` become `release_device`, joining
  `release_machine` and `release_media` — so the three pools read the
  same way and each says what it takes: `release_machine` cascades
  (every device ejected, severing, so each medium stays pooled with its
  claim and its buffered changes, then the devices, then the machine),
  `release_device` ejects first and frees the slot, and `release_media`
  severs its own link and then ends the claim. A release names an
  identity that resolves to nothing, unlike a lookup, which answers it.

  In C: `remanence_session_remove_device` and
  `remanence_machine_remove_device` become
  `remanence_session_release_device` and
  `remanence_machine_release_device`, and the lookups —
  `remanence_session_machine`, `remanence_session_device`,
  `remanence_machine_device`, `remanence_session_medium`,
  `remanence_device_medium` — return null for an absence without
  touching the error outs, which is why they take none. In Python:
  `Session.machine`, `Session.device` and `Machine.device` return `None`
  where nothing answers, and `Session.remove_device` /
  `Machine.remove_device` become `release_device`. An attachment
  identity that names no claimed slot at all still raises there: that is
  a refusal, not an empty slot.

- **The medium becomes the content handle, and the session grows a media
  pool.** The structural heart of the media-first storage model. A
  `Session` now owns two pools — machines, which are configuration, and
  media, which are state — and every content verb the device carried
  moves onto `Medium`: `identify`, `inspect`, `read_at`, `mode`,
  `assurance`, `format`, `size`, `is_modified`, `filesystem`, `volume`,
  `commit`, `rollback`, and the file plumbing beneath them. A medium
  answers whether or not a drive is configured for it, which is what lets
  a disk mastered out of an archive outlive the archive it came from.

  **`Session::load_media(source, format)` is the declared reading.** The
  source is the caller's own opened `std::fs::File`; the format is one
  concrete entry of the new `Format` set — `raw`, `qcow2`, `vdi`, `h8d`,
  `zip`, `7z` — checked by that format's own adapter and refused by name
  where the evidence cannot bear it. A classification could check
  nothing, so none is admitted (P3). `Session::load_discovery` pools a
  discovery the same way, and `Session::medium`/`media`/`release_media`
  are the pool's own verbs, `release_media` being **the one
  state-destroying verb** in the model.

  **`StorageDevice` slims to what a slot is.** It carries its attachment
  identity, its family and a link, and the new `DeviceView` carries
  `insert(media_id)`, `eject()` and `medium()` — the one edge between a
  machine's configuration and the session's state. Insert checks the
  device's family against the medium and refuses naming both sides (P14);
  **eject severs only**, so the claim, the assurance and every buffered
  change survive in the pool. `release_machine` tears a machine's
  configuration down and takes no state with it. `MachineView` is the
  borrow that holds a machine and the pool at once, and
  `Machine::compose_dos_letters` moves onto it.

  In C: `remanence_session_load_media` (taking an OS file handle the
  library adopts), `remanence_session_load_discovery`,
  `remanence_session_medium`, `remanence_session_media_count`/`_id`,
  `remanence_session_release_media`, `remanence_session_release_machine`,
  `remanence_device_insert`, `remanence_device_medium`,
  `remanence_device_media_id`, `remanence_format_count`/`_id`/`_name`,
  and the whole `remanence_medium_*` surface replacing the
  `remanence_device_*` content verbs. `remanence_device_load_media` and
  `remanence_device_load_discovery` are gone. In Python:
  `Session.load_media(source, format)` taking an open file or descriptor,
  `Session.load_discovery`, `Session.media`, `Session.medium`,
  `Session.release_media`, `Session.release_machine`, the new `Medium`
  class carrying the content verbs, `StorageDevice.insert`/`.eject`/
  `.medium`/`.media_id`, and a `formats()` function.

- **In-force P7 is amended: whoever opens owns the lock.** "Denying write
  permission to every other process is mandatory in all scenarios"
  becomes *mandatory where the library opens; caller-owned where the
  caller opened.* A local artifact now arrives as the caller's own opened
  file, and that open is the claim: the library checks it for exactly one
  thing — may it write through it? — honours the answer exactly, and adds
  no lock of its own. A handle affording no write makes a read-only
  medium whose write verbs refuse naming whose open it was.

  **A name recovered from a handle serves location only**, under an
  identity check that it still denotes the handle's own file: where the
  commit journal lands beside the artifact (P9), and where a qcow2
  backing file or a VDI parent is looked for next door (U6, D18). A
  handle this host cannot name refuses exactly those two journeys by name
  and serves everything else, so `Medium::path` and `image_path` answer
  `Option`, as do `Discovery::path`/`image_path` and the archive layer's
  path in an identification.

  The claim's class travels on the medium's assurance as the new `Claim`
  value (`library-opened`, `caller-opened`); `discover_media` and every
  file of a composed chain keep the library-opened form unchanged.

### Removed

- **The selected-observation reduction retires: one family, one reduction
  discipline.** Mastering a capture by *choosing* an observation of each
  location and reconciling the rest is gone, replaced by the gap-first
  reconstruction that reduces on the strength of all the evidence. A
  reduction that asked the caller which revolution to believe was
  answering a question the evidence can answer better, and keeping two
  reductions of one capture meant two accounts of the same disk that
  could disagree.

  Gone from Rust: `MasteringPolicy`, `MasteringPlan`, `MasteringPlanReport`,
  `MasteredMedium`, `MasteredLocation`, `ObservationPolicy`,
  `DuplicatePolicy`, `ProjectionPolicy`, `PulseStrengthPolicy`,
  `OriginPolicy`, and `CaptureSet::plan_c1541_mastering`. Gone from C:
  the whole `remanence_mastering_*` and `remanence_mastered_medium_*`
  surface with its policy structs and enums. Gone from Python: the five
  classes and the capture-set verb. Nothing is bridged or aliased; the
  journey those verbs served is `CaptureSet::plan_reconstruction` →
  `execute` → the image's own `describe_p64` / `write_p64`.

  **The presentation ladder keeps both of its entry points.**
  `RemanenceImage::materialize_c1541_bitstream` replaces the mastered
  medium's: an image carries no clock, so the ladder stands on the
  served projection of it — one multiply per point, at the family's
  reference frame — rather than on the image directly. In C,
  `remanence_image_materialize_c1541_bitstream`; in Python, the method of
  the same name on `RemanenceImage`. The P64 container's entry is
  unchanged.

  **The reduction's account now reaches the medium it projects.** The
  served projection carries the image's own provenance ahead of its two
  notes, so a P64's declared-loss account states the whole reduction it
  cannot express rather than only the projection's part of it.

  Note for whoever cuts the first release: the entries below that added
  this surface are in this same unreleased section, so nothing that ever
  shipped is being taken away, and a reader of the first release meets
  neither the mastering verbs nor their removal. Collapsing the section
  to its net effect is release-time editing, and this entry is the
  record until then.

### Added

- **The CBM DOS filesystem, above the sector layer.**
  `C1541Sectors::partition()`, and `filesystem_as("cbmdos")` through it,
  answers the disk's own directory as the
  **same `StorageSpace` a disk image resolves to** — the file verbs live
  on the namespace and on nothing else, so this is a second door onto
  the one node rather than a second node with the same verbs. The sector
  layer carries no file verbs itself: it may be asked what it resolves
  to, and may not be told to act as a namespace it is not.

  What it reads is what CBM DOS records: the BAM header as the space's
  **label** (the disk name, its identity and its DOS type, each a
  reading with the recorded PETSCII beside it), the **directory in the
  order it was written** (U4), walked along its own chain from where the
  BAM says it begins, and each entry's facts in CBM DOS's own spelling —
  PRG/SEQ/USR/REL, the locked and never-closed bits, the block count,
  the first block, the directory slot it was read from, and a relative
  file's record length and side sector. A name is sixteen PETSCII bytes
  padded with `0xA0`: the reading covers the ranges CBM DOS displays,
  marks anything else unread, and carries all sixteen bytes as recorded
  beside it, so this disk's autoboot name — a control sequence that
  types `LOAD"EA",8,1` when the directory is listed — survives whole.

  **A size is established by walking, not by trusting.** Every block but
  the last carries 254 bytes and the last carries what its own link
  field says it filled, so the entry's size is what the chain holds and
  the recorded block count travels beside it. Where the chain reaches a
  block the recording never yielded, the entry says so — `size-basis`
  naming which of the two its size came from, `chain-refusal` carrying
  what stopped the walk — and reading that file refuses rather than
  handing back the part that was reachable. One unrecovered sector
  qualifies its own file instead of taking the listing down with it.

  The filesystem adapter knows nothing about what is beneath it (P18):
  it consumes blocks by address and never sees a flux transition, a bit
  cell, a byte of GCR or a sector header. `LOAD"$"` — the directory as
  the drive's ROM synthesizes it — is deliberately out of scope.

  **`StorageSpace` gains `label()` and `evidence()`**, answered for both
  vantages: a namespace recognized on a volume answers from the
  inspection report that recognized it, and one presented over a layer
  no device composed answers from its own adapter. A space presented
  that way carries the namespace vantage alone — `read_at` refuses as
  `not-addressable`, because nothing composed an extent for it to be a
  position within. In C:
  `remanence_c1541_sectors_partition`, whose partition answers the same
  `RemanenceSpace` every `remanence_filesystem_*` verb already takes,
  plus `remanence_filesystem_label`, its readings, and
  `remanence_filesystem_evidence`. In Python: `C1541Sectors.partition()`
  answering a `Partition` whose `filesystem_as("cbmdos")` answers a
  `StorageSpace`, with `label()` and `evidence()` on it.

  One consequence for Rust callers: a `StorageSpace` now holds its
  borrow of whatever it reads through until it is dropped, which is what
  its documentation always claimed. Code that held a space across the
  drop of its session or device now drops the space first.

- **The 1541 sector layer: the recording's own sectors, above the
  encoded bytestream.** `C1541Bytestream::recognize_c1541_sectors` takes
  a declared `SectorPolicy` and reads the records the recording states
  for itself — header blocks, data blocks, both checksums, and the
  `(track, sector)` addressing served from them. It is the rung the
  ladder was missing, and it is **where the two layers below stop saying
  nothing about what their bytes mean**: they still assign no byte to a
  header or a sector, and this layer states what it derives instead of
  either of them having quietly meant more.

  **Every rule is the drive profile's** (P30). The C1541 entry gains a
  declared record grammar: which byte opens each block, how long each
  is, where the header states the track, the sector and the two
  disk-identity bytes, which span each checksum covers and how it is
  computed, and where the payload sits. Nothing is derived from what is
  being read, and a framed byte that opens neither block opens nothing.
  Pairing is grammatical rather than metric — the family writes one sync
  ahead of each block, so a record's data block is the block the
  recording carries next after its header — and the circle is closed, so
  a header at the end of a location pairs with a data block at its
  start.

  **Every claim carries its evidence** (P4): where it sits, the address
  the header states, both stated checksums beside both computed ones,
  how many of its bytes the codec left unresolved, whether the family's
  own declaration covers the address at all, and — where it does not
  read — which rule stands in the way and why. `read_sector` answers
  only where the recording is unambiguous, and every other outcome is a
  refusal naming its rule from the layer's own `SectorRule` set (P10):
  an address no record states, an address no claim of which reads, or
  one that several readable claims disagree about. Nothing is repaired
  and no block is ever filled in — which is the difference between this
  surface and the d64 rendition, whose grid has to put *something* in
  every one of its 683 slots.

  The layer is derived rather than a seventh active layer, and it holds
  no recording whole: its payloads stream into private session storage
  as they are recognized and are read back a bounded section at a time
  (P27).

  In C: `remanence_c1541_bytestream_recognize_sectors` with
  `RemanenceSectorPolicy`, `remanence_c1541_sectors_read` into a
  caller's buffer, and the `remanence_c1541_sectors_*` accessors over
  the locations, the claims with their rules and refusals, the contested
  addresses, the declared-loss account and the evidence. In Python:
  `C1541Bytestream.recognize_c1541_sectors` with `SectorPolicy`,
  answering a `C1541Sectors` whose `read_sector` returns `bytes`, with
  `SectorReport`, `SectorLocation`, `SectorClaim` and
  `ContestedAddress`.

- **A KryoFlux capture reduces to a remanence image on the strength of
  all the evidence, not the choice of one revolution.**
  `CaptureSet::plan_reconstruction` takes a declared policy — which
  recorded side, and whether the positions holding recordings are
  measured from the evidence or declared by the caller — and computes
  the whole gap-first reduction without writing anything. Every
  revolution of every location is aligned by **gap correspondence**
  (identity lives in the interval sequence, position in the angles); the
  **cell lattice** is measured from the intervals themselves; each
  revolution's spindle wander is corrected by a fitted **timebase
  warp**; angles are produced **gap-first** — snapped to the lattice
  where the crystal wrote them, kept and reported where the medium holds
  them off-lattice consistently across revolutions, integrated so
  closure solves the cell exactly; coherence is decided per transition
  and incoherent runs become `Unaligned` spans; and adjacent steps
  carrying the same recording merge under measured agreement, **the fat
  track measured rather than asserted**.

  **The reduction answers with the image itself.** `execute` returns the
  family's ordinary `RemanenceImage` — the same root a `.remanence`
  artifact opens to, and the same one the d64, g64 and p64 renditions
  hang on — carrying the reduction's declared policy and evidence as its
  provenance. There is no second root beside it: the account of how the
  image came to be belongs to the *plan*, which computed it before
  anything was written, and executing adds nothing to that account.

  **The plan's report is the reduction stated whole**: the side, every
  step position the capture swept, the positions its selection names as
  recordings, and per orbit where the instrument read it, where it
  actually sits in whole microns, how many revolutions stood behind it,
  each revolution's raw transition count, the count-spread discriminator
  in permille, the points and coherent points and unaligned spans it
  produced, the cell its closed revolution implies, how many intervals
  it kept off the lattice, and whether the fat-track merge admitted it.
  Beside that the declared-loss account — the unselected side, positions
  whose evidence names no recording, marker channels, capture metadata,
  retained foreign records, and flux recorded outside any bounded
  revolution — and the survey's facts with their basis stated per fact:
  **evidenced**, **measured**, **assumed**.

  In C: `remanence_capture_set_plan_reconstruction` with
  `RemanenceReconstructionPolicy`, the `remanence_reconstruction_*`
  accessors over the positions, the orbits and the account, and
  `remanence_reconstruction_plan_execute`, which consumes the plan and
  answers with a `RemanenceImage *`. The example consumer gains
  `identify --reconstruct <capture> [side]`. In Python:
  `CaptureSet.plan_reconstruction`, with `ReconstructionPolicy`,
  `ReconstructionPlan`, `ReconstructionReport` and `ReconstructedOrbit`,
  and `plan.execute()` answering with a `RemanenceImage`.

  It is the family's one reduction: the selected-observation mastering it
  succeeds retires with it, above.

- **The C64 renditions are mastered off the remanence image: d64, g64 and
  p64.** A `RemanenceImage` renders to all three — P29 acting where only
  the destination varies — and each is claimed twice: `describe_d64`,
  `describe_g64` and `describe_p64` compute everything and write nothing,
  and `write_d64`, `write_g64` and `write_p64` compute the same thing and
  put it somewhere. **The account a description carries is the account
  the write carries**, so a caller reads what a destination will not hold
  before anything exists to hold it.

  **g64**: each orbit clocked by the phase-locked half-window at its
  measured cell — or at its zone's nominal where the measured figure is
  not a recording's, since clocking an unformatted band at its own "cell"
  would run several revolutions long — packed under the `GCR-1541`
  grammar with one speed zone per half-track. **d64**: the recording's
  own sectors read by the family's group code — headers, data blocks,
  checksums, blocks allowed to wrap the origin, nothing repaired and
  nothing rejected — laid into the CBM DOS 683-block grid, addressed by
  the header's own track and sector, first write wins, whole tracks
  before the half-tracks between them so a fat track's shoulder never
  outbids its centre. An incomplete disk carries the error map, which is
  the declared-loss account made flesh. **p64**: one multiply from angle
  to cycle — 2²⁸ divisions onto 3,200,000 cycles, rounded to nearest,
  collisions nudged and recorded as such — over the coherent points only,
  served through the already-delivered P64 encode path.

  **An orbit with no pulse is absent rather than empty.** The p64
  projection skips it: an absent half-track claims never-written, where
  an empty chunk would claim formatted-then-erased, and those are
  different claims about the same disk.

  **Every rendition states its loss in the image's own terms.** Blocks
  the recording never yielded and sectors that failed their own checksum;
  transitions the image declines to read, which a clocked bit has no
  spelling for; orbits the 96 tpi grid cannot place and orbits past the
  grammar's last slot; a measured cell replaced by its zone's nominal;
  the plateau and guard widths no C64 format has a field for; and the
  centre radius each format replaces with a slot number. A destination
  that can hold no track at all is a named refusal rather than an empty
  artifact, an existing destination is a named refusal rather than an
  overwrite, and an interruption leaves the destination absent rather
  than half an artifact.

  The GCR sector reading beneath the d64 is crate-private analysis
  machinery, deliberately: it serves the renditions and is **not** the
  user-facing sector surface, which remains the media-first shape's own
  rung.

  In C: `remanence_image_describe_d64` / `_write_d64` and their
  `remanence_d64_report_*` accessors over the blocks read, the missing
  blocks and the account; `remanence_image_describe_g64` / `_write_g64`
  with `remanence_g64_report_*` over the half-tracks and the account; and
  `remanence_image_describe_p64` / `_write_p64`, which answer the
  delivered `RemanenceP64Report`. The example consumer gains
  `identify --renditions <path> <stem>`, and `identify --remanence`
  describes all three without writing them. In Python:
  `RemanenceImage.describe_d64`/`write_d64` and their g64 and p64
  counterparts, with the `D64Report`, `D64Block`, `G64Report` and
  `G64HalfTrack` records beside them.

- **The flux family's physical stratum is a surface, and its artifact is
  claimed in both directions.** `RemanenceImage::open` reads a
  `.remanence` artifact — the library's own flux format — and answers
  with the physical facts of one disk: what the medium *holds*, stated
  as facts of the surfaces, distinct from any capture of them and
  beneath the served medium a drive reads. It is fit to nothing,
  addressed by no drive's stepping, and carries no clock — a cell length
  is a property of a *recording*, recoverable from the image, never a
  field of it.

  **The shape crosses the surface; the model beneath it does not.**
  `inspect` answers the form factor, the angular unit every angle is
  stated over (2²⁸ divisions of one turn — a unit rather than a
  measurement, so equality is exact), the index holes as exact rationals
  with nothing radial, the surfaces, and every **orbit**: one recorded
  band at one radius, located by its centre radius in whole microns
  rather than by the step index of whichever instrument found it, and
  counted — how many points it holds, how many carry a sense a reversal
  can be drawn from, and how many spans the image declines to read,
  which is indeterminacy recorded rather than repaired into a guess. The
  points themselves stay beneath the root, chunked into private session
  storage under the declared cache bound (P27), so a whole side's
  million transitions are never resident at once. "Orbit", not "track":
  both the flux community and the recording formats use "track" and mean
  different radii by it.

  **Writing is the other direction of the same claim.**
  `RemanenceImage::write` encodes the image into a new artifact — the
  magic, a binary sentinel, a layout version gated before anything is
  believed (P8), then one zlib-framed DEFLATE stream through an encoder
  of the library's own, the core staying dependency-free. The bytes are
  deterministic: the same image spells the same artifact, every time.
  Byte identity with another implementation's writer is deliberately not
  claimed — two correct DEFLATE encoders legitimately differ — and the
  reader accepts any valid stream, which is what keeps every writer's
  artifacts readable here. An existing destination is a named refusal
  rather than an overwrite, and the P29 account comes back **empty**
  because this is the model's own artifact: an empty account is the
  claim that nothing was left behind, not an account nobody assembled.

  A flux artifact is reached through its own type rather than through a
  device, as the capture set and the P64 image already are: block and
  flux are disjoint families (P13), so there is no device to load one
  into.

  In C: `remanence_image_open`, `remanence_image_open_with_cache`, the
  `remanence_image_*` accessors over holes, surfaces, orbits and
  provenance, and `remanence_image_write` with its own report; the
  example consumer gains `identify --remanence <path> [write-to]`. In
  Python: `RemanenceImage`, a context manager like every other handle,
  with `inspect()` and `write()` and the `RemanenceImageReport`,
  `RemanenceHole`, `RemanenceOrbit` and `RemanenceWriteReport` records
  beside it.

- **An archive is a medium, and it enters a machine the way every medium
  does.** A `.zip` or `.7z` loads into an **archive-family device**
  (`DeviceFamily::ARCHIVE_DEVICE`, slot `arc0`) and its content is the
  namespace that device resolves to — the same `StorageSpace` a disk's
  filesystem is reached through, listing, statting and reading entries
  with the file verbs that live on one node. The media-type catalog gains
  the **virtual** family for it (P14's amendment): an archive carries no
  form factor, no coercivity and no addressable unit, and the one fact
  its family declares is the native vantage, a namespace where every
  physical family's is a space.

  **The vantage decides what the medium answers.** `inspect`, `volume`,
  `size`, `format`, `commit` and the positioned reads address a space,
  and an archive refuses them **by name** — saying it has no partition,
  no volume and no sector, and where its content is reached instead —
  rather than inventing a phantom volume to satisfy them. What every
  medium has is the evidence plane: the artifact's own bytes still read,
  and `image_size_bytes` still answers.

  **Directories are the grammar's own hierarchy.** An entry named
  `disks/boot.h8d` puts a `disks` in the listing, which is reading what
  the archive recorded rather than manufacturing a pseudo-file (P19); a
  directory the grammar records no entry of its own for says so in a
  declared fact. Entry facts travel in the grammar's own spelling, as
  every other provider's do.

  Archives are read and not written, as they were: a write intent is
  refused by name at the load, because a write would have to be encoded
  back into the archive's own grammar and no adapter claims that.

- **The nested artifact is the same journey again.** `File::discover`
  opens an archive entry as an artifact of its own and answers with the
  consumable `Discovery` a device loads it from — the third load form
  F51 named and D24 deferred to whichever feature minted the view. The
  claim is the one the archive already holds, so nothing is re-opened and
  no window exists between naming the entry and loading it (P7), and the
  child is loaded into a device of its own — in a machine of its own
  where one is being reconstructed, the host's archive never having been
  part of the machine whose disk it holds.

  **The child holds its own backing.** A stored entry is source-backed
  through the archive's claim and a coded one is session-backed in
  private session storage, and either way ejecting the archive — or
  removing its device altogether — takes nothing away from a disk already
  loaded from it.

  In C: `remanence_filesystem_discover`; in Python: `File.discover()`.
  The claim is bounded and says so: a file on a volume-backed filesystem
  is refused by name, its bytes being read through the filesystem that
  names it.

- **The volume and the filesystem are one node, and it addresses its own
  extent.** `StorageSpace` replaces the two types F48 delivered, carrying
  **two vantage traits on one object**: *volume* is addressable I/O —
  reads and writes by position within the extent the space names — and
  *filesystem* is namespace I/O, the file verbs. An object implements
  what it has: a FAT volume both, a volume bearing no filesystem the
  addressable one alone, a medium's own namespace the namespace one
  alone. The 0..1 the model asserted in prose is now carried by the type,
  and no phantom volume is invented for a namespace with no space beneath
  it.

  **Addressing within a space is the new reach.** Until now the only
  addressed reads were whole-medium, so a volume's boot record, the
  extents its filesystem calls free, or the bytes behind a file just
  listed all meant computing offsets against the medium by hand.
  `read_at` and `write_at` take positions **within the space**, bounded
  by the space's own extent — a read past its end names `outside-extent`
  rather than wandering into whatever follows — and they read through the
  session cache, so a caller sees the state its own buffered writes
  produced. Writes are buffered until commit and land in the active layer
  like every other write (P2, P23).

  **Both finders answer with the same node.** `device.filesystem()`
  resolves and `device.volume(id)` selects; each hands back a
  `StorageSpace`, and the hop between a volume and its filesystem is
  gone. A volume bearing no namespace is no longer a failed selection: it
  is a space that answers `is_addressable` and not `has_namespace`, with
  the recognizing seam's own refusal — category and rule intact — kept
  for whichever namespace verb asks.

  The rule set the seam owns is `SpaceRule`, F48's `NamespaceRule`
  widened to cover both vantages: `several-candidates`, `no-namespace`,
  `recognized-not-read`, `namespace-not-writable`, and the new
  `not-addressable` and `outside-extent`.

  In C, one opaque `RemanenceSpace` with the two vantages keeping their
  prefixes — `remanence_volume_is_addressable`, `_id`, `_start_bytes`,
  `_length_bytes`, `_read_at`, `_write_at` beside
  `remanence_filesystem_has_namespace`, `_kind`, `_entries`, `_stat`,
  `_get_file`, `_read_file`, `_write_file`, `_resize_file`,
  `_make_directory` — freed by `remanence_space_free`. In Python, one
  `StorageSpace` class with `is_addressable`, `has_namespace`,
  `start_bytes`, `length_bytes`, `read_at` and `write_at` beside the file
  verbs. The example C consumer prints a volume's extent and first bytes
  beside its listing.

- **File access lives on one node.** `Filesystem` is the namespace, and
  it is the only type that carries file verbs: `entries`, `stat`,
  `get_file`, `read_file`, `write_file`, `resize_file` and
  `make_directory` live there and nowhere else. A device holding a
  partitionable medium and bearing `get_file` would be a category error
  in the type rather than a refusal waiting to happen, so **the device
  exposes no file access at all** — it may be asked what it *resolves*
  to, and may not be told to act as something it isn't.

  **`device.filesystem()` is the resolver**, and it is P19's
  transparency clause as a method: it walks device → volume →
  filesystem where every seam has exactly one supported answer, and
  refuses naming the candidates where one does not. It creates nothing
  and never guesses. Where several volumes bear a filesystem, selection
  runs by the identity the inspection report issued —
  `device.volume(id).filesystem()` — and never by a position. A volume
  bearing no filesystem is a **named absence**, not an empty listing,
  and where recognition was attempted and refused the answer is that
  seam's own refusal, category and rule intact, rather than a coarser
  one. The resolver's refusals are categorized and rule-identified
  (P10) against a new `NamespaceRule` set: `several-candidates`,
  `no-namespace`, `recognized-not-read`, `namespace-not-writable`.

  **`get_file` answers with a `File`**, borrowed from the filesystem
  that names it, offering the bounded streamed form (`read_at`,
  `write_at`) beside the whole-value conveniences (`bytes`,
  `Filesystem::read_file`) — P27's two shapes on one view. It is where
  absence stops being an answer: `stat` asks whether something is
  there, `get_file` asks for the file, so nothing and a directory are
  both refused by name.

  **The HDOS catalog is reached the same way.** `list_hdos_files`'s
  selector-free signature stops being an inconsistency and becomes the
  resolver's transparent form: an H8D composes no volume, so
  `device.filesystem()` resolves to the namespace the medium bears
  itself, and `entries("")` lists it. The adapter that recognized a
  namespace is the one that opens it, so nothing in the resolver
  branches on a filesystem identifier; an adapter that recognizes what
  this release does not read — CP/M today — refuses by name. That
  lookup is bounded (P27): a medium composing no volume and larger than
  the bound the HDOS reader already declared is a named absence rather
  than a full scan.

  **One entry vocabulary, however the namespace was reached.** `Entry`
  carries the name as stored, the kind and the size, and whatever the
  recognizing filesystem declares beyond them travels as `EntryFact` in
  that filesystem's own spelling and order — HDOS's catalog date, flag
  letters and sector count are the delivered case. Nothing is
  normalized on the way through.

  `remanence_device_filesystem`, `remanence_device_volume`,
  `remanence_volume_*`, `remanence_filesystem_*`, `remanence_file_*` and
  `remanence_entry_*` are the C ABI's; `StorageDevice.filesystem()`,
  `StorageDevice.volume()`, `Volume`, `Filesystem`, `File`, `Entry` and
  `EntryFact` are Python's.

- **An artifact can be asked what it is before a machine is configured
  for it.** `discover_media(path)` is a first-class library function on
  no handle at all: it claims the artifact for the read, identifies it,
  and answers with the exact medium, the concrete device families served
  that medium, and the image format's **declared default device** —
  needing no session and no machine, because it consults catalogs and
  evidence rather than configuration, and mutating nothing.

  **What it answers with is a consumable handle, not a record.** A
  discovery holds the claim taken when the artifact was identified and
  the work that identification did, and `load_discovery` moves that
  state into a device rather than opening the artifact a second time —
  so nothing expensive runs twice and no window exists between the
  question and the load in which the file could change. The intent, the
  cache bound and the assurance a device then reports are the ones the
  discovery established. A load consumes the discovery either way: a
  refused load releases its claim with it rather than handing back a
  half-used handle, and asking again is always allowed.

  **Image formats now declare the device family whose disks they
  record.** It is a recording-side fact the media type cannot honestly
  hold — a ten-sector hard-sectored 5.25-inch disk is the article of more
  than one machine's drive, while an H8D records a Heathkit one — so it
  sits on the format: `h8d` declares the Heathkit H-17, `qcow2` and `vdi`
  declare the hard disk, and `raw` declares nothing, because a raw image
  says nothing about the machine it came from. Which families would
  *accept* a medium is the other question entirely, and is derived by
  asking the families themselves rather than kept as a second list.

  **One machine-level convenience sits over discovery**, and only one:
  `add_device_for(path)` adds a fresh device of the format-declared
  default family, loads the medium into it, and answers with that device
  — the same access path, composed. Where a format declares no default it
  refuses by name toward the two explicit acts, naming the drives the
  medium could go in, and leaves no device behind. A declaration nobody
  makes is a refusal, not a guess. There is no media-first spelling:
  with one storage handle it would return the same device.

  In C: `remanence_discover_media`, its `_with_cache` sibling, the
  `remanence_discovery_*` readers (including `_default_device`, null
  where a format declares none), `remanence_discovery_free`,
  `remanence_device_load_discovery` — which consumes and frees the
  discovery whatever it returns — and
  `remanence_machine_add_device_for` with its session spelling. In
  Python: `remanence.discover_media(path, writable=…)`, the `Discovery`
  object, `StorageDevice.load_discovery`, and `Machine.add_device_for` /
  `Session.add_device_for`. The example C consumer gains
  `identify --discover <path>`, and asks the artifact when it is told no
  device family rather than assuming a hard disk.

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
  names. It opens no artifact and composes no namespace over the
  result: the letter is what a consumer shows a user, and the volume
  identity is what it passes back into the namespace node. `remanence_dos_*`
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

- **The `archive[/entry]` path syntax and the standalone `Archive`
  listing are gone.** A path names a file; an entry is named through the
  namespace its archive bears. `Archive` and `ArchiveEntry` leave all
  three surfaces — with them the 12 `remanence_archive_*` C symbols and
  the Python `Archive`/`ArchiveEntry` classes — and the archive catalog
  seam beneath them is untouched, becoming the grammar's P12 adapter at
  the namespace seam. Two consequences are deliberate: loading a
  one-entry archive no longer silently opens the entry, and the
  ambiguity refusal for a many-membered archive goes with the guess that
  needed it. The KryoFlux capture-set adapter keeps
  `captures.7z/subtree`, which names a subtree of members read as one
  logical artifact rather than one medium inside an archive.

- **Two format readers became fallible, because a medium may present no
  disk.** `Discovery::format` and `Discovery::size` answer `Result`, and
  in C `remanence_device_format` and `remanence_discovery_format` take an
  out-parameter and return false where there is no disk image to report.
  `remanence_discovery_size` answers zero there. The recognized format's
  stable spelling — `image_format` — answers for both kinds, an archive
  grammar included.

- **P19 loses its serialized-artifact provider form**, which is what the
  P19 amendment says and D25 deferred to this feature: a medium may bear
  its namespace directly, its grammar being a P12 adapter at that seam.
  The namespace-mapping composer's three constraints stay in P19 until
  P35 has a machine namespace to take them.

- **"Container" is retired from this project's own vocabulary.** It is
  standard for five different things — an archive, an image container
  format, a multimedia container, a Docker container, a LUKS container —
  and can never disambiguate, so nothing this project names uses it. An
  identification now reports the **layers of an artifact's nesting**:
  `Layer`, `LayerKind` and `LayerLayout` in Rust, `Identification.layers`
  in place of `.containers`, `remanence_layer_*` and
  `remanence_identification_layer_count` on the C ABI, and the `Layer`
  class in Python. `RegionRole::Container` becomes `RegionRole::Structure`
  (`"structure"` in its stable spelling), because an extended partition
  is a structural region. `Error::InvalidImage`'s `container` field
  becomes `format` — the seam the refusal is attributed to. In-force P19
  is retitled to "The namespace is the common file-access seam" and P23's
  active-layer row is now **namespace**; neither changes what it claims.
  The word survives untouched where it is somebody else's: an *image
  container format* is the industry's term for qcow2, VDI and P64, and a
  retirement reaches this project's own vocabulary rather than
  quotations of the world's.

- **The file verbs moved off the device onto the namespace node**, with
  the volume identity becoming the selector between namespaces rather
  than an argument to every verb. `StorageDevice::{entries, stat,
  read_file, read_file_at, resize_file, write_file, write_file_at,
  make_directory}` are gone, along with `remanence_device_*` and the
  Python `StorageDevice` methods of the same names; each has its
  counterpart on `Filesystem` or `File` above. `FatEntry`/`FatEntryKind`
  become `Entry`/`EntryKind`, `RemanenceFatEntryList` and
  `remanence_fat_entry_*` become `RemanenceEntryList` and
  `remanence_entry_*`, and Python's `FatEntry` becomes `Entry`.

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

- **The standalone HDOS reader.** `list_hdos_files`, `read_hdos_file`
  and `HdosFile` are gone from all three surfaces, along with
  `StorageDevice::{list_hdos_files, read_hdos_file}`,
  `remanence_list_hdos_files`, `remanence_read_hdos_file`,
  `remanence_device_list_hdos_files`, `remanence_device_read_hdos_file`,
  `RemanenceHdosFileList` with its `remanence_hdos_file_*` accessors, and
  Python's module-level functions and `HdosFile` class. An HDOS catalog
  is one namespace among others and is walked through `Filesystem` like
  any other: `device.filesystem()` resolves to it, `entries("")` lists
  it, `get_file(name)` reaches a file. What the catalog records past a
  name, a kind and a size — the date, the flag letters, the sector
  count — travels as declared entry facts in HDOS's own spelling rather
  than as a second file type.

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
