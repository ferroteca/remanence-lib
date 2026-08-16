# remanence

Read and analyse disk images from Python — floppy and hard disk images from
vintage and modern systems alike.

Give it an image file and it will tell you what the file actually is, what
disk geometry it records, which drive it came from, and what filesystem is
on it — then let you list and read the files inside. It handles raw images,
QCOW2, VDI, Heathkit H8D, ZIP and 7z archives, KryoFlux flux captures and
P64, and it reads HDOS, CBM DOS and FAT filesystems.

This is the Python interface to
[remanence-lib](https://github.com/ferroteca/remanence-lib), which is
written in Rust. The wheel contains everything it needs: no other packages
to install, no external tools to call, not even for decompression.

> **This is an alpha release.** The API will change without notice, and
> installing needs `--pre`. The package is currently **tested on Windows
> only** — the code for Linux and macOS is there, but it is untested and
> unsupported for now.

## Install

```bash
pip install --pre remanence
```

Needs Python 3.10 or newer. One wheel covers every supported version, and
type hints are included, so editors and type checkers work out of the box.

## Getting started

You open the file; the library never opens it behind your back. You also
tell it what format the file is, and it checks that claim against the
file's actual contents — if you say H8D and hand it a ZIP, it says so
rather than guessing.

```python
import remanence

print(remanence.formats())          # the formats you can name

session = remanence.Session()
with open("disk.h8d", "rb") as source:
    medium = session.load_media(source, "h8d")

# What the file turned out to be, layer by layer, with a confidence
# score and a plain-language reason for each.
for layer in medium.identify().layers:
    print(layer.kind, layer.id, layer.confidence)
```

Because you opened the file, you control whether it can be written to. The
library asks your file handle that one question, respects the answer, and
takes no lock of its own. Closing your Python file afterwards is safe — the
library keeps its own copy of the descriptor.

### Reading the disk

Disk geometry comes back with a note of where each number came from, and if
two sources disagree, you are told rather than being handed one answer:

```python
geometry = medium.geometry
print(geometry.cylinders, geometry.heads,
      geometry.sectors_per_track, geometry.sector_bytes)
for reading in geometry.readings:
    print(" ", reading.source, reading.at, reading.detail)

first = medium.read_sector(0, 0, 1)     # sectors are numbered from one
```

### Reading the files

Files live behind a partition. An image with no partition table has a
single partition covering the whole disk, at index 0. You name the
filesystem you expect and the library verifies it:

```python
filesystem = medium.partition(0).filesystem_as("hdos")
for entry in filesystem.entries():
    print(entry.name, entry.size_bytes)
    # Anything else this filesystem records about the file — an HDOS
    # catalog date, its flag letters — in its own terms.
    for fact in entry.declared:
        print("   ", fact.key, fact.value)

data = filesystem.get_file("HDOS.SYS").bytes()
```

### Drives and machines

You can model the machine a disk belonged to, with drives as specific as
the real ones — a Commodore 1541, a Heathkit H-17, a hard disk. Putting a
disk in a drive and taking it out again changes nothing about the disk:

```python
device = session.add_device("h17")
print(device.attachment)            # heathfloppy0
device.insert(medium.id)
device.eject()                      # the drive stays, and so does the disk

session.release_media(medium.id)    # this is what actually discards it
```

### Identifying a file before you commit to it

`discover_media` answers "what is this?" on its own, and hands back a
result you can pass straight to a load, so the file is only opened once:

```python
discovery = remanence.discover_media("disk.h8d", writable=False)
print(discovery.article, discovery.accepting_devices, discovery.device_type)
found = session.load_discovery(discovery)

# Or do both at once, when the format says which drive recorded it.
drive = session.add_device_for("disk.h8d", writable=False)
```

## Beyond the basics

There is more than fits here: flux captures decoded down to bit and byte
level, KryoFlux capture sets loaded as a group, CBM DOS directories, and
creating blank disks from scratch. The
[full documentation](https://github.com/ferroteca/remanence-lib/blob/main/README.md)
covers all of it with examples.

## Links

- [Repository and full documentation](https://github.com/ferroteca/remanence-lib)
- [What it is good for](https://github.com/ferroteca/remanence-lib/blob/main/USE-CASES.md)
- [Changelog](https://github.com/ferroteca/remanence-lib/blob/main/CHANGELOG.md)
- [Contributing](https://github.com/ferroteca/remanence-lib/blob/main/CONTRIBUTING.md)

## License

GPL-3.0-only. See
[LICENSE](https://github.com/ferroteca/remanence-lib/blob/main/LICENSE).
