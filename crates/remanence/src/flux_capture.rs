// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The private flux-capture model (F30): raw magnetic capture evidence,
//! held without deciding what a medium, drive, codec, or filesystem
//! makes of it.
//!
//! A capture is never an active layer. It is authoritative image state
//! read by inspection and by mastering, and a reduction under P29 turns
//! it into the flux medium a drive is served from. Nothing here averages
//! timings, deduplicates pulses, chooses a cleanest pass, or turns
//! several recorded passes into one ideal rotation.
//!
//! Every item is crate-private and has no consumer until the capture
//! adapters (F31) land.
#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::checksum::crc32;
use crate::error::{Error, Result};
use crate::evidence::Provenance;

/// One tick of a capture's declared [`TimeBase`]. Never a wall-clock
/// unit, and never converted to floating point.
pub(crate) type Tick = u64;

/// The capture's declared timing basis: an exact positive rational
/// count of ticks per second.
///
/// It is retained as the source declared it. v1 never converts capture
/// timing to floating point or to a library-chosen sample rate, because
/// the common capture clocks are not exactly representable either way —
/// KryoFlux's sample clock is `((18432000 * 73) / 14) / 2 / 2` Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimeBase {
    numerator: u64,
    denominator: u64,
}

impl TimeBase {
    pub(crate) fn new(source: &str, numerator: u64, denominator: u64) -> Result<Self> {
        if numerator == 0 || denominator == 0 {
            return Err(Error::invalid_image(
                source,
                format!(
                    "capture declares a timing basis of {numerator}/{denominator} \
                     ticks per second, which states no rate to measure against"
                ),
            ));
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// The declared rate, in the source's own spelling.
    pub(crate) fn ticks_per_second(self) -> (u64, u64) {
        (self.numerator, self.denominator)
    }

    /// Whether two declared bases describe the same rate, compared by
    /// exact cross-multiplication rather than by float or by spelling.
    pub(crate) fn same_rate_as(self, other: &Self) -> bool {
        u128::from(self.numerator) * u128::from(other.denominator)
            == u128::from(other.numerator) * u128::from(self.denominator)
    }
}

/// What a timed marker records. Known mechanical and sensor events have
/// named kinds; every other source event is retained under its adapter's
/// namespace rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkerKind {
    Index,
    HardSector,
    WriteSplice,
    SourceMarker { namespace: String, code: u32 },
}

/// One timed event on a channel parallel to the flux transitions.
///
/// A marker is never a special transition value: it may share a
/// position with a transition or with another marker, and its recorded
/// order is evidence rather than something to normalize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marker {
    position: Tick,
    kind: MarkerKind,
    payload: Vec<u8>,
}

impl Marker {
    pub(crate) fn new(position: Tick, kind: MarkerKind) -> Self {
        Self {
            position,
            kind,
            payload: Vec::new(),
        }
    }

    /// Retains the source record's bytes verbatim beside the marker.
    pub(crate) fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        self
    }

    pub(crate) fn position(&self) -> Tick {
        self.position
    }

    pub(crate) fn kind(&self) -> &MarkerKind {
        &self.kind
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Where something sat in the artifact the adapter read, in bytes.
///
/// The capture side anchors to the source artifact, which is why this
/// is not the file-container layer's floor extent: a floor is addressed
/// in whatever units its presentation uses, and a capture's foreign
/// records point at bytes on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceRange {
    start: u64,
    length: u64,
}

impl SourceRange {
    pub(crate) fn new(start: u64, length: u64) -> Self {
        Self { start, length }
    }

    pub(crate) fn start(&self) -> u64 {
        self.start
    }

    pub(crate) fn length(&self) -> u64 {
        self.length
    }
}

/// A handle for one artifact this capture was assembled from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SourceId(u64);

/// One artifact a capture was read from, named as the adapter reached
/// it.
///
/// A logical capture is routinely many files — a capture set is one
/// disk spread over a stream per head per step position — so this is
/// what keeps a fact attributable to the member it came from once they
/// have been assembled into one layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceDescriptor {
    namespace: &'static str,
    artifact: String,
    range: SourceRange,
}

impl SourceDescriptor {
    pub(crate) fn new(
        namespace: &'static str,
        artifact: impl Into<String>,
        range: SourceRange,
    ) -> Self {
        Self {
            namespace,
            artifact: artifact.into(),
            range,
        }
    }

    pub(crate) fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// The path or archive-entry identity the adapter opened.
    pub(crate) fn artifact(&self) -> &str {
        &self.artifact
    }

    pub(crate) fn range(&self) -> SourceRange {
        self.range
    }
}

/// A source structure kept exactly as it was read, because this layer
/// has no named home for it yet.
///
/// This is the second of the two outcomes an opened source fact may
/// have, and it is deliberately not the comfortable one: a record stays
/// foreign only until a later revision gives its fact a named field.
/// Retaining beats discarding because a discard is silent and permanent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForeignRecord {
    source: SourceId,
    namespace: &'static str,
    type_id: String,
    ordinal: u64,
    range: SourceRange,
    payload: Vec<u8>,
}

impl ForeignRecord {
    pub(crate) fn new(
        source: SourceId,
        namespace: &'static str,
        type_id: impl Into<String>,
        range: SourceRange,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            source,
            namespace,
            type_id: type_id.into(),
            // Assigned when an envelope retains it; a record has no
            // place in the source's order until it has been kept.
            ordinal: 0,
            range,
            payload,
        }
    }

    /// The artifact this was read from.
    pub(crate) fn source(&self) -> SourceId {
        self.source
    }

    pub(crate) fn namespace(&self) -> &'static str {
        self.namespace
    }

    pub(crate) fn type_id(&self) -> &str {
        &self.type_id
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub(crate) fn range(&self) -> SourceRange {
        self.range
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// One key and value exactly as the source wrote them.
///
/// Distinct from a [`crate::evidence::DeclaredFact`], which is a value some namespace
/// has already interpreted: this is the unread text. It is held in an
/// ordered list and never a map, because a source may state a key more
/// than once and each statement is evidence — and because the order is
/// itself part of what was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MetadataRecord {
    namespace: &'static str,
    key: String,
    value: String,
    ordinal: u64,
}

impl MetadataRecord {
    pub(crate) fn new(
        namespace: &'static str,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            namespace,
            key: key.into(),
            value: value.into(),
            ordinal: 0,
        }
    }

    pub(crate) fn namespace(&self) -> &'static str {
        self.namespace
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// The value in the source's own spelling, unparsed. A declared
    /// decimal stays the decimal that was declared, however lossy the
    /// source's own rounding was.
    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }
}

/// What a derivative is, in the terms the source labelled it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DerivedKind {
    /// A stream the source itself resolved from its own raw capture.
    SolvedFlux,
    /// Any other explicitly labelled derivative, under its namespace.
    SourceDefined(String),
}

/// A derivative the source supplied beside its raw capture.
///
/// It is kept because discarding a source's own work would be a loss,
/// and held apart because accepting it as evidence would be a lie: a
/// solved stream is not proof that the raw capture it came from had one
/// ideal rotation. Nothing reaches it by reading a location's
/// observations; only a profile or presentation that declares its
/// selection rule may use it, and this layer declares none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DerivedCandidate {
    namespace: &'static str,
    kind: DerivedKind,
    ordinal: u64,
    range: SourceRange,
    provenance: Provenance,
}

impl DerivedCandidate {
    pub(crate) fn new(
        namespace: &'static str,
        kind: DerivedKind,
        range: SourceRange,
        provenance: Provenance,
    ) -> Self {
        Self {
            namespace,
            kind,
            ordinal: 0,
            range,
            provenance,
        }
    }

