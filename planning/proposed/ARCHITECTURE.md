# ARCHITECTURE (proposed)

> **Status:** drafted 2026-07-30 at the owner's direction, as input
> to the vision dictation (T1). These are candidate architectural
> principles; nothing here binds. A principle is pledged by moving
> to `pledged/` and **armed** only when it reaches root
> [ARCHITECTURE.md](../../ARCHITECTURE.md) — at which point a
> divergence in the code becomes a bug. Numbers come from the one
> global P-sequence and are never reused.

## P1 — Self-contained format implementations

Every format the library claims, it implements itself — from
published format documentation, in the library, with no external
tool, helper process, or runtime dependency behind any claim. A ZIP
is read by our reader, a DEFLATE stream by our decompressor, a qcow2
by our driver — never by shelling out. This is what makes the
library embeddable from C and Python without an environment around
it, and it is already true of everything shipped.

*(Consequence, worth stating at adjudication: reliquary's at-rest
architecture — qemu-nbd serving qcow2 to an NBD client — is
deliberately not ported. The native driver replaces both halves.)*

## P2 — Reading is harmless

Opening, identifying, listing, and extracting never mutate an image
— not a byte, not a timestamp of the payload. Write access is a
separate, explicit request, and every write path offers a commit
point that can be rolled back until it is committed. An archivist's
tool that damages what it examines has failed at the door.

## P3 — Claims are enumerated and refusals fail closed

What the library recognizes is a named, enumerated claim — formats,
versions, feature subsets — and anything outside the claim is a
named refusal, never a guess, a silent skip, or an untested
approximation. A partition type we cannot read is refused rather
than skipped, because skipping renumbers every volume after it; a
qcow2 feature bit we do not honor names itself in the error.

## P4 — Identification carries its evidence

No verdict without the observations that produced it. Every
identification names its evidence in human-readable terms, and
confidence is bounded and comparable. "h8d, confidence 100" is not
an answer; "matched expected size of 102400 bytes; matched file
extension '.h8d'" is.

## P5 — One semantic surface, three presentations

Every core capability is reachable from Rust, from C, and from
Python, with the same semantics, and a change to the surface lands
on all three presentations in the same change — never deferred. No
capability is binding-private.

## P6 — Unexpected means stop: fail immediately, write nothing, say why

When the library meets a situation it does not expect — a structure
that contradicts itself, a value no claim covers, a state an
operation cannot account for — it **fails immediately**: it writes
nothing, and it gives a clear indication of the reason. No partial
update, no best-effort continuation, no repair attempted on the
caller's behalf, and no error that names a symptom when the cause
is known.

Two consequences make the rule operative rather than aspirational:

- **Surprises are sought before mutation begins.** A mutating
  operation validates everything it can up front, so the unexpected
  is discovered while nothing has been written yet. Where P3 names
  what is outside the claim, this principle governs everything
  inside it that still goes wrong.
- **The reason is a diagnostic, not a shrug.** Every failure names
  what was expected, what was found, and where — an error a caller
  can act on, in the same discipline the error taxonomy already
  carries on the read paths.

P2's commit point is the backstop, not the excuse: roll-back exists
for the interruptions the world inflicts, never as license to start
writing before the checks are done.

## P7 — The file must never change under our feet

The library cannot support a file changing underneath it while it
works — not while writing, not while merely reading. So **denying
write permission to every other process is mandatory in all
scenarios**, from the moment a file is opened for analysis, and a
file for which that denial cannot be obtained is **not opened at
all**: fail fast, with the reason named (P6). A disk image held
open for writing by a running VM is the designed example — that
open *must* fail, immediately, by intent.

With deny-write-to-others secured, the library's own access decides
the session's mode:

- **Preferred: write permission for ourselves too.** Read/write
  access for the library, writes denied to everyone else, other
  processes' reads still admitted where the platform can express
  that. The full session: analyze, manipulate, save.
- **Fallback: read-only for ourselves.** When our own write
  permission cannot be had — a read-only file, read-only media —
  but the deny-write claim still can: analysis proceeds, and **all
  remanence write/change actions are denied**, each refusal naming
  the reason (P6).
- **Neither obtainable — another process holds write access — is
  the fail-fast case.** There is no analyze-anyway mode: a baseline
  that can move mid-analysis would make every result a maybe.

The claim is held for as long as the file is in use and released
only when the library is **completely done** with it — the session
closed, the handle dropped. There is no finer-grained lifecycle: no
claim-on-modify, no release-on-save. Open means claimed; done means
released.

What the invariant buys, in **both** modes:

- **The bytes analyzed are the bytes on disk, for the whole
  session.** No other writer can move the ground under an
  identification, an in-memory edit, or a save — the evidence (P4)
  stays true of the file it names, and P6's validate-then-write
  never validates state that can change before the write.
- **Files the library creates are born under the same claim** — a
  temporary working copy, a save-as target — and hold it until the
  library is done with them.
- **The library's own writes proceed under the claim held since
  open**, still running the full discipline at the moment of
  physical write — validation first (P6), a commit point until
  completion (P2).

Platform reality, in the platforms' own terms: on **Windows** the
mapping is native and mandatory, kernel-enforced against every
process — open read/write with `FILE_SHARE_READ` (preferred), retry
read-only with `FILE_SHARE_READ` (fallback), and a sharing
violation on both means another process holds write access: the
fail-fast case. On **POSIX** there are no sharing modes and file
locks are **advisory** (`flock`/`fcntl`, binding only processes
that check them): both modes take the exclusive advisory lock as
the deny-write claim — which also holds off cooperating readers,
there being no advisory spelling for admit-readers — and the open
mode (`O_RDWR` vs `O_RDONLY`) is decided by file permissions.
The running-VM case fails correctly there too, because QEMU's own
POSIX file driver takes image locks and is therefore a cooperating
process; against non-cooperating writers the claim is protocol, not
enforcement.

*(Open point for adjudication: on platforms where readers are
admitted, they remain admitted during the physical save itself — a
reader arriving mid-save can observe a torn file. The alternative
is a brief full exclusion around the save. As drafted, reads stay
admitted throughout.)*
