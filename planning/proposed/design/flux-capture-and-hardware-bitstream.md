<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Flux capture and hardware-bitstream design

> **Status:** proposed, not pledged. This design serves F28 and authorizes no implementation.

## Purpose

F28 gives raw magnetic capture formats one durable home and adds two durable family-specific states above it: hardware bitstream, then encoded bytestream. The split is deliberately below synchronization, sector recovery, and filesystem interpretation: it lets a programmed drive retain recording-level bytes without asking a capture adapter to decide what the recording means.

This design aligns with P22's timed flux floor and amends P23: hardware bitstream and encoded bytestream join flux as durable magnetic active layers. Each materialization becomes the session's durable mutable truth; a G64-style bitstream source may begin at the middle layer directly.

## Durable flux state

An SCP, A2R, or KryoFlux adapter decodes only its container and capture semantics into flux state. It preserves, where present:

- timed flux-transition observations, with their source resolution and declared timing basis;
- track and side identity, capture/revolution identity, and recorded order;
- index, hard-sector, and other sensor/marker channels parallel to flux;
- capture metadata, sampling conditions, and provenance; and
- absence, irregularity, weak or contradictory observations, and unsupported format features as evidence or named refusals.

A capture is not an assertion that one ideal revolution existed. Several recorded revolutions are parallel evidence, not duplicates to average or a license to choose the cleanest result. A media profile and hardware profile must say whether the drive observes a selected revolution, a repeatable sequence, a seeded variation, or another specified behavior. Without such a rule, the requested presentation is refused rather than normalized silently.

The flux state is circular and track-relative, as P22 requires. Index and hard-sector topology stay in their own channels. The session holds and caches this state under P27; it does not retain a whole capture merely because it was stored that way on disk.

## Internal flux-layer format

`FluxLayer v1` is the library's private, logical representation of that
state. It is the common target of capture-container adapters, not a new image
format, interchange format, or public iterator API. Its serialized backing
is private session storage; an adapter still owns the source file's grammar
and an encoder still owns projection back to that grammar.
### Capture-superset contract

The layer is a conglomerate of the capture facts that its admitted source
formats can express: SCP and KryoFlux timed flux and index data; A2R's raw and
solved streams, hard-sector observations, track locations, drive and capture
information, metadata, and extension chunks; and the equivalent facts in MFI
or a later adapter. It is not a least-common-denominator transition list. A
source fact has exactly one of two outcomes when an adapter opens it:

1. it maps to a named `FluxLayer` field with its source identity and
   provenance; or
2. it is retained verbatim as an ordered `ForeignRecord` in the capture
   envelope, with namespace, type/version, source location, payload, and any
   safely decoded summary.

An adapter refusing a source feature names the feature and leaves the source
unopened; an adapter which claims the feature may not discard it. This rule
covers a recognised but not yet modelled chunk just as much as an unknown
future extension. The envelope is private session state, not a promise that
unrelated formats share a public metadata vocabulary.

```text
CaptureEnvelope
  sources: ordered list<SourceDescriptor>
  media_hints: ordered list<DeclaredFact>
  capture_settings: ordered list<DeclaredFact>
  metadata: ordered list<MetadataRecord>
  foreign_records: ordered list<ForeignRecord>
  derived_candidates: ordered list<DerivedCandidate>
  surface_candidates: ordered list<SurfaceCandidate>

SourceDescriptor
  format: namespace + format version
  artifact path/entry identity and byte range
  declared time basis, capture mode, and integrity facts

ForeignRecord
  source: SourceDescriptor
  namespace + type + version + ordinal
  source byte range
  payload: bytes
  decoded_summary: optional DeclaredFact list

DerivedCandidate
  kind: solved_flux | source-defined derived form
  source records and transformation identity
  payload sections and provenance

SurfaceCandidate
  kind: resolved_magnetic_cells
  track-relative angular regions and state
  source records and provenance
```
`MetadataRecord` preserves both the source spelling and order of every key and
value; it is not collapsed into a lossy map. A source's media type, drive
profile, physical location convention, density hint, capture mode, hardware
or software version, timestamp, checksum, and write-splice fact are declared
facts, not conclusions that the layer is licensed to trust. An adapter may
interpret a fact only under its source namespace and keeps the original fact
beside that interpretation.

The currently admitted capture forms have named homes:

