# TASKS

The pledged work backlog. A **proposed** task lives in the issue
tracker — **this project has no tracker yet**, so until one exists
the task lane has no proposed state at all and raw intake arrives as
conversation with the owner. Nothing parks here awaiting a verdict;
arriving *is* the verdict.

**Everything in this file is pledged.** That is the whole of what
it means, and it is the one vocabulary ([README.md](README.md))
applying here exactly as it does in the directories: an entry is in
the *pledged* state, so entering it is approving it. Nothing waits
on a verdict, nothing needs a citation, a use case, or a decision of
its own, and there is nothing to promote — it arrived pledged.

**The state is `pledged`; the directory is not the home.**
`proposed/` and `pledged/` hold *demand and capability* — use
cases, principles, and the features that deliver them — each argued
at length before it is pledged. A task is none of those: it is
free-standing work too small to be a feature, and too small to need
the argument. That is the distinguisher, not size alone. So it
stays at the planning root, in the pledged state, among the
machinery.

Adding to it is governed, by the gate that covers writing anywhere
under `planning/` — see [README.md](README.md). The gate weighs most
here, this being the one governed act that grants approval with no
argument behind it, and it sits at entry only: once something is
here, anyone may pick it up. Authority is the owner alone today;
agents do not add tasks on their own initiative.

**A queue holds what waits.** Work that arrives already done never
appears here: there is nothing to schedule, only a decision to make,
and an entry filed and closed in one act is ceremony.

**There is no order here.** Nothing in this file is scheduled, and
nothing claims priority over anything else; whoever picks work up
picks whatever they like.

Housekeeping is the same instinct one size smaller: work tiny enough
and obvious enough that it needs no entry here **at all**, approved
as a class in advance, with the commit as its record. This file is
where the pre-approved work that is still worth writing down goes.

**A small surface change may be a task.** Housekeeping's surface
test does not read across to this queue: an item is admitted here on
size and kind, never refused for the surface it touches. What it may
not do is skip the **landing rules** in [SURFACES.md](SURFACES.md),
which bind a task exactly as they bind a feature.

**What never belongs here** is a use-case or principle amendment,
or a design decision. Those are argued, not queued.

## Every task is itemized

**A task carries a T-number** — `### T1 — <what to do>`, number and
name together. It is issued at entry — a task has no proposed state
here, so there is no earlier moment — and it evaporates when the
task is struck, work handles going with their work. Evaporating is
not reusable: the number retires and is never issued again, so one
surviving in a commit message never resolves to something else
later, and gaps are history.

Task numbers issue against [SEQUENCES.md](SEQUENCES.md), advanced in
the same edit that enters a task. Tasks are the one class whose whole
population can vanish — the queue empties and a struck task's only
record is its commit — so the sequence ledger is what records what
the task sequence has spent. It is **not** a status column.

## Tasks

### T2 — Obtain legal review of CLA.md

Before the first external contribution is accepted: review of
[CLA.md](../CLA.md) by a qualified lawyer, including the
deliberately unfilled governing-law clause (section 11) and the
assignment fallback for jurisdictions barring transfer (section 2,
§29 UrhG being the standing example). Record the outcome as a
D-number and fill section 11 from it.

### T4 — Rewire the C++ front-ends onto the C ABI

In `D:\Projects\remanence`: point the CLI and GTK4 GUI at
`remanence-ffi` (`include/remanence.h` plus the built library,
noting the MinGW-links-MSVC-cdylib path is verified) and retire the
C++ `lib/`. Work on the front-ends lands in that repository; any
surface gap the rewiring exposes lands here as an S2 change under
the landing rules.

### T8 — Give the storage model an optical drive and the medium it takes

`device_slots()` enumerates no optical device and `formats()` reads
no ISO 9660, so a caller assembling a machine that has a CD-ROM
cannot state one. The DOS letter composition already reasons about a
drive the storage model cannot hold: a report places an optical
letter from an `MSCDEX /L:` line read out of the guest's startup
files, and `LetterOutcome::OpticalDrive` names that outcome. The
letter is therefore inferred from a configuration file rather than
placed on a device the machine bears, and a machine holding an
optical drive that no `MSCDEX` line mentions is unaccountable.

Add the device and the medium it takes, so an optical drive is
configured like every other device and its letter is placed on
something the machine holds. The `MSCDEX` reading stays: it states
*which* letter, where the device is what states there is one.

Two questions belong to the work rather than to this entry —
whether a held drive no `MSCDEX` line mentions takes a letter at
all, and how much of ISO 9660 is owed. Holding and placing the
device is what the letter composition needs; reading the medium's
namespace is a separable claim and may be scoped as its own.

## Rejected

A thin index into [DECISIONS.md](DECISIONS.md) — what was refused,
and the D-number that refused it.
