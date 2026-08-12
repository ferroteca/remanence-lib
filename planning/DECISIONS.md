# DECISIONS

The adjudicated design-decision record. Each entry records what was
decided, by whom and when, what was weighed and declined, and where
it folded. The normative homes are elsewhere — root
[ARCHITECTURE.md](../ARCHITECTURE.md) and, once dictated, the
use-case and principle lists. This file is the adjudication trail,
and the guard against re-litigating: **anything recorded here as
killed, declined, or superseded is not revisited without new
evidence**, argued through the surface-change rule
([SURFACES.md](SURFACES.md)).

Decisions are numbered in the order first recorded — D1 the
earliest — and **a number is never reused**; the list reads
newest-first, so the top entry carries the highest number and a new
entry prepends with the next free one. The D-number is the
decision's citation handle everywhere: a decision names the vision
it supports — use cases (U-numbers), principles (P-numbers),
surfaces (S-numbers) — and it is citable downstream in design
documents, specifications, and code commits.

**The supports clause is not optional, and "none" is an answer.** A
decision genuinely demanded by nothing — a vocabulary or naming
choice — records `Supports (none)` and why. Prose in place of a
handle is the same gap wearing a sentence: a citation that resolves
to no number is not a citation, and only a numbered one can be
audited.

**A lifecycle act alone earns no entry.** Proposing, pledging,
promoting, delivering: location states the status and the commit
that moves the item is the record, so delivery evidence belongs in
that commit's message. Only a ruling made in the act's course — a
contested clause reading, a scope call, a withdrawal — is recorded
here, slim, as the ruling rather than the promotion around it.

An overruled or no-longer-relevant decision moves, number and text
intact, to the Retired decisions section at the bottom, its note
naming what overruled it — a retired decision binds nothing but
remains the record. **Entries keep the spellings of their time**: an
entry only partly overruled is annotated, never rewritten, and
correcting an entry's prose in place is never the answer — an error
and its discovery are part of the record.

## Open questions

Questions awaiting adjudication — the front of this record rather
than a separate one. Nothing here binds anything; a question leaves
this section when it is adjudicated — as a D-number only where the
ruling has no normative home, otherwise absorbed by the pledged or
in-force entry whose text carries the ruling — and the commit that
removes it is the record either way.

- **CLA legal review** — [CLA.md](../CLA.md) states intended terms
  but has not been reviewed by a lawyer, and its governing-law
  clause is deliberately unfilled. What turns on it: no external
  contribution can be accepted under it until reviewed. Settled by
  that review.

## Decisions

### D34 — Rulings made delivering the discovered geometry and the recording's coordinates

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-12. **Supports** S1, S2, S3; the pledged media-first design
(fact classes, and the kind-declared actions on the medium); in-force
P2, P3, P4, P6, P10, P14, P27; in-force U4, and pledged U28, U32.

Rulings made in F58's course. The delivery itself is recorded by the
commit; these are the calls made along the way.

**The floppy class is sector-addressed, and the addressing attribute
becomes total.** The delivered `addressing()` answered `Option`, `None`
for every floppy, with a note deferring the question to this feature.
The answer is that it was never a hard-drive fact: a floppy drive steps
to a track and reads records around it, which is exactly what a
cylinder, head and sector name. What the granularity rule keeps *out* of
the type is how many of each, not whether there are any. So the
attribute is total — `sector` or `block` for every device type — and the
cut it makes is the one the sector verbs need: the type declares that
there are coordinates, the medium's evidence says how many. The C and
Python spellings keep their nullability because the archive receiver is
no device type at all.

**An end tuple states nothing on its own, and is solved rather than
read.** The obvious inference — heads are the head number plus one,
sectors per track are the sector number — is wrong twice over: a drive
past what CHS can address writes a saturated tuple whose numbers name no
geometry, and a partition that ends mid-cylinder names a head that is a
floor rather than a count. What makes a tuple evidence is that the same
entry declares the same block a second way, as the last block of its own
LBA extent, so the geometry is whatever puts the one where the other
says it is. Where exactly one geometry within the field widths does
that, it is the reading; where several do, or none, the tuple states
nothing and nothing is inferred from it. Verification fills values under
a reading and never picks one.

**The load's declaration of a raw block size is a source, not an
override.** `Format::Raw` carries the block size because a raw image
records no addressable unit — but a table read in 512-byte blocks states
one too, and this release's MBR reading is written against exactly that.
Ranking the caller's declaration above the evidence would hide a real
contradiction about one disk, so the declaration enters as one reading
among the others and a disagreement reports as `Undetermined` like any
other. This is the fact-class rule holding: the declaration belongs to
the *load*, and what a medium's coordinates are stays discovered.