- An SCP revolution and an A2R timing or xtiming capture are `CaptureRun`s:
  exact transition intervals, source resolution, physical location, capture
  kind, and every index or hard-sector event remain separate facts. A2R's
  multiple index signals are not reduced to one revolution duration.
- A KryoFlux stream is a `CaptureRun` plus its source stream positions,
  asynchronous index events, transfer result, and hardware information.
- An A2R legacy bits capture is a source-defined derived candidate, retained
  as captured bits rather than falsely reclassified as flux timing.
- An A2R `SLVD` stream is a `solved_flux` derived candidate, tied to its
  source records and never substituted for raw runs.
- MAME MFI's resolved magnetic state is a `SurfaceCandidate` with ordered,
  track-relative angular regions whose states are orientation A, orientation
  B, neutral, damaged, or a source-defined state. Its splice position is a
  `WriteSplice` marker. It is a named supplied representation, not a decoder
  that `FluxLayer` applies to other captures.

This list is deliberately additive: when another source can record a fact not
covered by these forms, the adapter retains it as a `ForeignRecord` first;
the feature is not considered fully admitted until a later revision gives that
fact a named home. That prevents a convenient opaque bucket from becoming the
format's permanent blind spot.

`DerivedCandidate` keeps source-provided solved flux or another explicitly
labelled derivative alongside the raw capture. It never replaces raw runs,
never becomes an independently mutable peer layer, and is usable only through
a profile or presentation which declares its selection rule. This preserves
A2R's optional solved stream without treating a solved result as proof that
its raw capture had one ideal rotation.

On same-format save, every unchanged `ForeignRecord`, metadata record, and
unselected derived candidate is emitted unchanged or the adapter refuses that
save. A changed flux section may be encoded only if the adapter can preserve
all other envelope facts and express the change honestly; otherwise P13 makes
the session read-only or requires explicit conversion to an authoritative
format that can.

```text
FluxLayer
  envelope: CaptureEnvelope
  time_base: TimeBase
  tracks: ordered map<TrackKey, Track>
  provenance: Provenance

Track
  key: TrackKey
  runs: ordered list<CaptureRun>
  observations: ordered list<Observation>
  issues: ordered list<Issue>

CaptureRun
  id: CaptureRunId
  ordinal: u64
  transitions: ordered list<Tick>
  markers: ordered list<Marker>
  provenance: Provenance

Observation
  id: ObservationId
  ordinal: u64
  source: CaptureRunSlice
  span: Tick
  transitions: ordered list<Tick>
  markers: ordered list<Marker>
  provenance: Provenance

Marker
  position: Tick
  kind: Index | HardSector | WriteSplice | SourceMarker { namespace, code }
  payload: bytes
  provenance: Provenance
```

`TrackKey` is the adapter-declared, stable physical track/side identity. It
is not CHS: a capture may use a fractional step, an unnumbered head, or other
source identity which the layer carries without rounding it to cylinders or
heads. `ObservationId` is library-owned and stable for the lifetime of the
opened layer; `ordinal` preserves the source-record order when several
observations share a track. Neither one claims that an observation is a good,
complete, or uniquely selected revolution.

`TimeBase` is an exact positive rational count of ticks per second, retained
from the adapter's declared timing basis. Every `span`, transition position,
and marker position uses its integer ticks; v1 never converts capture timing
to floating point or a library-chosen sample rate. `span` is the circular
observation's declared circumference. A transition position is measured from
the observation origin and must be strictly less than `span`; consecutive
positions are strictly increasing. The cyclic interval from the last
transition back to the first is implied by `span`, so the source's wrap is
preserved without a duplicate boundary pulse. An observation with no
transitions is valid evidence. A zero or unknown circumference, an out-of-
range event, or an ordering violation is an adapter-level invalid or
degraded observation, never repaired by sorting, wrapping, or inventing a
period.

A `CaptureRun` preserves one source transfer in its actual time order, including
transitions before the first index and after the last index. `CaptureRunSlice`
records the exact event range from which a circular observation was bounded;
the observation's payload may share those indexed chunks rather than duplicate
them. A run with no two trustworthy index boundaries remains inspectable
capture evidence but supplies no circular observation. Container transport
controls — for example KryoFlux NOP padding and OOB stream-read/end records —
are not flux events: their decoded source positions, declared result, and
relevant device information remain provenance or issues beside the run. Index
OOB data itself becomes the timed marker evidence from which observations are
bounded. This retains the raw run without mistaking a protocol buffer boundary
for magnetic structure.

