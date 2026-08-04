# planning

Maintainer-facing planning. The directories are the classification;
file names carry no suffix, and a document's location tells you its
standing without reading a word of it.

## The one vocabulary

**A use case, a principle and a task carry the same lifecycle**:
each is *proposed*, *pledged*, *completed*, or *rejected*. One
vocabulary runs through the whole planning machinery — the same four
words classify demand, rules and work alike — and the directories
below are that vocabulary made physical.

- `proposed/` — argued but not pledged. **Nothing is worked from
  here.**
- `pledged/` — owed by the project, and not yet delivered.

Both appear when they first have content; an empty directory would
invite reading the structure as ceremony.

**There is no roadmap here, deliberately.** A roadmap promises an
order and a time this project does not commit to. `pledged/` says
the project will do it and nothing about when: it answers "is this
right?" with yes, "will it happen?" with yes, and "when?" with
nothing at all. A pledge nobody means is withdrawn to `proposed/`
or rejected outright, never left sitting. Big items wait in
`proposed/` and are bitten off one at a time, in no pre-promised
order.

Anything here that can be depended on carries a handle, so that one
item points at another by something stable rather than by a heading
someone may reword. Features take numbers — `F3 — <name>` — which
is the old milestone identifier and none the worse for it: what a
feature number does *not* carry is an order or a date. Designs take
no number of their own; a design serves one feature and is
identified by its path.

**A feature must be small enough to implement in one sprint**, and
is broken up when it is not. The sprint measures the feature rather
than scheduling it, and it is deliberately unspecified — for this
solo project with AI tooling, think minutes to hours, not weeks,
which makes an acceptable feature far smaller than "milestone"
suggests. The bound bites at the pledge: large, shapeless capability
is welcome in `proposed/`, and cutting it into implementable pieces
is part of what pledging it means. A split retires the parent's
number and issues a fresh one to each piece.

The handles of *vision* — use cases, principles, surfaces,
decisions — are permanent, and travel into the in-force lists on
delivery. Every handle sequence issues against
[SEQUENCES.md](SEQUENCES.md), which is advanced in the same edit that
issues a number. The handles of *work* evaporate: a delivered
feature stops existing as an item, leaving code and the norms that specify it, and
its number retires rather than being reused. Gaps in the sequence
are history, not a promise.

**References between items run down the lifecycle or sideways,
never up**: a proposed item may depend on a pledged one, and a
pledged item that cannot be completed without something still only
proposed has been pledged too early. A reference states an order
between two items, never a position in a queue. A date is a promise
and belongs nowhere here.

**The planning root holds what does not move.** The map, the rule,
the record, the queue and the ledger are machinery rather than
proposals, and none of them has a lifecycle state to be in:

- [README.md](README.md) — this map.
- [SURFACES.md](SURFACES.md) — the vetting rule. It governs
  `proposed/` at least as much as `pledged/`; it is the test a
  proposal is judged by, not a thing that was proposed.
- [DECISIONS.md](DECISIONS.md) — the adjudication record, which cuts
  across every state by design: open questions, decisions that
  pledged something, decisions that **refused** it, and a retired
  list binding nothing at all.
- [TASKS.md](TASKS.md) — the queue. Work entered there is small and
  **pre-approved**, so there is nothing to promote and no order to
  work it in.
- [SEQUENCES.md](SEQUENCES.md) — the handle ledger. It records the
  next number to issue for every handle class, and says nothing about
  status, priority, or work order.

The in-force artifacts live at the **repository root**, not here,
because they are claims about the code as it exists today:
[USE-CASES.md](../USE-CASES.md) (U1–U6, U22 and U23, every entry met by
the code) and [ARCHITECTURE.md](../ARCHITECTURE.md) (the whole-system
view, the application surface inventory S1–S3, and the
architectural principles, every principle honored by
the code). Together with the norms — currently the defining code, as
root ARCHITECTURE.md states — they are the project's **vision**.
What sits under `proposed/` and `pledged/` here is vision that has
not arrived yet; either directory appears when it next has
content.