**The article's addressable unit is not a geometry source.** The
logical-block article declares 512-byte blocks and every hard-drive spec
composes it, so it was available and was deliberately left out: the
article is what the *substrate* is, and a sector size is a fact about
what was recorded on it (D19's boundary, at "recorded"). Reading it here
would also manufacture a conflict with a raw load declaring some other
unit, out of a fact that was never about the recording.

**The extent states the cylinders the recording spans, and the sector
verbs check the content separately.** The strict reading — a cylinder
count only where the track geometry divides the extent exactly — was
weighed and declined: it is the delivered rule for the *filesystem's*
declared geometry, where inventing a number would be a false claim about
a boot record, but here it would leave every image whose size is not a
whole number of cylinders with no coordinates at all, which is most of
them. The extent reading answers the cylinder its last sector falls in,
plus one, and says in its own words how far short of that cylinder the
content stops. The gate that would otherwise be lost is not lost: a
coordinate inside the geometry and past the content is refused by name,
with a different sentence from one outside the geometry altogether.

**A geometry is whole or it is nothing.** Three of the four parts
settled is not a geometry with a hole in it — it addresses nothing — so
the state is `Unstated` and `unsettled()` names the missing parts. That
keeps three answers apart that a partial record would blur: the sources
agreed, the sources disagreed, and nothing spoke. `Unstated` and
`Undetermined` are deliberately two states for the same reason U4 keeps
blank apart from unreadable.

**Establishing a geometry never fails a load.** It runs beside the
partition pool, after it, over the positions the pool already
established — so nothing hunts for a volume — and a source that cannot
be read states nothing rather than refusing. A geometry is evidence
about an artifact, not a condition of opening one, and a degraded
session (P28) that can no longer read a boot record still loads and
still says what it does know.

**Weighed and declined:** a geometry the caller could declare onto a
loaded medium (the design's fact classes forbid it, and F60's authorship
is where a caller's own coordinates enter); ranking the sources so a
disagreement always resolves (it would settle by fiat what the artifact
leaves open, which is the one thing `Undetermined` exists to refuse);
`get_sector` on the block-addressed types under a synthesized geometry
(a `mbr-block-hd` records no cylinder or head, and answering one would
be the library asserting a drive nobody had).

### D33 — Rulings made delivering the device types and the articles they compose

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-12. **Supports** S1, S2, S3; the pledged media-first design and
the pledged P32 with its amendments; in-force P3, P5, P6, P10, P12, P14
(amended here to carry the device-type catalog), P16, P19; in-force U4,
and pledged U23, U25–U28, U32, U34.

Rulings made in F57's course. The delivery itself is recorded by the
commit; these are the calls made along the way.

**The archive receiver is a slot and not a device type, so `DeviceSlot`
is a second enum beside the catalog.** F57 says a device type names the
device a medium's content is *assumed recorded by*, and that archives
were recorded by none — `device_type()` answering `None` is the feature's
own sentence. An `Archive` variant inside `DeviceType` would contradict
it at the first call, since an archive medium would then answer `Some`.
But D27 keeps `arc0` visible in its machine's attachment namespace and
an archive still has to be seated somewhere, so what a device is typed by
is **either** a recording device or the receiver: `DeviceSlot::Recorded(t)
| DeviceSlot::Archive`, with `From<DeviceType>` so the ordinary call
reads `add_device(HardDrive::MbrBlock)`. The receiver's own `device_type()`
answers `None`, which is the same word meaning the same thing on both
sides of the insert check.

**An attachment identity names a place, so it carries the bay and not the
type.** Three hard-drive types take `hdd` and both Heathkit controllers
take `heathfloppy` — the granularity rule cuts the *recording*, and a
machine's bays are not cut the same way — so a slot prefix no longer
resolves to one device. `AttachmentId` became prefix-and-index, the type
moved onto `StorageDevice` where it already belonged, and the lowest free
slot counts by bay: two hard drives of different types cannot both be
`hdd0`, which is a fact about machines rather than about the catalog.
The delivered identity duplicated the device's own family; nothing is
lost by removing the duplicate.

**The interior-name refusal disappears rather than being reworded.** The
delivered catalog classified with interior entries and refused them at
`add_device` (P32's amendment). A two-level enum *is* that hierarchy, and
"some floppy" is no longer a value that can be spelled — F57's "a type
the library does not know fails to compile" covers the vague name as
well as the unknown one. The `is_a` query, the lineage and
`accepted_media` go with it; asking whether a device is a floppy is now
a `match` on the class.

**Where the declared scheme does not check out the answer stays the
direct partition, and D32's reason for that is only half removed.** D32
deferred the refusal to this feature on the ground that nothing then
distinguished a partitioned hard disk from a floppy image. The device
type now distinguishes them, and the floppy class is genuinely exempt
from the table read. But `Format::Raw` is typed to the hard-drive class
by F57's own text, so a bare FAT floppy image arrives declared as a
hard-drive recording — and refusing an unpartitioned one would refuse
every image this release reads that way. The check reads the table where
one is there and composes the direct partition where the content records
none, which is what F56 delivered and what F57 keeps.

**A schemeless medium is still classified, and a table it might hold is
content nothing claims.** Skipping the scheme step entirely would have
cost the floppy class its content outcome — blank, one bare volume, or
content nothing claims — which is evidence about the recording rather
than about a layout, and which is what decides whether the direct
partition composes a volume at all. So `mbr::classify` answers the three
no-scheme answers and never the fourth: a sector 0 carrying a boot
signature on a medium whose device type declares no scheme is reported
as content nothing claims, with a reason of its own saying the table was
not read because nobody declared one.

**A discovery over a format that records several device types asserts
none, and the pool refuses to take it.** The alternative was pooling such
a medium with `device_type()` answering `None` — but `None` means
*recorded by no device*, and a qcow2 was written by some hard drive.
Using it for "we do not know which" would corrupt the one word the model
spends on the honest absence, and the medium would in any case be
seatable in nothing and layoutable under nothing. So the refusal is at
the pool's plain door and at `add_device_for`, naming the types a
declaration may state, and `Discovery::device_type()` answers only where
the format records exactly one.

**The declaration a discovery cannot make is taken by
`load_discovery_as`, and the vantage doors are the precedent.** The
refusal above nearly cost a delivered capability: an artifact *inside an
archive* is reached only through `File::discover`, so a member no adapter
identifies — a KryoFlux stream, which is bytes to every enrolled
adapter — would have become unloadable, there being no file for
`load_media` to take. The library already has the shape for this:
`filesystem()` opens where the declared type determines a namespace and
`filesystem_as(id)` takes the caller's reading where nothing does. So
`load_discovery` is the plain door and `load_discovery_as(discovery,
device)` the declared one, the second checking the type against what the
recognizing format records. The claim is held across both, so the nested
journey keeps its one open. F67 is where discovery's shape is next
argued.

**`Discovery::default_device` collapses into `device_type` because the
two facts became one.** The delivered surface distinguished "the family
the format declares" from "the medium's own type", the medium having
none to carry. A medium now carries the device type, and where a format
records exactly one there is nothing left to distinguish: what the format
declares *is* what the medium will carry. `accepting_devices()` remains
the other question — where could this go — and `device_types()` is the
adapter's list, which is what the refusals name.

**The identity crosses C as its stable spelling rather than as an
integer constant.** F57 says "integer constants in C, enums in Python".
The catalog ships as strings on both, because every other enumerated
claim this ABI carries already does — formats, partition schemes,
partition types, drive profiles, the families this replaces — and the
generated header is derived from the Rust signatures, so one catalog
crossing as an integer would be the only one a C caller has to hold a
second table for. The stable spelling *is* the cross-language identity
the surface is built on, and P5's "same semantics" is served by using
it. Python takes the same spelling for the same reason; a Python enum
over it remains available later without moving anything.

**Three enumerated types are declared by no format in this release, and
that is the catalog working.** `HardDrive::Gpt` is enumerated because the
scheme is part of the hard-drive spec and GPT implies block addressing by
its own definition; no adapter records it, because none reads a GPT, so
declaring it is a named refusal rather than a silent reading of the wrong
table. `FloppyDrive::HeathH37` and `FloppyDrive::Sector` are the same
shape: named by the granularity rule, reachable when a format records
them. The catalog is the claim; what a format admits is a separate
declaration, and F57 asks for exactly that gap.

**Weighed and declined:** an `Archive` variant in `DeviceType` (above —
it makes the model's own sentence false); keeping the attachment identity
type-bearing by giving each device type its own slot prefix (`mbrsector0`
beside `mbrblock0` is a bay no machine has); refusing a declared scheme
that does not check out (above — it refuses every bare FAT image);
skipping content classification for schemeless media (above — it costs
the floppy class its volume); pooling a device-typeless medium
(above — it spends `None` on two meanings); and leaving the refusal
without the `_as` door (above — it makes an archived raw member
unloadable, there being no file to declare over).

### D32 — Rulings made delivering the partition pool and the vantage doors

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-10. **Supports** S1, S2, S3; the pledged media-first design; the
P19 amendment; in-force P3, P4, P10, P16, P17, P18, P21, P27; U4.

Rulings made in F56's course. The delivery itself is recorded by the
commit; these are the calls made along the way, and the first is the one
a later reader most needs.

*Annotation (D33, 2026-08-12): F57 has landed. The scheme moved from the
media type to the device spec as the first ruling below anticipated; the
refusal the second ruling deferred is **not** reinstated, and D33 records
why.*

**The pool populates under the medium's kind, because the device spec it
is owed does not exist yet.** F56 says the pool populates "under the
device spec, kind-determined for every type — the hard-drive class by its
spec's scheme, checked at load". Device specs are F57's, and F56 needs
nothing from F57 — so what names the scheme today is the only kind a
medium carries today: its media type. A space-native medium is laid out
under MBR and the table is checked at the load; a namespace-native one
bears the direct partition with no extent. **What was removed is the
probe, not the check**: the delivered partition catalog ranked layout
adapters against a device and fell through to a bare volume, which is a
reading being picked, and one specified scheme checked against the
content is not. When F57 lands, the scheme moves from the media type to
the device spec and nothing above it moves.

**Where the specified scheme does not check out, the answer is the
direct partition rather than a refusal.** "Checked at load" reads as a
refusal, and a refusal is wrong here for a reason F57 will remove: with
no device spec there is nothing distinguishing a partitioned hard disk
from a floppy image, so refusing a medium whose sector 0 is a boot record
would refuse every partitionless disk this release reads. The scheme
adapter's three no-scheme answers — a filesystem boot record, a blank
disk, content nothing claims — are what they always were, and each of
them now composes the direct partition instead of nothing.

**The pledged tree's one `Partition` is two Rust types, and Rust is the
reason.** The design draws the facts and both doors on one node, and
`partitions()` handing out several door-bearing nodes at once cannot be
written: a door composes a space over the medium, so each node would hold
a mutable borrow of the same medium. So the pool answers with
**`Partition`**, a borrow-free record carrying every fact the scheme
declared, and `partition(n)` answers with **`PartitionView`**, the borrow
that holds a partition and its medium at once. That is the split
`Machine`/`MachineView` and `StorageDevice`/`DeviceView` already are, one
tier down, rather than a new shape.

**Opening a door spends the view, which is the identity rule carried by
the type.** F56 says both doors hand out *the one* `StorageSpace` the
partition composes. Handing out `&mut` to a space the view held would
have said it too, and it would have made the view a second place a
composed space lives. Consuming instead makes the rule unforgeable: the
node comes back once, through whichever door was asked, carrying whatever
vantages the partition has — so which door was opened changes nothing
about what comes back, which is the identity rule stated exactly.
`Partition::is_addressable` and `bears_namespace` are the non-consuming
predicates for a caller who wants to ask before spending it.

**The direct partition is ordinal 0 and a scheme's own numbering starts
at 1.** MBR numbers its entries from one, so zero is the library's to
spend, and spending it there means the two never collide and the walk is
uniform. A medium recording a scheme bears no direct partition, and a
medium recording none bears exactly it.

**The direct partition never appears in the inspection report, and the
evidence answer is unchanged.** The pledged ledger says the evidence
answer (`partition_scheme: None`) stands while the navigation answer
gains the declared synthetic member, and the code says it the same way: a
composition act is provenance, so `DiskReport` derives its regions from
the scheme's entries alone and a medium recording no scheme still reports
none. `DiskReport` is now computed from the pool rather than being what
navigation goes through, which is the whole of its demotion — every fact
it reports is the fact it reported before.

**U4's identity clause survives the move of the file verbs, and was not
amended.** The in-force entry says an identity "names exactly the same
region, volume, or filesystem in every file verb that it named in this
report". The file verbs are now reached by the scheme's own ordinal, and
the identity travels with them: `StorageSpace::volume_id` answers the
same value the report issued for that partition's composition, and
`Partition::volume_id` answers it beside the ordinal. The identity is
still opaque, still the library's, still stable across opens, and still
never built by a caller from a number or a position — so the claim holds
in substance, and the ordinal is the schema adapter's own fact rather
than a second identity (P16 puts it there, and U4 already declares it
load-bearing where it says a refused entry keeps its place).

**A namespace declaration is a stable spelling, not a Rust enum.** The
partition *type* is enumerated because a scheme's type values are what a
declaration is checked against, and `PartitionType` is that set. The
namespace declaration is not the same kind of thing: it names the adapter
that will read it, and adapters are already named by stable spellings
everywhere they are reached — `"hdos"`, `"cpm"`, `CBM_DOS`, the FAT
kinds. So `filesystem_as` takes the spelling and refuses one outside the
claim by name (P3), which is what `Format::from_id` and
`media_profile::by_id` already do at their own boundaries. The claim is
four: `"fat"`, `"hdos"`, `"cpm"`, `"cbmdos"` — and `"cpm"` still refuses
at the open, recognition and reading being separate claims.

**`as_type` answers `Result<()>`, because the check is the whole of it.**
The verb exists so a caller can state their reading and be refused by
name where the recorded byte does not bear it; it settles nothing about
what the partition then hands out, since the namespace vantage opens
under the type the *scheme* declared. A verb whose value is its refusal
is unusual and is what this one is.

**The resolver's medium-namespace bound dissolves into the adapter's
own.** The 8 MiB bound existed because a resolver *searched* a medium's
own content for a namespace and a search needs one (P27). Nothing
searches now: a declaration names its adapter, and the adapter says how
much it will take whole — which is where the number came from in the
first place, and it stays there. `filesystem_catalog::recognize`,
`CatalogRecognition` and `SpaceRule::SeveralCandidates` go with the
search, having nothing left to tie or to break a tie between; the
catalog's probes stay where they were always used, identifying an
artifact's layers.

**Corrected in passing:** root ARCHITECTURE.md's S1 inventory still named
`Archive` and `ArchiveEntry`, which left all three surfaces when the
archive became a medium. The line was being edited for the partition
vocabulary anyway.

**Weighed and declined:** verifying every declared namespace at the load
so the doors could be lookups over verified readings as well as declared
ones (it makes a load read a boot record per partition, and it puts the
recognizing seam's refusal somewhere a caller cannot reach it — the
delivered shape has the door answer from the declaration and the space
carry the verified state, which is D25's ruling that a refused
recognition answers with its own refusal, kept); making the direct
partition addressable only where a volume composed (it would have made
the one member that is *defined* as the whole content refuse to address
the whole content on a blank disk); giving `partitions()` door-bearing
nodes by putting the medium behind a shared cell (interior mutability to
buy one call site is a shape this crate has nowhere else); enumerating
the namespace declaration as a Rust type beside `PartitionType` (above);
and amending U4 (above — its claim holds, and an amendment written to
excuse a change that did not break the claim is worse than no amendment).

### D31 — The declared format set enumerates what a medium *is*, so `p64` waits for the flux fold-in

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-10. **Supports** S1, S2, S3; in-force P3, P13; F53's own pledge,
F59.

F53 lists the format ids its declared reading claims as "`zip`, `7z`,
`h8d`, `qcow2`, `vdi`, `raw`, `p64`". Six of the seven are delivered
with it; **`Format::P64` is not, and moves to F59**, which is the
feature that folds the standalone `CaptureSet` and `P64Image` roots into
the model and already says "`Format::P64` loads the served form straight
in".

The reason is what a format id *does* here. A declaration names the
adapter that checks it and, through that adapter, what the medium turns
out to be — and this release's media are the block family and the
archive. A P64's own adapter declares flux (P13), so `Format::P64` would
have to answer with a flux medium: a media profile no flux artifact
carries yet, an insert check with nothing to check, and content verbs
with nothing behind them. Delivering that is F59's substance, and doing
it under F53's name would have been the fold-in wearing another
feature's number.

**Nothing is dropped by the move.** F53's number retires with its
delivery, so the id would have left no trace; F59's entry now carries it,
and the refusal a caller meets meanwhile is the enumerated one P3
requires — `Format::from_id("p64")` names what this release claims rather
than accepting a spelling that leads nowhere. The flux family stays
reached through its own types, exactly as before.

**Weighed and declined:** shipping `Format::P64` as a variant that
always refuses (a surface entry that never works is worse than an absent
one, and P3's enumerated-claim discipline is precisely against it); and
building the flux medium inside F53 (that is F59, and the sprint bound
bites at the pledge rather than at delivery).

### D30 — The discovery surface is reinstated: discovery is not a duplicate of loading

**Decided** Paul Galbraith, 2026-08-10. **Supports** S1, S2, S3; in-force
P3, P4, P7, P27; U-numbers none — the demand is a caller's, and the
question tier's own argument is where a use case for it belongs.

The media-first design demoted `discover_media`, its cache sibling, the
consumable `Discovery`, `load_discovery`, `add_device_for` and the
image-format `default_device` declaration out of S1–S3, on the reading
that the ask-first journey duplicated what a declared `load_media`
already does. **That reading was wrong, and the ruling is reversed.**
The two verbs answer different questions: loading says *make this a
medium under a format I name*, and discovery says *what is this?* — on
no handle at all, with nothing configured and nothing created. A caller
who does not yet know what an artifact is has no format to declare, and
telling them to guess one so the refusal can teach them the answer is
the ask-first journey wearing a worse shape.

**What makes it not a duplicate is now a stated constraint rather than
an observation: discovery holds the claim and builds no cache.** It
opens the artifact, takes the P7 claim, probes for the type, and stops —
no media state, no session cache, no spilled backing. The `Discovery`
stays consumable, so a load takes the open handle out of it: nothing
runs twice and no window opens between the question and the load. The
cache bound is the *load's* declaration and has no meaning at discovery,
so the delivered `discover_media_with_cache` and the bound travelling
into the device with a discovery go — the delivered surface materializes
today, and closing that gap is F67 rather than something this ruling
performs.

Three places carried the demotion and all three are corrected: F55 is
struck, the pledged media-first design's "the question tier is demoted,
not deferred" section is replaced by this ruling, and
[proposed/design/question-tier.md](proposed/design/question-tier.md)
stops describing itself as the demoted successor of a delivered surface.

**What is *not* reinstated is everything that tier still proposes.**
Ranked verdicts, policy templates, and gated derivation chains were
never delivered and stay in `proposed/`, to be argued as one thing. This
ruling reverses a removal; it pledges no extension, and the delivered
surface keeps the shape it has until one is argued.

**Weighed and declined:** leaving the demotion standing and letting the
question tier restore the surface when it is argued (the surface is
delivered and working, and removing it to re-add it later costs every
consumer a migration for a decision already known to rest on a false
premise); and reinstating it as delivered, cache and all (that is the
duplication complaint's one true grain — a discovery that materializes a
medium *is* doing the load's work, and the constraint above is what
keeps the two verbs distinct).

### D29 — What the swept flux-layer design deferred, kept where a design cannot go

**Decided** Paul Galbraith, 2026-08-10. **Supports** in-force P22, P29,
P30; U25, U26.

The remanence flux layer's design served F63 through F66, all now
delivered, so it is swept with the last handle that carried it — a
design is guidance toward work not yet done, and what was done is the
code. Its body described delivered surfaces and goes with it. **Its
deferrals do not**: a deferral is the reason a choice was *not* made,
which outlives the design and belongs here.

Four stand, none of them blocked, none of them pledged:

- **The divergence sidecar** — the reconstruction's account as its own
  text artifact beside the image. The account rides the in-memory report
  until a journey needs the file.
- **Flip-side pooling and the flippy transform's fitted origin** — the
  pipeline's seams admit a second capture group, and the work arrives
  when a flippy fixture does. The repository holds one disk captured
  twice in opposite directions, which is the evidence that would drive
  it.
- **Sector-anchored angle merging and checksum-selected arcs** — the
  anchoring licence was written into the delivered reduction; the
  machinery lands when fixtures demand it.
- **The unguided orchestration** — survey, recognise, rebuild both
  orientations without the caller naming a side. It belongs beside the
  question tier's argument rather than ahead of it.

A fifth is **overtaken rather than deferred**: the served projection as
a general verb (remanence image → flux medium). It now has two callers
— the p64 rendition and the presentation ladder's image entry — and is
still crate-private and still not a general verb, which remains the
right shape until something outside the crate needs one.

**Weighed and declined:** keeping the design file on the strength of its
deferral list alone (planning holds no delivered surface, and the list
is four bullets that fit here); and promoting the four to features
(none is argued yet, and pledging is what argument earns).

### D28 — U23 is withdrawn from the in-force list: its journey runs, but not in the shape it is owed

**Decided** Paul Galbraith, 2026-08-10. **Supports** U23, U25, U26;
in-force P13, P29, P30; F59.

U23 asked how a user accomplishes what it claims, and the answer was
that they do — through a surface that exists for captures alone. The
entry is therefore **moved from root [USE-CASES.md](../USE-CASES.md) to
[pledged/USE-CASES.md](pledged/USE-CASES.md)**, which is a withdrawal
rather than a delivery: it will return to the root list when the shape
below is built, keeping its number.

**What the journey should be**, in four steps, fixing the shape and not
the spelling:

1. `load_media("abc.7z")`, which answers with an **archive medium**,
   because an archive is a medium and its content is a namespace.
2. Take that namespace's files as a collection and `load_media` them —
   the second act, materializing the archive's contents into a floppy
   image through the same verb every other medium arrives through.
3. Get a disk back, reached the way every other medium is reached.
4. Save it as a P64 by naming the destination format.

**Step 1 is built. Steps 2, 3 and 4 are not.** `load_media` takes one
path and no collection; there is no disk kind a capture loads into, the
capture set being its own root outside the device model; and every write
verb the library has is format-specific and hangs on the root that
produced it. Steps 2 and 3 are the media-first fold F59 already pledges,
and are what U25 and U26 narrate one link earlier. Step 4 — **one verb
taking a destination format, rather than one verb per format** — is
pledged nowhere and is what U23 uniquely adds past them.

**Withdrawal was the honest move, not a rewrite in place.** The root
list is an implementation claim and a divergence from it is a bug, so an
entry whose journey the code performs *differently* cannot stay there by
having its prose adjusted to match what was built. That would make the
list describe the code instead of the code answering to the list, which
is the whole of what arming a use case means.

**Two clauses of the withdrawn entry do not survive into the pledge.**
Its claim that the reduction's every input is a named policy input the
caller states was never wholly true — the source-position-to-half-track
map has no policy field and is the profile's declaration (P30) — and the
media-first shape does not want it to be: U25 runs the reduction under
the profile's declared defaults, the caller growing their declaration
only where a family convention cannot decide. What the entry demands
past that stands unchanged: both accounts read before the write, loss in
the source's own terms, provenance that does not overstate itself,
determinism, and refusals that name the rule they broke.

**Consequence for F65.** The gap-first reconstruction's pledge ends
"the selected-observation reduction it succeeds retires with its
delivery", and that retirement was blocked while U23 was armed, because
the armed entry named the mastering profile as an owner. The block is
now lifted in kind but not in fact: pledged U23's *body* still names it,
so the retirement waits on U23 being rewritten around the four steps
above — where the reduction's policy is the profile's, not the
caller's — rather than on nothing at all.

**Step 2 does not fold into step 1, and the reason is structural.**
Step 1's call is already spoken for: it answers with the archive,
because an archive is a medium in its own right. Making the same verb
sometimes answer with the disk inside instead would give one call two
answers chosen by inspecting content, which is the discovery the
declared tier exists to keep out. Materializing a floppy out of an
archive's contents is a second act because it is one, and the caller
taking it is the caller declaring what they have.

**Weighed and declined:** amending U23 in place to describe the built
surface (see above — it inverts what the root list is for); leaving it
armed and treating the mismatch as prose to be clarified (the journey
differs in its shape, not its wording, and no clarification reaches
that); splitting step 4 out as a use case of its own (it has no journey
without steps 2 and 3, and a use case that cannot be walked is not one);
and folding step 2 into step 1 (above — the verb is already spoken for,
and overloading it buys one call at the cost of the rule that the caller
declares and the library checks).

### D27 — Rulings made delivering the uniform archive open

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; the U2 amendment, the P14
amendment, the P19 amendment; in-force P7, P12, P13, P19, P27.

Rulings made folding the archive journey into the storage model. The
delivery itself is recorded by the commit; these are the calls made in
its course, including the two the feature existed to settle.

**The archive slot is visible in its machine's attachment namespace.**
The alternative — a virtual slot kept behind the report — was weighed
and rejected for D23's reason one tier down: it would make the archive
the one device kind a caller cannot see, paid for at every seam that
lists devices, and it buys nothing. What the restriction would have been
written for is already handled by family: a namespace composer passes an
archive device over because an archive has no partition or volume for an
assignment rule to reach, and the mapping's provenance says it did.
`arc0` is an ordinary attachment identity.

**The backing relationship is settled by what the child holds, not by an
outliving rule.** A stored entry is source-backed through the archive's
own claim and a coded one is session-backed in private session storage
(P27) — the delivered split, unchanged. What this feature adds is that
the child *holds* its backing: the claim or the spool is refcounted into
the medium loaded from it, so ejecting the archive, or removing its
device altogether, takes nothing away from a disk already loaded. The
draft's "that machine must outlive the child" is therefore not a rule
the code needs, and stating it would have described a constraint the
implementation does not have.

**The two vantages are two states, not one state with empty fields.**
P14's "families own their representation" is applied at the state tier:
a space-native medium and a namespace-native one are separate kinds
behind one `MediumState`, and every verb that addresses a space passes
through one accessor that refuses on the other **by name** — naming the
vantage, not failing further in. That is what made the archive medium
additive rather than a rewrite of the block state, and it is why an
archive reports no phantom volume (D26) without anything having to
suppress one.

**A path names a file.** The `archive[/entry]` syntax is gone from the
medium journey: an entry is reached through the namespace its archive
bears and loaded from the file view that names it, which is the same
journey every other medium takes. Two consequences were accepted
deliberately. Loading a one-entry archive no longer silently opens the
entry — the old convenience guessed, and the namespace asks instead. And
the ambiguity refusal for a many-membered archive is gone with the guess
that needed it.

**The capture-set adapter keeps `captures.7z/subtree`.** That spelling
names a subtree of *members* read as one logical artifact — one disk per
stream per head per step position — not one medium named inside an
archive, and the flux family reaches it through its own type as P13
requires. It is not the syntax this feature retired.

**The file-view load lands as `File::discover`, and the claim it mints
from is bounded.** D24 deferred the third load form to whichever feature
minted the view; this is it. It answers with the delivered consumable
`Discovery`, so the nested artifact travels through exactly the path
`load_discovery` already served — no second load form, and the claim is
the archive's own, held continuously from naming the entry to loading
it (P7). The claim is stated as an **archive entry**: a file on a
volume-backed filesystem is refused by name, because backing a medium
from a cluster chain is new capability rather than a spelling, and P3
would rather refuse than half-answer.

**In-force P19's serialized-artifact provider form dissolves**, which is
what D25 deferred to here. A medium may bear its namespace directly —
an archive, a flat catalog on an unpartitioned disk — its grammar being
a P12 adapter at that seam. The composer's three constraints stay in
P19: P35 is still unbuilt, and D25's reasoning for leaving them is
unchanged.

**Weighed and declined:** recognizing an archive by its leading bytes
rather than by the extension its grammar answers to (a ZIP's signature
sits behind whatever stub precedes it, so signature recognition would
refuse the self-extracting archives the catalog reads today); giving
`identify` an archive-medium *media* layer beside the grammar layer (the
layer kind for a medium is spelled `physical-media`, and a virtual
medium has no physical anything — the media type is answered where it
belongs, on the handle); and keeping `Archive` as a read-only listing
beside the medium (two ways to walk one namespace is the second
interface the one-node claim exists to refuse).

### D26 — Volume and filesystem are two traits on one object, not two types

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; P17, P18, P19.

A ruling on the shape F48 delivered, made while weighing whether the
filesystem node could be embedded in the volume as the medium was
embedded in the device.

**The type merge was refused first, and for a good reason.** Embedding
`Filesystem` into `Volume` fails the test the device/medium merge passed.
That merge worked because no caller ever holds a medium outside a device;
this one breaks because two of the three providers have no volume at all —
an archive's content is a namespace with no space beneath it, and a
machine's composed namespace is assembled over several filesystems. A
merged type would have had to invent a phantom volume for each, which is
the invention refused when a zip's byte extent was ruled *encoding* rather
than a model space. The reverse merge fails too: absorbing volume into
filesystem makes swap and unformatted space unrepresentable, and P19's
honest absence is the 0 in the 0..1.

**Traits dissolve what the type merge could not.** One object,
`StorageSpace`, implementing addressable I/O, namespace I/O, or both,
carries every case without a phantom in either direction — and it is the
rule already applied one level up, where a device's vantages are
capability traits a family implements as it claims rather than a
hierarchy. What the prose asserted — one node, two vantages — the type
system now carries, and the 0..1 becomes trait presence with F48's
delivered `no-namespace` refusal already fitting.

**The capability, not the tidiness, is why it is a feature.** Addressed
reads today are whole-medium only; a volume's boot sector, its
unallocated extents, and the bytes behind a listed file all require
computing offsets against the medium by hand. The addressable trait
closes that on the object that already hands over the files.

**An earlier flag was withdrawn.** It had been recorded that F48 should
have spelled selection `device.filesystem(volume_id)`, on the ground that
the handle rule makes volumes values rather than handles. That reasoning
assumed a single device: a volume spanning partitions across several
devices is not a selector on any one of them, so it needs a handle. F48's
shape was right, and the gap it leaves is scope rather than handle-ness.

**The machine is the scope for anything spanning devices, and that stays
unpledged.** A composition is reached from the smallest scope that can
compose it — a namespace on one device's medium from that device, a
volume spanning that device's partitions from the device, a volume
spanning devices from the machine, and a namespace composed over several
filesystems from the machine, which is where P35 already puts it. The
rule is recorded here because it settles where future compositions hang;
the surface is not pledged, because multi-device volume composition is
not claimed (P17 defers it, U14 is proposed), and a machine-level
enumeration added today would flatten over devices without delivering a
capability — surface ahead of demand.

**Two boundary readings settled in passing.** A multi-partition volume
with no filesystem is ordinary and real — an LVM logical volume formatted
as swap, a spanned volume never formatted, raw database extents across
several disks — so the 0..1 needs no special case at the composed level.
A LaserDisc's analog program is **not** such a case: it is not a volume
at all, frames and time codes addressing program content rather than
storage. The test is whether it is an addressable space of the kind a
filesystem could occupy, which swap is and an analog program is not. One
disc can be both, since LV-ROM digital data carried in a program-channel
mapping is a genuine addressed extent that may bear a filesystem
(proposed F22). The 0 in 0..1 is for a space that could bear a namespace
and does not — never for content that was never a space.

**Weighed and declined:** merging the types in either direction (above);
naming the object `Volume` and accepting the stretch (an archive's
namespace typed as a volume undoes the strictness the vocabulary rulings
bought); leaving it as two types with the hop (the prose would keep
claiming one node while the surface showed two, and the addressable
capability would still be missing); and pledging the machine-scope
surface now (above).

**Folded into:** F52 in [pledged/FEATURES.md](pledged/FEATURES.md) and
the storage model design.

### D25 — The namespace node lands whole; the P19 amendment lands as far as the code honors it

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; the U2 amendment, the P19
amendment, P35; in-force P10, P18, P19, P23, P27.

Rulings made delivering the `Filesystem` node and the container
retirement it pays. The delivery itself is recorded by the commit; these
are the calls made in its course, and the first is the one a later
reader most needs.

**The P19 amendment could not fully arm, and landed in the part that
could.** The amendment says P19 keeps the convergence claim and loses two
things: the "serialized-container adapter" provider form, because an
archive is a medium whose grammar is a P12 adapter at the namespace seam;
and the namespace-mapping composer, which moves to P35. Neither loss is
available yet. An archive is *not* a medium until the uniform archive
open lands, so deleting that provider form now would unbind the journey
the code actually takes today; and P35 is pledged, not armed, so moving
the composer's three constraints out of P19 would delete from the
in-force list a rule the code implements and honors, leaving it bound by
nothing. **The root lists are implementation claims**, so what landed is
what the code honors: the retitle, the retired word purged from P19 and
from P23's active-layer row, and the amendment's positive claim — one
file-access interface however reached, with file access living on one
node and nowhere else. The rest is F49's, which is why that feature now
says so. The pledged scope-of-claim amendment (the coverage account) is
untouched and unclaimed: the delivered node produces no account, and
nothing here says it does.

**"Container" was retired into three different words, because it was
doing three different jobs.** At the P19 seam it becomes **namespace**,
which is the vocabulary ruling's own word and the one the node is named
for. In an identification it becomes a **layer** of the artifact's
nesting — `Layer`, `LayerKind`, `Identification.layers`,
`remanence_layer_*` — with the doc comments saying in as many words that
this is a different axis from P13's authoritative layer and P23's active
layer, because two disjoint enumerations sharing a word is the ambiguity
the retirement exists to end. On a region role it becomes **structure**,
an extended partition being a structural region. And on
`Error::InvalidImage` the `container` field becomes `format`, which is
what it always held: the seam a refusal is attributed to. What survives
is the word where it is somebody else's — an *image container format* is
the industry's term for qcow2, VDI and P64 — and the surviving uses were
audited one by one rather than swept.

**A `Volume` handle exists, and it is still not a thing to hold.** The
storage model rules volumes values, passed as selectors and never held;
the feature's own spelling is `device.volume(id).filesystem()`. Both are
satisfied by a borrowed selector: `Volume` carries the identity the
report issued and the extent, borrows the device it came from, and cannot
outlive it. It accepts no ordinal, because no format defines one.

**An entry declares what the node's vocabulary has no field for.** In-force
U2 claims the real names, sizes, **dates and flags** of an HDOS catalog,
and the node's common `Entry` names only the first two. Rather than keep a
second file type beside it — which is the "one file-access interface"
claim abandoned at the first filesystem that records more than three
facts — an entry carries `EntryFact`s in the recognizing filesystem's own
spelling and order: HDOS declares its catalog date, its flag letters, its
sector count and the raw values behind the readings. This is the flux
layer's two-outcome rule at the node's surface, and it is why the
standalone HDOS reader could be deleted rather than kept.

**"And nowhere else" reached the free functions too.** `list_hdos_files`,
`read_hdos_file` and `HdosFile` took a byte slice and belonged to no
node; keeping them would have left a second way to walk a namespace
outside the type that claims to be the only one. They are deleted from
all three surfaces and the reader is private behind the node.

**The recognizing adapter opens what it recognized.** The resolver needs
to reach a namespace a medium bears directly, and the obvious route —
read the filesystem id the catalog returns and `match` on it — is the
string-named rule in orchestration that P12 and P18 keep out. So the
catalog's adapters gained an `open`, whose default is a refusal naming a
namespace this release recognizes and does not read; CP/M is that case
today. **The lookup is bounded** by the byte count the HDOS reader
already declared, said once for the seam: a medium composing no volume
and larger than it is a named absence rather than a full scan of a
gigabyte (P27).

**A refused recognition answers with its own refusal.** Where a volume
composed and its filesystem seam attempted a recognition and refused, the
node hands that seam's error back — category and rule intact — instead of
a coarser "bears no filesystem" of its own. The seam that owns the
refusal already carries what explains it (P4, P10), and replacing it
would tell a caller less than the inspection report already holds.

**Weighed and declined:** landing the P19 amendment whole and arming a
narrowed P35 in the same act (P35's own claim is the machine namespace,
which nothing builds yet, so arming it would assert a node that does not
exist — and narrowing a principle at the moment of arming is a bigger
ruling than this feature's course); leaving the composer's clauses in P19
with a note that P35 will take them (a pointer to unbuilt work inside an
in-force principle is planning prose in the one place the project keeps
free of it); keeping `HdosFile` beside `Entry` so U2's dates and flags
had a typed home (two entry types is two interfaces, and the declared-fact
route was already the project's answer for a fact with no named field);
naming the identification's records `Recognition` (taken by the drive-profile
seam) or leaving them `Container` for F49 to rename (F48 names the C symbols
explicitly, and renaming the C mirror while the Rust type kept the retired
word would split one vocabulary across two surfaces); and giving the
resolver a whole-medium scan with no bound, which is the P27 violation the
HDOS reader's existing bound was written to prevent.

**Reopens if:** the machine namespace lands and P35 arms — at which point
the composer's three constraints move out of P19 as the amendment says,
and this entry's first ruling is spent.

### D24 — The file-view load waits for the node that mints the view

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; the P19 amendment, P35; in-force
P7.

F51 said `load_media` accepts "a path, a file view, or a discovery", and
two of the three landed with it. A **file view** is not something this
release has: the `Filesystem` node that mints one belongs to F48 and the
recursion journey that reaches an artifact through it belongs to F49, so
delivering the third form inside F51 would have meant minting another
feature's node to have an argument type for it.

Nothing a caller was promised is missing meanwhile. The path form
already carries the nested artifact — `archive/entry` resolves under the
same claim and loads into a device of its own — so what is deferred is
the *typed* spelling of a journey that works, and it lands with the type
rather than ahead of it.

This is recorded because F51's number retires with its delivery: without
an entry the deferral would leave no trace at all, and the next reader of
F48 or F49 would have nothing telling them the form is theirs to finish.

### D23 — Rulings made pledging the storage model

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-05. **Supports** S1, S2, S3; P7, P14, P19, P27, P32, P35.

The promotion itself is recorded by the commit that moved the documents,
not here. These are the rulings made in its course, which the moved text
settles only by being written that way.

**The pledge is scoped to the families claimed today.** The model
describes shapes other families would take, and those illustrations
pledge nothing: optical and tape media (proposed P24, P26) and volumes
composed across several regions (P17's deferred future) stay proposed,
and the design document says so in its own scope paragraph. The trim was
not cosmetic — a pledged item resting on a proposed one is pledged too
early, so the media-kind table, the two trees, and the spine's volume
line were narrowed until nothing pledged depended on anything proposed.

**F47 was split at the pledge and its number retired.** The sprint bound
bites at the pledge, and one feature carrying the two-act access path,
concrete device families, discovery, format-declared defaults, and the
one-step convenience was two. It became **F50** (the two acts and the
lineage-bearing family catalog) and **F51** (discovery, the declared
default, and the convenience over it), F51 needing F50. README's rule
for features governs: a split retires the parent's number and issues
fresh ones.

**The CHS/LBA clause was wrong and is struck.** The draft read "CHS and
LBA hard drives are separate partitionable *families*", which
contradicts the already-pledged P32 amendment: a device declares an
**addressing nature** when it is created, and that amendment
*deliberately declines* to confine a family to one nature, because a
hard drive answers both depending on the command issued. The owner's
own words had been "separate partitionable **devices**" — the pledged
mechanism exactly — and the drafting drifted. Both the new amendment and
the design document now defer to the pledged one instead of restating a
rival rule.

**One storage handle, two model nodes.** The device/medium split was
argued three times and survives as *model*, because U23 and D19's three
facts, U22's letters, and U24's flippy each need two nodes. It does not
survive as two *handles*: a caller never holds a medium outside a device
— discovery returns a discovery, every load goes into a device, a child
artifact gets its own device — so `Disk` merges into `StorageDevice`,
which homes the media state of whatever occupies it. The facts stay
attributed on the one handle, which is what keeps D19's pair sayable:
the medium states an index hole, the drive states no sensor for it.
`get_sector`-on-device-or-medium, an open question at the time, dissolves
rather than being answered — evidence the seam was an artifact of two
handles rather than of the model.

**A session holds machines; a machine holds devices.** Pledged P32 made
the session the device set. Nesting broke that: an archive on the host
was never part of the machine whose disk it contains, so reconstructing
that machine from `games.zip/boot.h8d` wants an archive device in one
machine and a drive in another. Inserting the machine lets each device
set hold only its own machine's configuration, while the session keeps
the meaning the principles already give it (P7 claims, P27 budget and
private storage) and owns every machine's lifetime, so a stored archive
entry may back a drive elsewhere in the same session without a lifetime
question. P32's "nothing groups sessions into a machine" is
untouched: the containment runs the other way.

**A session has one anonymous machine, and a machine carries an
identity.** Devices may be added to a session directly, landing in that
machine — one machine, not one conjured per call, so the unanswerable
"which device?" that killed the media-first one-step does not arise. The
anonymous machine is the one whose identity is **null**: the same kind of
thing as every other machine rather than one distinguished by a
behavior, which is also what U16's installation selection and P35's
provenance need in order to name a machine at all. It holds no
privileged position — it is not "machine zero", no attachment order it
carries is more meaningful than any other's, and moving a device from it
into a named machine is a reconfiguration rather than a rename.

**Every machine composes a namespace, the anonymous one included, and
provenance is the guard rather than a refusal.** Restricting the
anonymous machine from composing was weighed and rejected: it buys
nothing P35 does not already provide, since a derived mapping travels
with the machine facts and the applied rule and is never evidence. A
caller who adds two unrelated floppies and asks for letters gets a
deterministic answer stating exactly what produced it — surprising to a
naive caller, perhaps, and never dishonest. The archive case such a
restriction would have been written for is handled one level down by
family: an archive device has no partitions or volumes, so an assignment
rule never reaches it, and no machine-level rule was ever doing that
work. Uniformity is the other half — a restriction would have made the
anonymous machine behave unlike every other, a special case paid for at
each seam that touches machines. What survives as description rather
than rule is the usage: **the anonymous machine is where artifacts are
opened, a named machine where one is reconstructed.**

**Further conveniences are deliberately not pledged.** The explicit walk
is what the model owes past the anonymous machine, which is a structural
rule rather than a shortcut. The room is real — a default machine for a
single-machine session, a filesystem straight from a session — and each
is its own later proposal, weighed as the machine-level one-step was:
admissible where it declares, refused where it would guess. The
media-first machine-level spelling is dropped rather than kept, since
with one storage handle it would return the same device its device-first
twin does.

**Weighed and declined:** pledging the model whole, illustrations
included (it would have made pledged text rest on proposed principles);
keeping `Disk` as a `Medium` type beside the device (no journey produces
one, and the delegation it would require is the merge with extra
ceremony); renaming `Session` to `Machine` as the earlier draft had it
(the nesting case needs both words, and both already carry their meaning
in P7, P27, U22 and P35); loading a nested entry into a named machine
that also holds the host's archive (it would put a host-side wrapper in
an emulated machine's configuration and hand its composer a slot to
letter); an anonymous machine forbidden to compose a namespace
(provenance already states what a mapping was derived from, and the
restriction would have made one machine behave unlike the rest); and
giving the anonymous machine a reserved identity rather than a null one
(a name nobody chose, citable in provenance as though it had been
declared).

**Reopens if:** a claimed family needs a medium handle no device holds —
the mastering path is the candidate, since a `MasteredMedium` is a medium
in no device today, and this ruling deliberately leaves the flux handles
where they are.

**Annotated on delivery (F53, 2026-08-10): the "one storage handle"
ruling above is reversed, as the media-first storage model's ledger said
it would be.** The medium is now the pool-owned handle a caller holds and
every content verb answers on; the device slims to a slot, its family and
a link, with `insert`/`eject` the one edge between configuration and
state. D23's actual worry — lifetime questions from media held outside
the session — is answered structurally by the pool rather than by
refusing to hand a medium out. The rest of this entry stands: the machine
tier, the anonymous machine, and the reasons the device/medium split
survives as *model* are untouched, and `Disk`'s merge into
`StorageDevice` was the step that made this one sayable.

### D22 — P27 splits: the resource rule keeps the title, thread invisibility becomes P34

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** P2, P23, P27, P34.

The last of the three principles D20 reported as resisting compression,
and the same test D21 applied: two rules under one heading that fail
independently are two principles.

**The two rules, and the test.** The resource rule — sized by the
operation, the bounded working set, source-backed and session-backed
state, the residency classes, streaming everywhere — makes the testable
claim *peak memory bounded independently of source size*. The
concurrency rule — threads may predict, prefetch, and offload under four
invisibility rules — makes the testable claim *results, evidence, and
refusals identical at any thread count, including none*. A whole image
loaded resident with zero threads violates the first and honors the
second; a failed speculative read that reports its error violates the
second while memory stays bounded. Neither implies the other.

**The second rule is a determinism claim, not a memory claim.** It is
the rule any future concurrency must obey — parallel decode, deriving
layer extents ahead of demand — and P23's cache tie already cited it as
a distinct thing ("under P27's speculation rules"). Keeping it inside a
principle titled "memory holds a bounded working set" made the
determinism obligation citable only through a resource principle whose
title says nothing about it.

**P27 keeps the resource rule and its number; the concurrency rule is
P34.** The title match settled the direction: "sessions stream; memory
holds a bounded working set" describes the resource rule exactly. Of the
84 code citations of P27, nearly all are resource-half; the concurrency
sites are concentrated — the offload worker and speculative install in
`cache.rs`, the predictive reader in `source.rs` and `disk.rs` — and
were widened to P34 in the same change. The budget stays P27's, and
P34's demand rule spends it: a cross-reference, exactly as P23's caches
sit under P27's budget.

**Corrected in passing:** D21 stated that three P23 citations sat "in
released CHANGELOG entries". Every P-number citation in the changelog
sits under `Unreleased` today — the released headings hold none — so
that leg of D21's numbering argument was factually wrong. The ruling
stands on its other legs (77 citations across 21 files, the code's
citations being the state half), and per this record's own rule the D21
entry keeps its spelling; this note is the discovery.

**Weighed and declined:** leaving P27 whole (defensible as "the threads
exist only to spend the budget", but that reading makes the
determinism claim a sub-clause of a resource principle, and the two
claims are independently violable and independently testable); the
subsection compromise — one number, two subheadings — (cheaper, but
leaves two independently violable claims citable only as one number,
which is the ambiguity the P-sequence exists to prevent).

**Landed as:** P27 515 → ~370 words with a two-line pointer to P34; new
P34 at ~200 words in root [ARCHITECTURE.md](../ARCHITECTURE.md); four
comment citations widened in `cache.rs`, `source.rs`, and `disk.rs`; a
CHANGELOG entry under Unreleased beside the P23 one. SEQUENCES advances
P to 35 and D to 23. No S1–S3 surface is touched: the split renumbers
rules without changing what any of them claims.

**Reopens if:** either half is found to have lost a binding clause — the
clause returns, since this ruling moved rules without changing them.

### D21 — P23 splits into what an active layer is and how it changes; generate-flux was P29 all along

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** P13, P22, P23, P27, P29, P30, P33.

The first of the three principles D20 reported as resisting compression.
P23 was 2101 words before that pass and 1147 after, which is what a
principle looks like when it is two.

**Two rules were sharing a heading, and they fail independently.** What an
active layer *is* — one per independently mutable instance, the closed
six-member vocabulary, what cannot be one, one layer per instance in a
nested graph — is violated by two mutable copies of one state, or by a
session active at a representation outside the vocabulary. How an instance
*moves* — the ladder, the initial choice, materializing downward,
atomicity, no return — is violated by lowering an LBA device, by a partial
descent, or by the layer rising when a lower presentation closes. Neither
failure implies the other, which is the test that made this a split rather
than a trim.

**P23 keeps the state half and its number; the transition half is P33.**
The direction was settled by evidence, not preference. P23 has 77
citations across 21 files, and the code's are almost entirely the state
half — "the layer active for this composition" in `report.rs`, in the
generated C header and its Rust origin, and the module headers of
`hardware_bitstream.rs` and `encoded_bytestream.rs`. Three of the
citations are in released CHANGELOG.md entries, which AGENTS.md forbids
editing. Retiring P23 and issuing two fresh numbers — the rule README
states for *features*, whose handles evaporate on delivery — would have
orphaned all of it, and vision handles are permanent for exactly this
reason. Only `c1541_presentation.rs` and its integration test needed
their citations widened, and both were already citing P30 beside P23.

**Generate-flux was P29 restated, and D14 had already said so.** P23's
four generate-flux bullets map one-to-one onto P29: "ambiguity remains
ambiguity unless an explicit deterministic policy" and "a missing or
contradictory rule refuses" are both P29's *a reduction that no policy
names is a refusal, not a default*; "only detail absent from the source is
synthesized, with its provenance retained" is *the result is derived and
says so*; and "every known timing preserved at its known fidelity" is the
declared-loss account. D14 had already ruled that mastering's destination
"may be a new artifact or an active layer inside the session… Only the
destination differs; the inputs, the plan, and the declared-loss account
are the same." So the bullets were deleted and P33 cites P29 instead.

**P29 widened to match what it was already governing.** Its opening said
mastering derives "a new artifact"; it now derives a new *representation*,
with the destination named as the only thing that varies. "The loss is
declared before the write" becomes "before anything is produced", and the
interruption clause reads "a complete destination or none", because an
in-session destination is written to no file. No requirement of P29
changed — the principle simply stopped describing only half of its own
scope.

**Why this was invisible to the D20 pass.** The restatement was wearing
bullets. D20's rule catches a principle that re-explains a neighbour in
prose; it did not catch one that restates a neighbour's requirements as an
apparently operational checklist. That is the shape to look for in the two
principles still outstanding.

**Weighed and declined:** retiring P23 and issuing P33 and P34 to the two
halves (orphans 77 citations, three of them in immutable released
changelog text, to buy a symmetry nothing needs); giving the transition
half to P13 (P13 governs what the *artifact* records and can persist,
P33 what a *session* currently carries — the same distinction P23's own
authoritative-versus-active paragraph draws, and collapsing it here would
undo that); folding the transition half entirely into P29 rather than
issuing P33 (mastering governs the honesty of a descent, but not the
ladder, the initial-layer choice, the atomicity of rebinding, or the
one-way rule, none of which are reductions); and keeping the generate-flux
bullets under P33 as operational detail (that is the duplication D20 just
ruled against, and D14 had already collapsed the distinction they rested
on).

**Landed as:** P23 1147 → 698 words, new P33 at 482, P29 442 → 482, in
root [ARCHITECTURE.md](../ARCHITECTURE.md); citations widened in
`c1541_presentation.rs` and its integration test; a CHANGELOG entry under
Unreleased, because P-numbers appear in released entries and a reader
reconciling them needs to know P23 narrowed. SEQUENCES advances P to 34
and D to 22. No S1–S3 surface is touched: the split renumbers rules
without changing what any of them claims, and the C header's own P23
citation stays correct.

**Also corrected in passing:** `c1541_presentation.rs` described a
container as holding a medium "at rest", a term D2 retired from
library-side prose. The line was being edited for its citation anyway.

**Reopens if:** P23 at 698 words is found to be two rules again — the
vocabulary table is a fifth of it, and the remainder is one claim.

### D20 — A principle in force states the rule; the argument lives here

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** (none) — a form ruling about how the in-force
list is written. No numbered vision entry demands it, and it changes no
claim any of them makes.

**The observation that prompted it.** The nine principles written first
average 127 words. Everything from P10 onward averages 540, and P23 had
reached 2101 — an entire treatise per rule. The list had stopped being a
list of rules and become an essay collection, which makes it unreadable
as the thing it exists to be: the place a triage decision looks up what
binds.

**The rule adopted.** A principle in force states its claim, what it
binds, and **at most one line of why**. That is not an aesthetic
preference; it is the shape the first nine already had, and P5 is the
proof that a real principle survives it in 38 words.

**Three things a principle no longer carries, each because it has a home
that will not drift:**

- **The argument that settled it** — this file. Most of what was removed
  was already *here*, in duplicate: P30's "an angle, never a byte" was
  D12's sentence verbatim, and P23's capture/medium reasoning was D14's.
  A second copy in the norm is not a safeguard, it is a second thing to
  keep in step. Where the argument is worth finding, the principle now
  cites the D-number.
- **The enumerated sets a claim ranges over** — the code, which is the
  norm ([ARCHITECTURE.md](../ARCHITECTURE.md) "The application
  surfaces" says so). P10 listed its ten error categories and the DOS
  8.3 rule identities; P28 listed its conditions and which
  interpretation the gate is armed for; P14 listed the enrolled media
  types. Each principle keeps the rule that the set is enumerated, owned
  by its seam, and part of that seam's surface. What it drops is the
  transcript, which could only ever be a stale copy of an enum.
- **A restatement of a neighbouring principle** — a cross-reference.
  P23 re-explained P13, P22 and P27 before reaching its own subject.

**What was deliberately not cut.** Every normative clause, including the
ones wearing rhetorical clothes. The one class nearly lost on the first
pass was the **surface limit** buried at the end of each "Knock-on
requirements" section — "creates no public evidence iterator", "the flux
floor is not a public interface", "adds no multi-device opening". Those
sections were otherwise cross-references, and the limits were restored to
the body of P21, P22, P29 and P30 once the review caught them. A negative
claim about the surface is as binding as a positive one and is easier to
delete by accident.

**Planning prose is untouched, deliberately.** Under `planning/`,
precision and accuracy outrank brevity: an argument takes the length the
argument takes, and this file is the clearest case. The rule bites only
where a principle is *in force*, because that is where prose stops being
an argument and becomes the thing the code is measured against.

**Three principles resisted compression, and were reported rather than
re-cut.** They are the finding, not a failure of the pass, and each is a
candidate split rather than a candidate trim:

- **P23 (2101 → 1147 words)** reads as three rules sharing a heading: one
  active layer per independently mutable instance and the vocabulary it
  ranges over; how the initial layer is chosen and how a transition
  between layers behaves (generate-flux and its atomicity); and the
  layer/cache tie, which was a restatement of P27 and has now been folded
  into P27 where it belongs. The first two are separable and were not
  separated here.
- **P19 (805 → 543)** carries the file-container seam and, bolted to it,
  the namespace-mapping composer with its own three constraints. The
  composer derives a mapping where a system persisted none, which is a
  different act from exposing a namespace.
- **P27 (602 → 515)** carries a resource rule and a concurrency rule. The
  four rules that keep threading observationally invisible are a
  self-contained claim about behavior, not about size.

**Weighed and declined:** splitting those three in the same act (the
splits are principle amendments in their own right and each deserves its
own argument, which is exactly what this pass is trying to stop being
smuggled into an edit); a companion `RATIONALE.md` beside the principles
(a third home for prose, competing with both the norm and this file, and
D11 already refused that shape for delivered designs); keeping the
enumerated sets with a "may be stale" caveat (a caveat on a copy is an
admission the copy should not exist); and compressing the pledged and
proposed principle drafts in the same pass (they are planning prose,
which the ruling above deliberately exempts — they take this shape when
they arm, not before).

**Landed as:** root [ARCHITECTURE.md](../ARCHITECTURE.md), 1172 → 839
lines, with the rule itself stated under "The architectural principles"
so the next entry is written to it. No S1–S3 surface is touched and no
principle changed what it claims, so no code, binding, test, or changelog
entry moves with it.

**Reopens if:** a principle is found to have lost a binding clause — in
which case the clause returns to the principle, since this ruling removed
argument, transcript and restatement only.

### D19 — A media profile holds what the article is, and nothing that was recorded on it

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** S1, S2, S3; P3, P12, P14, P22, P30, P32.

Rulings made in P14's course, which its own text settles only in
principle.

**The boundary is drawn at "recorded".** P14 says a profile holds
"passive compatibility facts", and the contested reading is what that
excludes. It excludes everything a recording put on the medium: density,
encoding, track and sector counts, and the geometry an image format
declares all stayed where they were, and what moved into the catalog is
the article — form factor, coercivity, track density, hole topology, and
the sense of the write-protect mechanism. The test applied was whether
the fact is true of a blank disk in its sleeve.

**How many surfaces the medium certifies is not declared at all**, and
this is the sharpest case of that test. It is a genuine passive fact, and
nothing the library ever holds establishes it: a capture records what one
head saw, an image records what was written, and neither says whether the
physical disk was sold certified for one side or two. Declaring it from
the drive's recorded-surface count would be the drive's fact wearing the
medium's name. So the medium is silent, and P30's `Surfaces { recorded }`
and the image format's `sides` continue to answer the question they
actually answer.

**"Hard disk" and "floppy" leave the medium's vocabulary.** They were the
image-format descriptor's `media_kind` string, and central identification
code `match`ed on them to choose a display name — a string-named rule in
orchestration, which is exactly what P12 keeps out of it. A virtual
disk's medium is logical-block media; *hard disk* is the device family a
session's slot carries under P32, and the P32 amendment's rule that a
device's addressing nature never reaches the media instance is the same
rule one step over. The public spelling follows: `media_kind` becomes
`media_type` and carries an enrolled identity rather than a word.

**A drive profile declares the medium its family is served**, which is
not the violation it can look like. P14 forbids a *media profile* from
containing hardware behavior; a drive declaring which article it accepts
is a compatibility fact of the family, the same class as its rotation or
its density map, and the catalog entry it points at knows nothing about
the drive. It is also the only honest source for a mastered medium's
media type: a capture does not record what disk it was, so the name comes
from the family's declaration with provenance, as every other P29 policy
input does.

**The seam is crate-private**, as the drive-profile seam beside it is. A
media type reaches a caller as the name a medium answers with — in an
identification's physical-media layer and in the layered report's device
record — and the facts stay behind that name until something outside the
library needs one. Nothing about P14 requires a public flexible-media
fact today, and publishing one would fix a schema on the strength of two
enrolled entries.

**Weighed and declined:** one universal media schema with per-family
fields left empty (P14 refuses it in as many words, and the two claimed
families share no fact — a coercivity is meaningless for a logical-block
medium and a block size for a disk); leaving the block family's medium
unnamed, as `media_kind: None` left a raw image (a medium that cannot say
what it is makes P14 conditional, and block-active state *is*
logical-block media — that much the authoritative layer establishes);
letting the caller declare a medium's type at attach (nothing in the
delivered stack needs it, and a caller-asserted fact would have to travel
as provenance rather than as a declaration, which is a P29-shaped
surface built ahead of a demand); enrolling 8-inch and 3.5-inch entries
to prove the family generalizes (unused declarations no test measures,
and the 8-inch write-protect sense is inverted from the 5.25-inch one,
which is exactly the kind of fact to declare when a format needs it and
not before); and holding the media type only on the image-format
descriptor rather than on the medium (a medium is state *between* image
formats and drives, so reaching through the format to ask what the
medium is inverts the direction the principle draws).

**Reopens if:** a claimed family needs a fact the delivered schema cannot
hold, or a caller is found needing a medium's facts rather than its name.

### D18 — A VDI parent is searched for by identity, because the format records no path

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-08-04. **Supports** U6, S1; P3, P4, P6, P7.

A scope call made in F41's course, which its own text could not carry:
**the VDI format records the parent's identity and no path at all.** F41
was written as "names its parent by the parent's own identity rather than
by path alone", which reads as a path plus a check. There is no path. The
producing hypervisor resolves the identity through its own machine
registry — an XML document outside this library's claim, and one this
library will not acquire a reader for to open a disk image. So the choice
is not *how to check a resolved path* but *how to resolve at all*, and the
delivered answer is a **search by identity**.

**The search is bounded and named**: the directory holding the child, then
the directory above it — the layout this format's tooling produces, where
differencing images sit in a subdirectory of their own and the base image
stays in the folder above. In each, the file *named* for the identity is
nominated first, in both spellings the tooling writes it with, because
that is how a differencing image is named. Failing a nomination, the VDI
files beside it are examined and the one declaring the identity is the
parent.

**Nomination is checked, not trusted**, which is where F41's sentence
lands intact: a nominated file whose identity does not match is a refusal
rather than a fallback to searching, so a substitute standing where the
parent should be is never silently read in its place. Two matches in one
directory is a contradiction and refuses; none anywhere is the
missing-parent refusal, naming the identity looked for and every candidate
it could not examine (P4).

**A candidate that cannot be examined is not a failure of the open.** A
scanned file another process holds against the P7 claim, or one that is
not a VDI of the claimed major version, is recorded and passed over rather
than failing the chain — it was never established to be in the chain. A
*nominated* file is different: it is the parent by name, so contention on
it fails the open as P7 requires. Without that split, one unrelated locked
image in a directory would refuse an open that has nothing to do with it.

**Identity also replaces the path-visited cycle check** qcow2 uses. The
members' declared identities are what the chain carries, so a cycle is an
identity already in the chain — which catches an image naming itself as
squarely as it catches two naming each other, without canonicalizing a
path to find out.

**What this costs, stated rather than hidden:** a differencing image whose
parent sits outside those two directories does not open, and says so by
name. That is the missing-parent refusal F41 already enumerates, and the
alternative — widening the search until it finds something — is the
substitute this decision exists to refuse.

**Weighed and declined:** resolving only by the nominated name and never
searching (deterministic, and it cannot find a base image, which is named
by a person and not after its identity — it would have shipped a feature
that resolves snapshot-over-snapshot and fails the common case); reading
the producing hypervisor's machine registry to get a real path (a second
format, an XML reader in a crate that is deliberately dependency-free, and
a machine-configuration document this library has no claim over); taking
the parent's path from the caller through a new surface verb (it is
defensible, and it contradicts F41's "the top image opens" — the caller
would have to hold what the format was supposed to say); searching
recursively from the child (unbounded, and every directory added makes an
accidental identity collision likelier); and checking the parent's
modification stamp beside its identity, which the format also records
(it detects a parent changed since the branch, and F41 enumerates neither
it nor a refusal for it — a claim to widen deliberately, with the evidence
of a real chain it would have rejected, rather than in passing).

**Reopens if:** a VDI is found that records a parent path after all, or
the search is measured refusing a layout the format's own tooling
produces.

### D17 — A design document's purpose ends at delivery

**Decided** Paul Galbraith, 2026-08-04. **Supports** (none) — a records
ruling; no numbered vision entry demands it.

D11 held that a companion design survives the feature that carried it.
That is true of the *design* — the shape survives, embodied in the code,
which is why a handle can evaporate without losing anything. What it
authorized was the retention of a *document*, and that does not follow.
Its design-retention holding is overruled; the rest of D11 stands.

D11's positive argument — that the code implements the contract but is
not a readable statement of what a future provider must satisfy — names
a real need and puts it in the wrong place. Prose a future implementer
must satisfy is a normative specification, or it is a principle. What
the retention cost: nine further designs restated themselves as
permanent residents on a reason D11 never gave, that no S1–S3
specification has shipped, and one delivered design's caller-flow
example went on naming an entry point a later feature had retired.

**Weighed and declined:** narrowing D11 to its stated case, keeping a
design that touches no surface and states a contract for a future
provider (that need is a specification or a principle, and admitting
the category at all is what the nine claimed); an archive location for
delivered designs (a third mechanism, for content the code already
holds authoritatively); and maintaining each delivered design against
the code it describes (it sets prose to compete with the norm and
schedules the drift rather than ending it).

**Reopens if:** a delivered design is found to carry something that is
neither the code, a principle, nor a decision — that would name a gap
in those three rather than a reason to keep the document.

### D16 — NIB enters at the flux medium, with synthetic timings, to keep one ladder

**Decided** Paul Galbraith, 2026-08-03. **Supports** P13, P22, P23, P29, P31.

**NIB and NBZ enter the flux family, materializing into a flux medium whose
pulse timings are synthesized.** A pulse's position is computed from a bit
index and a declared cell width; nothing about it is recorded evidence, and the
flux medium's model already refuses to call it so, since every pulse names what
put it there. In-force P22 governs the rest unchanged — synthetic provenance is
retained, and protection, weak regions and timing evidence the source never
stored cannot be reproduced from it.

**What settles the rung is a characteristic, not a convenience.** The flux
layer's defining trait is that a rotational recording's start and stop are not
crisp — a disk has no natural beginning, its origin is given rather than found,
and the delivered medium already carries an origin statement saying which rule
located its circle, with the C1541 defaulting to the longest gap because that
drive never observes an index. One rung up the circle is crisp: a bitstream has
a definite cell count per revolution and a G64 writes each track's length down.
A NIB has the flux trait and not the bitstream one — a fixed window longer than
a revolution, overlapping itself, wrap nowhere recorded — so that is where it
enters, and the synthesized timings are the price of the placement rather than
the case for it.

**Corollary: manufactured transitions carry jitter, at half the family's
admissible reading deviation.** No drive writes at the tick, so pulses are not
placed at exact multiples of a declared cell; each is drawn seeded and recorded
as every other draw in this family is. The amount is derived from the profile's
existing reading band rather than declared as a second number, which says the
writing drive sat comfortably inside its own family's tolerance — and, more
usefully, makes a property checkable: every synthesized transition stays well
within the band that classifies it, so recovering a bitstream from a synthesized
medium returns exactly the bits that were synthesized. A round trip that could
lose a bit would make this whole placement unsafe.

Two constraints keep the factor honest. **Jitter is drawn on the interval, not
the absolute position** — two independently jittered positions put twice the
deviation into the interval between them, landing on the band edge and
misclassifying. And **the circle closes exactly**: jitter redistributes within a
revolution and never changes its total, so the wrap stays the one the reduction
declared rather than the sum of a random walk. Spindle speed variation is a
third thing, correlated across a revolution where these are per-transition, and
is left to its own declaration rather than folded in.

**The reason the placement was wanted is the hierarchy, and it is the owner's
call over an argued objection.** One entry point below one ladder keeps the model solid: everything
above the medium is then the ordinary route every other flux source takes —
read channel, bitstream, codec — instead of a second adapter shape entering
partway up. The artifact needs materialization either way, so materializing it
one rung lower costs a synthesis that is declared and buys a path that already
exists.

**What was weighed against it, and lost:** that a NIB records bits rather than
timings, so entering at flux asserts a content the file never held (D15, now
annotated). The objection is answered rather than dismissed — the timings are
declared synthetic at every pulse, so no claim of recorded evidence is made —
and the residue is accepted deliberately: the read channel will recover bits
from timings computed from bits, and the loop-point analysis a NIB needs does
not disappear by moving rungs, it moves with it. Both are the price of the
single ladder.

**G64 does not move with it.** It records its track lengths and positions, so it
is servable at the hardware bitstream as it stands, and the pledged P23
amendment already names it there as an image whose authoritative and initial
active layer are hardware bitstream. What sends NIB down is that it must be
materialized regardless; an artifact that need not be keeps its own rung.

**Read-only here is a capability, not a property of the rung.** No flux artifact
receives a write and no writable flux composition is claimed today, which is
what makes this placement simple — but that is each adapter's enumerated claim
under P3 and P13, and the project's current scope. In-force P22 continues to
say that a low-level composition claiming the physical path holds flux as
durable mutable state which receives modeled writes; it constrains work not yet
done, exactly as it did when it was armed. Nothing in this entry narrows it, and
a later write path — a modified medium encoded to a new artifact — needs no
amendment to arrive.

### D15 — A capture-form artifact is sorted by servability, not writability

> **Partly overruled by D16**, which moves NIB and NBZ into the flux family
> with synthesized timings. The first ruling below — that a capture-form
> artifact is not placed at a rung whose content it does not record — no longer
> binds for that class; the entry rung is a family's declared convention, and
> the honesty it protected is carried instead by declaring the synthesis at
> every pulse. **The second ruling stands unchanged**: servability, not
> writability, is what sorts the two modalities. Kept as written, per this
> record's rule that an entry only partly overruled is annotated rather than
> rewritten.

**Decided** Paul Galbraith, 2026-08-03. **Supports** P13, P22, P23, P27, P29,
P31.

Two rulings made while pledging P31, neither of which that principle's own
text would otherwise carry.

**NIB stays at the hardware bitstream and is not moved down to flux.** It was
weighed: the format records no track length, so a reader must analyse the
stream before it can serve a circle, and needing analysis before use reads
like raw evidence. It is not. P13's authoritative layer states what an
artifact actually records, and a NIB records bits a drive's read channel had
already recovered — placing it at flux would assert transition timings the
file has never held, which is the false provenance claim the P23 amendment
refuses from the other direction when it rules that generate-flux is
generate-medium. **Needing a reduction is what the modality is, and says
nothing about the content.** Where a composition genuinely wants a flux floor
beneath a NIB, that is the ordinary generate-flux transition, carrying
synthetic provenance and unable to reproduce evidence the source never stored.

**Writability sorts nothing, and the first cut at this used it.** The
distinction was initially drawn as G64 writable against NIB read-only, which
is wrong twice over: no artifact in this family is a writable backing, P64 and
G64 included, because writes land in the active layer and an artifact appears
only by an explicit encode building a new file. The axis is **servability** —
whether a session can truthfully serve one location by key from the file as it
stands, under P27's source-backed residence. That puts P64 and G64 on one side
and a stream set and a NIB on the other, which is the line that was actually
meant.

**Weighed and declined:** a new active-layer row for capture-form artifacts
(they carry no session's mutable truth at any rung, exactly as flux capture
carries none, so a row would have to be a row nothing is ever active at);
amending in-force P22's two-model clause to cover every rung (the clause is
scoped to the flux family where the models were found, and it is true as
written — generalizing the shape does not require rewriting the place it was
discovered).

### D14 — The flux family holds two models, and only the medium is ever active

**Decided** Paul Galbraith, 2026-08-03. **Supports** P13, P22, P23, P27, P29,
P30.

Rulings made while pledging the flux capture / flux medium split.

**One word was doing two jobs, and P22 already said so.** It reads that a
capture adapter may preserve several revolutions "while a normalized media
model may define one circular revolution" — two models, one name. They are
now **flux capture** and **flux medium**, and the boundary between them is a
test rather than a taxonomy: **disagreement across observations is a capture
fact, and strength is a medium fact.** A capture records that three passes
differed; a medium records that a pulse is weak; the conversion is a P29
reduction performed by neither model unasked.

**The medium is not a tidier capture.** What it adds — the rotational frame,
the family's addressing, the reference clock, the strength vocabulary, and
which surface is the disk — is absent from the flux and declared by a P30
profile. The measurement that settled it: the fixture was captured at 359.8
RPM on a 360 RPM instrument, and nothing in the flux knows a 1541 spins at
300. The medium is where declared knowledge and recorded evidence combine.

**Flux capture takes no active-layer row, for a concrete reason.** A drive
writing to a capture would have to choose which of several disagreeing
observations to overwrite, and no answer to that is better than another. It
stays authoritative image state under P13, read by inspection and by
mastering. P23's rule is scoped to independently mutable instances, and a
capture set opened to be inspected and mastered is not one.

**Capture becomes medium by mastering, not by lowering**, with the same
declared inputs whether the destination is a new artifact or an in-session
active layer. That supplies the mechanism the pledged P15 clause assumes
when it says a drive's floor may be "timed flux for a P64 or a raw capture":
a capture becomes a floor by being mastered under declared policy, never by
a normalization nobody named. For the same reason **generate-flux is
generate-medium** — fabricating instrument evidence from sectors would be a
false provenance claim in the clause most concerned with honest provenance.

**F30 is renamed, not split.** Its content was already entirely the capture
model, so nothing of it becomes the medium and its handle survives; the
medium takes F37. README's split rule reaches a feature cut into pieces, not
one whose subject is renamed.

**The promotion was compressed with the retarget.** Renaming pledged F30 and
retargeting pledged F33 and F34 cannot be done while the vocabulary they
would use exists only in `proposed/`, since a pledged item resting on a
proposed one is pledged too early. The amendments were therefore promoted in
the same act rather than the retarget being deferred.

**Weighed and declined:** one `FluxLayer` carrying both models behind a mode
discriminant — D9 already declined a kitchen-sink union record at this exact
layer, and this is that shape again; giving flux capture an active-layer row
of its own (no coherent write destination, and it would license a writable
capture-editing session nothing claims); keeping "flux" for the capture and
naming only the medium, which was rejected because P23's row already
*described* the medium, so renaming the row was both the smaller edit and
the truer one; splitting F30 into two fresh handles; and treating the medium
as a derived cache over the capture, which fails P27's own definition — a
derived cache is a clean-only accelerator regenerable from the layer below,
and a medium cannot be regenerated from a capture without the policy that
produced it.

**Folded into:** P22 and the P23 amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); pledged F30 (renamed),
F31, F33, F34, F36 and the new F37 in
[pledged/FEATURES.md](pledged/FEATURES.md); proposed F32 and its design; the
annotation on D8.

### D13 — The capture's two head designators are the disk's sides, not two capture channels

**Decided** Paul Galbraith, 2026-08-03. **Supports** U23, P29, P30.

A factual correction to pledged text, and a scope call that follows from it.

**The fixture was misread.** The `.0.raw` / `.1.raw` suffix on a Pinball
Construction Set stream is the KryoFlux head designator: the two are the
disk's two **sides**, not two passes over one surface. Side 1 is the
unrecorded back of a single-sided disk, measured as noise on every position
sampled — roughly 49,000 transitions per revolution with the count varying by
hundreds between passes, against side 0's tracks reproducing transition for
transition.

**Confirmed from the flip.** The source archive holds a second capture of the
same disk turned over, and it inverts exactly: there, head 1 reproduces
transition for transition and head 0 is the noise. The recorded surface
follows the flip to whichever head faces it, so the disk carries exactly one
recorded surface, established from both orientations. It is not a flippy.

**"Capture-channel identity" was never a second concept.** F31 already owned
"track and side identity", so the clause is struck rather than renamed.

**Side selection stays a policy input but stops being a judgment.** F33's
first input read as choosing which of two beliefs about one surface to
trust, and weighed accordingly. It is not that: P30's `Surfaces` declares
how many surfaces a family records and how a captured side maps onto one, so
for a 1541 the answer is declared, and a captured side the mapping does not
cover is refused. This is why the correction is not cosmetic — the input
that looked like the reduction's hardest call is answered by declaration,
and the reductions that actually carry risk are the timebase projection,
half-track admission, and the partial revolution outside the destination's
one rotation.

**The fixture is one capture, both heads.** It holds all 84 step positions
from each head in a single archive, named for the disk, which is the artifact
a real capture produces: a single-sided disk read in a two-head drive yields
both heads, and the operator archives the lot. Splitting the heads into two
archives would have pre-answered the question the library exists to answer.
Members carry the `.0.raw` / `.1.raw` designator rather than having it
stripped: a stream declares no track or side in its own out-of-band data, so
a member's name is the only place its position exists, and a fixture renamed
out of the convention would admit a grammar no real capture has.

**Weighed and declined:** leaving the vocabulary and adding a note (the
misreading had already been weighed into a pledged policy input, which is
exactly the damage a note does not undo); renaming "channel" to "side"
mechanically without revisiting F33's input (it would have preserved the
weighing that was the actual error); and recording this as an open question
rather than a decision, which would leave pledged text stating something
measured to be false.

**Folded into:** U23 in [pledged/USE-CASES.md](pledged/USE-CASES.md); F31 and
F33 in [pledged/FEATURES.md](pledged/FEATURES.md);
[AGENTS.md](../AGENTS.md); `../test-fixture-prep/prep_fixtures.py`;
`../test-fixture-prep/test-rigs/README.md`;
`crates/remanence/tests/sevenzip_catalog.rs`;
`crates/remanence/Cargo.toml` and the fixtures directory's `.gitignore`.

### D12 — Drive profiles own the knowledge a capture does not contain, and recognize structure without reading content

**Decided** Paul Galbraith, 2026-08-03. **Supports** P4, P12, P22, P23, P29,
P30.

Rulings made while pledging P30 and F36.

**The seam earns a principle.** P22 and P23 both rest on a "media profile"
and a "hardware profile" — the authority that says whether a drive observes
a selected revolution or a seeded variation, and the authority that makes a
downward synthesis honest — and neither names an owner. Knowledge assumed by
two principles and held by none is exactly the gap D8 found in P13 and
closed with P29, and the same reason applies here: a rule that binds every
future drive family does not belong in the design document of one mastering
profile. P30 states it.

**Recognition stops at structure.** A profile may read flux interval lengths
and the patterns they form; it may not resolve a bit value, assemble a byte,
name a sector, or validate a checksum. The test is what leaves the probe:
**an angle, never a byte.** This admits the landmark that makes recognition
work — a GCR sync is ten or more consecutive `1` bits, so in the interval
domain it is a run of minimum-length intervals, locatable without a clock,
without the encoding table, and without knowing what it introduces — while
refusing the ascent that would make every recognition depend on a
clock-recovery model.

**Discovery proposes; it never decides silently.** Verdicts are ranked,
carry P4 evidence, and may be pinned or overridden; a capture no profile
claims is a named refusal, and a lone enrolled profile never wins by being
alone. This does not weaken P29, whose policy inputs were always "supplied
by the caller **or declared by the profile**": recognition supplies
declarations with provenance, and a profile that cannot state a reduction
still refuses.

**The ruling was made against measurement.** Probing the prepared capture
set recovered all four 1541 speed zones at their documented track boundaries
with their documented sector counts, from interval statistics alone, with no
decoding — which is what established that the boundary above is a real place
to stand rather than a hopeful one. The same run also showed the cost of the
weaker alternative: a confidence figure without evidence hid a defect in the
probe's own cell estimate for one track, and only the evidence beside it
made the defect visible rather than reportable as a finding about the disk.

**Weighed and declined:** folding recognition into F33's design document
(D8's precedent — a design authorizes one feature, and this binds every
family); requiring the caller to declare the family in every case (the
evidence discriminates decisively, and a forced declaration puts an
unevidenced assertion into the plan's provenance); letting the probe ascend
to the hardware bitstream and recognize a family by decoding its sectors
(collapses the boundary between what a medium is and what a drive makes of
it, contradicts D8, and would make recognition depend on F32, which is only
proposed); a bare confidence scalar without the observations behind it
(P4 forbids it, and the measurement above showed why); and treating a
profile as a P12 image-format adapter (it owns no container grammar and
recognizes recorded state rather than an encoding).

**Folded into:** P30 in [pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md);
pledged F36; the annotation on D8.

### D11 — A design outlives the feature that carried it

**Decided** Paul Galbraith, 2026-08-03. **Supports** (none) — a records
ruling; no numbered vision entry demands it.

Two delivered features are struck from the pledged list, their handles
retired: the archive-catalog foundation and the file-container presentation
contract. The pledged list states that everything in it is owed, so a
delivered entry left standing makes it overstate the project's debt.

The archive-catalog entry was never struck on delivery because it was
**pledged two minutes after the code landed** — written retrospectively into
the owed list — so no delivery moment ever arrived at which the evaporate
rule applied. That is a defect in the record, not a change to the rule,
which has stood since the initial import. The lesson is the ordering, not
the rule: an entry describing work already done does not belong in a list of
what is owed.

[Overruled by D17: a design document's purpose ends at delivery, and it is swept with the feature whose handle evaporates.]

**A companion design does not evaporate with its feature.** README's sweep
covers a design whose *proposal dies*, and its one-way move out of
`planning/` covers a document describing a *delivered application surface*.
Neither reaches a design for delivered work that touches no surface, and the
file-container contract is exactly that: the code implements it, but the
code is not a readable statement of what a future provider must satisfy.
Deleting it would destroy the contract's only prose to satisfy a rule
written for a different case. It stays, restated as delivered, and a design
whose feature is struck is re-headed rather than swept.

**Weighed and declined:** sweeping the design with its feature on the
strict reading that a design serves one feature and dies with it (it would
leave the conformance rules discoverable only by reading the module);
moving it out of `planning/` under the delivered-surface rule (it describes
no surface — the feature's own scope was `Touches: none`); and leaving both
entries in place until a later cleanup, which is what let the first one
persist.

**Folded into:** [pledged/FEATURES.md](pledged/FEATURES.md).

### D10 — The truth is the lowest materialized layer; file container is an interface, not a layer

**Decided** Paul Galbraith, 2026-08-03. **Supports** P19, P23, P25.

**The rule, in the owner's words:** the lowest durable layer the session has
materialized is the source of truth. A file-container view has real
utility — display, envisioning structure, and the account of what an
interpretation claims — but it is not the truth. And there is **no container
layer above these systems at all**: a ZIP grammar, a FAT volume, a Commodore
directory each already hold their own structure and simply *present* a
file-container view of it.

In-force P23 already carries the first half for disks: the initial active
layer is "the least physically expressive durable media layer which
faithfully serves every presentation requested". This states it generally,
past disks to serialized containers. P23 needs no amendment — it already
separates the P19 interface from the active layer, and a ZIP's active
named-entry state is owned by its grammar.

The second half is a correction of this project's own drafting rather than
of P19: in-force P19 was always written as a **seam** whose adapters *expose*
a view and whose results *present* an interface. The word "layer" entered
through the F35 drafts and nowhere else. What F35 delivers is therefore the
interface providers present through and the vocabulary they answer in.

Four consequences fold into the pledged P19 scope-of-claim amendment and the F35 design.
**No materialized model**, so a provider answers about the directory it was
asked about instead of building an item pool for fifty thousand files, and
identity is the provider's own rather than an index into a pool that no
longer exists. **Nothing to invalidate**, so a floor that moves needs no
regeneration protocol. **One hook, not two concepts**: a footprint and a
content source were the same fact about different floors. **Coverage
everywhere**, since every presentation has a floor — a self-extractor stub is
an opaque region exactly as a protection track is, which overrules D9's
clause to the contrary.

**Weighed and declined:** a materialized model as the active layer for
serialized containers (it made ZIP and media structurally different for no
gain, and its footprints would go stale the moment a composition descended to
flux); a materialized model as a generated view above the floor (it kept an
invalidation protocol and a read-whole for no benefit the interface does not
already give); declaring a file container never active at all, which would
have contradicted in-force P23 and left a writable ZIP's pre-commit truth
unowned; and treating an archive's unaccounted bytes as adapter evidence
rather than opaque regions, which duplicated one concept in two
vocabularies.

**Folded into:** the pledged P19 scope-of-claim amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); the annotation on D9;
pledged F35 and its companion design.

### D9 — The file-container model's scope calls

**Decided** Paul Galbraith, 2026-08-03. **Supports** P19, P23.

Rulings made while pledging the file-container model foundation (F35) and
the P19 scope amendment.

**The unclaimed remainder is an "opaque region."** Opaque *to this
interpretation* — no implication that it is garbage, free, or unclaimed by
every layer; in the protection case it is load-bearing content, and over
flux it is angular track regions rather than bytes. The proposed U8 already
uses the phrase.

**An opaque region is an item, never an entry.** In-force P19's refusal to
manufacture pseudo-files stands untouched: the namespace lists only what the
source names, and the opaque remainder is itemized without a name,
reachable through the coverage account rather than by path.

[Overruled in part by D17: there is no design-level home for a delivered
feature's contract. The metadata contract lives in the code implementing
it; the principle-level half of this split stands.]

**The scope clause is principle-level; the metadata contract is
design-level.** The coverage obligation amends P19, while the superset
metadata contract stays in the companion design — the same split the flux
foundation made between the P22/P23 amendments and its design document.

**Coverage exists only over a materialized sub-layer.** A serialized
container's unaccounted source bytes (a self-extractor stub, padding) are
the adapter's evidence, not opaque regions; there is no layer beneath the
active file container for a footprint to address.

> **Overruled by D10** on this clause alone: a serialized container's own
> named-entry state is a floor like any other, so its unaccounted bytes are
> opaque regions and it carries an account. Every other ruling in this entry
> stands.

**Deleted-but-present entries are accounted, not itemized.** A scratched
CBM entry or FAT `0xE5` slot is part of the namespace structures' footprint;
itemizing it would be a recovery claim nothing pledges.

**v1 claims one content stream per file item.** Alternate data streams and
forks enter by the superset contract's additive named-home route or are
refused by name.

**Weighed and declined:** "blob" and other byte-shaped terms (wrong over
flux, and they imply extractability the view may not claim); "unclaimed
extent" (reads as nobody's when the truth is not-this-view's); "remnant"
(suggests leftover-from-deletion; protection tracks are deliberate); a
kitchen-sink union record with every metadata field optional (rejected once
already at the flux layer; the two-outcome rule is reused instead);
itemizing deleted entries (a recovery claim in disguise).

**Folded into:** the pledged P19 scope-of-claim amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); pledged F35 and its
companion design.

### D8 — Mastering a capture to P64 stops at flux, and gets its own principle

**Decided** Paul Galbraith, 2026-08-03. **Supports** U23, P29.

Two scope calls made while pledging U23.

**It stops at flux.** Converting a KryoFlux capture to P64 descends no
further than the flux layer: no hardware bitstream is materialized, no GCR
codec runs, no sector or filesystem interpretation is attempted. Both
endpoints are flux-shaped, so the intervening layers would be built only to
be discarded. Proposed F32 is therefore *not* a dependency of U23 and stays
in `proposed/`, which also keeps U23's pledge from resting on something only
proposed.

> **Annotated by D12**, which narrows nothing. Locating a synchronization
> landmark as a run of minimum-length flux intervals is not "a GCR codec
> running": no clock is recovered, no symbol is resolved, and what leaves
> the probe is an angle rather than a byte. The clause stands as written,
> and D12 states the boundary that keeps it checkable.
>
> **Annotated by D14** on the spelling only. The journey now stops at the
> **flux medium**, one rung above where this entry could name at the time,
> because the flux layer it spoke of has since been split in two. The
> ruling is unaffected: both endpoints remain the same shape, no hardware
> bitstream is materialized, no GCR codec runs, and F32 is still not a
> dependency of U23.

**And it earns a principle.** P13 already licenses the act — choosing another
authoritative layer is an explicit conversion creating a new image and naming
its loss — but names no owner for the reduction policy and no mechanism for
"naming the loss". Reading that into P13 would have made the strongest clause
in the conversion story an inference. P29 states it instead: declared policy
inputs, two owners, plan before write, derived provenance, reproducibility.

**Weighed and declined:** requiring F32 so a mastered image could be verified
by decoding it to sectors (verification is round trip through the P64
adapter's own decode, which tests the claim actually made); folding the
mastering rules into F33's design document alone (a design authorizes one
feature, and this rule binds every future destination format).

### D7 — The library names no consuming project

**Decided** Paul Galbraith, 2026-08-01. **Supports** (none) — a naming
ruling; no numbered vision entry demands it.

Documentation follows the dependency direction the code does: a consumer
may name the libraries it builds on, and this library names none of the
projects that build on it. In-force U3 and U4 named the consuming
application outright, inherited from the demand they were dictated from.
Both are reworded to the caller's voice — every claim, contract and symbol
unchanged — under authority compression. The rule's home is AGENTS.md,
"The library does not name its consumers"; it reaches every library-side
document, not only the ones a registry publishes — this record included,
where D2's weighed alternative is reworded to the caller's voice. A name
that survives sits inside the fixture-tooling permission, which runs the
other way: the project may name what it builds on.

**Weighed and declined:** keeping the name in the use cases on the grounds
that they are the owner's demand narrative and a real name is more concrete
than "my automation layer" — that concreteness is exactly what goes stale
inside a published artifact, and the use cases are the first library-side
document a newcomer reads.

**Folded into:** root [USE-CASES.md](../USE-CASES.md) (U3's title, opening
and drive-letter clause; U4's opening); [AGENTS.md](../AGENTS.md); D2's
weighed alternative; `crates/remanence/src/disk.rs` and
`crates/remanence/src/fat.rs` doc comments.

### D6 — Device identity is assigned, not requested

**Decided** Paul Galbraith, 2026-07-31. **Supports** P21.

D5 still defers multi-device topology, volumes spanning devices, and
cross-source transactions. Its refusal of preparatory identity was too
broad: a library-assigned, composition-scoped identity adds useful internal
structure without adding a caller-supplied datum. It gives identity no
global meaning and revives none of the machinery D5 deferred; P21 carries
the rule.

**Partially overrules:** D5's rejection of topology-ready identities. The
new evidence is that automatic identity and caller-authored topology have
different interface costs.

### D5 — Multi-device topology is deferred until a use demands it

**Decided** Paul Galbraith, 2026-07-31. **Supports** P17.

> **Partly overruled by D6:** the refusal of automatic device identity no
> longer binds; the deferral of multi-device topology and volumes stands.

The proposed P20 is withdrawn. Multi-device volumes are extremely unlikely
to enter Remanence, and the concrete cost of adding them later is an
ordinary refactor: qualify disk-local identities, supply several devices to
volume composition, and add cross-source write coordination if writing is
claimed. That does not justify making source, device, attachment, and
multi-parent provenance part of F19 or the architecture now. P20's number
is retired and will not be reused.

P17 remains the independent volume-composition seam. It supports current
whole-medium, partition-backed, and region-composed volumes without
promising or preparing for a volume spread across devices. If that use ever
becomes real, it receives its own proposal and surface design. Existing
disk-local identifiers retain their existing scope; no present interface
claims they are globally unique.

**Weighed and declined:** building topology-ready identities and
multi-parent provenance into F19; a multiple-source open with manual
`hdd0`/`hdd1` assignment [no longer declined: pledged P32 admits a session
device set with `hdd0`-style attachment identities, which in-force P21
routed to its own proposal rather than refusing; the deferrals of
volumes spanning devices and of cross-source transactions stand]; a
principle governing cross-file transactions before any multi-device write
use exists.

**Folded into:** proposed P17; the F19 design; withdrawal of proposed P20.

### D4 — "At rest" leaves the library's vocabulary; the surface is the `Disk` stack

**Decided** Paul Galbraith, 2026-07-30. **Supports** (none) — a
vocabulary ruling; no numbered vision entry demands it.

The term "at rest" is retired from library-side prose and comments.
It borrowed its meaning from the consumer's frame — a disk not held
by a running machine — a contrast this library cannot represent (it
has no concept of a machine); inside the library it distinguished
nothing, since every operation here works on an image as a file;
and it collides with the security-jargon sense of "data at rest".
The geometry/volumes/files read-write stack is named by its own
API: **the `Disk` surface** (in prose, the disk stack). Use cases
keep the consumer's voice, but "a stopped machine's" already
carries the whole meaning, so U3 and U4 drop the term too — a
wording-only amendment, landed under authority compression: no
claim, contract, or symbol changed, and no public symbol ever
carried the term.

**Weighed and declined:** keeping "at rest" as an established
project word (it was established by inheritance from the consumer's
design vocabulary, not by a decision here); "offline" (relative to
the same machine concept the library lacks).

**Folded into:** the U3/U4 rewording in root
[USE-CASES.md](../USE-CASES.md); root
[ARCHITECTURE.md](../ARCHITECTURE.md) "The system"; README.md;
AGENTS.md; doc comments in the three crates (the C header
regenerates from them); `tests/at_rest.rs` renamed `tests/disk.rs`;
the test-rigs prose; the drafts under `proposed/`.

### D3 — One upstream version; packaging versions derive; repacks are post-releases

**Decided** Paul Galbraith, 2026-07-30. **Supports** (none) — release
machinery; no numbered vision entry demands it.

The workspace SemVer is the sole upstream version. The PyPI version
is derived from it by maturin (`0.0.1-alpha.1` → `0.0.1a1`), never
hand-written. Repackaging an unchanged upstream — the distro-revision
case — is spelled as a PEP 440 post-release by appending `.post.N` to
the Python packaging crate's own Cargo version (`0.0.1a1.post1`);
whether a repack is warranted is the releaser's judgment, and only
the spelling is mechanized.

**Weighed and declined:** PEP 440 local versions (`+r1`, the true
distro-revision analog — PyPI rejects them on upload); a static
hand-maintained pyproject version (drifts from the lib; replaced by
derivation); bumping the upstream version for packaging-only changes
(misstates the library). PEP 440's discouragement of post-releases
on pre-releases was seen and consciously overridden — the
distro-revision model is the point.

**Folded into:** AGENTS.md "Versioning and releases";
`crates/remanence-py/pyproject.toml` (dynamic version).

### D2 — The commit point is an in-memory overlay, not qcow2 internal snapshots

**Decided** Paul Galbraith (via the owner-directed implementation),
2026-07-30. **Supports** P2, U3.

P2's commit point is implemented as an in-memory write overlay over
the virtual disk: every write buffers, reads see the buffered state,
`commit` writes through and flushes, `rollback` discards. The drafted
alternative — reproducing a caller's qcow2-internal-snapshot
protocol natively (the feature drafted as F4) — was superseded before
it was pledged.

**Weighed and declined:** internal snapshots as the commit point.
The overlay is uniform across raw and qcow2 images where snapshots
exist only for qcow2; it means **nothing whatever touches the host
file before commit** (stronger than snapshot-then-write under P6);
and it removes the snapshot-table machinery from the write claim
entirely — the write path refuses images carrying internal snapshots,
keeping the all-refcounts-are-one invariant checkable.

**Folded into:** root [ARCHITECTURE.md](../ARCHITECTURE.md) P2's
in-force text; `crates/remanence/src/device.rs` (the overlay) and
`disk.rs` (commit/rollback).

### D1 — The HDOS fixture images leave git and every published artifact

**Decided** Paul Galbraith, 2026-07-30. **Supports** (none) — no
numbered vision entry exists yet to demand it; the demand is the
licensing policy in [AGENTS.md](../AGENTS.md): the project must own
every line it ships, and the vintage HDOS distribution images are
not the project's to distribute — or at least that is not certain,
which is the same bar.

The fixture images under `crates/remanence/tests/fixtures/` are
excluded from **everything the project distributes or records**:
Python sdists and wheels, cargo packages, and the git repository
itself — history was rewritten to expunge them before any remote
existed, and the directory is ignored. They remain local-only test
data. Implemented as `package.exclude` on the core crate (governing
maturin sdists and `cargo publish` alike), the `.gitignore` entry,
and the history rewrite.

**Amended** Paul Galbraith, 2026-07-31. The exclusion was a whole
directory, which cost the project a fixtures directory it could use
at all. It is now **per file**: `crates/remanence/tests/fixtures/`
holds checked-in fixtures the project owns, and the third-party and
generated material sits beside them, named file by file in that
directory's own `.gitignore` — the ignore rule lives with the files
it governs, so adding a fixture is a local act. Nothing about what
D1 refuses to distribute changes; only the granularity does, and
`package.exclude` mirrors the same names.

**And the material is fetched, not carried.**
`../test-fixture-prep/prep_fixtures.py` downloads the HDOS 1.0 distribution
zip from `https://sebhc.github.io/sebhc/software/HDOS/HDOS_1-0.zip`
under a pinned SHA-256, extracting only the image the tests read;
the FreeDOS LiveCD downloads through the rig blueprint's own
reliquary media spec, likewise pinned, into
`../test-fixture-prep/test-rigs/cache/media` (git-ignored, outside the
crate). The FreeDOS qcow2 the rig builds lands in the fixtures
directory as a generated artifact. So a fresh checkout carries none
of it and can obtain all of it, which closes the accepted cost this
decision took on — the repair T5 tracked, struck with this change.

**Weighed and declined:** publishing the wheel without an sdist
(with no public repository, GPL object code would ship with no
corresponding source at all); annotating the fixtures in REUSE and
shipping them (the project cannot convey rights it does not hold);
keeping them in git as local-only history (any future push would
distribute the blobs).

**Folded into:** `crates/remanence/Cargo.toml` (`package.exclude`),
`crates/remanence/tests/fixtures/.gitignore`, root `.gitignore`,
`../test-fixture-prep/prep_fixtures.py`, AGENTS.md "Prior art and provenance
notes".

## Retired decisions

Overruled or no longer relevant, kept intact for the record. A
retired decision binds nothing.
