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
F38 has landed, on the filesystem record where that seam owns it. F24 lands on
whichever presentation is current when it is picked up; it neither waits for
F38 nor blocks it, and the answer it defines is the same either way.

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

Touches: S1, S2, S3. Supports: U22; P3, P4, P5, P19, P21. Needs: F38
delivered, for the stable volume identities and the composed-volume report
this maps over. D5's deferral is untouched: nothing here opens several
artifacts together.

## F38 — The layered disk inspection report

Add one evidence-bearing disk inspection operation whose result keeps the
pledged image, device, partition-schema, volume-composition, filesystem, and
active-layer seams distinct. The report names the opened image and
block-active device, states what the device's leading structure turned out to
be, and reports any recognized partition schema, every declared partition
region, each volume actually composed, and each filesystem actually
recognized on a volume. Typed relationships join those records, and evidence,
ambiguity, absence, and recognized refusals stay attached to the seam which
owns them.

The leading structure is one classified outcome the report *states* — blank; a
recognized partition schema, whether or not any volume composed from it; a
direct unpartitioned volume; or nonblank content no adapter claims — not a
flag beside two lists a consumer has to reconstruct the judgement from. The
last of those arms is a deliberate behavior change: unclaimed nonblank content
is a refusal from `discover` today, and becomes a reported outcome here.

Every declared region reports both its raw type value and a reading of what
that value declares, present whether or not the type is inside this feature's
read claim, and fit to quote in a refusal a user will read. The report supplies
opaque library-owned identities for its regions, volumes, and filesystems; a
public identity is a deterministic function of the layout's structure, never a
report index or a session counter, because U4's cross-open stability and P21's
opacity together admit nothing else.

F38 is additive. `DiskGeometry` and `geometry()` remain until F39 removes them,
so every presentation carries both models for exactly as long as the two
features are apart, and no presentation ever lags another. The Rust, C, and
Python surfaces expose the same report graph, relationship and identity
semantics, optional facts, and structured issues, and land together.

Scope is the formats U4 already needs: raw and qcow2 block devices, MBR
including extended and logical entries, a partitionless direct volume, and
FAT12/FAT16. F38 adds no format recognition and no orchestration path beside
the in-force adapter architecture's. It does **not** make FAT a P19
file-container provider — the delivered file-container contract holds
filesystem listings at their present shape until a feature presents them
through that seam, and that feature is neither of these.

Touches: S1, S2, S3. Supports: U4; P3–P5, P13, P16–P18, P21, P23, P27. Needs:
nothing pledged first; the adapter architecture it composes is in force. P19
is deliberately absent from that list: this feature reports a recognized
filesystem, and does not present one through the file-access seam.

Companion design:
[design/layered-disk-inspection-report.md](design/layered-disk-inspection-report.md).

## F39 — Opaque volume selection, and the end of the geometry surface

Retire the FAT-shaped disk surface F38 replaced. Volume-scoped file verbs stop
accepting a caller-parsed volume string and take the opaque volume identity the
inspection report issues; `DiskGeometry`, `geometry()`, and the flattened
partition and volume records are deleted from Rust, C, and Python together,
with the generated header committed. No compatibility alias or flattened view
of the old model survives.

The two surfaces cannot be separated by presentation: the C binding imports the
core's concrete geometry types directly, so deleting them is one change across
all three or it is not a change at all. What makes this feature separable from
F38 is order, not layering — F38 adds, F39 removes.

U3's file behavior is unchanged apart from how a volume is named. U4's wording
and every descriptive surface — examples, README, architecture, usage
documentation — move to the layered expression of the same stopped-machine,
stability, no-skipping, and known-cause guarantees, which is what arms U4
against the delivered surface.

Touches: S1, S2, S3. Supports: U3, U4; P5, P21. Needs: F38 delivered, for the
report and the identities this selects by.
