# remanence-lib

[![License](https://img.shields.io/badge/license-GPL--3.0--only-blue.svg)](LICENSE)

A self-contained disk image analysis library in Rust. A `Session` holds
machines, a machine holds family-typed storage devices, and a
`StorageDevice` is the one handle for a slot and the medium in it.
Devices are added and media are loaded, as two acts: a machine takes a
drive as concrete as the one it actually had — a Commodore 1541, a
Heathkit H-17, a hard disk — and a disk image, raw or an entry inside a
`.zip` or `.7z` archive, is loaded into it under a single claim. A
medium belonging in another drive is refused naming both sides, an empty
drive is configuration in its own right. `discover_media` answers what
an artifact is before any of that — the exact medium, the drives served
it, and the drive the image format declares for the disks it records —
and hands back a discovery holding that claim, which a load consumes so
nothing is opened twice; where a format declares a drive,
`add_device_for` composes both acts in one, and where it declares none a
raw image says nothing about its machine, so the caller states the
drive. The load identifies the image's container
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

A mastered medium — or the one a P64 holds at rest — can then be read
the way a drive reads it. The family's read channel clocks the medium's
pulses into a circular, track-relative **hardware bitstream**, and its
declared group code resolves that into the family's **encoded
bytestream**. Both rules are the drive profile's: the cell comes from
the density zone the family declares, the counter restarts at every
transition and admits one within half a cell of a boundary, and the
bytes come from the published sixteen-symbol GCR table. Every bit says
whether it was recorded or resolved by a declared rule; a location no
zone covers is refused rather than clocked at a neighbour's rate; and a
pattern the table does not assign keeps its own bits rather than
becoming the nearest value. Neither layer assigns anything above a byte:
no byte is a header, a data field, a sector or a file, and the framing
landmark the codec locates says where bytes begin and nothing about what
follows it.

A mastered medium can then be saved as a P64, and a P64 opened back.
The container's grammar and its own adaptive range coder are the
adapter's claim, stated in the module from the published format
description: the version is validated before anything else is touched,
every chunk is checked against its stored checksum and all of them
against the header's, and a version, reserved flag bit, or chunk
signature past the claim is refused by name. The adapter says what the
container will carry before it writes — a P64 records no policy, no
provenance and no located origin, and each of those is named and
counted first — and refuses a medium its claim cannot encode rather
than approximating one into it. An existing destination is a named
refusal, never an overwrite, and the artifact is built beside its
destination and moved into place whole, so an interruption leaves the
destination absent rather than half a file.

The library is dependency-free at runtime, including its own ZIP
central-directory reader, 7z header reader, RFC 1951 (DEFLATE) and
LZMA/LZMA2 decompressors, and native qcow2 v2/v3 and VDI drivers.

Beyond identification, loading a raw, qcow2 or VDI disk image into a
storage device takes a claim under a declared intent: a read session
denies writes to every other process while admitting other readers; a
writable session admits no observers at all; and an image whose claim
cannot be secured — one held by a running VM, say — is refused outright
at the load. It
inspects the disk as a layered report — the block-active device, what
its leading structure turned out to be, any recognized partition
schema, every region that schema declares, every volume composed, and
every filesystem recognized on one, each fact at the seam that owns it.
Every declared region carries both its raw type value and a reading of
what that value declares, so a type the release does not read still
explains itself. It gives each reported volume an opaque stable
identity, and uses that identity to work with files — list, stat, read, write
(overwriting in place), and create directories with their missing
parents — under a commit point: nothing touches the image until
`commit`, and `rollback` discards everything. A qcow2 whose content
lives partly in a backing chain — raw or qcow2 members, relative
paths resolved from the image that names them — opens as one composed
disk, every backing member claimed immutable for the session's life.
Writes allocate copy-on-write into the top image only and preserve the
backing relationship; a missing member, a cycle, or a chain past the
claimed depth is refused by name.
A VDI is an ordinary image of the same stack: its version is validated
before anything else is touched, its dynamically allocated, fixed and
differencing image types are claimed by name — the one other type the
format defines, undo, refused rather than attempted — and a block the
block map marks discarded reads as the zeroes the format says it holds,
never confused with an allocated block that happens to hold them. A write
into a block a dynamic image never allocated allocates one, inside the
commit and never during a read.
A VDI differencing image opens as one composed disk too, and the way it
finds its parent is the format's own: a VDI records the parent's
**identity** and no path at all, so the image declaring that identity is
searched for beside the child and in the directory above it, and that
identity is what checks the file the search found. A file standing where
the parent should be whose identity does not match is refused by name
rather than read as a substitute — evidence a backing path alone cannot
give. A missing parent, a cycle, a chain past the claimed depth, or a
parent whose own version or type falls outside the claim is refused at
the open.
Where the stopped machine ran DOS, it also answers which drive letter
named which volume. A DOS machine persisted no such map — its letters
were assigned at boot by a rule over the machine's own configuration, and
nothing on the disks records the result — so the mapping is derived: the
machine facts are the caller's, either asserted (which medium is in which
floppy slot, which disks are attached in what order, where a CD-ROM
driver was declared) or read from a machine's own device set in the order
its devices were added, with families no claimed rule letters passed over
by family; the library applies one named assignment rule over the reports
already inspected, and the answer says which volume each letter names.
The rule is a claim like any other: two MS-DOS variants are claimed by
name, stating the variant settles the map, and stating none leaves a
letter the variants disagree on undetermined with each rule's answer
rather than averaged into one that is neither. `LASTDRIVE`, `SUBST`,
`JOIN`, `ASSIGN`, a resident block-device driver and a network redirector
are outside every claimed rule, and a letter one of them could have
changed is reported undetermined rather than approximated.

An image that is short of what it declares is neither accepted whole nor
thrown away whole. Every open states what it established — verified, or
degraded with the condition that narrowed it — before anything is read,
so a caller meets a deficiency by being told rather than by an operation
failing halfway. A raw image whose FAT boot record declares more bytes
than the file holds opens degraded and read-only for the session's whole
life: the declaration, what the source actually holds, the first byte
that is missing and the exact extent that reads all come back as
evidence. Directories and files that are wholly present read normally; a
file whose cluster chain runs into the missing tail is refused by name
with its range, never clipped, zero-filled or served in part; and every
write, every commit and every other mutation is denied with the same
stable condition, in Rust, C and Python alike. Where the shortfall leaves
no safe bound to state — a boot record declaring two different sizes —
the medium is refused outright rather than read in part.

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
// A session holds machines; a machine holds devices; a device is the
// one handle for its slot and whatever medium occupies it. These verbs
// are the session's anonymous machine, the one whose identity is null.
// Devices are added and media are loaded, as two acts.
let mut session = remanence::Session::new();
let device = session.add_device(remanence::DeviceFamily::HEATHKIT_H17)?;
println!("{}", device.attachment());      // heathfloppy0
device.load_media("disk.h8d", remanence::AccessIntent::Read)?;
let identification = device.identify()?;
for container in &identification.containers {
    println!("{:?} {} ({}%)", container.kind, container.id, container.confidence);
}
let files = device.list_hdos_files()?;

// What the open established about the evidence beneath it, before
// anything is read from it.
let assurance = device.assurance()?;
println!("{} {:?}", assurance.outcome, assurance.condition);
for line in &assurance.evidence {
    println!("  {line}");
}

let archive = remanence::Archive::open("captures.7z")?;
for entry in archive.entries() {
    println!("{} ({} bytes)", entry.name, entry.uncompressed_size);
}
let hdd0 = session.add_device(remanence::DeviceFamily::HARD_DISK)?;
hdd0.load_media("captures.7z/track00.raw", remanence::AccessIntent::Read)?;

// Asking what an artifact is, before a machine has been configured for
// it. The discovery holds the claim under which that was established;
// a load consumes it, so nothing is opened twice.
let discovery = remanence::discover_media("disk.h8d", remanence::AccessIntent::Read)?;
println!("{} in {:?}", discovery.media_type(), discovery.accepting_families());
match discovery.default_device() {
    Some(family) => println!("the format records a {}", family),
    None => println!("the format declares no drive"),
}
let drive = session.add_device(remanence::DeviceFamily::HEATHKIT_H17)?;
drive.load_discovery(discovery)?;

// Or both acts at once, where the format declares the drive it
// records — refused by name where it declares none.
let drive = session.add_device_for("disk.h8d", remanence::AccessIntent::Read)?;

// The drive letters a DOS machine would have presented: the machine
// facts are the caller's — here its own device set, in attachment
// order — and the assignment rule is the library's.
let letters = session
    .anonymous_mut()
    .compose_dos_letters(Some(remanence::DosAssignmentRule::MsDos5), &[])?;
for mapping in &letters.mappings {
    println!("{}: {:?}", mapping.letter, mapping.outcome);
}

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
for family in remanence.device_families():
    print(family.id, family.name, family.is_concrete, family.accepted_media)

session = remanence.Session()
device = session.add_device("heathkit-h17")
print(device.attachment)            # heathfloppy0
device.load_media("HDOS_1-0.zip/HDOS_1-0_Issue_#50-00-00_890-1.h8d", writable=False)
print(device.assurance.outcome, device.assurance.condition, device.mode)
for c in device.identify().containers:
    print(c.kind, c.id, c.confidence)
for f in device.list_hdos_files():
    print(f.display_name, f.size_sectors, f.modified_date_string)
device.eject()                      # the drive stays; the disk goes

# What an artifact is, before a machine has been configured for it.
discovery = remanence.discover_media("disk.h8d", writable=False)
print(discovery.media_type, discovery.device_families, discovery.default_device)
drive = session.add_device("heathkit-h17")
drive.load_discovery(discovery)     # consumed: one claim, one open

# Or both acts at once, where the format declares the drive it records.
drive = session.add_device_for("disk.h8d", writable=False)

# The letters, from the machine's own device set — or from asserted
# facts, where the caller holds them instead.
drives = session.machine().compose_dos_letters()  # no variant stated:
for mapping in drives.mappings:                   # disagreement is reported
    print(mapping.letter, mapping.outcome, mapping.volume, mapping.reason)

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

    # What a 1541's read channel and GCR codec make of that medium.
    bits = medium.materialize_c1541_bitstream(remanence.ReadChannelPolicy(
        density="declared", unzoned="refuse", weak_pulse="seeded",
        seed=0x0123456789abcdef))
    bytes_ = bits.materialize_c1541_bytestream(remanence.GcrCodecPolicy(
        alignment="landmark", unassigned_symbol="declare-loss"))
    for track in bytes_.inspect().locations:
        print(track.half_track_numerator, track.bytes, track.resolved_bytes,
              track.alignments, track.unframed_bits)

    for loss in medium.describe_p64().declared_loss:
        print(loss.code, loss.count, loss.detail)
    medium.write_p64("pinball.p64")

with remanence.P64Image("pinball.p64") as image:
    for track in image.inspect().half_tracks:
        print(track.index, track.half_track_numerator, track.pulses,
              track.strong_pulses, track.weak_pulses)
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
