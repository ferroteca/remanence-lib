<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The file-container presentation

> **Status:** delivered; the feature that carried it has been struck and its
> handle retired (D11). This remains the written statement of the contract —
> implemented in `crates/remanence/src/file_container.rs`, and what a new
> provider is written against. It describes no application surface, so it
> stays here rather than moving out.

## There is no file-container layer

In-force P19 makes file containers a **seam**: adapters *expose* a
file-container view, and every file-bearing result *presents* the P19
interface. Nothing in it materializes a container above the systems that
present it, and this design does not add one.

A ZIP grammar, a FAT volume, a Commodore directory, an ISO 9660 filesystem,
and a composed multi-volume namespace are real systems that already hold
their own structure. Each of them can **present a file-container view**.
There is no intermediate representation for them to be copied into, no
generated tree standing above them, and no second place where a listing
lives. What this contract is, is the **interface they present through** and
the **vocabulary they answer in** — not a container.

This is not a small distinction:

- **It keeps reads bounded (P27).** A provider answers for the directory it
  was asked about by reading that directory. Building an item pool for a
  volume holding fifty thousand files, before the caller can list one of
  them, is the read-whole the library refuses everywhere else.
- **There is nothing to invalidate.** No copy exists to drift from the
  system it describes, so a floor that changes — a composition later
  descending to flux — needs no regeneration protocol. The caller asks
  again.
- **P23 is satisfied without argument.** A presentation is not an active
  layer, and P19's interface never made it one. The lowest durable layer the
  session has materialized remains the source of truth (D10), and no
  presentation is ever the truth or ever written through.

What providers share is a contract and a vocabulary, which is exactly what
stops each of them keeping a private notion of what a listing is.

## What a provider presents

The interface answers about one system's namespace, on demand, in the
vocabulary below. Its shape is navigational rather than wholesale: roots,
the entries of one container, one item's metadata, one item's hook, one
item's content, and — on request — the account.

```text
FileContainerView
  roots()                      -> ordered list<ItemRef>
  entries(container: ItemRef)  -> ordered list<Entry>
  item(ItemRef)                -> ItemFacts
  content(ItemRef)             -> ContentSource
  floor()                      -> Floor
  account()                    -> CoverageAccount        // computed on request

Entry
  name: RecordedName
  target: ItemRef
  ordinal: u64                 // the source's own listing order

ItemFacts
  kind: Container | File | OpaqueRegion
  size: optional SizeClaim     // File only
  hook: ordered list<FloorExtent>
  facts: ordered list<DeclaredFact>
  issues: ordered list<Issue>
  provenance: Provenance
```

`ItemRef` is the **provider's own identity** for an item — a directory
entry's location, a central-directory index, an inode — opaque to callers
and meaningful only to the provider that issued it. Nothing here assigns
identities, because nothing here holds a pool to index.

**Names and items stay distinct.** `entries` returns names reaching targets,
so one item may be reached by several names where a source records hard
links, a flat filesystem is one root whose entries all reach leaves, and
hierarchy is just a container whose entries reach containers. Listing order
is evidence, preserved as `ordinal`; a presentation never re-sorts a
source's listing.

A name is carried as its recorded bytes plus the encoding the provider
claims — PETSCII, an OEM code page, UTF-16 — with the decoded presentation
beside them, never in place of them. A ZIP name is UTF-8 only when the
grammar's own flag says so. Irregularities (trailing spaces, shift
characters, bytes outside the claimed encoding) stay in the bytes and are
reported as issues, never lossily substituted and forgotten.

## The floor, and the hook

Every presentation is a view *of* something: the lowest durable layer the
session has materialized, which is the truth (D10). That floor may be an
archive's own named-entry state, addressed CHS records, logical blocks, or
timed flux — whichever P23 made active for the composition that was asked
for.

Each item's **hook** is the extents it occupies in the floor, in the floor's
own addressing. One concept, not two: an archive member's byte range and a
file's cluster run are the same fact about different floors. `ContentSource`
resolves through the provider that owns the floor; the presentation is a map,
never a pipe, and holds no content.

## The account