    pub(crate) fn namespace(&self) -> &'static str {
        self.namespace
    }

    pub(crate) fn kind(&self) -> &DerivedKind {
        &self.kind
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub(crate) fn range(&self) -> SourceRange {
        self.range
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What an opened capture carries besides its flux: everything the
/// source stated, in the order it stated it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CaptureEnvelope {
    sources: Vec<SourceDescriptor>,
    metadata: Vec<MetadataRecord>,
    foreign_records: Vec<ForeignRecord>,
    derived_candidates: Vec<DerivedCandidate>,
}

impl CaptureEnvelope {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Enters an artifact this capture reads from, issuing its handle.
    pub(crate) fn declare_source(&mut self, descriptor: SourceDescriptor) -> SourceId {
        let id = SourceId(self.sources.len() as u64);
        self.sources.push(descriptor);
        id
    }

    pub(crate) fn sources(&self) -> &[SourceDescriptor] {
        &self.sources
    }

    pub(crate) fn source(&self, id: SourceId) -> Option<&SourceDescriptor> {
        self.sources.get(id.0 as usize)
    }

    /// Keeps one stated key and value, in the source's order.
    pub(crate) fn record_metadata(&mut self, mut record: MetadataRecord) {
        record.ordinal = self.metadata.len() as u64;
        self.metadata.push(record);
    }

    pub(crate) fn metadata(&self) -> &[MetadataRecord] {
        &self.metadata
    }

    /// Keeps a record this layer cannot name, in the source's order.
    pub(crate) fn retain_foreign(&mut self, mut record: ForeignRecord) {
        record.ordinal = self.foreign_records.len() as u64;
        self.foreign_records.push(record);
    }

    pub(crate) fn foreign_records(&self) -> &[ForeignRecord] {
        &self.foreign_records
    }

    /// Keeps a derivative beside the raw capture, in the source's order.
    pub(crate) fn retain_derived(&mut self, mut candidate: DerivedCandidate) {
        candidate.ordinal = self.derived_candidates.len() as u64;
        self.derived_candidates.push(candidate);
    }

    pub(crate) fn derived_candidates(&self) -> &[DerivedCandidate] {
        &self.derived_candidates
    }
}

/// A capture-wide handle for one observation, owned by the library and
/// stable for the life of the opened layer.
///
/// It is identity, not rank: it says nothing about whether the
/// observation is good, complete, or the one a drive should be served
/// from. It exists because the ordinal is only unique within a
/// location, while the backing addresses sections across the capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ObservationId(u64);

/// The most bytes a `u64` varint can occupy: ten groups of seven bits.
const MAX_VARINT_BYTES: usize = 10;

/// Appends one unsigned value, seven bits per byte, low group first,
/// with the high bit set on every byte but the last.
fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Reads one unsigned value, returning it and the bytes it used.
///
/// Refuses a value running off the end of the record and one encoded in
/// more groups than a `u64` holds, either of which would otherwise be
/// read as a plausible tick.
fn read_varint(source: &str, bytes: &[u8], at: usize) -> Result<(u64, usize)> {
    let mut value: u64 = 0;
    let mut used = 0;
    loop {
        let byte = *bytes.get(at + used).ok_or_else(|| {
            Error::invalid_image(
                source,
                format!(
                    "section ends mid-value at byte {}, so the chunk states \
                     fewer ticks than it encodes",
                    at + used
                ),
            )
        })?;
        if used == MAX_VARINT_BYTES {
            return Err(Error::invalid_image(
                source,
                format!(
                    "section encodes a value in more than {MAX_VARINT_BYTES} \
                     bytes at byte {at}, which no tick occupies"
                ),
            ));
        }
        // The last group has room for fewer than seven bits, and the
        // shift alone will not say so: shifting them out is in range
        // and would leave an ordinary-looking tick behind.
        let group = u64::from(byte & 0x7f);
        let shift = 7 * used as u32;
        if shift >= u64::BITS || group > (u64::MAX >> shift) {
            return Err(Error::invalid_image(
                source,
                format!("section encodes a value wider than a tick at byte {at}"),
            ));
        }
        value |= group << shift;
        used += 1;
        if byte & 0x80 == 0 {
            return Ok((value, used));
        }
    }
}

/// Delta-codes an observation's transitions into one chunk's payload.
///
/// The positions ascend, so every gap is unsigned and the first is
/// measured from the observation's own origin.
fn encode_transitions(transitions: &[Tick]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut previous: Tick = 0;
    for (ordinal, &position) in transitions.iter().enumerate() {
        let delta = position.checked_sub(previous).ok_or_else(|| {
            Error::invalid_image(
                "flux-capture",
                format!(
                    "transition {ordinal} at tick {position} does not advance past \
                     the preceding tick {previous}, so it has no unsigned gap"
                ),
            )
        })?;
        write_varint(&mut out, delta);
        previous = position;
    }
    Ok(out)
}

/// Reads a chunk's payload back into the ticks it was coded from.
fn decode_transitions(bytes: &[u8]) -> Result<Vec<Tick>> {
    let source = "flux-capture";
    let mut transitions = Vec::new();
    let mut position: Tick = 0;
    let mut at = 0;
    while at < bytes.len() {
        let (delta, used) = read_varint(source, bytes, at)?;
        at += used;
        position = position.checked_add(delta).ok_or_else(|| {
            Error::invalid_image(
                source,
                "section accumulates past the largest tick, so its deltas do \
                 not describe a run of transitions",
            )
        })?;
        transitions.push(position);
    }
    Ok(transitions)
}

/// One addressable run of an observation's transitions.
///
/// It carries the ticks it covers and how many it holds, so a reader
/// deciding from the index can skip a chunk without decoding it, and
/// its payload decodes on its own: the first value is an absolute tick
/// and only the rest are gaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransitionChunk {
    ordinal: u64,
    first: Tick,
    last: Tick,
    count: u64,
    payload: Vec<u8>,
}

impl TransitionChunk {
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// The first and last tick this chunk holds, inclusive.
    pub(crate) fn bounds(&self) -> (Tick, Tick) {
        (self.first, self.last)
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Splits an observation's transitions into chunks of at most
/// `records` each.
///
/// The boundary is a record count rather than a byte count or a tick
/// span, so the same transitions always split the same way — the
/// backing may be rebuilt and still address what an index says it does.
fn split_transitions(transitions: &[Tick], records: usize) -> Result<Vec<TransitionChunk>> {
    if records == 0 {
        return Err(Error::invalid_image(
            "flux-capture",
            "a transition chunk of zero records states no boundary to split on",
        ));
    }
    transitions
        .chunks(records)
        .enumerate()
        .map(|(ordinal, run)| {
            Ok(TransitionChunk {
                ordinal: ordinal as u64,
                first: *run.first().expect("chunks are never empty"),
                last: *run.last().expect("chunks are never empty"),
                count: run.len() as u64,
                payload: encode_transitions(run)?,
            })
        })
        .collect()
}

/// A capture-wide handle for one capture run, on the same terms as an
/// [`ObservationId`]: library-owned identity, never a rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CaptureRunId(u64);

/// What a section belongs to.
///
/// The ordering matters: a location's own sections come before those of
/// anything nested in it, so a reader walking the index in key order
/// meets a scope's metadata before the chunks it describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ScopeId {
    Track,
    Run(CaptureRunId),
    Observation(ObservationId),
}

/// What one section holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SectionKind {
    TrackMetadata,
    CaptureRunMetadata,
    ObservationMetadata,
    TransitionChunk,
    MarkerChunk,
    IssueChunk,
}

