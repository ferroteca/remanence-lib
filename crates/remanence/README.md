# remanence

Read and analyse disk images — floppy and hard disk images from vintage and
modern systems alike.

Give it an image file and it will tell you what the file actually is, what
disk geometry it records, which drive it came from, and what filesystem is
on it — then let you list and read the files inside. It handles raw images,
QCOW2, VDI, Heathkit H8D, ZIP and 7z archives, KryoFlux flux captures and
P64, and it reads HDOS, CBM DOS and FAT filesystems.

**It has no dependencies at all.** The ZIP and 7z readers, the DEFLATE
decoder and the LZMA decoders are all part of the library, and it never
shells out to an external tool.

> **This is an alpha release.** Before 1.0 there is no compatibility
> promise: when a part of the API changes, it changes across the Rust, C
> and Python interfaces together and the old form is removed. Read the
> [changelog](https://github.com/ferroteca/remanence-lib/blob/main/CHANGELOG.md)
> before upgrading.

## Getting started

You open the file; the library never opens it behind your back. You also
tell it what format the file is, and it checks that claim against the
file's actual contents — if you say H8D and hand it a ZIP, it says so
rather than guessing.

```rust
// Some formats record exactly one kind of drive, so naming the format is
// enough. Where a format could be any of several, you say which:
// `Format::Qcow2 { device: HardDrive::MbrBlock }`.
let mut session = remanence::Session::new();
let medium = session.load_media(
    std::fs::File::open("disk.h8d")?,
    remanence::Format::H8d,
)?;

// What the file turned out to be, layer by layer — the archive wrapper,
// the image format, the disk geometry, the likely filesystem — each with
// a confidence score and a plain-language reason.
let identification = medium.identify();
for layer in &identification.layers {
    println!("{:?} {} ({}%)", layer.kind, layer.id, layer.confidence);
}
let disk = medium.id();
```

Because you opened the file, you control whether it can be written to. The
library asks your handle that one question, respects the answer, and takes
no lock of its own.

When two candidate formats match equally well, the result is "unknown"
rather than whichever one happened to be checked first.

### Drives

You can model the drive a disk belonged in, as specific as
the real ones. Putting a disk in a drive and taking it out again changes
nothing about the disk:

```rust
let mut device = session.add_device(remanence::FloppyDrive::HeathH17)?;
println!("{}", device.attachment());      // heathfloppy0
device.insert(disk)?;
```

### Reading the files

Files live behind a partition. An image with no partition table has a
single partition covering the whole disk, at index 0. You name the
filesystem you expect and the library verifies it:

```rust
let medium = session.medium_mut(disk).expect("pooled");
let mut filesystem = medium
    .partition(0)
    .expect("the whole disk")
    .filesystem_as("hdos")?;
for entry in filesystem.entries("")? {
    println!("{} ({} bytes)", entry.name, entry.size_bytes);
    // Anything else this filesystem records about the file — an HDOS
    // catalog date, its flag letters — in its own terms.
    for fact in &entry.declared {
        println!("    {} = {}", fact.key, fact.value);
    }
}
let bytes = filesystem.get_file("HDOS.SYS")?.bytes()?;
```

### What the library knows before you read anything

Opening an image produces a summary of the evidence behind it — whether it
held up, what was checked, and whose file handle the access rests on:

```rust
let assurance = session.medium(disk).expect("pooled").assurance();
println!("{} {:?} {}", assurance.outcome, assurance.condition, assurance.claim);
for line in &assurance.evidence {
    println!("  {line}");
}
```

### Archives

An archive is treated as a disk in its own right: its contents are its
directory. A file inside it that turns out to be a disk image becomes a
disk of its own, and it outlives the archive it came from.

## Beyond the basics

There is more than fits here: flux captures decoded down to bit and byte
level, KryoFlux capture sets loaded as a group, CBM DOS directories, and
creating blank disks from scratch. The
[full documentation](https://github.com/ferroteca/remanence-lib/blob/main/README.md)
covers all of it with examples.

## Other languages

The same library is available to C and C++ through the
[`remanence-ffi`](https://crates.io/crates/remanence-ffi) crate, and to
Python as the [`remanence`](https://pypi.org/project/remanence/) package.

## Features

- `fixtures` — for testing only, and it switches on test files rather than
  any library code. Those tests need disk images that a preparation script
  downloads or builds, which are not in the repository. It is off by
  default, so a fresh clone can be tested straight away.

## Links

- [Repository and full documentation](https://github.com/ferroteca/remanence-lib)
- [What it is good for](https://github.com/ferroteca/remanence-lib/blob/main/USE-CASES.md)
- [Architecture](https://github.com/ferroteca/remanence-lib/blob/main/ARCHITECTURE.md)
- [Changelog](https://github.com/ferroteca/remanence-lib/blob/main/CHANGELOG.md)
- [Contributing](https://github.com/ferroteca/remanence-lib/blob/main/CONTRIBUTING.md)

## License

GPL-3.0-only. See
[LICENSE](https://github.com/ferroteca/remanence-lib/blob/main/LICENSE).

This is copyleft software. You may run, study, modify and redistribute it
freely, but anything you distribute that includes it must also be
GPL-3.0-only. It cannot be used in a proprietary product.
