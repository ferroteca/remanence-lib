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
than a separate one, since what settles them is an entry below.
Nothing here binds anything; a question leaves this section by
becoming a D-number, and the commit that moves it is the record.

- **The vision is not yet dictated** — no use cases, no
  architectural principles. What turns on it: the surface-change
  rule cannot triage against lists that do not exist, so
  significant decisions are flagged to the owner case by case.
  Settled by the owner dictating the first lists, in the owner's
  voice.
- **CLA legal review** — [CLA.md](../CLA.md) states intended terms
  but has not been reviewed by a lawyer, and its governing-law
  clause is deliberately unfilled. What turns on it: no external
  contribution can be accepted under it until reviewed. Settled by
  that review.

## Decisions

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

**Accepted cost, stated by the owner:** a fresh checkout cannot run
the fixture-driven integration tests — better to commit a broken
build than to commit something the project should not carry. T5
tracks the repair. *(T-numbers evaporate; once that task is struck,
its commit is the record.)*

**Weighed and declined:** publishing the wheel without an sdist
(with no public repository, GPL object code would ship with no
corresponding source at all); annotating the fixtures in REUSE and
shipping them (the project cannot convey rights it does not hold);
keeping them in git as local-only history (any future push would
distribute the blobs).

**Folded into:** `crates/remanence/Cargo.toml` (`package.exclude`),
`.gitignore`, AGENTS.md "Prior art and provenance notes".

## Retired decisions

Overruled or no longer relevant, kept intact for the record. A
retired decision binds nothing.