`account()` is the P19 scope-of-claim amendment made concrete, and it applies
to every presentation because every presentation has a floor. Every addressable unit
of the floor falls in exactly one class: the **data hook** of a named item,
the **structures the interpretation claims for itself**, space the
allocation metadata **claims free** (that claim, never a verdict that the
extent is disposable), or an **opaque region**.

Totality is true by construction: the provider claims what its
interpretation covers, and whatever remains becomes opaque regions. An
overlapping claim is refused at the claim, naming both sides.

The account is the one computed value here, because totality is inherently a
whole-artifact question — it is a report produced on request under P27, never
resident state, and computing it mutates nothing (P2).

**Being a presentation rather than the truth is what permits incompleteness.**
A truth layer must account for everything because it *is* everything; a view
may present what it can explain and name the rest. A mixed-structure disk
whose standard directory reads perfectly while parts of the medium are not
that filesystem at all is the ordinary case, not a damaged one.

The obligation does not vary with the kind of floor. A self-extractor stub,
padding between members, and data appended past an archive's directory are
opaque regions in the same sense a protection track is opaque to a Commodore
directory. An opaque region is itemized without a name — never a namespace
entry, since the pseudo-file rule stands; never reported as free space; never
silently dropped. Deleted-but-present directory entries are accounted inside
the structures they occupy and are not itemized; recovery is a separate claim.

## The metadata vocabulary

Providers answer in a shared vocabulary that is a conglomerate of what they
can express, under the two-outcome rule `FluxCapture v1` established one seam
down: a source fact either maps to a named field, or is retained as an
ordered, provider-namespaced `DeclaredFact` with its source spelling and
order preserved. A provider refusing a source feature names the feature; one
that claims a feature may not discard it.

The named core is deliberately minimal — kind, recorded names, `SizeClaim`,
and the hook. `SizeClaim` records what the claim is *about*: an exact stored
size, a size in allocation units, and a rounded record count (CP/M's
128-byte records) are different claims, not one number. Everything else is a
declared fact under its provider's namespace: timestamps in their source
precision, epoch, and zone semantics; attribute and permission flags;
ownership; file types and state bits (a CBM PRG/SEQ/USR/REL type with closed
and locked flags); record lengths; comments. Structures with no named home —
a ZIP extra field, a 7z property — are `ForeignRecord`s carrying namespace,
type, source range, payload, and any safely decoded summary.

The list is additive: a newly admitted provider's uncarried fact is retained
foreign first and fully admitted only when a later revision gives it a named
home. v1 claims one content stream per file item; alternate data streams and
forks enter by that route or are refused by name.

## Nesting is orthogonal to the physical layering

A presentation belongs to one state instance and has one floor. It does not
reach through its own items into other artifacts.

An ISO inside a 7z is a file container inside a file container, and they are
not one presentation. The 7z presents a view of its own named-entry state;
the ISO is one `File` item in it — bytes with a hook — and nothing more until
it is independently recognized (P12). Recognition creates a separate instance
with its own authoritative layer (P13), its own floor, and its own
presentation. Between the two sit an optical or block layer and a filesystem,
none of which appears in either view.

So file-container presentations alternate with physical layers rather than
containing them: container → media → filesystem → container → media, as deep
as an artifact goes. Depth in a namespace is not depth in the storage stack,
and a presentation never exposes an item's interior — only the item.

Joining instances into one navigable result is P25's artifact mapping, not
this seam's work. What this owes P25 is that each presentation is complete
and self-contained about its own floor, so a mapping has something
well-defined to join.

## Mutation

Nothing is written through a presentation. The provider owning the floor
performs a write against the floor, and a later call presents the result.
There is no buffered view state, no spill, no commit path here, and no
invalidation protocol — which is the whole benefit of not having a layer.
Several presentations may coexist over one floor, since none is mutable and
none is the truth.

## Deliberately outside v1

- Any public surface change: the delivered `Archive`, `ArchiveEntry`, and
  filesystem listings keep their shape until a feature presents them through
  this seam.
- Writing through a presentation, in any form.
- Recovery of deleted entries, repair, or interpretation of opaque regions.
- Alternate data streams and forks beyond the additive-admission rule.
- A universal metadata vocabulary across unrelated providers: a declared fact
  means what its owning namespace says, nothing more.
- Namespace-mapping derivation, drive letters, and multi-artifact
  composition, which remain their own proposals.
