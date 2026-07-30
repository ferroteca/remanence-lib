# The surface-change rule

> **Status:** the governing rule for changes to remanence-lib's
> application surfaces. The surfaces themselves are enumerated in
> root [ARCHITECTURE.md](../ARCHITECTURE.md) "The application
> surfaces" (S1–S4) — that enumeration is this rule's scope,
> answered by lookup — and norms today are the defining code, as
> that document states. This document says how a surface-changing
> decision is weighed: against the use cases and the architectural
> principles, which carry equal weight. **Neither list is in force
> yet** (the vision awaits the owner's dictation), so until they
> exist the triage below cannot be run to completion — a
> surface-changing proposal is flagged to the owner rather than
> self-approved, and the owner may compress the steps as authority
> always may.

## The decision surface

The numbered use cases and the P-numbered principles are the
decision surface, once dictated: numbered so a decision, review, or
specification section can cite what it serves — and so a proposed
change can be rejected by naming what it costs. The root lists hold
only what is in force; proposed changes are tracked under
`proposed/`, numbering from the same global sequences and moving
over when adopted. A number is never reused.

## The housekeeping boundary

One class of work is exempt, and its boundary is drawn here because
here is where it would be walked around. **Housekeeping** approves
small cleanups and small reported defects as a standing class — tiny
in scope *and* clearly a problem — so they need no citation and no
adjudication.

**It stops at the application surfaces, absolutely: a change that
touches any surface the enumeration names is automatically not
housekeeping**, whatever its diff looks like, and takes the rule
below instead. That test is asked first and answered by lookup, so
it is a checklist, not a judgement. **The norm is part of the
surface**: with the code as norm, a change to the defining code of
S1–S4 *is* a surface change — an edit to `pub` items, `rmn_*`
symbols or their contracts, the Python module surface, or the
format-dialect grammar is gated however small its diff. Only a
change that alters no contract is housekeeping-eligible. This
matters because housekeeping's other two tests ("tiny", "clearly a
problem") are judged by whoever wants to do the work, and the
smallest-looking change is the one most likely to be a contract
change wearing a small diff.

**The boundary is housekeeping's alone.** It exists *because*
housekeeping is ungoverned — approved as a class in advance — so
the surface exclusion is the whole of what stands between that
class and an unreviewed contract change. It does **not** reach the
pledged task queue ([TASKS.md](TASKS.md)), where the gate sits at
entry and only authority may enter anything: **a small surface
change may be a task**, admitted on size and kind and never refused
for the surface it touches. What it may not do is skip the landing
rules below, which bind a task exactly as they bind a feature.

## The rule

Requests triage by their use-case and principle impact. A change is
significant precisely when approving it requires a use case or a
principle to be adjusted; a significant change is not argued as a
feature on its own merits — **the amendment is the argument**, and
the surface change follows from the amended list. A significant
proposal that cannot be phrased as "the use cases should say ..." or
"the principles should say ..." is not ready to decide.

- **No use-case or principle impact, or strong alignment with the
  existing lists.** An easy decision to approve; cite what is
  served, or state that nothing is disturbed.
- **Adds a new use, or a new principle.** More work — the new entry
  must be drafted, numbered, and weighed for coherence — but, being
  additive, still an easy decision.
- **Misaligned with the use cases or a principle.** The hard case,
  argued very vigorously: draft the amendment under `proposed/` and
  make the argument; if the argument wins, the amendment moves to
  `pledged/` — the move is the pledge and the commit is its record —
  and only then does work start. A misaligned change that can
  propose no amendment has nothing to argue and is rejected,
  regardless of its elegance.

**Authority may compress the steps.** The staged workflow above is
the route for someone who cannot approve their own change. A person
holding governance authority — the owner alone today — may land a
surface or norm change outright, in a single commit, being entitled
to perform every step it needs. That is an *execution* of the
governance steps all at once rather than a bypass: compressed in
time, not reduced in content, so the amendment, the decision entry
and the norm update still land with it. Anyone without that
authority takes the staged route, and finished work from them is
refused on one of two grounds, **never identity**: not having argued
the merit, or having argued it and not won — and every refusal
states what would change the answer.

Every approved change then lands the same way:

1. **Name every surface it touches, by number.** A change rarely
   touches one — S1 changes usually reach S2 and S3. An
   intentionally single-surface change states why the others are
   unaffected.
2. **Land it coherently and completely** — every affected surface,
   binding, document, example, and test moved to the new shape, the
   old one deleted. Pre-1.0, no compatibility is promised (see
   AGENTS.md); cheap execution does not make the decision cheap.
3. **Record it.** Amendments are drafted under `proposed/`, pledged
   by being moved out of it, and reach the root lists when
   delivered, keeping their numbers; rulings go to
   [DECISIONS.md](DECISIONS.md) when they have no normative home.