/// The complete address of one section of the backing.
///
/// Every part is load-bearing: a location, what within it, of what
/// kind, and which chunk. Two sections never share a key, because a
/// collision would leave one unreachable and serve the other in its
/// place — silently, and as if it were the evidence asked for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SectionKey {
    track: TrackKey,
    scope: ScopeId,
    kind: SectionKind,
    ordinal: u64,
}

impl SectionKey {
    pub(crate) fn new(
        track: TrackKey,
        scope: ScopeId,
        kind: SectionKind,
        ordinal: u64,
    ) -> Self {
        Self {
            track,
            scope,
            kind,
            ordinal,
        }
    }

    pub(crate) fn track(&self) -> &TrackKey {
        &self.track
    }

    pub(crate) fn scope(&self) -> ScopeId {
        self.scope
    }

    pub(crate) fn kind(&self) -> SectionKind {
        self.kind
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }
}

/// The exact event range, in a run's own ticks, that one circular
/// observation was bounded from.
///
/// Bounding rebases an observation's events to its own origin, so the
/// observation itself can no longer say where in the run it sat. This
/// is the whole of the route back to the evidence as recorded, which is
/// why an observation cannot exist without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureRunSlice {
    run_ordinal: u64,
    start: Tick,
    end: Tick,
}

impl CaptureRunSlice {
    pub(crate) fn new(run_ordinal: u64, start: Tick, end: Tick) -> Self {
        Self {
            run_ordinal,
            start,
            end,
        }
    }

    pub(crate) fn run_ordinal(&self) -> u64 {
        self.run_ordinal
    }

    pub(crate) fn start(&self) -> Tick {
        self.start
    }

    pub(crate) fn end(&self) -> Tick {
        self.end
    }
}

/// Where one section sits in the backing, and what it should check to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SectionLocation {
    offset: u64,
    length: u64,
    checksum: u32,
}

impl SectionLocation {
    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn length(&self) -> u64 {
        self.length
    }

    pub(crate) fn checksum(&self) -> u32 {
        self.checksum
    }
}

/// The ordered map from a complete section key to where its bytes are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SectionIndex {
    entries: BTreeMap<SectionKey, SectionLocation>,
}

impl SectionIndex {
    pub(crate) fn locate(&self, key: &SectionKey) -> Option<SectionLocation> {
        self.entries.get(key).copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Somewhere the backing's bytes can be read from at an offset.
///
/// The seam exists so the layer's own tests can assert what a read
/// touched, and so the backing does not care whether its bytes are in
/// private session storage or anywhere else.
pub(crate) trait ByteSource {
    fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()>;
}

/// Builds a backing by appending sections in key order.
///
/// The index is finished only once every section it references is
/// complete, which is what makes a half-written backing detectable
/// rather than a layer with holes in it.
#[derive(Debug, Default)]
pub(crate) struct SectionWriter {
    bytes: Vec<u8>,
    entries: BTreeMap<SectionKey, SectionLocation>,
    last: Option<SectionKey>,
}

impl SectionWriter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Appends one section, which must sort after every section already
    /// appended.
    pub(crate) fn append(&mut self, key: SectionKey, payload: Vec<u8>) -> Result<()> {
        if let Some(last) = &self.last
            && &key <= last
        {
            return Err(Error::invalid_image(
                key.track.namespace,
                format!(
                    "section {:?} does not sort after the preceding {:?}, so the \
                     backing is not being emitted in key order",
                    key.kind, last.kind
                ),
            ));
        }
        let location = SectionLocation {
            offset: self.bytes.len() as u64,
            length: payload.len() as u64,
            checksum: crc32(&payload),
        };
        self.bytes.extend_from_slice(&payload);
        self.entries.insert(key.clone(), location);
        self.last = Some(key);
        Ok(())
    }

    pub(crate) fn finish(self) -> (Vec<u8>, SectionIndex) {
        (
            self.bytes,
            SectionIndex {
                entries: self.entries,
            },
        )
    }
}

/// Reads one section, and nothing else.
///
/// The index says where the record is and what it checks to, both of
/// which are verified before the bytes are believed — a record that
/// changed under the index is refused rather than handed back as flux
/// that never existed.
pub(crate) fn read_section(
    source: &impl ByteSource,
    index: &SectionIndex,
    key: &SectionKey,
) -> Result<Vec<u8>> {
    let location = index.locate(key).ok_or_else(|| {
        Error::invalid_image(
            key.track.namespace,
            format!("backing holds no section for {:?} of this location", key.kind),
        )
    })?;
    let length = usize::try_from(location.length).map_err(|_| {
        Error::invalid_image(
            key.track.namespace,
            "backing states a section longer than this host can address",
        )
    })?;
    let mut payload = vec![0u8; length];
    source.read_at(location.offset, &mut payload)?;
    if crc32(&payload) != location.checksum {
        return Err(Error::invalid_image(
            key.track.namespace,
            format!(
                "section at byte {} does not check to what the index states, so \
                 its bytes are not the ones that were written",
                location.offset
            ),
        ));
    }
    Ok(payload)
}

/// One circular, track-relative observation bounded out of a capture
/// run: a declared circumference, the transitions within it, and the
/// marker channels recorded alongside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Observation {
    id: ObservationId,
    provenance: Provenance,
    source: CaptureRunSlice,
    /// This location's source-record order, assigned when a track
    /// admits it. Carried rather than derived from list position,
    /// because a section-addressable backing may load one observation
    /// without its neighbours.
    ordinal: u64,
    span: Tick,
    transitions: Vec<Tick>,
    markers: Vec<Marker>,
}

impl Observation {
    /// Bounds an observation, refusing evidence that contradicts itself.
    ///
    /// `source` names the capture format for the diagnostic, so a
    /// refusal reads in the source's own terms (P4, P6).
    pub(crate) fn new(
        source: &str,
        provenance: Provenance,
        slice: CaptureRunSlice,
        span: Tick,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<Self> {
        if span == 0 {
            return Err(Error::invalid_image(
                source,
                "observation declares a span of zero, which states no \
                 circumference to measure its transitions against",
            ));
        }
        let mut previous: Option<Tick> = None;
        for (ordinal, &position) in transitions.iter().enumerate() {
            if position >= span {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "observation transition {ordinal} at tick {position} \
                         is not below the declared span of {span}"
                    ),
                ));
            }
            if let Some(previous) = previous
                && position <= previous
            {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "observation transition {ordinal} at tick {position} \
                         does not advance past the preceding tick {previous}"
                    ),
                ));
            }
            previous = Some(position);
        }
        for (ordinal, marker) in markers.iter().enumerate() {
            if marker.position() >= span {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "observation marker {ordinal} at tick {} is not below \
                         the declared span of {span}",
                        marker.position()
                    ),
                ));
            }
        }
        Ok(Self {
            // Both are assigned when a capture admits it: neither is
            // knowable until the observation has a home.
            id: ObservationId(0),
            provenance,
            source: slice,
            ordinal: 0,
            span,
            transitions,
            markers,
        })
    }

    pub(crate) fn id(&self) -> ObservationId {
        self.id
    }

    /// How this observation came to be known.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The run and range this was bounded from.
    pub(crate) fn source(&self) -> CaptureRunSlice {
        self.source
    }

    /// Its place in this location's source-record order. Not a rank:
    /// it says nothing about whether the observation is good, complete,
    /// or the one a drive should be served from.
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub(crate) fn span(&self) -> Tick {
        self.span
    }

    pub(crate) fn transitions(&self) -> &[Tick] {
        &self.transitions
    }

    pub(crate) fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// The cyclic interval from the last transition back to the first.
    ///
    /// The source's wrap is preserved by the span rather than by a
    /// duplicate boundary pulse, so this is derived and never stored.
    /// `None` when the observation recorded no transitions at all.
    pub(crate) fn wrap_interval(&self) -> Option<Tick> {
        let first = *self.transitions.first()?;
        let last = *self.transitions.last()?;
        Some(self.span - last + first)
    }
}

