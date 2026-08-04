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
delivering feature's ordinary job, as F39 does for U4.

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

## F43 — One claim, one medium surface

Merge the library's two top-level surfaces into one. `Session` and `Disk`
are presently unrelated types over the same file — the session identifies
and reads bytes, the disk inspects and performs file verbs, and neither
mentions the other — and they cannot coexist over one artifact, because
each takes its own P7 claim on it. That is not a tidiness problem. Two
ways in that structurally cannot both be used on the same image is a defect
in the surface, and it is the thing standing between the library and any
device tier above it.

One claim serves everything. A medium is opened once, and identification,
bounded reads, the layered inspection report, the volume-scoped file verbs,
commit and rollback are all served from that one claim. Which of the two
present open paths survives is this feature's substance and its whole risk:
the identification path (`ImageSource`) carries the session cache, the
predictive reader, and `archive[/entry]` support through spooled backing,
while the disk path carries the declared access intent, the recovery
journal, and the image adapters — whose `open_disk` seam takes a concrete
file device rather than anything more general. Neither is a superset, and
**no capability either side has today may be dropped to make them meet**:
an archive entry must still identify, and a qcow2 chain must still commit.

The adapter seam is P12 contract surface, so whether it changes shape or
archive spooling moves beneath it is a decision this feature makes and
records, not one it may leave to the call site.

Access intent is declared at open as the disk path already requires (P7,
P1): a read open takes no stronger claim than it needs, a write open that
cannot secure its claim is refused by name rather than degraded silently,
and the mode holds for the medium's whole life. The identification path's
present behavior of quietly degrading to read-only does not survive the
merge, because one surface cannot hold both rules.

Nothing above the merge changes. The report keeps its content, its seams,
and its opaque region, volume and filesystem identities; the file verbs
keep the identities F39 gave them; the P27 cache bound stays caller-declared
with its stated default.

Touches: S1, S2, S3. Supports: U1, U3, U4, U5, U6, U23; P1, P2, P5, P7,
P9, P12, P13, P27. Needs: nothing pledged first.

## F44 — The session device set

Give a session a set of storage devices where it holds one medium. A device
is a durable, family-typed slot: it has an identity, a family, and zero or
one attached medium, and it outlives what is attached to it. The merged
medium surface F43 delivers becomes what a device holds, and the block-family
capability a caller obtains from it.

A device carries an **attachment identity** in P21's sense — that principle
already distinguishes "an attachment identity such as `hdd0`" from the
opaque device identity it assigns an addressed virtual device, and this
feature adds the first of the former without disturbing the latter. It is
composed from the family and an index (`hdd0`, `floppy0`), caller-facing and
predictable, which is the deliberate opposite of the opaque identities the
delivered report issues for regions, volumes and filesystems. The
distinction is what the identity *is a fact about*: a device is machine
configuration the caller supplies, of the same kind U22 already calls its
own, while a region is evidence read off a disk. A caller may name the
**slot** an attach lands in but never a name; an attach naming no slot takes
the lowest free index for that family.

Attach and detach are **machine-down operations**, and this feature claims
only that state — there is no hardware composition to be running yet, so
the rule is stated and cheap to honor now and does not have to be
retrofitted when one exists. An index freed by a detach may be reused by a
later same-family attach, which is safe for exactly that reason and is not
the renumbering U4 refuses for evidence-bearing lists. A device refuses a
medium outside its family by name (P3, P14).

Nothing is reachable except through a device, because a medium opened beside
the session would belong to no machine and contradict the principle on the
public surface. **Volume, region and filesystem identities are not
re-derived**, and P21 is why — device identity qualifies otherwise-local
identifiers "only where more than one device makes that distinction
necessary," and an interface already scoped to one disk "may continue to
accept a disk-local identity." Because every file verb is reached through
the device that owns the medium, the identities stay device-scoped and F39's
contract holds unchanged. A flat session-wide volume list would have forced
the qualification D5 priced, and this feature does not create one.

Scope is deliberately the tier and nothing above it. This feature adds no
new media family, no region enumeration for a family that lacks one, no
move of file access onto a region, and no device capability presenting
`Hardware<C>` — each is separate work that needs this delivered first. The
one family it must carry is the block family the library already reads, so
`hdd0` is real and `floppy0` is not yet.

U1's, U3's and U4's journeys are unchanged in what they achieve and change
in how they are reached, so their wording moves to the device expression of
the same guarantees — the delivering feature's ordinary job, as F39 did for
U4.

Touches: S1, S2, S3. Supports: U1, U3, U4, U22; P3, P5, P14, P21, P27,
P32. Needs: F43 delivered, for the one merged medium surface a device
holds.

Companion design:
[design/session-storage-devices.md](design/session-storage-devices.md).
