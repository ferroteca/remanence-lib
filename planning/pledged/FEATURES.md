<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged at the owner's direction. F19 is owed by the project,
> with no promise of order or time; its number evaporates on delivery
> without being reused. The companion
> [design](design/image-format-modules.md) and P12, P13, P16–P19, and
> P21–P23 in [ARCHITECTURE.md](ARCHITECTURE.md) travel with this feature.
> Pre-1.0, every affected presentation moves coherently and the old shape
> is deleted rather than bridged.

## F19 — Image-format modules and the built-in catalogs

Replace the caller-authored format-definition registry with internal,
role-specific adapters and built-in catalogs. Image-format,
partition-layout, and filesystem recognition remain separate seams with
separate interfaces. P17 places volume composition between the latter two;
F19 preserves the present direct region-to-volume case but adds no complex
volume implementation or catalog. Serialized-file-container and
filesystem adapters converge on the P19 file-container view; F19 does not
add a system-wide namespace composer. Move the
current H8D, qcow2, HDOS, and CP/M identification rules behind the
applicable adapters; route identification through those adapters so
`Session` and the detection machinery name no image format and interpret
no heuristic string. Existing ZIP, qcow2, MBR, FAT, and HDOS
implementations remain the owners of their behavior and are enrolled or
composed at the seam where their kind varies rather than rewritten into
one universal adapter.

The public declaration experiment retires coherently. S1 loses
`FormatRegistry`, `ContainerFormat`, `FilesystemFormat`, the embedded
definition-text constants and parser entry points,
`Session::open_with_registry`, `Session::registry`, and any public
`DiskImage` construction that depends on a registry record. S3 loses the
Python reflections and `Session.with_registry`. S2 is unchanged because
the C ABI never presented the registry. S4 — the format-definition text
format — disappears, along with its built-in definition files and the
claim that a new format can be supplied without code. Root architecture,
README, examples, and tests land on the same truth in this feature.

Detection continues to satisfy P3 and P4. An adapter distinguishes no
match from a format recognized but invalid; every match and refusal
carries its evidence. The catalog at each seam chooses only a unique
strongest match.
An unresolved tie produces an unknown layer whose evidence names the
competing formats rather than allowing catalog order to invent a verdict.
No descriptor may be enrolled without the behavior that validates and
interprets what it claims.

Each loaded image names exactly one authoritative image layer declared by
its image-format adapter. Each independently mutable open state also names
one active durable layer. Derived representations distinguish decoded
evidence from synthetic detail and never become independently mutable
copies. F19 carries authoritative-layer, active-layer, and provenance
identity through identification and opening; later media and
hardware-emulation features use it to decide which read and write
derivations are valid rather than guessing from a filename or product model.

Every addressed virtual device created by image-format composition also
receives an opaque P21 identity from Remanence. It is scoped to the loaded
composition and is distinct from a hardware attachment such as `hdd0`.
The ordinary single-image path supplies nothing, and disk-local volume
identifiers remain sufficient through an interface already scoped to one
disk. F19 carries device identity internally and in provenance without
adding a caller-authored topology surface.

The simple legacy-floppy path remains simple. When identification yields
one image format, one direct whole-medium volume, and one filesystem, file
access composes those seams automatically and exposes its P19 file
container. No partition selection, synthetic namespace root, media
attachment, caller-provided device identity, or drive emulation is required
merely to list or extract files.

Delivery is demonstrated by locality: a test-only implementation at any
one seam is recognized by enrolling it in that seam's test catalog without
editing central dispatch. Every shipped image format, partition-layout
scheme, and filesystem is tested through the same interface callers reach.
Existing H8D, qcow2, HDOS, CP/M, file-container, disk, and filesystem
behavior remains covered at its current public surfaces. The core gains no
runtime dependency.

Touches: S1, S3, S4. Supports: U1, U2, U5; P1, P3, P4, P5; P12,
P13, P16–P19, P21–P23, and P27. Needs: P12, P13, P16–P19, and
P21–P23 pledged.