/// One source transfer, preserved in its actual recorded time order.
///
/// A run holds everything the container transferred, including the flux
/// recorded before its first index and after its last — evidence that
/// bounding into circular observations does not consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureRun {
    ordinal: u64,
    provenance: Provenance,
    transitions: Vec<Tick>,
    markers: Vec<Marker>,
}

impl CaptureRun {
    pub(crate) fn new(
        source: &str,
        ordinal: u64,
        provenance: Provenance,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<Self> {
        let mut previous: Option<Tick> = None;
        for (index, &position) in transitions.iter().enumerate() {
            if let Some(previous) = previous
                && position <= previous
            {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "capture run transition {index} at tick {position} does not \
                         advance past the preceding tick {previous}, so the run does \
                         not state its recorded time order"
                    ),
                ));
            }
            previous = Some(position);
        }
        Ok(Self {
            ordinal,
            provenance,
            transitions,
            markers,
        })
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// How the run itself came to be known — what the adapter declared
    /// about the transfer, not anything this layer concluded.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn transitions(&self) -> &[Tick] {
        &self.transitions
    }

    pub(crate) fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Bounds the run's circular observations at its index markers.
    ///
    /// Each consecutive pair of indices bounds one half-open window
    /// `[start, end)`, re-based to its own origin. A run with fewer than
    /// two indices supplies none: it remains inspectable evidence that
    /// simply cannot state a circumference, and no period is invented
    /// for it.
    pub(crate) fn observations(&self, source: &str) -> Result<Vec<Observation>> {
        let indices: Vec<Tick> = self
            .markers
            .iter()
            .filter(|marker| marker.kind() == &MarkerKind::Index)
            .map(Marker::position)
            .collect();

        indices
            .windows(2)
            .map(|bounds| {
                let (start, end) = (bounds[0], bounds[1]);
                if end <= start {
                    return Err(Error::invalid_image(
                        source,
                        format!(
                            "capture run index at tick {end} does not advance past \
                             the preceding index at tick {start}"
                        ),
                    ));
                }
                let transitions = self
                    .transitions
                    .iter()
                    .filter(|&&position| position >= start && position < end)
                    .map(|&position| position - start)
                    .collect();
                let markers = self
                    .markers
                    .iter()
                    .filter(|marker| marker.position() >= start && marker.position() < end)
                    .map(|marker| {
                        Marker::new(marker.position() - start, marker.kind().clone())
                            .with_payload(marker.payload().to_vec())
                    })
                    .collect();
                Observation::new(
                    source,
                    Provenance::new(self.provenance.source).note(format!(
                        "bounded from capture run {} between the index markers                          at ticks {start} and {end}",
                        self.ordinal
                    )),
                    CaptureRunSlice::new(self.ordinal, start, end),
                    end - start,
                    transitions,
                    markers,
                )
            })
            .collect()
    }
}

/// A source's own location number, held exactly.
///
/// Sources step in fractions — a 1541 half-track is a real address, not
/// a rounding of a whole one — so a position is an exact rational and
/// never a float. It is reduced on construction, which is exact, so
/// that one location has one key however the source spelled it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourcePosition {
    numerator: u64,
    denominator: u64,
}

impl SourcePosition {
    /// A whole step, the common case.
    pub(crate) fn whole(position: u64) -> Self {
        Self {
            numerator: position,
            denominator: 1,
        }
    }

    /// A fractional step, in the source's own terms.
    pub(crate) fn fraction(source: &str, numerator: u64, denominator: u64) -> Result<Self> {
        if denominator == 0 {
            return Err(Error::invalid_image(
                source,
                format!(
                    "capture addresses a location as {numerator}/{denominator} of a \
                     step, which states no position"
                ),
            ));
        }
        let divisor = greatest_common_divisor(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }
}

impl Ord for SourcePosition {
    /// Compared by exact cross-multiplication. Comparing the reduced
    /// parts field by field would order 5/2 after 3/1.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (u128::from(self.numerator) * u128::from(other.denominator))
            .cmp(&(u128::from(other.numerator) * u128::from(self.denominator)))
    }
}

impl PartialOrd for SourcePosition {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn greatest_common_divisor(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// The adapter-declared physical identity of one captured location.
///
/// It is not CHS, and nothing here rounds it toward cylinders or heads:
/// the adapter names the location in its source's own terms and the
/// layer carries that name unchanged. A source that numbers no head
/// has none, which is a different fact from head zero.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TrackKey {
    namespace: &'static str,
    position: SourcePosition,
    head: Option<u64>,
}

impl TrackKey {
    /// A whole step on a numbered head — what most captures declare.
    pub(crate) fn new(namespace: &'static str, position: u64, head: u64) -> Self {
        Self::at(namespace, SourcePosition::whole(position), Some(head))
    }

    pub(crate) fn at(
        namespace: &'static str,
        position: SourcePosition,
        head: Option<u64>,
    ) -> Self {
        Self {
            namespace,
            position,
            head,
        }
    }

    pub(crate) fn position(&self) -> SourcePosition {
        self.position
    }

    pub(crate) fn head(&self) -> Option<u64> {
        self.head
    }
}

/// Everything a capture holds for one location.
///
/// A track that exists with no observations is not the same fact as a
/// track the capture never supplied: the first is a location the source
/// declared and had no usable capture of, the second is a location the
/// capture is silent about. The layer is sparse so the two stay apart,
/// and neither is repaired into the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Track {
    key: TrackKey,
    observations: Vec<Observation>,
}

impl Track {
    pub(crate) fn new(key: TrackKey) -> Self {
        Self {
            key,
            observations: Vec::new(),
        }
    }

    pub(crate) fn key(&self) -> &TrackKey {
        &self.key
    }

    pub(crate) fn observations(&self) -> &[Observation] {
        &self.observations
    }

    /// Takes an observation the capture has already identified, giving
    /// it the next ordinal in this location's source-record order.
    fn push_observation(&mut self, id: ObservationId, mut observation: Observation) {
        observation.id = id;
        observation.ordinal = self.observations.len() as u64;
        self.observations.push(observation);
    }
}

/// One opened capture: its declared timing basis and the locations it
/// supplied evidence for.
#[derive(Debug, Clone)]
pub(crate) struct FluxCapture {
    envelope: CaptureEnvelope,
    time_base: TimeBase,
    tracks: BTreeMap<TrackKey, Track>,
    next_observation_id: u64,
}

impl FluxCapture {
    pub(crate) fn new(time_base: TimeBase) -> Self {
        Self {
            envelope: CaptureEnvelope::new(),
            time_base,
            tracks: BTreeMap::new(),
            next_observation_id: 0,
        }
    }

    /// Everything the source stated besides its flux.
    pub(crate) fn envelope(&self) -> &CaptureEnvelope {
        &self.envelope
    }

    pub(crate) fn envelope_mut(&mut self) -> &mut CaptureEnvelope {
        &mut self.envelope
    }

    /// Admits an observation at a location the capture has declared,
    /// issuing its capture-wide identity.
    ///
    /// A location the capture never supplied is refused rather than
    /// created on the way past, which would turn silence about a
    /// location into a declaration of it.
    pub(crate) fn admit_observation(
        &mut self,
        key: &TrackKey,
        observation: Observation,
    ) -> Result<ObservationId> {
        let id = ObservationId(self.next_observation_id);
        let track = self.tracks.get_mut(key).ok_or_else(|| {
            Error::invalid_image(
                key.namespace,
                format!(
                    "capture has no location {:?} to admit an observation to",
                    key.position
                ),
            )
        })?;
        track.push_observation(id, observation);
        self.next_observation_id += 1;
        Ok(id)
    }

