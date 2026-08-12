// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! What a capture is made of: exact time, the events a source
//! recorded, and the place it recorded them.
//!
//! Nothing here interprets. A [`TimeBase`] is an exact rational and is
//! never reduced to a library-chosen sample rate; a [`Marker`] keeps
//! the position and order the source gave it; a [`ForeignRecord`] is
//! kept whole precisely because this layer cannot name it. [`TrackKey`]
//! and [`SourcePosition`] are the address side of the same refusal —
//! the adapter names a location in its source's own terms, including
//! fractional steps and the absent head that is not head zero, and the
//! key carries that name unrounded.

use crate::error::{Error, Result};
use crate::evidence::Provenance;

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
    // The chunk codec reads these directly; nothing outside `capture` does.
    pub(super) position: Tick,
    pub(super) kind: MarkerKind,
    pub(super) payload: Vec<u8>,
    pub(super) provenance: Provenance,
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
pub(crate) struct SourceId(pub(super) u64);

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
pub(crate) struct ObservationId(pub(super) u64);

/// A source's own location number, held exactly.
///
/// Sources step in fractions — a 1541 half-track is a real address, not
/// a rounding of a whole one — so a position is an exact rational and
/// never a float. It is reduced on construction, which is exact, so
/// that one location has one key however the source spelled it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourcePosition {
    pub(super) numerator: u64,
    pub(super) denominator: u64,
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
    // The backing reads these three directly to write and read a section
    // key; nothing outside `capture` does.
    pub(super) namespace: &'static str,
    pub(super) position: SourcePosition,
    pub(super) head: Option<u64>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

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
}
