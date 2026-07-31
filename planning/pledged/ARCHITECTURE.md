# ARCHITECTURE (pledged)

> **Status:** drafted 2026-07-30 and pledged 2026-07-31, both at
> the owner's direction, from the same demand as the use cases
> beside this file ([USE-CASES.md](USE-CASES.md)). One principle,
> owed by the project;
> a principle is **armed** only when it reaches root
> [ARCHITECTURE.md](../../ARCHITECTURE.md) — at which point a
> divergence in the code becomes a bug. Numbers come from the one
> global P-sequence and are never reused.

## P9 — Interruption never invents a third state

P2 makes commit the only moment the image changes; this principle
armors that moment. An interruption at any point during commit — a
killed process, lost power — leaves state the next open reconciles
**before exposing the disk**, and after reconciliation the image is
wholly the old state or wholly the committed new state, never a
partial third state.

The mechanism is the library's choice, beneath the commit point D2
already settled: the overlay remains where writes buffer and where
rollback lives; what this principle adds is a durability boundary
under the overlay's write-through, such as a durable undo journal.
Nothing here reopens D2. Any recovery artifact is private transient
state: no user-owned file, no cleanup verb, no contract about its
shape or location.

Evidence is out-of-process, by definition: an in-process rollback
test proves nothing about the crash case. Fault injection
terminates a separate process after each durability boundary in
commit and proves the next open reconciles correctly.