    pub(crate) fn insert_track(&mut self, track: Track) {
        self.tracks.insert(track.key().clone(), track);
    }

    pub(crate) fn track(&self, key: &TrackKey) -> Option<&Track> {
        self.tracks.get(key)
    }

    /// Every supplied location, in key order — which is the source's
    /// own addressing sorted, not the order the adapter happened to
    /// hand them over in.
    pub(crate) fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    fn index_at(position: Tick) -> Marker {
        Marker::new(position, MarkerKind::Index)
    }

    /// A synthetic observation for this layer's own invariant tests,
    /// bounded from the whole of run zero — a coherent slice rather
    /// than a placeholder, since no observation exists without one.
    fn observed(
        source: &'static str,
        span: Tick,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<Observation> {
        Observation::new(
            source,
            Provenance::new(source),
            CaptureRunSlice::new(0, 0, span),
            span,
            transitions,
            markers,
        )
    }

    /// A run as its adapter transferred it, for tests that are about
    /// the run's own invariants rather than about provenance.
    fn captured(
        source: &'static str,
        ordinal: u64,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<CaptureRun> {
        CaptureRun::new(source, ordinal, Provenance::new(source), transitions, markers)
    }

    fn kryoflux_timebase() -> TimeBase {
        TimeBase::new("kryoflux", 18_432_000 * 73, 56).expect("the rate is stated")
    }

    #[test]
    fn a_capture_holds_a_track_under_the_key_its_adapter_declared() {
        // The adapter names the location in its own terms; the layer
        // stores it and hands it back, having interpreted nothing.
        let key = TrackKey::new("kryoflux", 36, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());

        capture.insert_track(Track::new(key.clone()));

        assert_eq!(capture.track(&key).map(Track::key), Some(&key));
    }

    #[test]
    fn an_unsupplied_location_differs_from_one_declared_and_holding_nothing() {
        // Two different facts, and the layer is sparse so that they stay
        // different: an absent key means the capture supplied nothing for
        // that location, while a present track with no observations means
        // the source declared the location and had no usable capture of
        // it. Collapsing them would invent evidence in one direction and
        // erase it in the other.
        let declared = TrackKey::new("kryoflux", 36, 0);
        let unsupplied = TrackKey::new("kryoflux", 37, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());

        capture.insert_track(Track::new(declared.clone()));

        assert!(capture.track(&unsupplied).is_none());
        let track = capture.track(&declared).expect("the location was declared");
        assert!(track.observations().is_empty());
    }

    #[test]
    fn a_fractional_step_and_an_unnumbered_head_survive_unrounded() {
        // The layer's loudest refusal: a TrackKey is not CHS. A source
        // addressing half-tracks, and one numbering no head at all, are
        // carried as stated — 18.5 is neither 18 nor 19, and an absent
        // head is absent rather than quietly becoming head 0.
        let half = SourcePosition::fraction("c1541", 37, 2).expect("the step is stated");
        let key = TrackKey::at("c1541", half, None);

        assert_eq!(key.position(), half);
        assert_eq!(key.head(), None);
        assert_ne!(key.position(), SourcePosition::whole(18));
        assert_ne!(key.position(), SourcePosition::whole(19));
    }

    #[test]
    fn an_observation_names_the_run_and_the_exact_range_it_was_bounded_from() {
        // An observation rebases its transitions to its own origin, so
        // once bounded it can no longer say where in the run it sat.
        // The slice is the whole of the route back, and it keeps the
        // run's own ticks rather than the rebased ones.
        let run = captured(
            "kryoflux",
            3,
            vec![50, 150, 400, 950],
            vec![index_at(100), index_at(900)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();
        let slice = observations[0].source();

        assert_eq!(slice.run_ordinal(), 3);
        assert_eq!((slice.start(), slice.end()), (100, 900));
        // Rebased payload, unrebased slice: 150 became 50, and the
        // slice still says the window opened at 100.
        assert_eq!(observations[0].transitions(), [50, 300]);
    }

    #[test]
    fn several_observations_on_one_track_keep_the_source_record_order() {
        // Revolutions arrive in the order the source recorded them, and
        // the ordinal is what preserves that once they are addressed
        // individually. Nothing reorders them by span, transition
        // count, or any other notion of a better pass.
        let run = captured(
            "kryoflux",
            0,
            vec![150, 250, 1050],
            vec![index_at(100), index_at(900), index_at(1700)],
        )
        .unwrap();
        let key = TrackKey::new("kryoflux", 36, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());
        capture.insert_track(Track::new(key.clone()));
        for observation in run.observations("kryoflux").unwrap() {
            capture
                .admit_observation(&key, observation)
                .expect("the location was declared");
        }

        let track = capture.track(&key).expect("the location was declared");
        let ordinals: Vec<u64> = track
            .observations()
            .iter()
            .map(Observation::ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1]);

        // And the ordinals agree with the run they were bounded from,
        // which is the independent record of that order.
        let starts: Vec<Tick> = track
            .observations()
            .iter()
            .map(|observation| observation.source().start())
            .collect();
        assert_eq!(starts, [100, 900]);
    }

    #[test]
    fn a_source_record_the_layer_cannot_name_is_retained_whole_and_in_order() {
        // The two-outcome rule: a source fact either maps to a named
        // field or is kept verbatim as foreign. Nothing is dropped for
        // being unrecognized, because a convenient discard is exactly
        // how a format's blind spot becomes permanent.
        let mut envelope = CaptureEnvelope::new();
        let stream = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "Pinball(1of2)00.0.raw",
            SourceRange::new(0, 184_534),
        ));

        envelope.retain_foreign(ForeignRecord::new(
            stream,
            "kryoflux",
            "oob",
            SourceRange::new(1024, 12),
            vec![1, 2, 3],
        ));
        envelope.retain_foreign(ForeignRecord::new(
            stream,
            "kryoflux",
            "oob",
            SourceRange::new(2048, 4),
            vec![9],
        ));

        let held = envelope.foreign_records();
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].namespace(), "kryoflux");
        assert_eq!(held[0].type_id(), "oob");
        assert_eq!(held[0].payload(), [1, 2, 3]);
        assert_eq!(held[0].range(), SourceRange::new(1024, 12));
        // Retention order is the source's order, and the ordinal is
        // what preserves it once records are addressed individually.
        assert_eq!(
            held.iter().map(ForeignRecord::ordinal).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(held[1].payload(), [9]);
    }

