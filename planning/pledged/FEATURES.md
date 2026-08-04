<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FEATURES (pledged)

> **Status:** pledged, not delivered. Every feature here is owed by the project, but no entry promises an order, date, or implementation approval.

## F24 — The FAT label answer, whole, at the filesystem seam

Make a recognized FAT volume's label one complete answer: the label, or the
fact that the volume has none. `NO NAME` is the format's own spelling of
unlabeled, so it is absence — decided where the format is known rather than by
a string comparison in every consumer that displays a drive.

FAT records a label in two places, the boot record's field and the root
directory's volume-ID entry, and a volume may carry either, both, or
disagreeing values. Choosing between them is a policy about FAT, so the
filesystem adapter holds it and states it: the root-directory entry is the
label DOS itself displays and answers wherever it exists; the boot-record
field answers where it does not; `NO NAME` at either source is absence. Both
readings stay beside the answer as evidence (P4), so a caller which needs the
literal bytes has them without opening a sector, and no caller has to know
which of the two it should have looked at.

The boot-record field is only a field where the format says it is. It belongs
to the extended boot record and exists only under that structure's own
signature; where the signature is absent the volume has no such field, which
is a third state distinct from the field being present and blank. Reading the
offset regardless would manufacture a label out of whatever bytes happen to
sit there, which is the invention this library refuses everywhere else — the
same rule that keeps a derived cylinder count absent unless it divides
exactly.

Nothing else may become a label. A directory name, a filesystem kind, a file
inside the volume, and the image's own filename are not evidence of one, and
an unlabeled volume is reported unlabeled rather than given a placeholder.

The label sits today on the volume record the disk report returns and, once
the delivered inspection report has it, on the filesystem record where that
seam owns it. F24 lands on
whichever presentation is current when it is picked up; it neither waits for
that report nor blocks it, and the answer it defines is the same either way.

Touches: S1, S2, S3. Supports: U2, U4, U22; P3, P4, P5, P18. Needs: nothing
pledged first.

## F25 — DOS 8.3 name rules owned at the file-access seam

Make every 8.3 name decision the file-access seam's own, and make each refusal
name the rule it broke.

Reads match without regard to case and return the name as stored. That is the
behavior today; this feature states it as a claim rather than leaving it a
property of the implementation, so a caller may rely on showing the user what
the directory actually holds. Writes validate and normalize at the same seam:
the caller supplies the name it has, and the library uppercases, pads, and
stores it. A caller uppercasing first is performing the library's rule in the
one place it cannot be checked against the format.

A name outside the namespace is refused with a rule identity from one
enumerated set, under the P10 amendment:

- an empty base;
- a base longer than eight characters;
- an extension longer than three;
- more than one separator, or one where the format does not allow it;
- a character the format excludes, naming the character;
- a reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
  `LPT1`–`LPT9`), with or without an extension; and
- a leading or trailing space in a component.

The reserved-device rule is the one the code does not enforce at all today;
the others are enforced and refused with a single undifferentiated diagnostic,
which is what leaves a consumer reimplementing the set to say which rule was
broken. Nothing is truncated, transliterated, or repaired to fit — a refused
name is refused (P6), and the caller decides what to do about it.

The stored escape for a leading `0xe5` byte stays internal. It encodes a
stored name; it is not a rule a caller can break.

F25 is where the P10 amendment ([ARCHITECTURE.md](ARCHITECTURE.md)) arrives in
the surface, being its first and only present demand. So this feature carries
both halves: the rule-identity field on the error, in Rust, C and Python
together with the generated header, and the DOS 8.3 rule set that is the first
thing to populate it. The field is not a Rust-only addition that a later
feature reflects outward.

Touches: S1, S2, S3. Supports: U3, U22; P3, P5, P6, P10, P18, P19. Needs:
nothing delivered first.

## F26 — The DOS drive-letter composer

Deliver the namespace-mapping composer of the P19 composer amendment for DOS.
Given the machine facts the caller asserts — medium, slot, and attachment
order — and the volumes already composed from the images it inspected, return
which volume each drive letter names, as an answer built from a named rule
rather than from the order things happen to appear in.

Floppy slots take `A:` and `B:`, and a single-floppy machine's second letter is
the phantom-drive convention rather than a second volume. Hard-disk volumes take
letters from `C:` upward under the claimed rule. CD-ROM letters follow only
where the caller declares the resident driver's placement, because nothing on
the disks records it and the driver could put it anywhere.

