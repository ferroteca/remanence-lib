# remanence-ffi

The C and C++ interface to
[remanence-lib](https://github.com/ferroteca/remanence-lib), a library for
reading and analysing disk images — floppy and hard disk images from
vintage and modern systems alike.

This crate is something you build, not something you add to a Rust project.
It produces a static library and a shared library, and generates the C
header
[`include/remanence.h`](https://github.com/ferroteca/remanence-lib/blob/main/crates/remanence-ffi/include/remanence.h)
as part of the build. If you are writing Rust, use the
[`remanence`](https://crates.io/crates/remanence) crate directly instead.

> **This is an alpha release.** Before 1.0 there is no compatibility
> promise: when a part of the API changes, it changes across the Rust, C
> and Python interfaces together and the old form is removed. Read the
> [changelog](https://github.com/ferroteca/remanence-lib/blob/main/CHANGELOG.md)
> before upgrading.

## From C

Every failure returns a status code drawn from a small, stable set, so you
can decide what to do about a failure without parsing message text. Where
the failure came from a named rule, you get that name too. Anything you
have to free is documented as yours to free; anything the session owns is
handed to you as a view, and freeing it is not your job.

There is a complete worked example at
[`examples/identify.c`](https://github.com/ferroteca/remanence-lib/blob/main/crates/remanence-ffi/examples/identify.c),
with build instructions in the comment at the top.

## From C++

C++ callers get a friendlier header —
[`include/remanence.hpp`](https://github.com/ferroteca/remanence-lib/blob/main/crates/remanence-ffi/include/remanence.hpp),
header-only, C++17. It is built on top of the C interface rather than
alongside it, and it covers every function the C interface exports. What it
adds is convenience: objects clean up after themselves, and failures arrive
as a single exception type carrying the same stable category code. The C
interface remains the standard one and is still available.

```cpp
#include <remanence.hpp>

#include <iostream>

// One catch block for the whole program: every failure arrives as
// remanence::Error, carrying a category code you can branch on.
try {
    remanence::Session session;

    // Find out what a file is before setting anything else up.
    remanence::Discovery found = remanence::discover_media("disk.h8d");
    std::cout << found.article().value_or("?") << ' ' << found.size() << '\n';

    // The load takes over the discovery, so the file is opened only once.
    remanence::Medium medium = session.load_discovery(std::move(found));

    // What the file turned out to be, layer by layer, with a confidence
    // score for each.
    remanence::Identification what = medium.identify();
    for (const remanence::Layer& layer : what.layers()) {
        std::cout << layer.id().value_or("?") << ' '
                  << unsigned{layer.confidence()} << "%\n";
    }

    // Files live behind a partition; an image with no partition table has
    // one partition covering the whole disk. Nothing here needs freeing
    // by hand, and there are no status codes to check.
    remanence::Filesystem filesystem = medium.partition(0)->filesystem_as("hdos");
    remanence::EntryList listing = filesystem.entries();
    for (const remanence::Entry& entry : listing.entries()) {
        std::cout << entry.name() << ' ' << entry.size_bytes() << '\n';
    }
    remanence::FileData data = filesystem.read_file("HDOS.SYS");

    // Putting a disk in a drive and taking it out again changes nothing
    // about the disk. release_media is what actually discards it.
    remanence::StorageDevice drive = session.add_device("h17");
    drive.insert(medium.id());
    drive.eject();
    session.release_media(medium.id());
} catch (const remanence::Error& refusal) {
    std::cerr << static_cast<int>(refusal.category()) << ": "
              << refusal.what() << '\n';
}
```

The matching example is
[`examples/identify.cpp`](https://github.com/ferroteca/remanence-lib/blob/main/crates/remanence-ffi/examples/identify.cpp),
next to the C one.

## Features

- `leak-probe` — for testing only. Keeps a count of the library's live
  memory allocations and exports it, so a C test can check that freeing a
  handle gives back everything creating it took. Off by default: a released
  build has neither the counter nor the exported symbol.
- `fixtures` — for testing only, and it switches on a test file rather than
  any library code. That test needs a disk capture a preparation script
  downloads, which is not in the repository.

## Links

- [Repository and full documentation](https://github.com/ferroteca/remanence-lib)
- [Changelog](https://github.com/ferroteca/remanence-lib/blob/main/CHANGELOG.md)
- [Contributing](https://github.com/ferroteca/remanence-lib/blob/main/CONTRIBUTING.md)

## License

GPL-3.0-only. See
[LICENSE](https://github.com/ferroteca/remanence-lib/blob/main/LICENSE).
