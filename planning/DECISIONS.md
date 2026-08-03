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
document, not only the ones a registry publishes.

**Weighed and declined:** keeping the name in the use cases on the grounds
that they are the owner's demand narrative and a real name is more concrete
than "my automation layer" — that concreteness is exactly what goes stale
inside a published artifact, and the use cases are the first library-side
document a newcomer reads. Sweeping this record too: entries keep the
spellings of their time, so D1's and D2's mentions stand.

**Folded into:** root [USE-CASES.md](../USE-CASES.md) (U3's title, opening
and drive-letter clause; U4's opening); [AGENTS.md](../AGENTS.md).

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
alternative — reproducing reliquary's qcow2-internal-snapshot
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
