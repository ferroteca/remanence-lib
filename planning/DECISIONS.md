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
[pledged/FEATURES.md](pledged/FEATURES.md);
[pledged/design/flux-capture-foundation.md](pledged/design/flux-capture-foundation.md)
(renamed from `flux-layer-foundation.md`),
[pledged/design/flux-medium-foundation.md](pledged/design/flux-medium-foundation.md),
`c1541-flux-mastering.md`, `p64-image-adapter.md`, `kryoflux-capture-set.md`,
`file-container-presentation.md`; proposed F32 and its design; the annotation
on D8.

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
[pledged/design/c1541-flux-mastering.md](pledged/design/c1541-flux-mastering.md);
[AGENTS.md](../AGENTS.md); `testing-prep/prep_fixtures.py`;
`testing-prep/test-rigs/README.md`;
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
pledged F36 and
[pledged/design/drive-profile-recognition.md](pledged/design/drive-profile-recognition.md);
the annotation on D8.

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

**Folded into:** [pledged/FEATURES.md](pledged/FEATURES.md);
[pledged/design/file-container-presentation.md](pledged/design/file-container-presentation.md).

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

Four consequences fold into the pledged P19 amendment and the F35 design.
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

**Folded into:** the pledged P19 amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); the annotation on D9;
pledged F35, whose companion design is renamed by this ruling to
[pledged/design/file-container-presentation.md](pledged/design/file-container-presentation.md).

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

**Folded into:** the pledged P19 amendment in
[pledged/ARCHITECTURE.md](pledged/ARCHITECTURE.md); pledged F35 and its
companion design, drafted as `file-container-layer-foundation.md` and
renamed by D10 to
[pledged/design/file-container-presentation.md](pledged/design/file-container-presentation.md).

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
`hdd0`/`hdd1` assignment; a principle governing cross-file transactions
before any multi-device write use exists.

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
`testing-prep/prep_fixtures.py` downloads the HDOS 1.0 distribution
zip from `https://sebhc.github.io/sebhc/software/HDOS/HDOS_1-0.zip`
under a pinned SHA-256, extracting only the image the tests read;
the FreeDOS LiveCD downloads through the rig blueprint's own
reliquary media spec, likewise pinned, into
`testing-prep/test-rigs/cache/media` (git-ignored, outside the
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
`testing-prep/prep_fixtures.py`, AGENTS.md "Prior art and provenance
notes".

## Retired decisions

Overruled or no longer relevant, kept intact for the record. A
retired decision binds nothing.