The assignment rule is the substance of this feature and its whole risk. DOS
did not letter volumes in the order a report lists them: the usual rule takes
the first primary DOS partition of each disk in attachment order, then the
logical drives of the extended partitions across those disks in the same order,
then such remaining primaries as the variant assigns at all — and the variants
differ exactly there. F26 therefore claims named rules by variant (P3). Where
the caller states which variant the machine ran, the composer applies that
rule; where it does not, a letter on which the claimed variants disagree is
reported undetermined rather than settled by choosing the most common one.
`LASTDRIVE`, `SUBST`, `JOIN`, `ASSIGN`, a block-device driver, and a network
redirector are outside every claimed rule, and a mapping they would have
changed is undetermined, not approximated.

The composer answers with mappings: each established letter names a volume by
the identity its report issued, and every letter it could not establish says so
with the reason. It opens no artifact, takes the reports the caller already
holds, and composes no file container over the result — the letter is what a
consumer shows a user, and the identity is what it passes back into a file
verb. Composing a rooted namespace over the mapping is separately admitted by
P19 and is not this feature.

F26 is where the P19 composer amendment ([ARCHITECTURE.md](ARCHITECTURE.md))
arrives in the surface, being its only present demand, so this feature carries
both the composer form and the DOS rule that is the first thing to use it. The
variant set it claims is the risk it names above, and enumerating one variant
honestly is what delivers this — not implementing every DOS that shipped.

Touches: S1, S2, S3. Supports: U22; P3, P4, P5, P19, P21. Needs: nothing
pledged first — the stable volume identities and the composed-volume
report this maps over are delivered. D5's deferral is untouched: nothing
here opens several artifacts together.

## F40 — The VDI image adapter

Claim the standalone VDI container as an ordinary block-family image
format: one adapter owning its recognition and evidence, its version gate,
its declared image types, its block map, and its read and write paths. A
VDI opens, identifies, inspects, and reads and writes files exactly as a
raw or qcow2 image does, through the same session and the same evidence
model, because the adapter is the only thing that knows it is a VDI.

The header's declared version is validated before anything else is touched
and a version above the claim fails immediately, naming what it found and
what is supported (P8). The declared image type is an enumerated claim
(P3): the fixed and dynamically allocated types this feature reads and
writes are claimed by name, and every other type the format defines —
differencing among them, which is F41 — is refused by name rather than
attempted. A block map entry marking a block unallocated reads as zeroes
where the format says so, and is never confused with a block that is
allocated and happens to be zero.

Writing follows the delivered disk stack unchanged: reads never alter the
image, writes buffer to the session cache under its declared bound, and
commit is the single durable moment with its recovery journal beneath it
(P2, P9, P27). Allocating a new block in a dynamically allocated image
happens inside commit, never during a read.

The work is what P12 says an ordinary image format costs: the module, its
tests, and one mechanical enrollment in the built-in catalog — plus the
one place the `Disk` surface selects a container by magic, which is a
second selection path and not the catalog. Nothing central learns a VDI
branch, and no shared module acquires a VDI parameter.

On delivery this feature widens a claim the descriptive surfaces state, so
U1's identification journey gains a format and U3's and U4's "qcow2 or
raw" wording moves to name what the library then claims. That is the
delivering feature's ordinary job, as the delivered device tier did for
U1, U3 and U4.

Touches: S1, S2, S3. Supports: U1, U3, U4; P1, P3–P5, P7–P9, P12, P13,
P27. Needs: nothing pledged first.

## F41 — VDI differencing chains

Make a VDI differencing image a first-class disk, as a qcow2 with a
backing file already is: the top image opens and the whole chain composes
as one disk, reads resolving through it block by block and writes
allocating copy-on-write into the top image only.

A differencing VDI names its parent by the parent's own identity rather
than by path alone, so resolution is checked rather than assumed: a
candidate parent whose identity does not match what the child declares is
a named refusal, not a silently accepted substitute. That is the
difference from qcow2's backing chain worth stating, because it is the one
place this format gives the library evidence qcow2 does not.

Every failure mode qcow2's chain already names is named here too, in the
same vocabulary and with the same refusal discipline (P3, P6): a missing
parent, a cycle, a chain deeper than the claimed bound, and a parent whose
own version or image type falls outside the claim. Each is a refusal at
the open, never a partial interpretation and never a fallback to reading
the top image alone.

A parent is claimed immutable for the session's life (P7) and is never
modified or flattened. After commit, the chain relationship stands and the
delivering hypervisor's own tooling reads the changed guest bytes — the
same guarantee U6 already makes for qcow2, which is what makes this a
first-class disk rather than a convenience.

Identification is deliberately untouched: a differencing VDI identifies as
the VDI container it is, exactly as U5 says of qcow2. On delivery U6's
wording moves to name the differencing formats the library then claims.

Touches: S1, S2, S3. Supports: U1, U3, U6; P1, P3–P8, P12, P13, P27.
Needs: F40 delivered, for the container, the version gate, and the block
map this composes through.