Use cases and principles run through **three** states, not two:
drafted (`proposed/`) → pledged (`pledged/`) → in force (the root
lists). Pledging and delivery are different events, and the gap
between them is real — the root lists are implementation claims, so
pledging an entry can never put it there. This is also what arms a
principle: below the root list it is pledged vision and a shortfall
is unbuilt work; at the root list the project asserts the code
honors it, and a divergence becomes a bug.

**Design sits with what it serves.** A design for one feature lives
beside that feature — `proposed/design/` or `pledged/design/` — so
the design and the demand it answers move together. A design is
guidance toward work not yet done, so both its ends are the same
end: a design whose proposal dies is swept with the proposal, and a
design whose feature is delivered is swept with the handle that
evaporates. Nothing carries it past delivery — what was done is the
code, a rule binding every later change is a principle, and the
reason a choice was made is a decision. A design left standing is a
second statement of the norm beside the code, and being prose it
drifts silently. `design/` at this level holds only open design
problems belonging to no single feature; the whole-system view
itself is root [ARCHITECTURE.md](../ARCHITECTURE.md).

**Nothing under `planning/` describes a delivered surface.** Once an
application surface ships, its normative specification is current
truth and moves out of here. That is a one-way move: a norm never
comes back.

## How an idea enters

**An idea enters this project through three work queues:**

1. **Issues** — the raw, unfiltered intake. **This project has no
   issue tracker yet**; until one exists the task lane has no
   proposed state at all, and raw intake arrives as conversation
   with the owner.
2. **The `proposed/` directory** — the same idea argued in the
   project's own vocabulary, as a drafted use case, principle or
   feature. Nothing is worked from here until it is pledged.
3. **[TASKS.md](TASKS.md)** — small, **pre-approved** work. Entering
   it is approving it, so it needs no citation and no decision, and
   there is no order to work it in.

Nothing flows without starting in one of them; the only exception is
a small raw commit approved under housekeeping.

**Writing into `planning/` is a governed act.** Everything here is
the project speaking in its own voice, so the same gate governs all
three acts — entering a document in `proposed/`, promoting it to
`pledged/`, and entering work in [TASKS.md](TASKS.md). Only what
each grants differs: a live argument, a pledge, an approval. The
gate weighs most on the last, which *is* the whole vetting with
nothing behind it. It sits at entry only; anyone may pick up what is
already there. Authority is the owner alone today.

**Housekeeping** is the same instinct one size below the third
queue: small cleanups and small reported defects — tiny in scope
*and* crystal clear they are a problem — are approved as a class, in
advance, and are too small to be worth writing down at all. A
qualifying item is approved on sight and needs no entry anywhere;
whoever lands the work invokes the bucket by naming it in the
commit, and the commit is the record.

Refusing is half of both rules. Housekeeping's surface test is its
first gate and it is a lookup, not a judgement —
[ARCHITECTURE.md](../ARCHITECTURE.md) enumerates the surfaces, and
the rule that weighs a hit is [SURFACES.md](SURFACES.md). It governs
that bucket only; the third queue's gate is authority at entry
(above). A use-case or principle amendment and a design decision are
never admissible to either bucket. Past that, doubt escalates: if it
has to be argued in, it does not belong in.

## How an idea is pledged

**The move is the act.** Promoting a document — or an entry within
one — from `proposed/` to `pledged/`, or from `pledged/` to the
root standing list, *is* the pledge, and the commit that does it
is the record. There is no separate register to keep in step, and
nothing is pledged by being cited somewhere.

Every pledged item cites what demands it: a use case (its U-number,
in force at the root or still drafted under `proposed/`) or an
architectural principle (its P-number), which drives work just as
well. When a proposal dies, the sweep finds every item that falls
out with it.
