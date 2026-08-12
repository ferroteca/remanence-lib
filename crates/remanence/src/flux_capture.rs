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
use std::sync::Mutex;

use crate::checksum::crc32;
use crate::error::{Error, Result};
use crate::evidence::{Issue, Provenance};

/// One tick of a capture's declared [`TimeBase`]. Never a wall-clock
/// unit, and never converted to floating point.
pub(crate) type Tick = u64;

/// A declared timing basis: an exact positive rational count of ticks
/// per second.
///
/// Both of the flux family's models declare one — a capture the
/// instrument's, a medium its profile's reference clock — and neither
/// rate is exactly representable any other way.
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
    provenance: Provenance,
}

impl Marker {
    pub(crate) fn new(position: Tick, kind: MarkerKind, provenance: Provenance) -> Self {
        Self {
            position,
            kind,
            payload: Vec::new(),
            provenance,
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

    /// How this marker came to be known.
    ///
    /// A source's own index record and a pattern some profile
    /// synthesized are both markers; only this tells them apart, which
    /// is what keeps an absent marker channel absent evidence rather
    /// than a regular pattern nobody recorded.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Where something sat in the artifact the adapter read, in bytes.
///
/// The capture side anchors to the source artifact, which is why this
/// is not the namespace layer's floor extent: a floor is addressed
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
    source: SourceId,
    key: String,
    value: String,
    ordinal: u64,
}

impl MetadataRecord {
    pub(crate) fn new(
        namespace: &'static str,
        source: SourceId,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            namespace,
            source,
            key: key.into(),
            value: value.into(),
            ordinal: 0,
        }
    }

    pub(crate) fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// The artifact that stated it. A capture assembled from many
    /// members has many sources stating the same key, and a fact whose
    /// speaker is unrecorded is a fact nobody can go back to.
    pub(crate) fn source(&self) -> SourceId {
        self.source
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
pub(crate) fn write_varint(out: &mut Vec<u8>, mut value: u64) {
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
pub(crate) fn read_varint(source: &str, bytes: &[u8], at: usize) -> Result<(u64, usize)> {
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

/// Writes a marker channel's records into one chunk's payload.
///
/// Positions are absolute. A marker channel is not a run of ascending
/// ticks — two markers may share a position, and a later one may sit
/// earlier on the circle — so there is no unsigned gap to code against,
/// and the recorded sequence is what is written.
fn encode_markers(markers: &[Marker]) -> Vec<u8> {
    let mut out = Vec::new();
    for marker in markers {
        write_varint(&mut out, marker.position);
        match &marker.kind {
            MarkerKind::Index => write_varint(&mut out, 0),
            MarkerKind::HardSector => write_varint(&mut out, 1),
            MarkerKind::WriteSplice => write_varint(&mut out, 2),
            MarkerKind::SourceMarker { namespace, code } => {
                write_varint(&mut out, 3);
                write_varint(&mut out, namespace.len() as u64);
                out.extend_from_slice(namespace.as_bytes());
                write_varint(&mut out, u64::from(*code));
            }
        }
        write_varint(&mut out, marker.payload.len() as u64);
        out.extend_from_slice(&marker.payload);
        write_text(&mut out, marker.provenance.source);
        write_varint(&mut out, marker.provenance.notes.len() as u64);
        for note in &marker.provenance.notes {
            write_text(&mut out, note);
        }
    }
    out
}

/// Writes one length-prefixed UTF-8 string.
pub(crate) fn write_text(out: &mut Vec<u8>, text: &str) {
    write_varint(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// Reads a marker chunk's payload back into the records it was coded
/// from, in the order they were recorded.
fn decode_markers(source: &str, bytes: &[u8], known: &[&'static str]) -> Result<Vec<Marker>> {
    let mut markers = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let (position, used) = read_varint(source, bytes, at)?;
        at += used;
        let (tag, used) = read_varint(source, bytes, at)?;
        at += used;
        let kind = match tag {
            0 => MarkerKind::Index,
            1 => MarkerKind::HardSector,
            2 => MarkerKind::WriteSplice,
            3 => {
                let (namespace, used) = read_text(source, bytes, at)?;
                at += used;
                let (code, used) = read_varint(source, bytes, at)?;
                at += used;
                let code = u32::try_from(code).map_err(|_| {
                    Error::invalid_image(
                        source,
                        "marker states a source code wider than the channel carries",
                    )
                })?;
                MarkerKind::SourceMarker { namespace, code }
            }
            other => {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "marker states a kind {other}, which this version has no \
                         reading of"
                    ),
                ));
            }
        };
        let (length, used) = read_varint(source, bytes, at)?;
        at += used;
        let length = usize::try_from(length).map_err(|_| {
            Error::invalid_image(
                source,
                "marker states a payload longer than this host can address",
            )
        })?;
        let payload = bytes
            .get(at..at + length)
            .ok_or_else(|| {
                Error::invalid_image(
                    source,
                    "marker chunk ends inside the payload it states, so it holds \
                     fewer records than it claims",
                )
            })?
            .to_vec();
        at += length;
        let (spelling, used) = read_text(source, bytes, at)?;
        at += used;
        let namespace = known
            .iter()
            .find(|candidate| **candidate == spelling)
            .copied()
            .ok_or_else(|| {
                Error::invalid_image(
                    source,
                    format!(
                        "marker states the namespace {spelling:?}, which this reader \
                         cannot place"
                    ),
                )
            })?;
        let (notes, used) = read_varint(source, bytes, at)?;
        at += used;
        let mut provenance = Provenance::new(namespace);
        for _ in 0..notes {
            let (note, used) = read_text(source, bytes, at)?;
            at += used;
            provenance = provenance.note(note);
        }
        markers.push(Marker {
            position,
            kind,
            payload,
            provenance,
        });
    }
    Ok(markers)
}

/// Reads one length-prefixed UTF-8 string, and the bytes it used.
pub(crate) fn read_text(source: &str, bytes: &[u8], at: usize) -> Result<(String, usize)> {
    let (length, used) = read_varint(source, bytes, at)?;
    let length = usize::try_from(length).map_err(|_| {
        Error::invalid_image(
            source,
            "record states text longer than this host can address",
        )
    })?;
    let raw = bytes
        .get(at + used..at + used + length)
        .ok_or_else(|| Error::invalid_image(source, "record ends inside the text it states"))?;
    let text = std::str::from_utf8(raw)
        .map_err(|_| Error::invalid_image(source, "record states text that is not text"))?;
    Ok((text.to_owned(), used + length))
}

/// One addressable run of a marker channel.
///
/// Its bounds are the lowest and highest position it holds, not its
/// first and last: markers are not ordered by position, so the ends
/// would let a reader skipping by tick range skip a chunk covering the
/// tick it wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkerChunk {
    ordinal: u64,
    lowest: Tick,
    highest: Tick,
    count: u64,
    payload: Vec<u8>,
}

impl MarkerChunk {
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// The lowest and highest tick this chunk holds, inclusive.
    pub(crate) fn bounds(&self) -> (Tick, Tick) {
        (self.lowest, self.highest)
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Splits a marker channel into chunks of at most `records` each.
///
/// Infallible where the transition split is not: any sequence of
/// markers is a valid one, there being no ordering for it to violate.
fn split_markers(markers: &[Marker], records: usize) -> Vec<MarkerChunk> {
    markers
        .chunks(records.max(1))
        .enumerate()
        .map(|(ordinal, run)| MarkerChunk {
            ordinal: ordinal as u64,
            lowest: run.iter().map(Marker::position).min().unwrap_or(0),
            highest: run.iter().map(Marker::position).max().unwrap_or(0),
            count: run.len() as u64,
            payload: encode_markers(run),
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
    pub(crate) fn new(track: TrackKey, scope: ScopeId, kind: SectionKind, ordinal: u64) -> Self {
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

/// What a backing may be keyed by.
///
/// The backing is one mechanism serving both of the flux family's
/// models (P22), and they do not address alike: a capture is addressed
/// by the location its *source* named, a medium by the location its
/// *profile* declares. Sharing the record stream and the ordered index
/// while keeping the two addressings apart is the whole of why this is
/// a seam rather than one concrete key.
pub(crate) trait SectionAddress: Clone + Ord {
    /// The namespace a refusal about this section reads under (P4).
    fn namespace(&self) -> &'static str;

    /// Appends this address, self-delimiting so an index node can hold
    /// a run of them without a separate length table.
    fn write(&self, out: &mut Vec<u8>);

    /// Reads one address back, returning it and the bytes it used.
    ///
    /// `known` is the set of namespaces this reader can place. The
    /// backing is private session state, so a namespace outside that
    /// set means the index is not the one this layer wrote, and the
    /// layer refuses rather than resolving it to something plausible
    /// and addressing the wrong sections from then on.
    fn read(source: &str, bytes: &[u8], at: usize, known: &[&'static str])
    -> Result<(Self, usize)>;
}

impl SectionAddress for SectionKey {
    fn namespace(&self) -> &'static str {
        self.track.namespace
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_section_key(out, self);
    }

    fn read(
        source: &str,
        bytes: &[u8],
        at: usize,
        known: &[&'static str],
    ) -> Result<(Self, usize)> {
        read_section_key(source, bytes, at, known)
    }
}

/// Writes one complete section key, self-delimiting so a node can hold
/// a run of them without a separate length table.
fn write_section_key(out: &mut Vec<u8>, key: &SectionKey) {
    let namespace = key.track.namespace.as_bytes();
    write_varint(out, namespace.len() as u64);
    out.extend_from_slice(namespace);
    write_varint(out, key.track.position.numerator);
    write_varint(out, key.track.position.denominator);
    // Zero is no head at all, which is a different fact from head zero.
    match key.track.head {
        None => write_varint(out, 0),
        Some(head) => write_varint(out, head.saturating_add(1)),
    }
    match key.scope {
        ScopeId::Track => write_varint(out, 0),
        ScopeId::Run(CaptureRunId(id)) => {
            write_varint(out, 1);
            write_varint(out, id);
        }
        ScopeId::Observation(ObservationId(id)) => {
            write_varint(out, 2);
            write_varint(out, id);
        }
    }
    write_varint(
        out,
        match key.kind {
            SectionKind::TrackMetadata => 0,
            SectionKind::CaptureRunMetadata => 1,
            SectionKind::ObservationMetadata => 2,
            SectionKind::TransitionChunk => 3,
            SectionKind::MarkerChunk => 4,
            SectionKind::IssueChunk => 5,
        },
    );
    write_varint(out, key.ordinal);
}

/// Reads one complete section key, returning it and the bytes it used.
///
/// `known` is the set of namespaces this reader can place. The backing
/// is private session state, so a namespace outside that set means the
/// index is not the one this layer wrote, and the layer refuses rather
/// than resolving it to something plausible and addressing the wrong
/// sections from then on.
fn read_section_key(
    source: &str,
    bytes: &[u8],
    at: usize,
    known: &[&'static str],
) -> Result<(SectionKey, usize)> {
    let mut cursor = at;
    let (length, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let length = usize::try_from(length).map_err(|_| {
        Error::invalid_image(
            source,
            "index names a namespace longer than this host can address",
        )
    })?;
    let raw = bytes
        .get(cursor..cursor + length)
        .ok_or_else(|| Error::invalid_image(source, "index ends inside the namespace it names"))?;
    cursor += length;
    let spelling = std::str::from_utf8(raw)
        .map_err(|_| Error::invalid_image(source, "index names a namespace that is not text"))?;
    let namespace = known
        .iter()
        .find(|candidate| **candidate == spelling)
        .copied()
        .ok_or_else(|| {
            Error::invalid_image(
                source,
                format!(
                    "index names the namespace {spelling:?}, which this reader cannot \
                     place, so the index is not the one this layer wrote"
                ),
            )
        })?;

    let (numerator, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let (denominator, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    if denominator == 0 {
        return Err(Error::invalid_image(
            source,
            "index states a location over zero steps, which is no position",
        ));
    }
    // A written position is always reduced, so an unreduced one is
    // corruption — and reducing it here would collide with the key
    // already indexed under its reduced form, leaving one of the two
    // sections unreachable while the other answered for it.
    if greatest_common_divisor(numerator, denominator) != 1 {
        return Err(Error::invalid_image(
            source,
            format!(
                "index states the location {numerator}/{denominator}, which is not \
                 in the reduced form every written key carries"
            ),
        ));
    }
    let (head, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let (scope_tag, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let scope = match scope_tag {
        0 => ScopeId::Track,
        1 | 2 => {
            let (id, used) = read_varint(source, bytes, cursor)?;
            cursor += used;
            if scope_tag == 1 {
                ScopeId::Run(CaptureRunId(id))
            } else {
                ScopeId::Observation(ObservationId(id))
            }
        }
        other => {
            return Err(Error::invalid_image(
                source,
                format!(
                    "index states a section scope {other}, which this version has no reading of"
                ),
            ));
        }
    };
    let (kind_tag, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let kind = match kind_tag {
        0 => SectionKind::TrackMetadata,
        1 => SectionKind::CaptureRunMetadata,
        2 => SectionKind::ObservationMetadata,
        3 => SectionKind::TransitionChunk,
        4 => SectionKind::MarkerChunk,
        5 => SectionKind::IssueChunk,
        other => {
            return Err(Error::invalid_image(
                source,
                format!(
                    "index states a section kind {other}, which this version has no reading of"
                ),
            ));
        }
    };
    let (ordinal, used) = read_varint(source, bytes, cursor)?;
    cursor += used;

    Ok((
        SectionKey {
            track: TrackKey {
                namespace,
                position: SourcePosition {
                    numerator,
                    denominator,
                },
                head: head.checked_sub(1),
            },
            scope,
            kind,
            ordinal,
        },
        cursor - at,
    ))
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

/// How many sections one index leaf describes.
///
/// Tests build backings with a tiny capacity to force several leaves
/// out of a handful of sections, the way the cache tests use a tiny
/// bound to force eviction.
pub(crate) const LEAF_ENTRIES: usize = 64;

/// Marks the end of a backing, and says which shape it is.
const FOOTER_MAGIC: u32 = 0x464c_5831;
const FOOTER_VERSION: u32 = 1;
/// magic, version, root offset, root length.
const FOOTER_BYTES: usize = 4 + 4 + 8 + 8;

/// Somewhere the backing's bytes can be read from at an offset.
///
/// The seam exists so the layer's own tests can assert what a read
/// touched, and so the backing does not care whether its bytes are in
/// private session storage or anywhere else.
pub(crate) trait ByteSource {
    fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()>;
}

/// Somewhere a backing's bytes are appended as it is built.
///
/// The seam is the other half of [`ByteSource`], and it exists for the
/// same reason the reader is bounded: a capture assembled from a
/// hundred and sixty-eight members would otherwise be built whole in
/// memory before a byte of it could be read back, which is the one
/// thing P27 says a session never does. A vector sink serves the
/// layer's own tests; private session storage serves an adapter.
pub(crate) trait ByteSink {
    fn append(&mut self, bytes: &[u8]) -> Result<()>;
}

impl ByteSink for Vec<u8> {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.extend_from_slice(bytes);
        Ok(())
    }
}

/// Private session storage a backing streams into (P27).
///
/// It belongs here rather than beside either model because both stream
/// into the same mechanism: what differs between a capture and a medium
/// is the address, not the storage.
pub(crate) struct SessionBacking {
    file: std::sync::Arc<std::fs::File>,
    written: u64,
}

impl SessionBacking {
    pub(crate) fn create() -> Result<Self> {
        Ok(Self {
            file: std::sync::Arc::new(crate::cache::session_storage_file()?),
            written: 0,
        })
    }

    /// The same storage, read back a bounded section at a time.
    pub(crate) fn into_source(self) -> SessionSource {
        SessionSource(self.file)
    }
}

impl ByteSink for SessionBacking {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        crate::device::write_all_at(&self.file, self.written, bytes)
            .map_err(|error| Error::io(format!("failed to write a layer backing: {error}")))?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

pub(crate) struct SessionSource(std::sync::Arc<std::fs::File>);

impl ByteSource for SessionSource {
    fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
        crate::device::read_exact_at(&self.0, offset, into)
            .map_err(|error| Error::io(format!("failed to read a layer backing: {error}")))
    }
}

/// Builds a backing by appending sections in key order.
///
/// The index is finished only once every section it references is
/// complete, which is what makes a half-written backing detectable
/// rather than a layer with holes in it.
#[derive(Debug)]
pub(crate) struct SectionWriter<K: SectionAddress, S: ByteSink = Vec<u8>> {
    sink: S,
    /// How many bytes have gone into the sink, which is where the next
    /// section will sit. The sink itself is not asked: it may be a file
    /// this writer is not the only holder of.
    written: u64,
    entries: Vec<(K, SectionLocation)>,
    last: Option<K>,
    leaf_entries: usize,
}

impl<K: SectionAddress> Default for SectionWriter<K, Vec<u8>> {
    fn default() -> Self {
        Self::with_leaf_capacity(LEAF_ENTRIES)
    }
}

impl<K: SectionAddress> SectionWriter<K, Vec<u8>> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// A writer whose index leaves hold at most `leaf_entries` sections.
    pub(crate) fn with_leaf_capacity(leaf_entries: usize) -> Self {
        Self::to_sink(Vec::new(), leaf_entries)
    }

    /// Closes a backing built in memory. Infallible where the general
    /// form is not: a vector accepts every byte offered to it.
    pub(crate) fn finish(self) -> Vec<u8> {
        self.seal().expect("a vector sink accepts every byte").0
    }
}

impl<K: SectionAddress, S: ByteSink> SectionWriter<K, S> {
    /// A writer emitting into `sink`, whose index leaves hold at most
    /// `leaf_entries` sections.
    pub(crate) fn to_sink(sink: S, leaf_entries: usize) -> Self {
        Self {
            sink,
            written: 0,
            entries: Vec::new(),
            last: None,
            leaf_entries: leaf_entries.max(1),
        }
    }

    /// Appends one section, which must sort after every section already
    /// appended.
    pub(crate) fn append(&mut self, key: K, payload: Vec<u8>) -> Result<()> {
        if let Some(last) = &self.last
            && &key <= last
        {
            return Err(Error::invalid_image(
                key.namespace(),
                "a section does not sort after the one appended before it, so the \
                 backing is not being emitted in key order",
            ));
        }
        let location = SectionLocation {
            offset: self.written,
            length: payload.len() as u64,
            checksum: crc32(&payload),
        };
        self.sink.append(&payload)?;
        self.written += payload.len() as u64;
        self.entries.push((key.clone(), location));
        self.last = Some(key);
        Ok(())
    }

    /// Closes the backing: leaves in key order, then a root naming each
    /// leaf's first key, then the fixed footer that locates the root.
    /// Returns the sink and the backing's total length.
    ///
    /// The index is appended only once every section it references is
    /// complete, so a backing cut short has no footer and is refused
    /// rather than exposed as a layer with holes in it.
    pub(crate) fn seal(mut self) -> Result<(S, u64)> {
        let mut leaves: Vec<(K, u64, u64)> = Vec::new();
        for run in self.entries.chunks(self.leaf_entries) {
            let mut leaf = Vec::new();
            write_varint(&mut leaf, run.len() as u64);
            for (key, location) in run {
                key.write(&mut leaf);
                write_varint(&mut leaf, location.offset);
                write_varint(&mut leaf, location.length);
                write_varint(&mut leaf, u64::from(location.checksum));
            }
            let offset = self.written;
            self.sink.append(&leaf)?;
            self.written += leaf.len() as u64;
            leaves.push((run[0].0.clone(), offset, leaf.len() as u64));
        }

        let mut root = Vec::new();
        write_varint(&mut root, leaves.len() as u64);
        for (first, offset, length) in &leaves {
            first.write(&mut root);
            write_varint(&mut root, *offset);
            write_varint(&mut root, *length);
        }
        let root_offset = self.written;
        self.sink.append(&root)?;
        self.written += root.len() as u64;

        let mut footer = Vec::with_capacity(FOOTER_BYTES);
        footer.extend_from_slice(&FOOTER_MAGIC.to_le_bytes());
        footer.extend_from_slice(&FOOTER_VERSION.to_le_bytes());
        footer.extend_from_slice(&root_offset.to_le_bytes());
        footer.extend_from_slice(&(root.len() as u64).to_le_bytes());
        self.sink.append(&footer)?;
        self.written += footer.len() as u64;
        Ok((self.sink, self.written))
    }
}

/// Finds where one section sits, reading only the index path to it.
///
/// Three bounded reads whatever the capture's size: the fixed footer,
/// the root, and the one leaf whose range covers the key. The index is
/// never resident whole, which is what lets a capture of any size open
/// under a bound that knew nothing of that size.
pub(crate) fn locate_section<K: SectionAddress>(
    source: &dyn ByteSource,
    total_bytes: u64,
    key: &K,
    known: &[&'static str],
) -> Result<Option<SectionLocation>> {
    let namespace = key.namespace();
    if total_bytes < FOOTER_BYTES as u64 {
        return Err(Error::invalid_image(
            namespace,
            "backing is shorter than its own footer, so no index was ever appended",
        ));
    }
    let mut footer = [0u8; FOOTER_BYTES];
    source.read_at(total_bytes - FOOTER_BYTES as u64, &mut footer)?;
    if u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) != FOOTER_MAGIC {
        return Err(Error::invalid_image(
            namespace,
            "backing does not end in an index footer, so it was never completed",
        ));
    }
    let version = u32::from_le_bytes([footer[4], footer[5], footer[6], footer[7]]);
    if version != FOOTER_VERSION {
        return Err(Error::invalid_image(
            namespace,
            format!(
                "backing states index version {version}, which this build has no \
                 reading of"
            ),
        ));
    }
    let root_offset = u64::from_le_bytes(footer[8..16].try_into().expect("eight bytes"));
    let root_length = u64::from_le_bytes(footer[16..24].try_into().expect("eight bytes"));
    let root = read_span(source, namespace, total_bytes, root_offset, root_length)?;

    // Descend: the last leaf whose first key does not exceed the wanted
    // one is the only leaf that can hold it.
    let mut at = 0;
    let (count, used) = read_varint(namespace, &root, at)?;
    at += used;
    let mut chosen: Option<(u64, u64)> = None;
    for _ in 0..count {
        let (first, used) = K::read(namespace, &root, at, known)?;
        at += used;
        let (offset, used) = read_varint(namespace, &root, at)?;
        at += used;
        let (length, used) = read_varint(namespace, &root, at)?;
        at += used;
        if &first <= key {
            chosen = Some((offset, length));
        } else {
            break;
        }
    }
    let Some((offset, length)) = chosen else {
        return Ok(None);
    };

    let leaf = read_span(source, namespace, total_bytes, offset, length)?;
    let mut at = 0;
    let (count, used) = read_varint(namespace, &leaf, at)?;
    at += used;
    for _ in 0..count {
        let (held, used) = K::read(namespace, &leaf, at, known)?;
        at += used;
        let (offset, used) = read_varint(namespace, &leaf, at)?;
        at += used;
        let (length, used) = read_varint(namespace, &leaf, at)?;
        at += used;
        let (checksum, used) = read_varint(namespace, &leaf, at)?;
        at += used;
        if &held == key {
            let checksum = u32::try_from(checksum).map_err(|_| {
                Error::invalid_image(namespace, "index states a check wider than a CRC-32")
            })?;
            return Ok(Some(SectionLocation {
                offset,
                length,
                checksum,
            }));
        }
    }
    Ok(None)
}

/// Reads one bounded span, refusing one the backing cannot contain
/// before anything is sought or allocated.
fn read_span(
    source: &dyn ByteSource,
    namespace: &'static str,
    total_bytes: u64,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > total_bytes)
    {
        return Err(Error::invalid_image(
            namespace,
            format!(
                "index points at bytes {offset}..+{length}, which lie outside the \
                 backing's {total_bytes} bytes"
            ),
        ));
    }
    let length = usize::try_from(length).map_err(|_| {
        Error::invalid_image(
            namespace,
            "index states a record longer than this host can address",
        )
    })?;
    let mut span = vec![0u8; length];
    source.read_at(offset, &mut span)?;
    Ok(span)
}

/// Reads one section, and nothing else.
///
/// The index says where the record is and what it checks to, both of
/// which are verified before the bytes are believed — a record that
/// changed under the index is refused rather than handed back as flux
/// that never existed.
pub(crate) fn read_section<K: SectionAddress>(
    source: &dyn ByteSource,
    total_bytes: u64,
    key: &K,
    known: &[&'static str],
) -> Result<Vec<u8>> {
    let location = locate_section(source, total_bytes, key, known)?.ok_or_else(|| {
        Error::invalid_image(
            key.namespace(),
            "backing holds no section under the address asked for",
        )
    })?;
    let payload = read_span(
        source,
        key.namespace(),
        total_bytes,
        location.offset,
        location.length,
    )?;
    if crc32(&payload) != location.checksum {
        return Err(Error::invalid_image(
            key.namespace(),
            format!(
                "section at byte {} does not check to what the index states, so \
                 its bytes are not the ones that were written",
                location.offset
            ),
        ));
    }
    Ok(payload)
}

/// This layer's bounded working set of decoded sections (P27).
///
/// Caching is per modeled durable layer under one declared session
/// budget, so this is the capture's own and not the device stack's: that
/// one is addressed by extent offset over a virtual disk, and a
/// capture is addressed by section key.
///
/// Every entry is clean. A capture is read evidence — a modelled write
/// has no coherent destination in it, since nothing says which of
/// several disagreeing observations a drive would overwrite — so there
/// is no dirty class here to spill. Writes land in the medium reduced
/// from the capture, which reuses this backing and brings that half of
/// the policy with it.
#[derive(Debug)]
pub(crate) struct SectionCache<K: SectionAddress> {
    resident: BTreeMap<K, CachedSection>,
    bytes_resident: u64,
    bound: u64,
    clock: u64,
}

#[derive(Debug)]
struct CachedSection {
    payload: Vec<u8>,
    /// When this entry was last served, for choosing what to drop.
    used: u64,
}

impl<K: SectionAddress> SectionCache<K> {
    /// A cache bounded at `bytes` of resident section payloads.
    ///
    /// A bound smaller than one section narrows the working set rather
    /// than refusing service: the section still loads, it simply does
    /// not stay.
    pub(crate) fn with_bytes(bytes: u64) -> Self {
        Self {
            resident: BTreeMap::new(),
            bytes_resident: 0,
            bound: bytes,
            clock: 0,
        }
    }

    pub(crate) fn resident_bytes(&self) -> u64 {
        self.bytes_resident
    }

    /// Whether this section is currently in the working set.
    pub(crate) fn holds(&self, key: &K) -> bool {
        self.resident.contains_key(key)
    }

    /// Serves one section, reading it through the index on a miss.
    ///
    /// A hit costs no I/O. A miss reads one bounded record range and
    /// never a whole capture, which is the promise the whole
    /// section-addressed backing exists to keep.
    pub(crate) fn section(
        &mut self,
        source: &dyn ByteSource,
        total_bytes: u64,
        key: &K,
        known: &[&'static str],
    ) -> Result<&[u8]> {
        self.clock += 1;
        if !self.resident.contains_key(key) {
            let payload = read_section(source, total_bytes, key, known)?;
            self.make_room(payload.len() as u64);
            self.bytes_resident += payload.len() as u64;
            self.resident.insert(
                key.clone(),
                CachedSection {
                    payload,
                    used: self.clock,
                },
            );
        }
        let clock = self.clock;
        let entry = self.resident.get_mut(key).expect("just made resident");
        entry.used = clock;
        Ok(&entry.payload)
    }

    /// Drops least-recently-served entries until `wanted` more bytes
    /// fit. Clean state is always evictable, so this never fails.
    fn make_room(&mut self, wanted: u64) {
        while !self.resident.is_empty() && self.bytes_resident + wanted > self.bound {
            let coldest = self
                .resident
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
                .expect("the map is not empty");
            if let Some(dropped) = self.resident.remove(&coldest) {
                self.bytes_resident -= dropped.payload.len() as u64;
            }
        }
    }
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
                        Marker::new(
                            marker.position() - start,
                            marker.kind().clone(),
                            marker.provenance().clone(),
                        )
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

    /// The position, as an exact reduced ratio of the source's steps.
    pub(crate) fn parts(self) -> (u64, u64) {
        (self.numerator, self.denominator)
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

pub(crate) fn greatest_common_divisor(a: u64, b: u64) -> u64 {
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

    pub(crate) fn at(namespace: &'static str, position: SourcePosition, head: Option<u64>) -> Self {
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

/// What an opened capture keeps resident for one capture run: its
/// identity, its shape, and how it came to be known — never its
/// payload.
///
/// The transitions and markers themselves live in the backing and load
/// one bounded section at a time, because a capture set is routinely a
/// hundred and sixty-eight members and holding every pulse of it
/// resident is the assumption P27 forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureRunEntry {
    id: CaptureRunId,
    ordinal: u64,
    source: SourceId,
    transitions: u64,
    markers: u64,
    indices: u64,
    /// The last transition's tick — the extent of what was recorded,
    /// not a circumference. A run states no period.
    extent: Tick,
    before_first_index: u64,
    after_last_index: u64,
    transition_chunks: u64,
    marker_chunks: u64,
    provenance: Provenance,
}

impl CaptureRunEntry {
    pub(crate) fn id(&self) -> CaptureRunId {
        self.id
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// The artifact this run was transferred from.
    pub(crate) fn source(&self) -> SourceId {
        self.source
    }

    pub(crate) fn transitions(&self) -> u64 {
        self.transitions
    }

    pub(crate) fn markers(&self) -> u64 {
        self.markers
    }

    /// How many of those markers are index events — the count that
    /// says how many circular observations the run could bound.
    pub(crate) fn indices(&self) -> u64 {
        self.indices
    }

    pub(crate) fn extent(&self) -> Tick {
        self.extent
    }

    /// Transitions recorded before the first index and after the last:
    /// evidence bounding into circular observations does not consume,
    /// counted here so it is visibly retained rather than assumed.
    pub(crate) fn before_first_index(&self) -> u64 {
        self.before_first_index
    }

    pub(crate) fn after_last_index(&self) -> u64 {
        self.after_last_index
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What an opened capture keeps resident for one observation, on the
/// same terms as [`CaptureRunEntry`]: identity and shape, never the
/// pulses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationEntry {
    id: ObservationId,
    ordinal: u64,
    span: Tick,
    source: CaptureRunSlice,
    transitions: u64,
    markers: u64,
    provenance: Provenance,
}

impl ObservationEntry {
    pub(crate) fn id(&self) -> ObservationId {
        self.id
    }

    /// Its place in this location's source-record order. Not a rank.
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub(crate) fn span(&self) -> Tick {
        self.span
    }

    /// The run and range this was bounded from.
    pub(crate) fn source(&self) -> CaptureRunSlice {
        self.source
    }

    pub(crate) fn transitions(&self) -> u64 {
        self.transitions
    }

    pub(crate) fn markers(&self) -> u64 {
        self.markers
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
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
    runs: Vec<CaptureRunEntry>,
    observations: Vec<ObservationEntry>,
    issues: Vec<Issue>,
}

impl Track {
    pub(crate) fn new(key: TrackKey) -> Self {
        Self {
            key,
            runs: Vec::new(),
            observations: Vec::new(),
            issues: Vec::new(),
        }
    }

    pub(crate) fn key(&self) -> &TrackKey {
        &self.key
    }

    /// The source transfers recorded at this location, in the order
    /// they were transferred.
    pub(crate) fn runs(&self) -> &[CaptureRunEntry] {
        &self.runs
    }

    pub(crate) fn observations(&self) -> &[ObservationEntry] {
        &self.observations
    }

    pub(crate) fn issues(&self) -> &[Issue] {
        &self.issues
    }

    /// Takes an observation the capture has already identified, giving
    /// it the next ordinal in this location's source-record order.
    fn push_observation(&mut self, id: ObservationId, observation: &Observation) {
        self.observations.push(ObservationEntry {
            id,
            ordinal: self.observations.len() as u64,
            span: observation.span,
            source: observation.source,
            transitions: observation.transitions.len() as u64,
            markers: observation.markers.len() as u64,
            provenance: observation.provenance.clone(),
        });
    }
}

/// Where an opened capture's sections are read back from, and the
/// bounded working set they are served through (P27).
struct CaptureBacking {
    bytes: Box<dyn ByteSource + Send + Sync>,
    total_bytes: u64,
    cache: Mutex<SectionCache<SectionKey>>,
    /// The namespaces this reader can place. An index naming another
    /// one is not the index this layer wrote.
    known: Vec<&'static str>,
}

impl std::fmt::Debug for CaptureBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureBacking")
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

/// One opened capture: its declared timing basis and the locations it
/// supplied evidence for.
#[derive(Debug)]
pub(crate) struct FluxCapture {
    envelope: CaptureEnvelope,
    time_base: TimeBase,
    tracks: BTreeMap<TrackKey, Track>,
    next_observation_id: u64,
    next_run_id: u64,
    backing: Option<CaptureBacking>,
}

impl FluxCapture {
    pub(crate) fn new(time_base: TimeBase) -> Self {
        Self {
            envelope: CaptureEnvelope::new(),
            time_base,
            tracks: BTreeMap::new(),
            next_observation_id: 0,
            next_run_id: 0,
            backing: None,
        }
    }

    pub(crate) fn time_base(&self) -> TimeBase {
        self.time_base
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
        observation: &Observation,
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

    fn backing(&self, namespace: &'static str) -> Result<&CaptureBacking> {
        self.backing.as_ref().ok_or_else(|| {
            Error::invalid_image(
                namespace,
                "capture has no backing to read its evidence from, so nothing \
                 was ever written for it to serve",
            )
        })
    }

    /// Reads one section through the bounded working set.
    fn section(&self, key: &SectionKey) -> Result<Vec<u8>> {
        let backing = self.backing(key.track.namespace)?;
        let mut cache = backing
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .section(
                backing.bytes.as_ref(),
                backing.total_bytes,
                key,
                &backing.known,
            )
            .map(<[u8]>::to_vec)
    }

    /// Whether a section is currently in the working set. The layer's
    /// own bounded-reload test asks; nothing else does.
    fn holds_section(&self, key: &SectionKey) -> bool {
        self.backing.as_ref().is_some_and(|backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .holds(key)
        })
    }

    /// One run's transitions, read back from the backing a chunk at a
    /// time. Never the whole capture, and never another location.
    pub(crate) fn run_transitions(
        &self,
        key: &TrackKey,
        run: &CaptureRunEntry,
    ) -> Result<Vec<Tick>> {
        let mut ticks = Vec::with_capacity(run.transitions as usize);
        for ordinal in 0..run.transition_chunks {
            let section = SectionKey::new(
                key.clone(),
                ScopeId::Run(run.id),
                SectionKind::TransitionChunk,
                ordinal,
            );
            ticks.extend(decode_transitions(&self.section(&section)?)?);
        }
        Ok(ticks)
    }

    /// One run's marker channels, in the order they were recorded.
    pub(crate) fn run_markers(&self, key: &TrackKey, run: &CaptureRunEntry) -> Result<Vec<Marker>> {
        let backing = self.backing(key.namespace)?;
        let known = backing.known.clone();
        let mut markers = Vec::with_capacity(run.markers as usize);
        for ordinal in 0..run.marker_chunks {
            let section = SectionKey::new(
                key.clone(),
                ScopeId::Run(run.id),
                SectionKind::MarkerChunk,
                ordinal,
            );
            markers.extend(decode_markers(
                key.namespace,
                &self.section(&section)?,
                &known,
            )?);
        }
        Ok(markers)
    }

    /// One observation, rebuilt from the run it was bounded from.
    ///
    /// The observation's payload shares the run's indexed chunks rather
    /// than duplicating them, so this reads the run's evidence over the
    /// recorded range and rebases it to the observation's own origin —
    /// the same cut, made again from the same bytes.
    pub(crate) fn observation(
        &self,
        key: &TrackKey,
        entry: &ObservationEntry,
    ) -> Result<Observation> {
        let run = self
            .track(key)
            .and_then(|track| {
                track
                    .runs
                    .iter()
                    .find(|run| run.ordinal == entry.source.run_ordinal())
            })
            .ok_or_else(|| {
                Error::invalid_image(
                    key.namespace,
                    format!(
                        "observation was bounded from capture run {}, which this \
                         location does not hold",
                        entry.source.run_ordinal()
                    ),
                )
            })?;
        let (start, end) = (entry.source.start(), entry.source.end());
        let transitions = self
            .run_transitions(key, run)?
            .into_iter()
            .filter(|position| *position >= start && *position < end)
            .map(|position| position - start)
            .collect();
        let markers = self
            .run_markers(key, run)?
            .into_iter()
            .filter(|marker| marker.position() >= start && marker.position() < end)
            .map(|marker| {
                Marker::new(
                    marker.position() - start,
                    marker.kind().clone(),
                    marker.provenance().clone(),
                )
                .with_payload(marker.payload().to_vec())
            })
            .collect();
        let mut observation = Observation::new(
            key.namespace,
            entry.provenance.clone(),
            entry.source,
            entry.span,
            transitions,
            markers,
        )?;
        observation.id = entry.id;
        observation.ordinal = entry.ordinal;
        Ok(observation)
    }
}

/// How many transitions or markers one chunk of the backing holds.
///
/// A record count rather than a byte count, so the same evidence always
/// splits the same way however the backing is rebuilt.
pub(crate) const CHUNK_RECORDS: usize = 4096;

/// Builds an opened capture and its backing together: the payload
/// streams into the sink as each location arrives, and only identity
/// and shape stay resident.
///
/// Locations must arrive in ascending key order, which is what the
/// section index requires and what makes a half-written backing
/// detectable rather than a layer with holes in it.
pub(crate) struct CaptureBuilder<S: ByteSink> {
    writer: SectionWriter<SectionKey, S>,
    capture: FluxCapture,
    chunk_records: usize,
}

impl<S: ByteSink> CaptureBuilder<S> {
    pub(crate) fn new(time_base: TimeBase, sink: S) -> Self {
        Self {
            writer: SectionWriter::to_sink(sink, LEAF_ENTRIES),
            capture: FluxCapture::new(time_base),
            chunk_records: CHUNK_RECORDS,
        }
    }

    /// A builder whose chunks hold at most `records` each. The layer's
    /// own tests use a tiny value to force several chunks out of a
    /// handful of transitions.
    pub(crate) fn with_chunk_records(mut self, records: usize) -> Self {
        self.chunk_records = records;
        self
    }

    pub(crate) fn envelope_mut(&mut self) -> &mut CaptureEnvelope {
        self.capture.envelope_mut()
    }

    /// Adds one location and everything captured at it: its transfers,
    /// the circular observations they bound, and whatever was qualified
    /// about either.
    pub(crate) fn add_location(
        &mut self,
        key: TrackKey,
        source: SourceId,
        runs: &[CaptureRun],
        issues: Vec<Issue>,
    ) -> Result<()> {
        let namespace = key.namespace;
        let mut track = Track::new(key.clone());
        track.issues = issues;
        self.writer.append(
            SectionKey::new(key.clone(), ScopeId::Track, SectionKind::TrackMetadata, 0),
            encode_track_metadata(runs.len() as u64, &track.issues),
        )?;
        if !track.issues.is_empty() {
            self.writer.append(
                SectionKey::new(key.clone(), ScopeId::Track, SectionKind::IssueChunk, 0),
                encode_issues(&track.issues),
            )?;
        }

        // Every run's sections before any observation's: a run scope
        // sorts ahead of an observation scope, so interleaving them
        // would emit the backing out of key order.
        for run in runs {
            let id = CaptureRunId(self.capture.next_run_id);
            self.capture.next_run_id += 1;
            let transitions = split_transitions(run.transitions(), self.chunk_records)?;
            let markers = split_markers(run.markers(), self.chunk_records);
            let indices: Vec<Tick> = run
                .markers()
                .iter()
                .filter(|marker| marker.kind() == &MarkerKind::Index)
                .map(Marker::position)
                .collect();
            let entry = CaptureRunEntry {
                id,
                ordinal: run.ordinal(),
                source,
                transitions: run.transitions().len() as u64,
                markers: run.markers().len() as u64,
                indices: indices.len() as u64,
                extent: run.transitions().last().copied().unwrap_or(0),
                before_first_index: match indices.first() {
                    Some(first) => count_below(run.transitions(), *first),
                    None => run.transitions().len() as u64,
                },
                after_last_index: match indices.last() {
                    Some(last) => {
                        run.transitions().len() as u64 - count_below(run.transitions(), *last)
                    }
                    None => 0,
                },
                transition_chunks: transitions.len() as u64,
                marker_chunks: markers.len() as u64,
                provenance: run.provenance().clone(),
            };
            self.writer.append(
                SectionKey::new(
                    key.clone(),
                    ScopeId::Run(id),
                    SectionKind::CaptureRunMetadata,
                    0,
                ),
                encode_run_metadata(&entry),
            )?;
            for chunk in &transitions {
                self.writer.append(
                    SectionKey::new(
                        key.clone(),
                        ScopeId::Run(id),
                        SectionKind::TransitionChunk,
                        chunk.ordinal(),
                    ),
                    chunk.payload().to_vec(),
                )?;
            }
            for chunk in &markers {
                self.writer.append(
                    SectionKey::new(
                        key.clone(),
                        ScopeId::Run(id),
                        SectionKind::MarkerChunk,
                        chunk.ordinal(),
                    ),
                    chunk.payload().to_vec(),
                )?;
            }
            track.runs.push(entry);
        }
        self.capture.insert_track(track);

        for run in runs {
            for observation in run.observations(namespace)? {
                let id = self.capture.admit_observation(&key, &observation)?;
                let entry = self
                    .capture
                    .track(&key)
                    .and_then(|track| track.observations().last())
                    .expect("the observation was just admitted")
                    .clone();
                self.writer.append(
                    SectionKey::new(
                        key.clone(),
                        ScopeId::Observation(id),
                        SectionKind::ObservationMetadata,
                        0,
                    ),
                    encode_observation_metadata(&entry),
                )?;
            }
        }
        Ok(())
    }

    /// Closes the backing and hands back the capture it addresses,
    /// together with the sink and its length so the caller can attach
    /// whatever it wrote into.
    pub(crate) fn seal(self) -> Result<(FluxCapture, S, u64)> {
        let (sink, total) = self.writer.seal()?;
        Ok((self.capture, sink, total))
    }
}

/// How many of `transitions` sit strictly below `tick`. The list
/// ascends, so this is a bound rather than a scan.
fn count_below(transitions: &[Tick], tick: Tick) -> u64 {
    transitions.partition_point(|position| *position < tick) as u64
}

impl FluxCapture {
    /// Attaches the backing this capture's sections were written into,
    /// with the working set they are served through.
    pub(crate) fn attach_backing(
        &mut self,
        bytes: Box<dyn ByteSource + Send + Sync>,
        total_bytes: u64,
        cache_bytes: u64,
        known: Vec<&'static str>,
    ) {
        self.backing = Some(CaptureBacking {
            bytes,
            total_bytes,
            cache: Mutex::new(SectionCache::with_bytes(cache_bytes)),
            known,
        });
    }

    /// The backing's total length in bytes.
    pub(crate) fn backing_bytes(&self) -> u64 {
        self.backing
            .as_ref()
            .map_or(0, |backing| backing.total_bytes)
    }

    /// How much of the backing is currently resident.
    pub(crate) fn resident_bytes(&self) -> u64 {
        self.backing.as_ref().map_or(0, |backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .resident_bytes()
        })
    }
}

/// The version every metadata section this layer writes carries. An
/// unknown one refuses that section rather than being read as this one.
const METADATA_VERSION: u64 = 1;

pub(crate) fn encode_provenance(out: &mut Vec<u8>, provenance: &Provenance) {
    write_text(out, provenance.source);
    write_varint(out, provenance.notes.len() as u64);
    for note in &provenance.notes {
        write_text(out, note);
    }
}

pub(crate) fn decode_provenance(
    source: &str,
    bytes: &[u8],
    at: usize,
    known: &[&'static str],
) -> Result<(Provenance, usize)> {
    let mut cursor = at;
    let (spelling, used) = read_text(source, bytes, cursor)?;
    cursor += used;
    let namespace = known
        .iter()
        .find(|candidate| **candidate == spelling)
        .copied()
        .ok_or_else(|| {
            Error::invalid_image(
                source,
                format!(
                    "backing states the provenance namespace {spelling:?}, which this \
                     reader cannot place, so the backing is not the one this layer wrote"
                ),
            )
        })?;
    let (count, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let mut provenance = Provenance::new(namespace);
    for _ in 0..count {
        let (note, used) = read_text(source, bytes, cursor)?;
        cursor += used;
        provenance = provenance.note(note);
    }
    Ok((provenance, cursor - at))
}

fn encode_track_metadata(runs: u64, issues: &[Issue]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, runs);
    write_varint(&mut out, issues.len() as u64);
    out
}

fn encode_issues(issues: &[Issue]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, issues.len() as u64);
    for issue in issues {
        write_text(&mut out, issue.code);
        write_text(&mut out, &issue.detail);
    }
    out
}

fn encode_run_metadata(entry: &CaptureRunEntry) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, entry.id.0);
    write_varint(&mut out, entry.ordinal);
    write_varint(&mut out, entry.source.0);
    write_varint(&mut out, entry.transitions);
    write_varint(&mut out, entry.markers);
    write_varint(&mut out, entry.indices);
    write_varint(&mut out, entry.extent);
    write_varint(&mut out, entry.before_first_index);
    write_varint(&mut out, entry.after_last_index);
    write_varint(&mut out, entry.transition_chunks);
    write_varint(&mut out, entry.marker_chunks);
    encode_provenance(&mut out, &entry.provenance);
    out
}

/// Reads one run's metadata section back into the entry it was written
/// from. `known` places the namespaces the reader can resolve.
fn decode_run_metadata(
    source: &str,
    bytes: &[u8],
    known: &[&'static str],
) -> Result<CaptureRunEntry> {
    let mut at = 0;
    let next = |at: &mut usize| -> Result<u64> {
        let (value, used) = read_varint(source, bytes, *at)?;
        *at += used;
        Ok(value)
    };
    let version = next(&mut at)?;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            source,
            format!(
                "backing states run metadata version {version}, which this build has no reading of"
            ),
        ));
    }
    let entry = CaptureRunEntry {
        id: CaptureRunId(next(&mut at)?),
        ordinal: next(&mut at)?,
        source: SourceId(next(&mut at)?),
        transitions: next(&mut at)?,
        markers: next(&mut at)?,
        indices: next(&mut at)?,
        extent: next(&mut at)?,
        before_first_index: next(&mut at)?,
        after_last_index: next(&mut at)?,
        transition_chunks: next(&mut at)?,
        marker_chunks: next(&mut at)?,
        provenance: Provenance::new("flux-capture"),
    };
    let (provenance, _) = decode_provenance(source, bytes, at, known)?;
    Ok(CaptureRunEntry {
        provenance,
        ..entry
    })
}

fn encode_observation_metadata(entry: &ObservationEntry) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, entry.id.0);
    write_varint(&mut out, entry.ordinal);
    write_varint(&mut out, entry.span);
    write_varint(&mut out, entry.source.run_ordinal());
    write_varint(&mut out, entry.source.start());
    write_varint(&mut out, entry.source.end());
    write_varint(&mut out, entry.transitions);
    write_varint(&mut out, entry.markers);
    encode_provenance(&mut out, &entry.provenance);
    out
}

fn decode_observation_metadata(
    source: &str,
    bytes: &[u8],
    known: &[&'static str],
) -> Result<ObservationEntry> {
    let mut at = 0;
    let next = |at: &mut usize| -> Result<u64> {
        let (value, used) = read_varint(source, bytes, *at)?;
        *at += used;
        Ok(value)
    };
    let version = next(&mut at)?;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            source,
            format!(
                "backing states observation metadata version {version}, which this \
                 build has no reading of"
            ),
        ));
    }
    let id = ObservationId(next(&mut at)?);
    let ordinal = next(&mut at)?;
    let span = next(&mut at)?;
    let slice = CaptureRunSlice::new(next(&mut at)?, next(&mut at)?, next(&mut at)?);
    let transitions = next(&mut at)?;
    let markers = next(&mut at)?;
    let (provenance, _) = decode_provenance(source, bytes, at, known)?;
    Ok(ObservationEntry {
        id,
        ordinal,
        span,
        source: slice,
        transitions,
        markers,
        provenance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    fn index_at(position: Tick) -> Marker {
        Marker::new(
            position,
            MarkerKind::Index,
            Provenance::new("kryoflux").note("index OOB record"),
        )
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
        CaptureRun::new(
            source,
            ordinal,
            Provenance::new(source),
            transitions,
            markers,
        )
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
                .admit_observation(&key, &observation)
                .expect("the location was declared");
        }

        let track = capture.track(&key).expect("the location was declared");
        let ordinals: Vec<u64> = track
            .observations()
            .iter()
            .map(ObservationEntry::ordinal)
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
        let source = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 184_534),
        ));
        for (key, value) in [
            ("host_date", "2014.11.01"),
            ("sck", "24027428.5714285"),
            ("ick", "3003428.5714285625"),
            ("host_date", "2014.11.02"),
        ] {
            envelope.record_metadata(MetadataRecord::new("kryoflux", source, key, value));
        }

        let held = envelope.metadata();
        // Every stated fact names the member that stated it: with a
        // hundred and sixty-eight of them saying the same keys, a fact
        // whose speaker went unrecorded is one nobody can go back to.
        assert!(held.iter().all(|record| record.source() == source));
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
            .admit_observation(&key, &observed("a2r", 800, vec![10], Vec::new()).unwrap())
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

    /// The namespaces this layer's own tests can place.
    const KNOWN: &[&str] = &["kryoflux", "c1541", "a2r", "scp"];

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
    fn loading_one_section_reads_only_the_index_path_and_that_section() {
        // The P27 promise, now that the index is in the backing too: a
        // miss reads one bounded record range plus the index path to
        // it — the fixed footer, the root, and the one leaf that can
        // hold the key. Never a leaf that cannot, and never the whole
        // capture.
        let track = TrackKey::new("kryoflux", 36, 0);
        // Two sections per leaf, so six sections make three leaves.
        let mut writer = SectionWriter::with_leaf_capacity(2);
        let mut wanted = None;
        for ordinal in 0..6 {
            let (key, ticks, payload) = transition_section(&track, ordinal);
            if ordinal == 5 {
                wanted = Some((key.clone(), ticks));
            }
            writer.append(key, payload).expect("keys ascend");
        }
        let bytes = writer.finish();
        let total = bytes.len() as u64;
        let (key, expected) = wanted.unwrap();

        let source = CountingSource::new(bytes);
        let payload = read_section(&source, total, &key, KNOWN).expect("the section is indexed");

        assert_eq!(decode_transitions(&payload).unwrap(), expected);

        // Footer, root, one leaf, one section: four bounded reads, and
        // the count does not grow with the number of leaves.
        let reads = source.reads();
        assert_eq!(reads.len(), 4, "{reads:?}");
        assert_eq!(reads[0].1, FOOTER_BYTES as u64);
        let located = locate_section(&source, total, &key, KNOWN)
            .unwrap()
            .expect("the section is indexed");
        assert_eq!(reads[3], (located.offset(), located.length()));
        // Nothing read spanned the whole backing.
        assert!(reads.iter().all(|(_, length)| *length < total), "{reads:?}");
    }

    #[test]
    fn a_section_touched_twice_is_read_once() {
        // P27's hit. The working set is whatever the operation touched,
        // and touching it again costs nothing: the second request is
        // served from what the first decoded, index path included.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let (key, ticks, payload) = transition_section(&track, 0);
        writer.append(key.clone(), payload).expect("keys ascend");
        let bytes = writer.finish();
        let total = bytes.len() as u64;

        let source = CountingSource::new(bytes);
        let mut cache = SectionCache::with_bytes(64 * 1024);

        let first = cache
            .section(&source, total, &key, KNOWN)
            .expect("the section is indexed")
            .to_vec();
        let after_first = source.reads().len();
        let second = cache
            .section(&source, total, &key, KNOWN)
            .expect("the section is indexed")
            .to_vec();

        assert_eq!(decode_transitions(&first).unwrap(), ticks);
        assert_eq!(first, second);
        assert_eq!(
            source.reads().len(),
            after_first,
            "the second request re-read"
        );
    }

    #[test]
    fn a_clean_section_evicts_under_the_bound_and_re_reads_from_the_backing() {
        // The other half of P27: clean state is always evictable,
        // sound because the P7 claim pins the source, so a dropped
        // section re-reads at will. A bound too small for two sections
        // still serves both — it narrows the working set, it never
        // refuses.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let mut keys = Vec::new();
        for ordinal in 0..2 {
            let (key, _, payload) = transition_section(&track, ordinal);
            keys.push(key.clone());
            writer.append(key, payload).expect("keys ascend");
        }
        let bytes = writer.finish();
        let total = bytes.len() as u64;

        let source = CountingSource::new(bytes);
        let one_section = locate_section(&source, total, &keys[0], KNOWN)
            .unwrap()
            .expect("the section is indexed")
            .length();
        let mut cache = SectionCache::with_bytes(one_section);

        cache.section(&source, total, &keys[0], KNOWN).unwrap();
        cache.section(&source, total, &keys[1], KNOWN).unwrap();
        let before_reload = source.reads().len();
        cache.section(&source, total, &keys[0], KNOWN).unwrap();

        // The first was evicted to admit the second, so asking again
        // costs reads: it came back from the backing rather than
        // having been kept.
        assert!(source.reads().len() > before_reload);
        assert!(cache.resident_bytes() <= one_section);
    }

    #[test]
    fn loading_one_section_neither_materializes_nor_invalidates_another() {
        // Sections are addressed individually the whole way down, so
        // work on one is work on one. A backing that pulled in
        // neighbours would turn a bounded read into a cascade; one that
        // invalidated them would make every read cost the last one.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let mut keys = Vec::new();
        for ordinal in 0..3 {
            let (key, _, payload) = transition_section(&track, ordinal);
            keys.push(key.clone());
            writer.append(key, payload).expect("keys ascend");
        }
        let bytes = writer.finish();
        let total = bytes.len() as u64;

        let source = CountingSource::new(bytes);
        let mut cache = SectionCache::with_bytes(64 * 1024);

        cache.section(&source, total, &keys[0], KNOWN).unwrap();
        cache.section(&source, total, &keys[2], KNOWN).unwrap();

        // The section between them was never touched.
        assert!(!cache.holds(&keys[1]));

        // And the first survived the second: it serves without I/O.
        let settled = source.reads().len();
        cache.section(&source, total, &keys[0], KNOWN).unwrap();
        assert_eq!(source.reads().len(), settled);
    }

    #[test]
    fn a_section_key_round_trips_through_the_encoding_the_index_stores() {
        // Every part of the key is addressing, so every part has to
        // survive: a fractional position, an unnumbered head, and each
        // scope and kind. What comes back has to sort where it sorted
        // before, or the index it was written into no longer describes
        // where anything is.
        let known = ["kryoflux", "c1541"];
        let keys = [
            SectionKey::new(
                TrackKey::new("kryoflux", 36, 1),
                ScopeId::Observation(ObservationId(7)),
                SectionKind::TransitionChunk,
                3,
            ),
            SectionKey::new(
                TrackKey::at(
                    "c1541",
                    SourcePosition::fraction("c1541", 37, 2).unwrap(),
                    None,
                ),
                ScopeId::Track,
                SectionKind::TrackMetadata,
                0,
            ),
            SectionKey::new(
                TrackKey::new("c1541", 0, 0),
                ScopeId::Run(CaptureRunId(0)),
                SectionKind::MarkerChunk,
                u64::MAX,
            ),
        ];

        for key in &keys {
            let mut encoded = Vec::new();
            write_section_key(&mut encoded, key);
            let (decoded, used) =
                read_section_key("flux-capture", &encoded, 0, &known).expect("what was written");

            assert_eq!(&decoded, key);
            assert_eq!(used, encoded.len(), "the key states its own extent");
        }
    }

    #[test]
    fn an_index_stating_an_unreduced_position_is_refused() {
        // A written key is always reduced, so 74/4 in an index is
        // corruption. It matters more than it looks: reducing it here
        // would silently collide with the 37/2 already indexed, and
        // one of the two sections would become unreachable while the
        // other answered for it.
        let mut encoded = Vec::new();
        write_varint(&mut encoded, "c1541".len() as u64);
        encoded.extend_from_slice(b"c1541");
        write_varint(&mut encoded, 74);
        write_varint(&mut encoded, 4);
        for value in [0, 0, 0, 0] {
            write_varint(&mut encoded, value);
        }

        let error = read_section_key("flux-capture", &encoded, 0, &["c1541"]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn an_index_naming_a_namespace_the_reader_does_not_know_is_refused() {
        // The backing is private session state, so a namespace the
        // reader cannot place means the index is not the one this
        // layer wrote. Refusing beats resolving it to something
        // plausible and addressing the wrong sections thereafter.
        let key = SectionKey::new(
            TrackKey::new("kryoflux", 36, 0),
            ScopeId::Track,
            SectionKind::TrackMetadata,
            0,
        );
        let mut encoded = Vec::new();
        write_section_key(&mut encoded, &key);

        let error = read_section_key("flux-capture", &encoded, 0, &["scp"]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("kryoflux"), "{error}");
    }

    #[test]
    fn bounding_keeps_each_markers_own_provenance_beside_the_cut() {
        // Two different facts about one observation. The observation
        // records that it was cut from a run; a marker inside it
        // records how that marker came to be known. Bounding rebuilds
        // its markers from parts, so the second is exactly the fact a
        // careless rebuild drops — leaving a source's index record
        // reading as though this layer had inferred it.
        let index_record = || {
            Marker::new(
                0,
                MarkerKind::Index,
                Provenance::new("kryoflux").note("index OOB record"),
            )
        };
        let run = CaptureRun::new(
            "kryoflux",
            0,
            Provenance::new("kryoflux"),
            vec![150, 400],
            vec![
                Marker::new(100, MarkerKind::Index, index_record().provenance().clone()),
                Marker::new(900, MarkerKind::Index, index_record().provenance().clone()),
            ],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();
        let observation = &observations[0];

        assert_eq!(
            observation.markers()[0].provenance().notes,
            ["index OOB record"]
        );
        assert!(
            observation.provenance().notes[0].contains("bounded"),
            "{:?}",
            observation.provenance().notes
        );
    }

    #[test]
    fn a_marker_chunk_keeps_absolute_positions_and_the_recorded_order() {
        // Markers are parallel timed evidence, not transitions. Two may
        // share a position and a later one may sit earlier on the
        // circle, so there is no unsigned gap to delta-code and no
        // order to restore by sorting: positions stay absolute and the
        // sequence is kept exactly as recorded.
        let markers = vec![
            index_at(700),
            Marker::new(700, MarkerKind::WriteSplice, Provenance::new("a2r")),
            index_at(100),
            Marker::new(
                42,
                MarkerKind::SourceMarker {
                    namespace: "kryoflux".into(),
                    code: 0x0b,
                },
                Provenance::new("kryoflux"),
            )
            .with_payload(vec![1, 2, 3]),
            Marker::new(0, MarkerKind::HardSector, Provenance::new("kryoflux")),
        ];

        let encoded = encode_markers(&markers);
        let decoded =
            decode_markers("kryoflux", &encoded, KNOWN).expect("what was encoded decodes");

        assert_eq!(decoded, markers);
    }

    #[test]
    fn a_marker_chunk_states_the_span_it_covers_not_its_ends() {
        // A transition chunk's bounds are its first and last tick,
        // which works only because transitions ascend. Markers do not,
        // so a chunk states the lowest and highest it holds — take the
        // ends instead and a reader skipping by tick range would skip
        // a chunk that covers the tick it wanted.
        let markers = vec![index_at(700), index_at(100), index_at(400)];

        let chunks = split_markers(&markers, 8);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].bounds(), (100, 700));
        assert_eq!(chunks[0].count(), 3);
        assert_eq!(
            decode_markers("kryoflux", chunks[0].payload(), KNOWN).unwrap(),
            markers
        );
    }

    #[test]
    fn markers_split_at_deterministic_boundaries_preserving_order() {
        let markers: Vec<Marker> = (0..5).map(|n| index_at(100 - n * 10)).collect();

        let chunks = split_markers(&markers, 2);

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks.iter().map(MarkerChunk::ordinal).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        let rejoined: Vec<Marker> = chunks
            .iter()
            .flat_map(|chunk| decode_markers("kryoflux", chunk.payload(), KNOWN).unwrap())
            .collect();
        assert_eq!(rejoined, markers);
    }

    #[test]
    fn a_marker_chunk_that_does_not_decode_is_refused() {
        // Same rule as flux: a chunk yields the markers it claims or
        // it yields nothing.
        let intact = encode_markers(&[Marker::new(
            42,
            MarkerKind::SourceMarker {
                namespace: "kryoflux".into(),
                code: 0x0b,
            },
            Provenance::new("kryoflux"),
        )
        .with_payload(vec![1, 2, 3])]);

        // Truncated before the payload it promised.
        assert_eq!(
            decode_markers("kryoflux", &intact[..intact.len() - 2], KNOWN)
                .unwrap_err()
                .category(),
            ErrorCategory::InvalidImage
        );

        // A kind this version has no reading of.
        let mut unknown = Vec::new();
        write_varint(&mut unknown, 10);
        write_varint(&mut unknown, 99);
        assert_eq!(
            decode_markers("kryoflux", &unknown, KNOWN)
                .unwrap_err()
                .category(),
            ErrorCategory::InvalidImage
        );
    }

    #[test]
    fn a_section_whose_bytes_changed_under_the_index_is_refused() {
        // The index states what the record should check to. A backing
        // that answered anyway would hand back flux that never existed.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let (key, _, payload) = transition_section(&track, 0);
        writer.append(key.clone(), payload).expect("keys ascend");
        let mut bytes = writer.finish();
        let total = bytes.len() as u64;

        bytes[2] ^= 0x01;

        let error = read_section(&CountingSource::new(bytes), total, &key, KNOWN).unwrap_err();
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

        writer
            .append(second, second_payload)
            .expect("the first append is in order");
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
            .admit_observation(
                &first,
                &observed("kryoflux", 800, vec![10], Vec::new()).unwrap(),
            )
            .expect("the location was declared");
        let other = capture
            .admit_observation(
                &second,
                &observed("kryoflux", 800, vec![10], Vec::new()).unwrap(),
            )
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
                &observed("kryoflux", 800, vec![10], Vec::new()).unwrap(),
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

        let order: Vec<SourcePosition> = capture
            .tracks()
            .map(|track| track.key().position())
            .collect();

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
        let run = captured("kryoflux", 0, vec![50, 150], vec![index_at(100)]).unwrap();

        assert_eq!(run.transitions(), [50, 150]);
        assert!(run.observations("kryoflux").unwrap().is_empty());
    }

    #[test]
    fn a_run_records_its_transitions_in_recorded_time_order() {
        let error = captured("kryoflux", 0, vec![150, 50], Vec::new()).unwrap_err();

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
        let observation = observed("kryoflux", 800, vec![10, 400], vec![index_at(400)]).unwrap();

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
            Marker::new(700, MarkerKind::WriteSplice, Provenance::new("a2r")),
            index_at(100),
        ];
        let observation = observed("a2r", 800, vec![10], markers).unwrap();

        let positions: Vec<Tick> = observation.markers().iter().map(Marker::position).collect();
        assert_eq!(positions, [700, 700, 100]);
        assert_eq!(observation.markers()[1].kind(), &MarkerKind::WriteSplice);
    }

    #[test]
    fn a_marker_outside_the_span_is_refused() {
        let error = observed("a2r", 800, vec![10], vec![index_at(800)]).unwrap_err();

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
            Provenance::new("kryoflux"),
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
        assert!(
            message.contains("20"),
            "should name the offending tick: {message}"
        );
        assert!(
            message.contains("30"),
            "should name what it followed: {message}"
        );
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
        assert!(
            message.contains("800"),
            "should name the offending tick: {message}"
        );
        assert!(message.contains("span"), "should name the span: {message}");
    }

    /// A capture of two locations, each one transfer of five
    /// transitions bracketed by three index records, built through the
    /// same route an adapter takes.
    fn built() -> (FluxCapture, TrackKey, TrackKey) {
        let front = TrackKey::new("kryoflux", 0, 0);
        let back = TrackKey::new("kryoflux", 0, 1);
        let mut builder = CaptureBuilder::new(kryoflux_timebase(), Vec::new())
            // Two records a chunk, so the evidence below spans several
            // sections and a read has to walk them.
            .with_chunk_records(2);
        let source = builder.envelope_mut().declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 4096),
        ));
        for key in [&front, &back] {
            let run = captured(
                "kryoflux",
                0,
                vec![50, 150, 400, 950, 1400],
                vec![index_at(100), index_at(900), index_at(1300)],
            )
            .unwrap();
            builder
                .add_location(
                    key.clone(),
                    source,
                    std::slice::from_ref(&run),
                    vec![Issue::new("kryoflux-example", "a qualification")],
                )
                .expect("locations arrive in key order");
        }
        let (mut capture, bytes, total) = builder.seal().expect("the backing seals");
        capture.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20, vec!["kryoflux"]);
        (capture, front, back)
    }

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    #[test]
    fn a_built_capture_keeps_only_shape_resident_and_reads_evidence_from_the_backing() {
        let (capture, front, _) = built();
        let track = capture.track(&front).expect("the location was added");
        let run = &track.runs()[0];

        // What stays in memory is a count, not a pulse.
        assert_eq!(run.transitions(), 5);
        assert_eq!(run.markers(), 3);
        assert_eq!(run.indices(), 3);
        assert_eq!(run.extent(), 1400);
        assert_eq!(run.before_first_index(), 1);
        assert_eq!(run.after_last_index(), 1);
        assert_eq!(track.issues()[0].code, "kryoflux-example");

        // And the evidence itself comes back exactly as recorded,
        // including the flux outside the indices.
        assert_eq!(
            capture.run_transitions(&front, run).expect("the run reads"),
            [50, 150, 400, 950, 1400]
        );
        let markers = capture.run_markers(&front, run).expect("the markers read");
        assert_eq!(
            markers.iter().map(Marker::position).collect::<Vec<_>>(),
            [100, 900, 1300]
        );
        assert!(
            markers
                .iter()
                .all(|marker| marker.kind() == &MarkerKind::Index)
        );
    }

    #[test]
    fn an_observation_is_read_back_from_the_run_it_shares_chunks_with() {
        // The backing holds no second copy of an observation's pulses:
        // the slice addresses the run's own chunks, and reading the
        // observation makes the same cut again from the same bytes.
        let (capture, front, _) = built();
        let track = capture.track(&front).expect("the location was added");
        let entry = &track.observations()[0];

        assert_eq!(entry.span(), 800);
        assert_eq!(entry.transitions(), 2);
        assert_eq!((entry.source().start(), entry.source().end()), (100, 900));

        let observation = capture.observation(&front, entry).expect("it reads back");
        assert_eq!(observation.transitions(), [50, 300]);
        assert_eq!(observation.span(), 800);
        assert_eq!(observation.id(), entry.id());
        assert_eq!(observation.ordinal(), entry.ordinal());
        assert_eq!(observation.markers()[0].position(), 0);
    }

    #[test]
    fn one_locations_evidence_loads_without_touching_another() {
        // Two locations, one backing: reading the first leaves the
        // second exactly where it was, which is what a section-keyed
        // backing exists to give.
        let (capture, front, back) = built();
        let front_run = &capture.track(&front).expect("added").runs()[0];
        let back_run = &capture.track(&back).expect("added").runs()[0];

        capture
            .run_transitions(&front, front_run)
            .expect("the first location reads");

        let chunk = |key: &TrackKey, run: &CaptureRunEntry| {
            SectionKey::new(
                key.clone(),
                ScopeId::Run(run.id()),
                SectionKind::TransitionChunk,
                0,
            )
        };
        assert!(capture.holds_section(&chunk(&front, front_run)));
        assert!(!capture.holds_section(&chunk(&back, back_run)));
    }

    #[test]
    fn a_declared_bound_evicts_and_re_reads_rather_than_refusing() {
        // The bound narrows the working set; it never refuses service.
        let front = TrackKey::new("kryoflux", 0, 0);
        let mut builder =
            CaptureBuilder::new(kryoflux_timebase(), Vec::new()).with_chunk_records(1);
        let source = builder.envelope_mut().declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 4096),
        ));
        let run = captured(
            "kryoflux",
            0,
            vec![50, 150, 400, 950, 1400],
            vec![index_at(100), index_at(1300)],
        )
        .unwrap();
        builder
            .add_location(
                front.clone(),
                source,
                std::slice::from_ref(&run),
                Vec::new(),
            )
            .expect("one location");
        let (mut capture, bytes, total) = builder.seal().expect("the backing seals");
        // One byte of working set: every chunk still loads, none stays.
        capture.attach_backing(Box::new(Bytes(bytes)), total, 1, vec!["kryoflux"]);

        let entry = &capture.track(&front).expect("added").runs()[0];
        assert_eq!(
            capture.run_transitions(&front, entry).expect("reads"),
            [50, 150, 400, 950, 1400]
        );
        assert!(
            capture.resident_bytes() <= 8,
            "{}",
            capture.resident_bytes()
        );
    }

    #[test]
    fn the_backings_metadata_sections_read_back_into_what_wrote_them() {
        // The sections are what makes the backing self-describing, so
        // they are written to be read rather than only to be counted.
        let (capture, front, _) = built();
        let track = capture.track(&front).expect("added");
        let run = &track.runs()[0];
        let entry = &track.observations()[0];

        let section = capture
            .section(&SectionKey::new(
                front.clone(),
                ScopeId::Run(run.id()),
                SectionKind::CaptureRunMetadata,
                0,
            ))
            .expect("the run metadata reads");
        assert_eq!(
            &decode_run_metadata("kryoflux", &section, &["kryoflux"]).expect("it decodes"),
            run
        );

        let section = capture
            .section(&SectionKey::new(
                front.clone(),
                ScopeId::Observation(entry.id()),
                SectionKind::ObservationMetadata,
                0,
            ))
            .expect("the observation metadata reads");
        assert_eq!(
            &decode_observation_metadata("kryoflux", &section, &["kryoflux"]).expect("it decodes"),
            entry
        );
    }

    #[test]
    fn locations_arriving_out_of_key_order_are_refused() {
        // The section index is ordered, so a builder handed a location
        // out of order is refused rather than writing a backing whose
        // index cannot address it.
        let mut builder = CaptureBuilder::new(kryoflux_timebase(), Vec::new());
        let source = builder.envelope_mut().declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 4096),
        ));
        let run = captured("kryoflux", 0, vec![50], Vec::new()).unwrap();
        builder
            .add_location(
                TrackKey::new("kryoflux", 1, 0),
                source,
                std::slice::from_ref(&run),
                Vec::new(),
            )
            .expect("the first location");
        let error = builder
            .add_location(
                TrackKey::new("kryoflux", 0, 0),
                source,
                std::slice::from_ref(&run),
                Vec::new(),
            )
            .expect_err("a location behind the last is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }
}
