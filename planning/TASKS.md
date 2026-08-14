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

### T7 — The DOS letter composer asserts what it could read

`filesystem/dos_letters.rs` opens on "there is no evidence to read",
which holds for the drive-letter map and not for the rule's inputs.
The DOS variant is readable from the boot volume's `IO.SYS` /
`MSDOS.SYS` / `COMMAND.COM`, and every `ResidentCondition` —
`LASTDRIVE`, `SUBST`, `JOIN`, `ASSIGN`, a block-device driver, a
network redirector — is declared in `CONFIG.SYS` or `AUTOEXEC.BAT`:
text files on a FAT volume this library already reads. Making the
caller assert them contradicts the module's own second constraint,
that evidence outranks a rule.

What the caller states is a **machine**: devices, their slots and
attachment order, and the media in them. Everything else is derived.
Which device boots is not an exception — bootability is evidence (the
boot signature, the active partition, a kernel in the root directory),
and the era's firmware order is a claimed rule like any other. From
the booting volume follow the DOS version and what its `CONFIG.SYS`
declares, and from those the letters. `DosAssignmentRule` becomes what
was detected plus an explicit override, and undetermined narrows to a
machine with nothing bootable or a version outside the claim.
Detection is itself an enumerated claim (P3), wanting its own
recognition vocabulary and named refusals.

One assertion survives, and it moves rather than stays: an emulated
machine's boot order is set by its host — reliquary declares `boot` in
a blueprint and mutates it with `set-boot-order`, so a machine can
boot its fixed disk with a bootable floppy in the slot. That is a
property of the machine model, defaulting to the claimed firmware
rule, and not an argument to the composer. Separately: no claimed rule
names FreeDOS.

The composer is public surface, so the landing rules bind. If the
reshaping proves larger than a task it is a feature, and this entry
retires into it.

## Rejected

A thin index into [DECISIONS.md](DECISIONS.md) — what was refused,
and the D-number that refused it.