Marker channels are parallel timed evidence, never special transition values.
Known index and hard-sector events have the named kinds above; every other
source event retains an adapter namespace, source code, opaque payload, and
provenance. Markers may share a position with one another or a transition and
remain in their recorded order. An absent marker channel is absent evidence,
not a regular marker pattern inferred from track geometry. A media or
mechanism profile which supplies a regular pattern records it as synthesized
provenance beside its own markers.

The logical layer is sparse: a missing `TrackKey` means the capture supplied
no observation for that location, while a present track with an empty
observation list records a source-declared track with no usable captures.
Duplicate, weak, contradictory, truncated, or otherwise qualified evidence
stays as ordered observations and issues; no v1 normalization averages
timings, chooses a cleanest pass, deduplicates pulses, fills a gap, or turns
several passes into one ideal rotation. Profiles above flux select or vary
among observations under their declared policy.

### Reference implications

MAME's MFI design confirms the usefulness of explicit track records, absolute
rotational position, and separately persisted write-splice information for
fast drive-facing access. Its resolved magnetic-cell state is deliberately not
`FluxLayer`'s capture representation: resolving transitions into orientation
cells would discard timing disagreement and make a MAME-style normalization a
hidden decoder. When a source supplies a write splice, v1 retains it as a
named source marker or source metadata, not as a reason to convert its flux
into cells.

KryoFlux stream documentation distinguishes flux intervals from asynchronous
index and control/OOB records, and notes that a stream brackets its indexed
revolutions with data before the first and after the last index. That is the
reason for `CaptureRun` and `CaptureRunSlice` above. The library decodes the
container once into its evidence-bearing run and indexed observations; it does
not preserve the container's ring-buffer padding as magnetic data.

SuperCard Pro's SCP format independently validates a sparse track-offset table
and a per-revolution descriptor carrying exact index duration and the recorded
flux extent. `FluxLayer` retains those facts as a track key, observation span,
and provenance, but does not inherit SCP's fixed 0–163 track numbering,
physical-side arithmetic, five-revolution limit, or a common disk-type hint as
its own truth. SCP's index and splice capture modes also show why capture
boundary policy is evidence: an index-queued boundary, an inferred splice, and
a source-supplied write splice stay distinct rather than being normalized into
one assumed rotational origin.

`FluxLayer v1` backing is section-addressable. A section key is
`(TrackKey, ScopeId, SectionKind, ordinal)`, where `ScopeId` is a
`CaptureRunId` or `ObservationId` and `SectionKind` is
track metadata, capture-run metadata, observation metadata, transition chunk,
marker chunk, or issue chunk. Large transition and marker sequences split at deterministic
record-count boundaries; the chunks preserve both source order and their
inclusive tick range. Thus a drive can load one capture run or track observation, one
transition span, or one marker channel without decoding another track,
observation, or whole revolution.

The backing is a length-delimited record stream plus a persistent ordered
section index. A fixed footer locates the index root; its nodes are themselves
length-delimited records and are loaded on demand, so an enormous capture does
not require a resident whole index. An index leaf maps a complete section key
to its byte offset, encoded length, decoded count, tick bounds where useful,
and checksum. Adapters building a session-backed layer emit sections in key
order and append the index only after every referenced section is complete;
an incomplete private backing is discarded, never exposed as a layer.

Transition positions are delta-coded as unsigned ticks inside their ordered
chunks. Marker positions remain absolute ticks, so their recorded order need
not be chronological. Record lengths, index bounds, counts, checksums, and
decoded arithmetic are checked before seeking, allocation, or addition.
Unknown section versions or malformed index records refuse that layer access;
they are not reconstructed by scanning the entire capture or guessing an
index.

The normal P27 cache owns decoded section payloads and index nodes under the
session-wide budget. Clean entries evict and reload from the source-backed or
private session-backed record by the index; an altered entry spills to private
session storage and remains the authoritative session result. The dirty map
is keyed by the same complete section key, so loading, replacing, saving, and
committing one section neither materializes nor invalidates unrelated ones.
A cache miss may read one bounded record range plus the necessary index path,
never a whole flux layer as a design assumption.

