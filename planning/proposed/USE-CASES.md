# USE-CASES (proposed)

> **Status:** drafted 2026-07-30 at the owner's direction, as input to
> the vision dictation (T1). Nothing here binds; entries are argued
> here, pledged by moving to `pledged/`, and reach the root list on
> full delivery. Numbers come from the one global U-sequence and are
> never reused. The owner's voice governs — these drafts are raw
> material for him to reword, split, or reject.

## U1 — Identify a disk image I know nothing about

I have a file that claims to be a disk image — maybe raw, maybe
sitting inside a `.zip`. I open it and remanence tells me, layer by
layer, what it is: the archive wrapper, the image format, the
physical media it represents, the probable filesystem — each with a
confidence and the evidence behind it, never a bare verdict. When it
doesn't know, it says "unknown" rather than guessing.

*(Delivered by the code today for the starter registry — h8d over
zip — so on pledge this arms immediately.)*

## U2 — Browse a vintage volume and pull files out of it

Once an image is identified, I list its catalog — HDOS today —
with the real names, sizes, dates and flags, and I copy a chosen
file's bytes out to the host, without ever booting anything or
mutating the image.

*(Listing is delivered; extraction of file contents is not yet
implemented.)*

## U3 — Reliquary reads and writes a stopped machine's disks at rest

Reliquary, my QEMU automation layer, needs to reach inside a stopped
machine's disk image on the host — qcow2 or raw — find the FAT12 or
FAT16 volumes behind the MBR, and list, get, and put files and
directories in guest terms, without booting the guest and without
any external helper process: the library does the format work
itself. Reading never changes the image. Writing is a separate,
explicit mode with a commit point: until I commit, everything I
wrote can be rolled back cleanly.

*(This is the at-rest capability currently implemented inside
reliquary — `at_rest.py`, `nbd.py`, and the qemu-nbd-plus-snapshot
protocol in `backend_qemu.py` — moving down into this library, with
the qemu-nbd delegation replaced by a native qcow2 driver.)*

## U4 — qcow2 images are first-class citizens of identification

Opening a qcow2 in the workbench identifies it like anything else:
the qcow2 container layer with its version and virtual size, the
partitions inside the virtual disk, the volumes inside those — the
same session, the same evidence model, the same registry-driven
detection that identifies an h8d today.
