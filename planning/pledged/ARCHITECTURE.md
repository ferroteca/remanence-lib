# ARCHITECTURE (pledged)

> **Status:** drafted 2026-07-30 and pledged 2026-07-31, both at
> the owner's direction, from the same demand as the use cases
> beside this file ([USE-CASES.md](USE-CASES.md)). Two principles
> and one amendment to an in-force principle, owed by the project;
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

## P10 — Every refusal is machine-addressable

A refusal's human diagnostic (P6) is not its interface. Every error
carries, beside its message, a stable machine-readable category
from one enumerated set — the same category in Rust, in C, and in
Python (P5) — so an embedder maps behavior without parsing text no
release promises to keep. The category set is itself part of the
surface: adding a category is a surface change; rewording a message
never is.

The initial set, covering every refusal the library makes today:
`locked`, `invalid-image`, `unsupported`, `read-only`, `not-found`,
`not-directory`, `is-directory`, `no-space`, `io`.

## Amendment — P7 takes the caller's intent, and the claim covers the chain

P7's core is untouched: denying write permission to every other
process is mandatory in all scenarios, from open, and a file for
which the denial cannot be obtained is not opened at all,
immediately, with the reason named. Three clauses change or arrive:

- **The session's mode is declared, not discovered.** Today the
  library's own access decides the mode — read/write preferred,
  read-only fallback. Amended: the caller declares read or write
  intent at open. A writable open that cannot secure its own write
  access fails **at the open**, never by silent fallback, so a
  refused write claim surfaces before the first mutation rather
  than at it. A read open takes no stronger access than it needs.
- **A writable session admits no observers.** A write claim
  excludes every other read or write that could observe a partial
  mutation, for the session's whole life; a read session keeps
  admitting other readers, as today. *(This settled the formerly
  open question in [DECISIONS.md](../DECISIONS.md) — readers during
  the physical save — in the direction of exclusion; the commit
  that pledged this amendment is the record.)*
- **The claim covers every file of a backing chain, consistently.**
  The top image is claimed per the declared intent; every backing
  file is claimed immutable through this access — writes denied to
  others, the library's own access read-only. Contention anywhere
  in the chain is an immediate, named failure, never a hidden wait.