A capture adapter may use source-backed sections only when its own container
can truthfully locate and decode a section by key. A sequential or
whole-capture container streams once into the indexed private backing, then
all later random access uses that backing. The backing is an implementation
detail: it may be compacted or migrated internally, provided the `FluxLayer
v1` facts, keys, and order remain unchanged. It is never written beside a
caller's image and never treated as a format an external caller can depend on.

Flux mutation uses copy-on-write at one indexed section key, preserving an
immutable source-derived base and a bounded dirty overlay under P27.

A mutation carries its derivation and may add only facts
the active-layer operation actually creates; it cannot silently overwrite or
discard captured evidence. Commit is available only when the authoritative
capture adapter can encode the resulting layer without unclaimed loss, as
P13 requires.

## Hardware-bitstream layer

The hardware-bitstream layer belongs to a specific drive family. It holds the serial, clocked, circular track-relative bit state at the boundary the family actually makes observable. It is manufactured from selected flux plus the declared mechanics and read-channel configuration, or loaded directly from an authoritative bitstream image. It is durable session state with its own P27 backing and mutable write set, not a cache or a generic public intermediate representation.

For a Commodore 1541, the layer sits after flux-pulse detection and the family's timing/recovery behavior, but before sync recognition and before GCR decoding. A run of zero or one decisions in this layer therefore does not mean a sync mark, GCR symbol, byte, sector, or file. Those are separate, higher interpretations controlled by the drive firmware or another explicitly claimed presentation.

Other families need not share that exact cut. A family may have a different bitstream form, or none; no universal bitstream schema is introduced. The common point is directional: a family profile may materialize its bitstream from flux, and any further FM/MFM/GCR decoding is a higher interpretation rather than an alteration of this durable layer.

An open has exactly one active layer. Flux-to-bitstream materialization is atomic: once validation succeeds, the bitstream replaces flux as the active mutable state and carries provenance for its source observations and profile. A write lands in the active bitstream. Commit is permitted only when every change can project to the image's authoritative layer without unclaimed loss; a flux-authoritative source whose bitstream write cannot be mastered back to flux is read-only or requires explicit conversion. Any bitstream-to-flux mastering transition is explicit, provenance-bearing, and subject to the same loss rule.

## Encoded-bytestream layer

Encoded bytestream is the next durable layer. In the 1541 family, GCR decoding materializes the circular byte sequence from hardware bitstream, with the codec, byte phase, and source bitstream provenance retained. It does not assign the result to a sync run, block header, data field, sector, or file. Those interpretations are above the bytestream. No known disk image format is claimed to begin at this layer; the design leaves that possibility open without inventing a generic source dialect.

## Boundaries and non-goals


## CHS and filesystem above the bytestream

The complete magnetic-disk interpretation path is flux → hardware bitstream
→ encoded bytestream → CHS → filesystem. A declared family decoder performs
synchronization and interprets encoded bytes as headers, data fields, and
sectors to materialize CHS; this is not licensed merely by having bytes.
P18 then recognizes and exposes a filesystem above that CHS view. CHS remains
the durable active media layer; the filesystem is its higher derived seam.
- Capture adapters do not reconstruct sectors, declare an encoding, or erase source disagreement in order to make a convenient byte stream.
- Hardware bitstream and encoded bytestream are durable, but sync, headers, data fields, sectors, and files are higher interpretations.
- Flux and bitstream do not become generic S1–S3 iterator APIs. Public access remains at the named drive-hardware or higher presentation seam.
- SCP, A2R, and KryoFlux are capture-container commitments, not a promise that every variant, controller, or capture-device extension is supported.
- Nothing here changes P13's authoritative-layer rule: a capture-backed open may write only when changes can return to the original capture format without unclaimed loss; otherwise it is read-only or requires explicit conversion.

## Delivery shape

The ownership model may be pledged as one feature, but individual format adapters and drive-family paths are independently sized implementation work. The first adapter must demonstrate preservation of multiple revolutions and marker channels. The first bitstream adapter must demonstrate a G64-style track active directly, and the first hardware path must demonstrate that a 1541 observes pre-sync, pre-GCR-decoding bits materialized from flux rather than from a sector shortcut. Every delivered public opening or hardware presentation changes S1, S2, and S3 coherently.
