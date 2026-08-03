<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# FileContainerLayer v1 foundation

> **Status:** pledged, not delivered. This design serves F35 and authorizes no implementation outside that feature.

## Purpose

F35 establishes the private, common file-container model behind the P19
seam: the one representation that file-bearing providers materialize into or
present through. Serialized-container catalogs (ZIP and 7z today), P18
filesystem adapters, and P19 namespace composers are all providers of this
model; none of them keeps a private second notion of what a file listing is.
It is the file-container analog of `FluxLayer v1`: a common internal target,
not a public iterator, not an interchange format, and not a new archive
grammar.

The model serves both standings P23 distinguishes. Over a serialized
container the model **is the durable active layer** — the named-entry state
is the session's mutable truth. Over a filesystem on materialized media it
is a **derived presentation** whose facts project into that media's active
state and whose scope over the backing is stated by the P19 amendment's
coverage account. One model, two standings; footprints and coverage exist
only in the second.

## Items and names

The namespace is edges over items, not a tree of records:

```text
FileContainerLayer
  roots: ordered list<ItemId>            // each a container item
  items: map<ItemId, Item>
  coverage: optional CoverageAccount     // derived standing only
  facts: ordered list<DeclaredFact>      // container-level metadata
  foreign_records: ordered list<ForeignRecord>
  provenance: Provenance

Item
  id: ItemId
  kind: Container | File | OpaqueRegion
  edges: ordered list<Edge>              // Container only
  content: optional ContentSource        // File only
  size: optional SizeClaim               // File only
  footprint: optional list<BackingExtent>
  facts: ordered list<DeclaredFact>
  issues: ordered list<Issue>
  provenance: Provenance

Edge
  name: RecordedName
  target: ItemId
  ordinal: u64                           // source order

RecordedName
  bytes: recorded name bytes, exactly as stored
  encoding: the adapter's claimed encoding
  decoded: presentation string, carrying conversion provenance
```

`ItemId` is library-owned and stable for the lifetime of the opened layer,
like `ObservationId` one seam down. Hierarchy is optional by construction: a
flat filesystem is one root container whose edges all reach leaves, and a
deeper tree is container items carrying edges of their own. Several edges
may reach one item where a source records hard links; several roots exist
where a composed namespace has them, and P19's existing rule governs their
exposure. An item with no edge is reachable only through the coverage
account — which is what keeps an opaque region out of the namespace without
losing it from the view.

Directory order is evidence and is preserved as `ordinal`; the model never
re-sorts a source's listing. A name is carried as its recorded bytes plus
the encoding the adapter claims — PETSCII, an OEM codepage, UTF-16 — with
the decoded presentation beside them, never in place of them. Name
irregularities (trailing spaces, shift characters, bytes outside the claimed
encoding) stay in the recorded bytes and are reported as issues, not
repaired.

## Item kinds

- **Container** — an item that owns edges. It claims namespace structure,
  not disk allocation.
- **File** — an item with extractable content. Its `ContentSource` is a
  bounded, on-demand source in the existing catalog discipline: content
  decodes through the provider when read, under P27's working set, never
  resident whole as a design assumption. `SizeClaim` records the byte size
  the provider actually claims, with its basis — an exact stored size, a
  size in allocation units, or a rounded record count (CP/M's 128-byte
  records) are different claims, not one number.
- **OpaqueRegion** — the itemized remainder of the coverage account: a
  bounded extent of the backing layer the interpretation does not claim,
  per the P19 amendment. It has a footprint and provenance, never a name,
  never a decoded content claim. Reading it is reading backing-layer
  evidence; interpreting it belongs to whatever lower seam claims it.

## The metadata superset contract

The model is a conglomerate of the file metadata its admitted providers can
express, under exactly the two-outcome rule `FluxLayer v1` established: a
source fact either maps to a named model fact with its source identity and
provenance, or is retained as an ordered, adapter-namespaced record. An
adapter refusing a source feature names the feature; an adapter which claims
a feature may not discard it.

The named common core is deliberately minimal — only what navigation and
extraction need: item identity, kind, edges with recorded names,
`SizeClaim`, and `ContentSource`. Everything else is a `DeclaredFact` under
the owning adapter's namespace with source spelling and order preserved:
timestamps in their source precision, epoch, and zone semantics; attribute
and permission flags; ownership; file types and their state bits (a CBM
PRG/SEQ/USR/REL type with closed and locked flags); record lengths;
comments. Structures the model does not interpret at all — a ZIP extra
field, a 7z property it has no home for — are `ForeignRecord`s in the flux
design's sense: namespace, type, source range, payload, optional decoded
summary.

This list is additive, exactly as it is one layer down: when a newly
admitted provider records a fact these forms cannot carry, the adapter
retains it first, and the fact is fully admitted only when a later revision
gives it a named home. v1 claims one content stream per file item; a source
whose files carry several (alternate data streams, resource forks) is
admissible only by that additive route or refused by name.

## Footprints and coverage

In the derived standing, every item may carry a **footprint**: the extents
it occupies in the materialized backing layer, stated in that layer's own
addressing — CHS records, block numbers, or track-relative angular flux
regions — because that is the only vocabulary in which the coverage
account's totality can be checked. The adapter's allocation-unit mapping
(cluster size, interleave, reserved areas) is provenance on the footprint,
not a second address space in the model.

`CoverageAccount` is the P19 amendment made concrete: a total, exclusive
classification of the backing extent into item data, namespace structures,
claimed-free space, and opaque regions. It is computable on demand (P27)
and mutates nothing (P2). Deleted-but-present directory entries — a
scratched CBM entry, a FAT `0xE5` slot — are accounted inside the namespace
structures they physically occupy and are not itemized; recovery is a
separate claim nothing here makes.

In the active-layer standing there is no footprint and no account. A
serialized container's unaccounted source bytes — a self-extractor stub,
padding, garbage between entries — are the adapter's evidence and issues
under P3 and P4, not opaque regions.

## Backing, mutation, and bounds

The model's structure and metadata are session state under P27's bounded
working set; content stays behind bounded `ContentSource` reads in the
existing catalog discipline. Where the model is the active layer, mutation
follows P2 unchanged: altered entries buffer in the session cache, spill to
private session storage, and reach the artifact only at commit through the
owning adapter's encoder, which refuses any save it cannot make honestly
(P13). Where the model is a derived presentation, mutation is the backing
media's business through the owning filesystem adapter's write claim; this
foundation adds representation, not a write path.

## Deliberately outside v1

- Any public surface change: the delivered `Archive` and filesystem
  listings keep their shape until a feature moves them onto this model.
- Recovery of deleted entries, repair, or interpretation of opaque regions.
- Alternate data streams and forks beyond the additive-admission rule.
- A universal metadata vocabulary shared across unrelated providers: a
  declared fact means what its owning namespace says, nothing more.
- Namespace-mapping derivation, drive letters, and multi-artifact
  composition, which remain their own proposals.