    #[test]
    fn metadata_keeps_the_sources_own_spelling_and_order_including_repeats() {
        // Not a map. A stream states its keys more than once — two
        // KFInfo blocks, each with its own `host_date` — and both are
        // evidence, so a keyed collection would silently keep one.
        //
        // The values are why the spelling is kept too: a KryoFlux
        // stream declares `sck` as a truncation of 168192000/7, and
        // `ick` as a rounding that is not even that value over eight.
        // The layer records what was written and believes neither.
        let mut envelope = CaptureEnvelope::new();
        for (key, value) in [
            ("host_date", "2014.11.01"),
            ("sck", "24027428.5714285"),
            ("ick", "3003428.5714285625"),
            ("host_date", "2014.11.02"),
        ] {
            envelope.record_metadata(MetadataRecord::new("kryoflux", key, value));
        }

        let held = envelope.metadata();
        assert_eq!(
            held.iter().map(MetadataRecord::key).collect::<Vec<_>>(),
            ["host_date", "sck", "ick", "host_date"]
        );
        assert_eq!(held[0].value(), "2014.11.01");
        assert_eq!(held[3].value(), "2014.11.02");
        assert_eq!(held[1].value(), "24027428.5714285");
        assert_eq!(
            held.iter().map(MetadataRecord::ordinal).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn a_derived_candidate_sits_beside_the_raw_runs_and_never_stands_in_for_them() {
        // A source's own solved stream is its derivative, not proof
        // that the raw capture had one ideal rotation behind it. It is
        // kept, labelled as derived, and reachable only by asking the
        // envelope for it — whatever reads a location's observations
        // never meets it, so it cannot be mistaken for evidence.
        let key = TrackKey::new("a2r", 36, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());
        capture.insert_track(Track::new(key.clone()));
        capture
            .admit_observation(&key, observed("a2r", 800, vec![10], Vec::new()).unwrap())
            .expect("the location was declared");

        capture.envelope_mut().retain_derived(DerivedCandidate::new(
            "a2r",
            DerivedKind::SolvedFlux,
            SourceRange::new(4096, 256),
            Provenance::new("a2r").note("source supplied a solved stream"),
        ));

        // The raw evidence is exactly as it was.
        assert_eq!(capture.track(&key).unwrap().observations().len(), 1);
        // And the derivative is held apart, saying what it is.
        let derived = capture.envelope().derived_candidates();
        assert_eq!(derived.len(), 1);
        assert_eq!(derived[0].kind(), &DerivedKind::SolvedFlux);
        assert_eq!(derived[0].range(), SourceRange::new(4096, 256));
    }

    #[test]
    fn a_capture_assembled_from_many_artifacts_keeps_each_source_distinct() {
        // One logical capture is made of many files — the prepared
        // fixture is 168 archive members — so "which artifact did this
        // come from" has to survive assembly. A retained record names
        // the member it was read from, not the set, because a set-wide
        // answer would be no answer at all.
        let mut envelope = CaptureEnvelope::new();
        let first = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "Pinball(1of2)00.0.raw",
            SourceRange::new(0, 184_534),
        ));
        let second = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "Pinball(1of2)00.1.raw",
            SourceRange::new(0, 264_965),
        ));

        envelope.retain_foreign(ForeignRecord::new(
            second,
            "kryoflux",
            "oob",
            SourceRange::new(1024, 12),
            vec![1, 2, 3],
        ));

        assert_ne!(first, second);
        assert_eq!(envelope.sources().len(), 2);
        assert_eq!(
            envelope.source(second).map(SourceDescriptor::artifact),
            Some("Pinball(1of2)00.1.raw")
        );
        // The record traces to one member, and it is the right one.
        assert_eq!(envelope.foreign_records()[0].source(), second);
    }

    #[test]
    fn a_section_key_addresses_one_thing_and_keys_order_deterministically() {
        // Sections are emitted in key order and the index is searched
        // in that order, so it has to be total and stable. Sections of
        // one location are told apart by scope, then kind, then chunk
        // ordinal — a collision would make one section unreachable and
        // silently serve another in its place.
        let track = TrackKey::new("kryoflux", 36, 0);
        let later = TrackKey::new("kryoflux", 38, 0);
        let run = ScopeId::Run(CaptureRunId(0));
        let observation = ScopeId::Observation(ObservationId(0));

        let mut keys = vec![
            SectionKey::new(later.clone(), ScopeId::Track, SectionKind::TrackMetadata, 0),
            SectionKey::new(track.clone(), observation, SectionKind::TransitionChunk, 1),
            SectionKey::new(track.clone(), observation, SectionKind::TransitionChunk, 0),
            SectionKey::new(track.clone(), observation, SectionKind::MarkerChunk, 0),
            SectionKey::new(track.clone(), run, SectionKind::CaptureRunMetadata, 0),
            SectionKey::new(track.clone(), ScopeId::Track, SectionKind::TrackMetadata, 0),
        ];
        let scrambled = keys.clone();
        keys.sort();

        // Location first, so one track's sections are contiguous.
        assert_eq!(keys[0].track(), &track);
        assert_eq!(keys[5].track(), &later);
        // Within a location: track scope, then run, then observation.
        assert_eq!(keys[0].scope(), ScopeId::Track);
        assert_eq!(keys[1].scope(), run);
        // Within one scope, chunk ordinals ascend.
        let chunks: Vec<u64> = keys
            .iter()
            .filter(|key| key.kind() == SectionKind::TransitionChunk)
            .map(SectionKey::ordinal)
            .collect();
        assert_eq!(chunks, [0, 1]);
        // And no two of these six are the same section.
        let mut unique = scrambled;
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 6);
    }

    #[test]
    fn a_transition_chunk_delta_codes_and_decodes_back_to_the_exact_ticks() {
        // Flux is what the layer exists to carry, so this coding is
        // exact or it is worthless: one delta misread shifts every
        // transition after it, and nothing downstream could tell.
        //
        // The boundary rows are the point. A varint goes wrong at its
        // byte boundaries, so 127/128 and 16383/16384 are pinned, and
        // the table takes a new row whenever another input turns up.
        for original in [
            vec![],
            vec![0],
            vec![0, 1],
            vec![10, 400, 790],
            vec![127, 128, 16_383, 16_384],
            vec![0, u64::MAX],
            vec![u64::MAX],
        ] {
            let encoded = encode_transitions(&original).expect("the ticks ascend");
            assert_eq!(
                decode_transitions(&encoded).expect("what was encoded decodes"),
                original,
                "round trip for {original:?}"
            );
        }
    }

    #[test]
    fn transitions_that_do_not_ascend_are_refused_rather_than_wrapped() {
        // The delta is unsigned, so a descending pair has no encoding.
        // Refusing beats the wrap that would otherwise write a gap of
        // eighteen quintillion ticks and read back as valid flux.
        let error = encode_transitions(&[400, 10]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_chunk_that_does_not_decode_is_refused_rather_than_partly_believed() {
        // P6: a section yields the flux it claims or it yields nothing.
        // Handing back the transitions decoded before the damage would
        // be the worst outcome available — a short revolution that
        // reads as a real one.
        let intact = encode_transitions(&[10, 400, 790]).expect("the ticks ascend");

        // Truncated mid-value: a multi-byte delta loses its last byte.
        let truncated = &intact[..intact.len() - 1];
        assert_eq!(
            decode_transitions(truncated).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );

        // More groups than a tick occupies.
        let overlong = vec![0x80; MAX_VARINT_BYTES + 1];
        assert_eq!(
            decode_transitions(&overlong).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );

        // Deltas accumulating past the largest tick.
        let mut accumulating = Vec::new();
        write_varint(&mut accumulating, u64::MAX);
        write_varint(&mut accumulating, 1);
        assert_eq!(
            decode_transitions(&accumulating).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );

        // A full-width value whose last group carries bits no tick has
        // room for. The shift is in range, so only the value itself
        // says this is malformed — and silently dropping the overflow
        // would turn it into an ordinary-looking tick.
        let mut overflowing = vec![0x80; MAX_VARINT_BYTES - 1];
        overflowing.push(0x7f);
        assert_eq!(
            decode_transitions(&overflowing).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );
    }

    #[test]
    fn a_long_transition_sequence_splits_into_independently_decodable_chunks() {
        // A real revolution is tens of thousands of transitions — the
        // fixture's outermost track carries 33,396 — so chunking is
        // what lets a reader take a span without the whole revolution.
        //
        // The split is by record count, so it is reproducible; each
        // chunk states the ticks it covers, so a reader can skip it
        // from the index without decoding it; and each decodes alone,
        // which is the property the whole backing rests on.
        let transitions: Vec<Tick> = (0..2500).map(|n| n * 3).collect();

        let chunks = split_transitions(&transitions, 1000).expect("the ticks ascend");

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks
                .iter()
                .map(TransitionChunk::ordinal)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(chunks[0].bounds(), (0, 2997));
        assert_eq!(chunks[1].bounds(), (3000, 5997));
        assert_eq!(chunks[2].bounds(), (6000, 7497));
        assert_eq!(chunks[2].count(), 500);

        // Each chunk decodes to its own absolute ticks with no
        // neighbour present — a reader that wants the middle of a
        // revolution decodes one chunk, not the ones before it.
        assert_eq!(
            decode_transitions(chunks[1].payload()).unwrap(),
            transitions[1000..2000]
        );

        // And the chunks together are the original, in order.
        let rejoined: Vec<Tick> = chunks
            .iter()
            .flat_map(|chunk| decode_transitions(chunk.payload()).unwrap())
            .collect();
        assert_eq!(rejoined, transitions);
    }

    #[test]
    fn splitting_into_chunks_of_no_records_is_refused() {
        let error = split_transitions(&[10, 20], 0).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    /// A byte source that records every range asked of it, so a test
    /// can assert what a read actually touched rather than trusting it.
    struct CountingSource {
        bytes: Vec<u8>,
        reads: std::cell::RefCell<Vec<(u64, u64)>>,
    }

    impl CountingSource {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                bytes,
                reads: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn reads(&self) -> Vec<(u64, u64)> {
            self.reads.borrow().clone()
        }
    }

    impl ByteSource for CountingSource {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            self.reads.borrow_mut().push((offset, into.len() as u64));
            let at = offset as usize;
            into.copy_from_slice(&self.bytes[at..at + into.len()]);
            Ok(())
        }
    }

    fn transition_section(track: &TrackKey, ordinal: u64) -> (SectionKey, Vec<Tick>, Vec<u8>) {
        let ticks: Vec<Tick> = (0..50).map(|n| ordinal * 1000 + n * 3).collect();
        let chunk = split_transitions(&ticks, 50).expect("the ticks ascend");
        let key = SectionKey::new(
            track.clone(),
            ScopeId::Observation(ObservationId(0)),
            SectionKind::TransitionChunk,
            ordinal,
        );
        (key, ticks, chunk[0].payload().to_vec())
    }

    #[test]
    fn loading_one_section_reads_only_that_sections_own_bytes() {
        // The P27 promise where it begins: a miss reads one bounded
        // record range, never a whole capture. A backing assembled from
        // a 168-member capture set would otherwise decode its way
        // through gigabytes to answer for one span of one track.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let mut wanted = None;
        for ordinal in 0..3 {
            let (key, ticks, payload) = transition_section(&track, ordinal);
            if ordinal == 1 {
                wanted = Some((key.clone(), ticks));
            }
            writer.append(key, payload).expect("keys ascend");
        }
        let (bytes, index) = writer.finish();
        let (key, expected) = wanted.unwrap();

        let source = CountingSource::new(bytes);
        let payload = read_section(&source, &index, &key).expect("the section is indexed");

        assert_eq!(decode_transitions(&payload).unwrap(), expected);
        // Exactly one read, of exactly this section's extent — neither
        // of its neighbours was touched, let alone decoded.
        let located = index.locate(&key).expect("the section is indexed");
        assert_eq!(source.reads(), [(located.offset(), located.length())]);
    }

    #[test]
    fn a_section_whose_bytes_changed_under_the_index_is_refused() {
        // The index states what the record should check to. A backing
        // that answered anyway would hand back flux that never existed.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let (key, _, payload) = transition_section(&track, 0);
        writer.append(key.clone(), payload).expect("keys ascend");
        let (mut bytes, index) = writer.finish();

        bytes[2] ^= 0x01;

        let error = read_section(&CountingSource::new(bytes), &index, &key).unwrap_err();
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn sections_appended_out_of_key_order_are_refused() {
        // Sections are emitted in key order so the index can be built
        // and searched in it; accepting them out of order would make
        // the ordering a hope rather than a property.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let (second, _, second_payload) = transition_section(&track, 1);
        let (first, _, first_payload) = transition_section(&track, 0);

        writer.append(second, second_payload).expect("the first append is in order");
        let error = writer.append(first, first_payload).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_bounded_observation_records_that_it_was_cut_rather_than_recorded() {
        // P4: a fact travels with how it came to be known. These are
        // not revolutions the source handed over — they were cut from a
        // run at its index markers, and the provenance says so, so that
        // nothing downstream can read a derived bound as a recorded
        // rotation.
        let run = CaptureRun::new(
            "kryoflux",
            0,
            Provenance::new("kryoflux").note("stream transferred whole"),
            vec![150, 250, 1050],
            vec![index_at(100), index_at(900), index_at(1700)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();

        assert_eq!(run.provenance().notes, ["stream transferred whole"]);
        let bounded = observations[0].provenance();
        assert_eq!(bounded.source, "kryoflux");
        assert!(
            bounded.notes.iter().any(|note| note.contains("index")),
            "the bounding should name its mechanism: {:?}",
            bounded.notes
        );
    }

    #[test]
    fn identity_is_capture_wide_where_the_ordinal_is_only_per_location() {
        // Both locations hold their first observation, so both ordinals
        // are 0. The backing addresses sections by id, so the two must
        // still be tellable apart — that is the whole of what the id is
        // for, and it ranks nothing.
        let first = TrackKey::new("kryoflux", 36, 0);
        let second = TrackKey::new("kryoflux", 38, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());
        capture.insert_track(Track::new(first.clone()));
        capture.insert_track(Track::new(second.clone()));

        let one = capture
            .admit_observation(&first, observed("kryoflux", 800, vec![10], Vec::new()).unwrap())
            .expect("the location was declared");
        let other = capture
            .admit_observation(&second, observed("kryoflux", 800, vec![10], Vec::new()).unwrap())
            .expect("the location was declared");

        assert_ne!(one, other);
        for key in [&first, &second] {
            let held = &capture.track(key).unwrap().observations()[0];
            assert_eq!(held.ordinal(), 0);
        }
        assert_eq!(capture.track(&first).unwrap().observations()[0].id(), one);
    }

    #[test]
    fn admitting_an_observation_to_an_undeclared_location_is_refused() {
        // Creating the location on the way past would erase the very
        // distinction the sparse layer keeps: a location the capture
        // never supplied would become one it declared.
        let mut capture = FluxCapture::new(kryoflux_timebase());

        let error = capture
            .admit_observation(
                &TrackKey::new("kryoflux", 36, 0),
                observed("kryoflux", 800, vec![10], Vec::new()).unwrap(),
            )
            .unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn locations_iterate_in_position_order_whatever_order_they_arrived_in() {
        // 18.5 sorts between 18 and 19. Comparing the reduced parts
        // field by field would put 37/2 last, having compared 37
        // against 19 and stopped there.
        let half = SourcePosition::fraction("c1541", 37, 2).expect("the step is stated");
        let mut capture = FluxCapture::new(kryoflux_timebase());
        for position in [SourcePosition::whole(19), SourcePosition::whole(18), half] {
            capture.insert_track(Track::new(TrackKey::at("c1541", position, None)));
        }

        let order: Vec<SourcePosition> =
            capture.tracks().map(|track| track.key().position()).collect();

        assert_eq!(
            order,
            [SourcePosition::whole(18), half, SourcePosition::whole(19)]
        );
    }

    #[test]
    fn a_position_stated_over_zero_steps_is_refused() {
        let error = SourcePosition::fraction("scp", 37, 0).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn one_location_has_one_key_however_the_source_spelled_it() {
        // Reduction is exact, not rounding, so a source free to write
        // 74/4 does not thereby address a location apart from 37/2.
        let reduced = SourcePosition::fraction("c1541", 37, 2).expect("the step is stated");
        let unreduced = SourcePosition::fraction("c1541", 74, 4).expect("the step is stated");

        assert_eq!(reduced, unreduced);
        assert_eq!(
            TrackKey::at("c1541", reduced, None),
            TrackKey::at("c1541", unreduced, None)
        );
    }

    #[test]
    fn a_run_keeps_the_flux_recorded_before_and_after_its_indices() {
        // A real KryoFlux stream brackets its indexed revolutions: the
        // fixture's first track carries 17,168 transitions before the
        // first index. None of that is dropped by bounding.
        let run = captured(
            "kryoflux",
            0,
            vec![50, 150, 400, 950],
            vec![index_at(100), index_at(900)],
        )
        .unwrap();

        assert_eq!(run.transitions(), [50, 150, 400, 950]);
    }

    #[test]
    fn consecutive_indices_bound_one_observation_rebased_to_its_origin() {
        let run = captured(
            "kryoflux",
            0,
            vec![50, 150, 400, 950],
            vec![index_at(100), index_at(900)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();

        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].span(), 800);
        // 150 and 400 fall inside; 50 precedes the first index and 950
        // follows the last, so neither enters the circle.
        assert_eq!(observations[0].transitions(), [50, 300]);
    }

    #[test]
    fn three_indices_bound_two_distinct_observations() {
        let run = captured(
            "kryoflux",
            0,
            vec![150, 250, 1050],
            vec![index_at(100), index_at(900), index_at(1700)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].transitions(), [50, 150]);
        assert_eq!(observations[1].transitions(), [150]);
    }

    #[test]
    fn a_run_without_two_indices_supplies_no_circular_observation() {
        // Still inspectable capture evidence — it simply cannot state a
        // circumference, and none is invented for it.
        let run =
            captured("kryoflux", 0, vec![50, 150], vec![index_at(100)]).unwrap();

        assert_eq!(run.transitions(), [50, 150]);
        assert!(run.observations("kryoflux").unwrap().is_empty());
    }

    #[test]
    fn a_run_records_its_transitions_in_recorded_time_order() {
        let error =
            captured("kryoflux", 0, vec![150, 50], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_timebase_keeps_its_declared_rational_exactly() {
        // KryoFlux's sample clock is ((18432000 * 73) / 14) / 2 / 2 Hz,
        // which has no exact binary or decimal form. It is retained as
        // the ratio the source declared, never rounded to a float.
        let base = TimeBase::new("kryoflux", 18_432_000 * 73, 14 * 2 * 2).unwrap();

        assert_eq!(base.ticks_per_second(), (18_432_000 * 73, 56));
    }

    #[test]
    fn a_timebase_is_not_reduced_to_a_library_chosen_sample_rate() {
        // Equal rates declared differently stay as declared: the source
        // spelling is evidence, and comparison is exact arithmetic.
        let declared = TimeBase::new("scp", 50, 2).unwrap();
        let same_rate = TimeBase::new("scp", 25, 1).unwrap();

        assert_eq!(declared.ticks_per_second(), (50, 2));
        assert!(declared.same_rate_as(&same_rate));
    }

    #[test]
    fn a_zero_or_absent_timebase_rate_is_refused() {
        assert_eq!(
            TimeBase::new("scp", 0, 1).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );
        assert_eq!(
            TimeBase::new("scp", 24_000_000, 0).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );
    }

    #[test]
    fn a_marker_may_share_a_position_with_a_transition() {
        // Marker channels are parallel timed evidence, so an index
        // pulse coinciding with a reversal is ordinary, not a clash.
        let observation =
            observed("kryoflux", 800, vec![10, 400], vec![index_at(400)]).unwrap();

        assert_eq!(observation.transitions(), [10, 400]);
        assert_eq!(observation.markers().len(), 1);
        assert_eq!(observation.markers()[0].position(), 400);
    }

    #[test]
    fn markers_keep_their_recorded_order_and_are_never_sorted() {
        // Two markers may share a position, and a later marker may sit
        // earlier on the circle: the source's order is the evidence.
        let markers = vec![
            index_at(700),
            Marker::new(700, MarkerKind::WriteSplice),
            index_at(100),
        ];
        let observation = observed("a2r", 800, vec![10], markers).unwrap();

        let positions: Vec<Tick> = observation.markers().iter().map(Marker::position).collect();
        assert_eq!(positions, [700, 700, 100]);
        assert_eq!(observation.markers()[1].kind(), &MarkerKind::WriteSplice);
    }

    #[test]
    fn a_marker_outside_the_span_is_refused() {
        let error =
            observed("a2r", 800, vec![10], vec![index_at(800)]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("marker"), "{error}");
    }

    #[test]
    fn an_unmodelled_source_event_keeps_its_namespace_code_and_payload() {
        // The two-outcome rule at the marker channel: a source event
        // the layer does not model is retained, never dropped.
        let marker = Marker::new(
            42,
            MarkerKind::SourceMarker {
                namespace: "kryoflux".into(),
                code: 0x0b,
            },
        )
        .with_payload(vec![1, 2, 3]);
        let observation = observed("kryoflux", 800, Vec::new(), vec![marker]).unwrap();

        assert_eq!(observation.markers()[0].payload(), [1, 2, 3]);
        match observation.markers()[0].kind() {
            MarkerKind::SourceMarker { namespace, code } => {
                assert_eq!(namespace, "kryoflux");
                assert_eq!(*code, 0x0b);
            }
            other => panic!("expected a retained source marker, got {other:?}"),
        }
    }

    #[test]
    fn a_zero_span_is_refused_rather_than_given_an_invented_period() {
        let error = observed("scp", 0, Vec::new(), Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("span"), "{error}");
    }

    #[test]
    fn an_observation_with_no_transitions_is_valid_evidence() {
        // An unrecorded or blank revolution is something the capture
        // observed, not a malformed record.
        let observation = observed("scp", 800, Vec::new(), Vec::new()).unwrap();

        assert_eq!(observation.span(), 800);
        assert!(observation.transitions().is_empty());
        assert_eq!(observation.wrap_interval(), None);
    }

    #[test]
    fn the_cyclic_wrap_is_implied_by_the_span_not_a_duplicate_pulse() {
        // Last transition at 790, first at 10, circumference 800: the
        // interval closing the circle is 20, and no boundary pulse is
        // stored to say so.
        let observation = observed("scp", 800, vec![10, 400, 790], Vec::new()).unwrap();

        assert_eq!(observation.transitions(), [10, 400, 790]);
        assert_eq!(observation.wrap_interval(), Some(20));
    }

    #[test]
    fn a_lone_transition_wraps_to_itself_across_the_whole_span() {
        let observation = observed("scp", 800, vec![10], Vec::new()).unwrap();

        assert_eq!(observation.wrap_interval(), Some(800));
    }

    #[test]
    fn out_of_order_transitions_are_refused_rather_than_sorted() {
        let error = observed("scp", 800, vec![10, 30, 20], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(message.contains("20"), "should name the offending tick: {message}");
        assert!(message.contains("30"), "should name what it followed: {message}");
    }

    #[test]
    fn a_repeated_transition_position_is_refused() {
        // Strictly increasing, so a duplicate pulse is contradictory
        // evidence rather than something to deduplicate.
        let error = observed("scp", 800, vec![10, 20, 20], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_transition_at_or_past_the_span_is_refused_rather_than_wrapped() {
        let error = observed("kryoflux", 800, vec![10, 20, 800], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(message.contains("800"), "should name the offending tick: {message}");
        assert!(message.contains("span"), "should name the span: {message}");
    }
}
