<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Reading ImageDisk

Design for [F68](../FEATURES.md#f68--imagedisk-read-presenting-sectors-in-the-order-the-recording-numbers-them),
serving U1 and U2 under P3–P5, P8, P10, P12–P14, P21, P27 and P28. The
ordering ruling it rests on is D60.

## What the format is

An ImageDisk file is an ASCII header terminated by `0x1a`, then one record
per track. A track record states its encoding mode and data rate, its
cylinder and head, how many sectors it holds and how large they are, and
then a **sector-id map**: the id carried by each sector, in the physical
order the sectors were recorded. Two optional maps follow where the head
byte's high bits say so, giving each sector's stated cylinder and head
where those differ from the track's own.

Then come the sector records themselves, each opening with a type byte.
The nine types are the cross of three facts: whether the data is stored
literally or as one repeated fill byte, whether the address mark was a
deleted-data mark, and whether the sector was read with an error. A tenth
value, zero, means the sector was not recovered at all.

## The ordering ruling, which is the whole shape of this feature

An ImageDisk track holds its sectors in **physical** order and states
their ids separately. A raw dump of the same disk holds them in **id**
order, the physical interleave having been flattened by whoever dumped it.
The same recording therefore arrives in two different orders depending on
which format carried it, and something has to resolve that.

D60 puts the resolution in the image adapter, and the reasoning is that
the evidence lives there and nowhere else. The id map is *in the ImageDisk
file*. No layer above can see it, so any layer above resolving the order
would be applying a rule it cannot check. Conversely the adapter cannot
resolve anything the format does not state — a raw dump says nothing about
interleave, so nothing is resolved for one, and what remains is a
declaration somebody else makes.

The Heath CP/M disks are the worked example. Their hard-sectored dumps
need a four-way skew declared in the CP/M layout, because the artifact
states no ids and the interleave lived in the drive's BIOS. Their
soft-sectored ImageDisk images need none, because the interleave was
recorded into the sector numbering and the format states it. Same
filesystem, same release, same geometry; the difference is entirely where
the interleave was written down, and the ruling puts each half where its
evidence is.

## Why this needs no new device seam

With ids resolved, a track's sectors laid end to end in id order are a
linear extent, and a uniform image is those extents concatenated. That is
exactly what the flat presentation every other sector image uses already
serves, so nothing about the device seam changes.

The record-structured seam the earlier proposal called for is therefore
not built here. It was called for because a format storing per-sector
encoded records "has nowhere to be" — but that is about *writing*, where a
record's encoded length changes under the caller's hand, and about
addressing a non-uniform disk by coordinate. Reading a uniform one needs
neither. Building the seam anyway would be structure ahead of the demand
that has to shape it, which is D58's reasoning arriving at a second seam.

## Geometry, and the case this feature declines

A uniform image declares one geometry tuple, and the sector verbs address
in it as they do for an H8D.

A non-uniform image declares **none**. Its bytes still read, and every
filesystem above it works, because a filesystem addresses by offset and
not by coordinate; what it loses is `read_sector`, which refuses through
the geometry seam's existing unstated-geometry rule.

That is an honest floor rather than a shortfall dressed up: the recording
genuinely has no single coordinate system, and the alternative — declaring
the majority track's geometry and quietly mis-addressing the odd one — is
the failure this library exists to refuse. Giving such a recording real
per-track coordinates is F80, and it is a change to the geometry seam and
all three surfaces rather than a change to this adapter.

## What is not payload

**An unavailable sector is not zeroes.** Type zero says the sector was
never recovered. Its extent is excluded from what the load establishes as
readable, so a read touching it is refused with its range under the same
P28 machinery a short raw image uses, and a read that avoids it succeeds.
Nothing is filled.

**A deleted or errored sector is payload with a fact attached.** Its bytes
are what the artifact holds and are served; that the address mark said
deleted, or that the recovery reported an error, is counted into the
load's evidence. This release counts them rather than attaching them to
individual sectors, because the image layer has no per-sector fact channel
and inventing one for a count is the wrong order of work.

**Compression is encoding, not content.** A compressed record expands to
its fill byte repeated; that it was stored compressed is evidence about
the artifact and is counted, and it is the fact F69 will need most, since
it is what makes a record's encoded length change on write.

## Refusals

Each of these is a named refusal rather than a repair (P3):

- a sector-data type byte outside the ten the format defines;
- a cylinder or head map that disagrees with its own track header;
- two sectors in one track claiming the same id, which makes the id
  ordering ambiguous and there is no reading that resolves it;
- a track record that runs past the end of the file;
- a sector size the format's exponent does not define.

## What proves it

The Heath CP/M 2.2.03 soft-sectored distribution is the real artifact:
three ImageDisk files whose contents are already known from the
hard-sectored release, so the id ordering is checkable against files whose
bytes have been read another way. `ASM.COM` opening with a stack load and
Digital Research's 1978 copyright is the assertion that the ordering is
right, exactly as it is for the raw dumps.

Synthetic images carry the refusals and the non-uniform case, since an
artifact with a duplicate sector id is not something to go looking for in
the wild.
