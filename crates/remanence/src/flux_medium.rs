// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The private flux-medium model (F37): what a drive would read, as
//! distinct from what an instrument recorded.
//!
//! It is the upper of the flux family's two models (P22). A
//! [`crate::flux_capture::FluxCapture`] holds several observations of a
//! source location and no opinion about which one the disk was; a medium
//! holds one circular pulse stream per location the family addresses, in
//! that family's own frame, and asserts exactly what a drive would read.
//! Neither stands in for the other, and the boundary is one sentence:
//! **disagreement across observations is a capture fact, and strength is
//! a medium fact.**
//!
//! What the medium adds is what the flux does not contain — the
//! rotational frame, the family's addressing, the reference clock, the
//! strength vocabulary — and every one of those is declared by a P30
//! drive profile. That is why this is a second model rather than a tidier
//! first one: the medium is where declared family knowledge and recorded
//! evidence combine.
//!
//! **It is never recovered evidence.** Every pulse is
//! selected-and-projected from a capture, synthesized downward from a
//! higher layer (P13, P29), or found already a medium in a container
//! that holds one at rest — and there is no constructor that does not
//! name what put it there. A medium cannot be built by opening a
//! capture, only by reducing one.
//!
//! It holds no bitcell, no recovered clock, no synchronization, no
//! symbol and no byte. Those are the hardware bitstream and above, and a
//! medium that reached them would erase the distinction between what a
//! medium is and what a drive makes of it.
//!
//! The backing is the flux-capture layer's, keyed here by the family's
//! own addressing rather than a source's (P27). Every item is
//! crate-private: the mastering profile builds one of these and the P64
//! adapter reads and writes one, and nothing outside the crate sees a
//! pulse.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::evidence::Provenance;
use crate::flux_capture::{
    ByteSink, ByteSource, CHUNK_RECORDS, LEAF_ENTRIES, SectionAddress, SectionCache,
    SectionLocation, SectionWriter, TimeBase, decode_provenance, encode_provenance,
    greatest_common_divisor, locate_section, read_text, read_varint, write_text, write_varint,
};

/// One cycle of the frame's declared reference clock. An angle, not a
/// duration: a position is a count of cycles from the frame's origin.
pub(crate) type Cycle = u64;

/// How a medium's content came to exist.
///
/// There is no default, and none of these is recovered evidence — which
/// is the one claim this layer never makes. A medium that could not say
/// which of them it is would be asserting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Derivation {
    /// Reduced from recorded evidence under a declared policy (P29).
    SelectedAndProjected,
    /// Synthesized downward from a higher layer (P13).
    Synthetic,
    /// Found already a medium, in a container that holds one at rest. The
    /// content was derived by whoever wrote it and the container records
    /// nothing about how; this library derived none of it and does not
    /// guess on the writer's behalf.
    Stored,
}

impl Derivation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SelectedAndProjected => "selected-and-projected",
            Self::Synthetic => "synthetic",
            Self::Stored => "stored",
        }
    }
}

/// How the reduction located the circle's start.
///
/// A circle has no natural one, so this records the rule that was
/// applied rather than implying a rule that was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginRule {
    /// An index datum from the evidence placed it.
    Index,
    /// A write splice — a seam — placed it.
    Seam,
    /// The policy declared it outright, the evidence having located
    /// nothing.
    Declared,
}

/// Where the circle starts, and what put it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OriginStatement {
    rule: OriginRule,
    evidence: Provenance,
}

impl OriginStatement {
    pub(crate) fn new(rule: OriginRule, evidence: Provenance) -> Self {
        Self { rule, evidence }
    }

    pub(crate) fn rule(&self) -> OriginRule {
        self.rule
    }

    /// What located it — a seam angle, an index datum, a declaration.
    pub(crate) fn evidence(&self) -> &Provenance {
        &self.evidence
    }
}

/// The exact result of projecting a source instant into this frame.
///
/// The whole cycles are the position; the remainder is the part that
/// fell between two cycles, as an exact fraction of one. A reduction
/// declares that remainder as loss (P29) rather than letting the frame
/// round it away, which is why this hands back both rather than a
/// rounded answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Projection {
    cycles: u64,
    remainder: u128,
    per_cycle: u128,
}

impl Projection {
    pub(crate) fn cycles(&self) -> u64 {
        self.cycles
    }

    /// What did not divide, as a fraction of one cycle. A zero
    /// numerator is an exact projection.
    pub(crate) fn remainder(&self) -> (u128, u128) {
        (self.remainder, self.per_cycle)
    }

    pub(crate) fn is_exact(&self) -> bool {
        self.remainder == 0
    }
}

/// The circle a medium's positions are angles in: a declared reference
/// clock, how many of its cycles one rotation takes, and where the
/// circle was given its start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RotationalFrame {
    reference_clock: TimeBase,
    cycles_per_rotation: u64,
    origin: OriginStatement,
}

impl RotationalFrame {
    pub(crate) fn new(
        profile: &str,
        reference_clock: TimeBase,
        cycles_per_rotation: u64,
        origin: OriginStatement,
    ) -> Result<Self> {
        if cycles_per_rotation == 0 {
            return Err(Error::invalid_image(
                profile,
                "frame declares a rotation of zero cycles, which states no circle \
                 to place an angle on",
            ));
        }
        Ok(Self {
            reference_clock,
            cycles_per_rotation,
            origin,
        })
    }

    pub(crate) fn reference_clock(&self) -> TimeBase {
        self.reference_clock
    }

    pub(crate) fn cycles_per_rotation(&self) -> u64 {
        self.cycles_per_rotation
    }

    pub(crate) fn origin(&self) -> &OriginStatement {
        &self.origin
    }

    /// How long one rotation takes, in seconds, exactly.
    ///
    /// A 1541 turns at 300 RPM against a 16 MHz reference, which is one
    /// fifth of a second and 3,200,000 cycles — a ratio no float states
    /// and this one does.
    pub(crate) fn rotation_period(&self) -> (u128, u128) {
        let (numerator, denominator) = self.reference_clock.ticks_per_second();
        reduce_u128(
            u128::from(self.cycles_per_rotation) * u128::from(denominator),
            u128::from(numerator),
        )
    }

    /// Projects a count of `from`'s ticks onto this frame's cycles,
    /// exactly.
    ///
    /// The arithmetic runs against both declared bases and never through
    /// a float or a library-chosen rate: `ticks` of one rate is
    /// `ticks * this_rate / that_rate` cycles, and what does not divide
    /// comes back beside the answer instead of being lost in it.
    pub(crate) fn project(&self, profile: &str, from: TimeBase, ticks: u64) -> Result<Projection> {
        let (clock, per_clock) = self.reference_clock.ticks_per_second();
        let (source, per_source) = from.ticks_per_second();
        let overflow = || {
            Error::invalid_image(
                profile,
                "projecting that instant onto this frame runs past what exact \
                 arithmetic can hold here",
            )
        };
        let numerator = u128::from(ticks)
            .checked_mul(u128::from(clock))
            .and_then(|product| product.checked_mul(u128::from(per_source)))
            .ok_or_else(overflow)?;
        let denominator = u128::from(per_clock)
            .checked_mul(u128::from(source))
            .ok_or_else(overflow)?;
        let cycles = u64::try_from(numerator / denominator).map_err(|_| overflow())?;
        Ok(Projection {
            cycles,
            remainder: numerator % denominator,
            per_cycle: denominator,
        })
    }
}

/// The family's own addressing for one location, as the P30 profile
/// declares it.
///
/// It is not CHS and is never renumbered into it: a 1541 half-track is a
/// real address, so the position is an exact reduced rational rather
/// than a rounding toward a whole track. `surface` is which recorded
/// surface the family addresses; a family that numbers none has none,
/// which is a different fact from surface zero.
///
/// It is deliberately not the capture side's track key. That one is the
/// location a *source* named; this is the location a *profile* declares,
/// and the two are the same only by coincidence of a particular
/// reduction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocationKey {
    profile: &'static str,
    numerator: u64,
    denominator: u64,
    surface: Option<u64>,
}

impl LocationKey {
    /// A whole step of the family's addressing.
    pub(crate) fn new(profile: &'static str, position: u64, surface: u64) -> Self {
        Self {
            profile,
            numerator: position,
            denominator: 1,
            surface: Some(surface),
        }
    }

    /// A fractional step — a half-track — in the family's own terms.
    pub(crate) fn fraction(
        profile: &'static str,
        numerator: u64,
        denominator: u64,
        surface: Option<u64>,
    ) -> Result<Self> {
        if denominator == 0 {
            return Err(Error::invalid_image(
                profile,
                format!(
                    "profile addresses a location as {numerator}/{denominator} of a \
                     step, which states no position"
                ),
            ));
        }
        let divisor = greatest_common_divisor(numerator, denominator);
        Ok(Self {
            profile,
            numerator: numerator / divisor,
            denominator: denominator / divisor,
            surface,
        })
    }

    pub(crate) fn profile(&self) -> &'static str {
        self.profile
    }

    /// The position, as an exact reduced ratio of the family's steps.
    pub(crate) fn position(&self) -> (u64, u64) {
        (self.numerator, self.denominator)
    }

    pub(crate) fn surface(&self) -> Option<u64> {
        self.surface
    }
}

impl Ord for LocationKey {
    /// Compared by exact cross-multiplication. Comparing the reduced
    /// parts field by field would order 5/2 after 3/1.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.profile
            .cmp(other.profile)
            .then_with(|| {
                (u128::from(self.numerator) * u128::from(other.denominator))
                    .cmp(&(u128::from(other.numerator) * u128::from(self.denominator)))
            })
            .then_with(|| self.surface.cmp(&other.surface))
    }
}

impl PartialOrd for LocationKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// How reliably one pulse reads.
///
/// The vocabulary is the profile's, not any container's: the medium does
/// not pre-flatten itself into P64's spelling, or WOZ's, or MFI's. A
/// destination adapter states what it can carry and refuses what it
/// cannot (P12, P29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Strength {
    state: u32,
    seed_domain: Option<u64>,
}

impl Strength {
    /// A pulse that reads the same every time.
    pub(crate) fn certain(state: u32) -> Self {
        Self {
            state,
            seed_domain: None,
        }
    }

    /// A pulse whose reading varies, with the domain that makes the
    /// variation reproducible (P29). Without one, a stochastic reading
    /// could not be repeated and the medium would not be able to say so.
    pub(crate) fn seeded(state: u32, seed_domain: u64) -> Self {
        Self {
            state,
            seed_domain: Some(seed_domain),
        }
    }

    /// The profile's own code for this state.
    pub(crate) fn state(&self) -> u32 {
        self.state
    }

    pub(crate) fn seed_domain(&self) -> Option<u64> {
        self.seed_domain
    }
}

/// One pulse: an angle on the circle, and how reliably it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pulse {
    position: Cycle,
    strength: Strength,
}

impl Pulse {
    pub(crate) fn new(position: Cycle, strength: Strength) -> Self {
        Self { position, strength }
    }

    pub(crate) fn position(&self) -> Cycle {
        self.position
    }

    pub(crate) fn strength(&self) -> Strength {
        self.strength
    }
}

/// What one medium-level fact says. These are the facts about a location
/// that are not about any one pulse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MediumFactKind {
    WriteProtect,
    Unformatted,
    /// Where the track's write splice sits, as an angle — recorded when
    /// the reduction located one, and absent when it did not.
    Seam { angle: Cycle },
    /// This location's content matched an adjacent one. Carried as a
    /// stated fact rather than resolved: the evidence a capture supplies
    /// cannot disambiguate a real duplicate from a stepper that did not
    /// move, and guessing would put an unevidenced claim in the medium.
    Duplicate { of: LocationKey },
    /// Any other fact, under the namespace that owns its meaning.
    SourceFact { namespace: String, code: u32 },
}

/// One fact about a location, with how it came to be known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediumFact {
    kind: MediumFactKind,
    payload: Vec<u8>,
    provenance: Provenance,
}

impl MediumFact {
    pub(crate) fn new(kind: MediumFactKind, provenance: Provenance) -> Self {
        Self {
            kind,
            payload: Vec::new(),
            provenance,
        }
    }

    pub(crate) fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        self
    }

    pub(crate) fn kind(&self) -> &MediumFactKind {
        &self.kind
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What one section of a medium's backing holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MediumSectionKind {
    LocationMetadata,
    PulseChunk,
    FactChunk,
}

/// The complete address of one section of a medium's backing.
///
/// Three parts where a capture's has four: a medium's sections belong to
/// the location itself, there being nothing nested inside a location to
/// scope them to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct MediumSectionKey {
    location: LocationKey,
    kind: MediumSectionKind,
    ordinal: u64,
}

impl MediumSectionKey {
    pub(crate) fn new(location: LocationKey, kind: MediumSectionKind, ordinal: u64) -> Self {
        Self {
            location,
            kind,
            ordinal,
        }
    }

    pub(crate) fn location(&self) -> &LocationKey {
        &self.location
    }

    pub(crate) fn kind(&self) -> MediumSectionKind {
        self.kind
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }
}

impl SectionAddress for MediumSectionKey {
    fn namespace(&self) -> &'static str {
        self.location.profile
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_location_key(out, &self.location);
        write_varint(
            out,
            match self.kind {
                MediumSectionKind::LocationMetadata => 0,
                MediumSectionKind::PulseChunk => 1,
                MediumSectionKind::FactChunk => 2,
            },
        );
        write_varint(out, self.ordinal);
    }

    fn read(
        source: &str,
        bytes: &[u8],
        at: usize,
        known: &[&'static str],
    ) -> Result<(Self, usize)> {
        let mut cursor = at;
        let (location, used) = read_location_key(source, bytes, cursor, known)?;
        cursor += used;
        let (kind, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        let kind = match kind {
            0 => MediumSectionKind::LocationMetadata,
            1 => MediumSectionKind::PulseChunk,
            2 => MediumSectionKind::FactChunk,
            other => {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "index states a section kind {other}, which this version has \
                         no reading of"
                    ),
                ));
            }
        };
        let (ordinal, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        Ok((
            Self {
                location,
                kind,
                ordinal,
            },
            cursor - at,
        ))
    }
}

/// Writes one location key into a section address.
///
/// It is shared with the layers above the medium: they address by the
/// same family locations, and two spellings of one key would put two
/// answers about where a location's sections sit into one crate.
pub(crate) fn write_location_key(out: &mut Vec<u8>, key: &LocationKey) {
    write_text(out, key.profile);
    write_varint(out, key.numerator);
    write_varint(out, key.denominator);
    // Zero is no surface at all, which is a different fact from
    // surface zero.
    match key.surface {
        None => write_varint(out, 0),
        Some(surface) => write_varint(out, surface.saturating_add(1)),
    }
}

pub(crate) fn read_location_key(
    source: &str,
    bytes: &[u8],
    at: usize,
    known: &[&'static str],
) -> Result<(LocationKey, usize)> {
    let mut cursor = at;
    let (spelling, used) = read_text(source, bytes, cursor)?;
    cursor += used;
    let profile = known
        .iter()
        .find(|candidate| **candidate == spelling)
        .copied()
        .ok_or_else(|| {
            Error::invalid_image(
                source,
                format!(
                    "backing names the profile {spelling:?}, which this reader cannot \
                     place, so the backing is not the one this layer wrote"
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
            "backing states a location over zero steps, which is no position",
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
                "backing states the location {numerator}/{denominator}, which is not \
                 in the reduced form every written key carries"
            ),
        ));
    }
    let (surface, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    Ok((
        LocationKey {
            profile,
            numerator,
            denominator,
            surface: surface.checked_sub(1),
        },
        cursor - at,
    ))
}

/// What a medium keeps resident for one location: its identity, its
/// shape, and how it came to be known — never its pulses.
///
/// The two absences differ, and the map is sparse so that they stay
/// different: a missing key means the medium claims nothing at that
/// location, while a location present with no pulses is one the
/// reduction claims is recorded and blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Location {
    key: LocationKey,
    pulses: u64,
    facts: u64,
    pulse_chunks: u64,
    fact_chunks: u64,
    provenance: Provenance,
}

impl Location {
    pub(crate) fn key(&self) -> &LocationKey {
        &self.key
    }

    pub(crate) fn pulses(&self) -> u64 {
        self.pulses
    }

    pub(crate) fn facts(&self) -> u64 {
        self.facts
    }

    /// How this location's content came to be known — the reduction
    /// that produced it, never an observation of a recording.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Where a medium's sections are read back from, and the bounded working
/// set they are served through (P27).
struct MediumBacking {
    bytes: Box<dyn ByteSource + Send + Sync>,
    total_bytes: u64,
    cache: Mutex<SectionCache<MediumSectionKey>>,
    known: Vec<&'static str>,
}

impl std::fmt::Debug for MediumBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediumBacking")
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

/// One circular pulse stream per location the family addresses, in that
/// family's own frame: what a drive would read.
#[derive(Debug)]
pub(crate) struct FluxMedium {
    profile: &'static str,
    frame: RotationalFrame,
    derivation: Derivation,
    locations: BTreeMap<LocationKey, Location>,
    provenance: Provenance,
    backing: Option<MediumBacking>,
}

impl FluxMedium {
    /// Which drive profile's declarations set this medium's frame,
    /// addressing and strength vocabulary.
    pub(crate) fn profile(&self) -> &'static str {
        self.profile
    }

    pub(crate) fn frame(&self) -> &RotationalFrame {
        &self.frame
    }

    /// Whether this medium was reduced from evidence or synthesized
    /// downward. There is no third answer, and nothing here is
    /// recovered evidence.
    pub(crate) fn derivation(&self) -> Derivation {
        self.derivation
    }

    /// The policy that produced this medium. A medium cannot exist
    /// without one.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn location(&self, key: &LocationKey) -> Option<&Location> {
        self.locations.get(key)
    }

    /// Every location the medium claims, in the family's own addressing
    /// order.
    pub(crate) fn locations(&self) -> impl Iterator<Item = &Location> {
        self.locations.values()
    }

    /// Attaches the backing this medium's sections were written into,
    /// with the working set they are served through.
    ///
    /// Under P27 a medium is normally session-backed — produced by a
    /// reduction into private session storage — while a source that can
    /// truthfully locate one location by key may be source-backed
    /// instead. That is a residence question and changes nothing about
    /// what the layer means.
    pub(crate) fn attach_backing(
        &mut self,
        bytes: Box<dyn ByteSource + Send + Sync>,
        total_bytes: u64,
        cache_bytes: u64,
    ) {
        let known = vec![self.profile];
        self.backing = Some(MediumBacking {
            bytes,
            total_bytes,
            cache: Mutex::new(SectionCache::with_bytes(cache_bytes)),
            known,
        });
    }

    pub(crate) fn backing_bytes(&self) -> u64 {
        self.backing
            .as_ref()
            .map_or(0, |backing| backing.total_bytes)
    }

    pub(crate) fn resident_bytes(&self) -> u64 {
        self.backing.as_ref().map_or(0, |backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .resident_bytes()
        })
    }

    fn backing(&self) -> Result<&MediumBacking> {
        self.backing.as_ref().ok_or_else(|| {
            Error::invalid_image(
                self.profile,
                "medium has no backing to read its pulses from, so nothing was ever \
                 written for it to serve",
            )
        })
    }

    fn section(&self, key: &MediumSectionKey) -> Result<Vec<u8>> {
        let backing = self.backing()?;
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
    fn holds_section(&self, key: &MediumSectionKey) -> bool {
        self.backing.as_ref().is_some_and(|backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .holds(key)
        })
    }

    /// Where a section sits, without reading it. Used to show that one
    /// location's evidence is addressable on its own.
    fn locate(&self, key: &MediumSectionKey) -> Result<Option<SectionLocation>> {
        let backing = self.backing()?;
        locate_section(
            backing.bytes.as_ref(),
            backing.total_bytes,
            key,
            &backing.known,
        )
    }

    /// One location's pulses, read back a bounded chunk at a time.
    /// Never the whole medium, and never another location.
    pub(crate) fn pulses(&self, location: &Location) -> Result<Vec<Pulse>> {
        let mut pulses = Vec::with_capacity(location.pulses as usize);
        for ordinal in 0..location.pulse_chunks {
            let key = MediumSectionKey::new(
                location.key.clone(),
                MediumSectionKind::PulseChunk,
                ordinal,
            );
            pulses.extend(decode_pulses(self.profile, &self.section(&key)?)?);
        }
        Ok(pulses)
    }

    /// The pulses of one location falling in the half-open angular span
    /// `[from, to)`.
    ///
    /// A drive serves an arc at a time. A chunk's payload opens with
    /// its first pulse's absolute angle, so the walk ends at the first
    /// chunk that opens past the span and the rest of the location is
    /// never decoded — and no other location is touched at all. The one
    /// chunk past the arc is the price of a record-count boundary: what
    /// a chunk covers is not knowable until it is read.
    pub(crate) fn pulses_within(
        &self,
        location: &Location,
        from: Cycle,
        to: Cycle,
    ) -> Result<Vec<Pulse>> {
        let rotation = self.frame.cycles_per_rotation;
        if from >= to || to > rotation {
            return Err(Error::invalid_image(
                self.profile,
                format!(
                    "the span {from}..{to} is not an arc of a circle of {rotation} \
                     cycles; an arc across the origin is two spans, and composing \
                     them is the caller's to state"
                ),
            ));
        }
        let mut pulses = Vec::new();
        for ordinal in 0..location.pulse_chunks {
            let key = MediumSectionKey::new(
                location.key.clone(),
                MediumSectionKind::PulseChunk,
                ordinal,
            );
            let chunk = decode_pulses(self.profile, &self.section(&key)?)?;
            let opens_past_the_span = chunk.first().is_some_and(|pulse| pulse.position >= to);
            pulses.extend(
                chunk
                    .into_iter()
                    .filter(|pulse| pulse.position >= from && pulse.position < to),
            );
            if opens_past_the_span {
                break;
            }
        }
        Ok(pulses)
    }

    /// One location's medium-level facts, in the order the reduction
    /// stated them.
    pub(crate) fn facts(&self, location: &Location) -> Result<Vec<MediumFact>> {
        let known = self.backing()?.known.clone();
        let mut facts = Vec::with_capacity(location.facts as usize);
        for ordinal in 0..location.fact_chunks {
            let key =
                MediumSectionKey::new(location.key.clone(), MediumSectionKind::FactChunk, ordinal);
            facts.extend(decode_facts(self.profile, &self.section(&key)?, &known)?);
        }
        Ok(facts)
    }
}

/// Builds a medium and its backing together: the pulses stream into the
/// sink as each location arrives, and only identity and shape stay
/// resident.
///
/// This is the only way a medium comes into being, and it cannot be
/// started without naming the policy that produced it — which is what
/// makes "a `FluxMedium` cannot be built by opening a capture, only by
/// reducing one" a property of the code rather than a convention.
///
/// Locations must arrive in ascending key order, which is what the
/// section index requires and what makes a half-written backing
/// detectable rather than a medium with holes in it.
#[derive(Debug)]
pub(crate) struct MediumBuilder<S: ByteSink> {
    writer: SectionWriter<MediumSectionKey, S>,
    medium: FluxMedium,
    chunk_records: usize,
}

impl<S: ByteSink> MediumBuilder<S> {
    /// Starts a medium under `policy`, which must state what put its
    /// content there — the reduction that produced it, or the container
    /// it was found at rest in.
    ///
    /// A policy that names nothing is refused: P29's rule is that a
    /// reduction no policy names is a refusal rather than a default, and
    /// a medium built from an unstated one would carry provenance that
    /// says only that something happened.
    pub(crate) fn new(
        profile: &'static str,
        frame: RotationalFrame,
        derivation: Derivation,
        policy: Provenance,
        sink: S,
    ) -> Result<Self> {
        if policy.notes.is_empty() {
            return Err(Error::invalid_image(
                profile,
                "medium states no policy for the reduction that produced it, and a \
                 reduction no policy names is a refusal rather than a default",
            ));
        }
        Ok(Self {
            writer: SectionWriter::to_sink(sink, LEAF_ENTRIES),
            medium: FluxMedium {
                profile,
                frame,
                derivation,
                locations: BTreeMap::new(),
                provenance: policy,
                backing: None,
            },
            chunk_records: CHUNK_RECORDS,
        })
    }

    /// A builder whose chunks hold at most `records` each. The layer's
    /// own tests use a tiny value to force several chunks out of a
    /// handful of pulses.
    pub(crate) fn with_chunk_records(mut self, records: usize) -> Self {
        self.chunk_records = records.max(1);
        self
    }

    /// Adds one location and everything the medium claims at it.
    ///
    /// A location addressed by another profile is refused: the frame,
    /// the addressing and the strength vocabulary are one profile's
    /// declarations, and a location from another would be measured
    /// against a circle that is not its own.
    pub(crate) fn add_location(
        &mut self,
        key: LocationKey,
        pulses: &[Pulse],
        facts: &[MediumFact],
        provenance: Provenance,
    ) -> Result<()> {
        let profile = self.medium.profile;
        if key.profile != profile {
            return Err(Error::invalid_image(
                profile,
                format!(
                    "location is addressed by the profile '{}' where this medium's \
                     frame is declared by '{profile}'",
                    key.profile
                ),
            ));
        }
        let rotation = self.medium.frame.cycles_per_rotation;
        let mut previous: Option<Cycle> = None;
        for (ordinal, pulse) in pulses.iter().enumerate() {
            if pulse.position >= rotation {
                return Err(Error::invalid_image(
                    profile,
                    format!(
                        "pulse {ordinal} at cycle {} is not below the {rotation} \
                         cycles of one rotation, so it is not an angle on this circle",
                        pulse.position
                    ),
                ));
            }
            if let Some(previous) = previous
                && pulse.position <= previous
            {
                return Err(Error::invalid_image(
                    profile,
                    format!(
                        "pulse {ordinal} at cycle {} does not advance past the \
                         preceding cycle {previous}",
                        pulse.position
                    ),
                ));
            }
            previous = Some(pulse.position);
        }
        for fact in facts {
            if let MediumFactKind::Seam { angle } = fact.kind
                && angle >= rotation
            {
                return Err(Error::invalid_image(
                    profile,
                    format!(
                        "seam at cycle {angle} is not below the {rotation} cycles of \
                         one rotation, so it is not an angle on this circle"
                    ),
                ));
            }
        }

        let pulse_chunks: Vec<Vec<u8>> = pulses
            .chunks(self.chunk_records)
            .map(encode_pulses)
            .collect();
        let fact_chunks: Vec<Vec<u8>> = facts.chunks(self.chunk_records).map(encode_facts).collect();
        let location = Location {
            key: key.clone(),
            pulses: pulses.len() as u64,
            facts: facts.len() as u64,
            pulse_chunks: pulse_chunks.len() as u64,
            fact_chunks: fact_chunks.len() as u64,
            provenance,
        };

        self.writer.append(
            MediumSectionKey::new(key.clone(), MediumSectionKind::LocationMetadata, 0),
            encode_location_metadata(&location),
        )?;
        for (ordinal, payload) in pulse_chunks.into_iter().enumerate() {
            self.writer.append(
                MediumSectionKey::new(key.clone(), MediumSectionKind::PulseChunk, ordinal as u64),
                payload,
            )?;
        }
        for (ordinal, payload) in fact_chunks.into_iter().enumerate() {
            self.writer.append(
                MediumSectionKey::new(key.clone(), MediumSectionKind::FactChunk, ordinal as u64),
                payload,
            )?;
        }
        self.medium.locations.insert(key, location);
        Ok(())
    }

    /// Closes the backing and hands back the medium it addresses,
    /// together with the sink and its length.
    pub(crate) fn seal(self) -> Result<(FluxMedium, S, u64)> {
        let (sink, total) = self.writer.seal()?;
        Ok((self.medium, sink, total))
    }
}

/// The version every metadata section this layer writes carries.
const METADATA_VERSION: u64 = 1;

fn encode_location_metadata(location: &Location) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, location.pulses);
    write_varint(&mut out, location.facts);
    write_varint(&mut out, location.pulse_chunks);
    write_varint(&mut out, location.fact_chunks);
    encode_provenance(&mut out, &location.provenance);
    out
}

/// Delta-codes one chunk of pulses.
///
/// The positions ascend, so every gap is unsigned and the first is the
/// angle from the frame's origin. A strength travels with its pulse
/// rather than in a parallel channel: they are one fact.
fn encode_pulses(pulses: &[Pulse]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, pulses.len() as u64);
    let mut previous = 0;
    for (index, pulse) in pulses.iter().enumerate() {
        write_varint(
            &mut out,
            if index == 0 {
                pulse.position
            } else {
                pulse.position - previous
            },
        );
        previous = pulse.position;
        write_varint(&mut out, u64::from(pulse.strength.state));
        // Zero is a pulse that does not vary, which is a different fact
        // from one seeded on domain zero.
        write_varint(
            &mut out,
            pulse
                .strength
                .seed_domain
                .map_or(0, |domain| domain.saturating_add(1)),
        );
    }
    out
}

fn decode_pulses(profile: &str, bytes: &[u8]) -> Result<Vec<Pulse>> {
    let mut at = 0;
    let (version, used) = read_varint(profile, bytes, at)?;
    at += used;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            profile,
            format!("backing states pulse chunk version {version}, which this build has no reading of"),
        ));
    }
    let (count, used) = read_varint(profile, bytes, at)?;
    at += used;
    let mut pulses = Vec::new();
    let mut position: Cycle = 0;
    for index in 0..count {
        let (gap, used) = read_varint(profile, bytes, at)?;
        at += used;
        position = if index == 0 {
            gap
        } else {
            position.checked_add(gap).ok_or_else(|| {
                Error::invalid_image(profile, "pulse chunk accumulates past one rotation")
            })?
        };
        let (state, used) = read_varint(profile, bytes, at)?;
        at += used;
        let state = u32::try_from(state).map_err(|_| {
            Error::invalid_image(profile, "pulse chunk states a strength wider than its vocabulary")
        })?;
        let (seed, used) = read_varint(profile, bytes, at)?;
        at += used;
        pulses.push(Pulse::new(
            position,
            Strength {
                state,
                seed_domain: seed.checked_sub(1),
            },
        ));
    }
    if at != bytes.len() {
        return Err(Error::invalid_image(
            profile,
            format!(
                "pulse chunk decodes {count} pulses out of {at} of its {} bytes, so \
                 it is not the chunk the index describes",
                bytes.len()
            ),
        ));
    }
    Ok(pulses)
}

fn encode_facts(facts: &[MediumFact]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, facts.len() as u64);
    for fact in facts {
        match &fact.kind {
            MediumFactKind::WriteProtect => write_varint(&mut out, 0),
            MediumFactKind::Unformatted => write_varint(&mut out, 1),
            MediumFactKind::Seam { angle } => {
                write_varint(&mut out, 2);
                write_varint(&mut out, *angle);
            }
            MediumFactKind::Duplicate { of } => {
                write_varint(&mut out, 3);
                write_location_key(&mut out, of);
            }
            MediumFactKind::SourceFact { namespace, code } => {
                write_varint(&mut out, 4);
                write_text(&mut out, namespace);
                write_varint(&mut out, u64::from(*code));
            }
        }
        write_varint(&mut out, fact.payload.len() as u64);
        out.extend_from_slice(&fact.payload);
        encode_provenance(&mut out, &fact.provenance);
    }
    out
}

fn decode_facts(profile: &str, bytes: &[u8], known: &[&'static str]) -> Result<Vec<MediumFact>> {
    let mut at = 0;
    let (version, used) = read_varint(profile, bytes, at)?;
    at += used;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            profile,
            format!("backing states fact chunk version {version}, which this build has no reading of"),
        ));
    }
    let (count, used) = read_varint(profile, bytes, at)?;
    at += used;
    let mut facts = Vec::new();
    for _ in 0..count {
        let (tag, used) = read_varint(profile, bytes, at)?;
        at += used;
        let kind = match tag {
            0 => MediumFactKind::WriteProtect,
            1 => MediumFactKind::Unformatted,
            2 => {
                let (angle, used) = read_varint(profile, bytes, at)?;
                at += used;
                MediumFactKind::Seam { angle }
            }
            3 => {
                let (of, used) = read_location_key(profile, bytes, at, known)?;
                at += used;
                MediumFactKind::Duplicate { of }
            }
            4 => {
                let (namespace, used) = read_text(profile, bytes, at)?;
                at += used;
                let (code, used) = read_varint(profile, bytes, at)?;
                at += used;
                let code = u32::try_from(code).map_err(|_| {
                    Error::invalid_image(profile, "fact chunk states a code wider than the format holds")
                })?;
                MediumFactKind::SourceFact { namespace, code }
            }
            other => {
                return Err(Error::invalid_image(
                    profile,
                    format!("fact chunk states a kind {other}, which this version has no reading of"),
                ));
            }
        };
        let (length, used) = read_varint(profile, bytes, at)?;
        at += used;
        let length = usize::try_from(length).map_err(|_| {
            Error::invalid_image(profile, "fact chunk states a payload longer than this host can address")
        })?;
        let payload = bytes
            .get(at..at + length)
            .ok_or_else(|| Error::invalid_image(profile, "fact chunk ends inside a payload"))?
            .to_vec();
        at += length;
        let (provenance, used) = decode_provenance(profile, bytes, at, known)?;
        at += used;
        facts.push(MediumFact::new(kind, provenance).with_payload(payload));
    }
    if at != bytes.len() {
        return Err(Error::invalid_image(
            profile,
            format!(
                "fact chunk decodes {count} facts out of {at} of its {} bytes, so it \
                 is not the chunk the index describes",
                bytes.len()
            ),
        ));
    }
    Ok(facts)
}

/// Reduces an exact ratio, so one rate has one spelling.
fn reduce_u128(numerator: u128, denominator: u128) -> (u128, u128) {
    let (mut a, mut b) = (numerator, denominator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let divisor = a.max(1);
    (numerator / divisor, denominator / divisor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    const C1541: &str = "c1541";

    /// The 1541's reference clock: 16 MHz, exactly.
    fn drive_clock() -> TimeBase {
        TimeBase::new(C1541, 16_000_000, 1).expect("the rate is stated")
    }

    /// 300 RPM against that clock: one rotation is one fifth of a
    /// second, so 3,200,000 cycles.
    const CYCLES_PER_ROTATION: u64 = 3_200_000;

    fn frame() -> RotationalFrame {
        RotationalFrame::new(
            C1541,
            drive_clock(),
            CYCLES_PER_ROTATION,
            OriginStatement::new(
                OriginRule::Index,
                Provenance::new(C1541).note("origin placed at the index datum"),
            ),
        )
        .expect("the frame states a circle")
    }

    fn policy() -> Provenance {
        Provenance::new(C1541).note("selected the second observation of each location")
    }

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    /// A medium of two half-track locations, built the only way one can
    /// be built.
    fn mastered() -> (FluxMedium, LocationKey, LocationKey) {
        let first = LocationKey::new(C1541, 17, 0);
        let second =
            LocationKey::fraction(C1541, 35, 2, Some(0)).expect("a half-track is an address");
        let mut builder = MediumBuilder::new(
            C1541,
            frame(),
            Derivation::SelectedAndProjected,
            policy(),
            Vec::new(),
        )
        .expect("the policy is stated")
        // Two records a chunk, so the pulses below span several
        // sections and a read has to walk them.
        .with_chunk_records(2);

        builder
            .add_location(
                first.clone(),
                &[
                    Pulse::new(0, Strength::certain(3)),
                    Pulse::new(64, Strength::seeded(1, 0)),
                    Pulse::new(128, Strength::seeded(2, 0x0123_4567_89ab_cdef)),
                    Pulse::new(3_199_999, Strength::certain(3)),
                ],
                &[
                    MediumFact::new(
                        MediumFactKind::Seam { angle: 1_600_000 },
                        Provenance::new(C1541).note("write splice located half a turn in"),
                    ),
                    MediumFact::new(
                        MediumFactKind::Duplicate {
                            of: LocationKey::new(C1541, 18, 0),
                        },
                        Provenance::new(C1541).note("matched the next whole step"),
                    )
                    .with_payload(vec![0xaa, 0xbb]),
                ],
                Provenance::new(C1541).note("reduced from observation 1"),
            )
            .expect("the first location");
        // Recorded and blank, which is not the same as absent.
        builder
            .add_location(
                second.clone(),
                &[],
                &[MediumFact::new(
                    MediumFactKind::Unformatted,
                    Provenance::new(C1541).note("no pulses admitted at this half-step"),
                )],
                Provenance::new(C1541).note("reduced from observation 1"),
            )
            .expect("the second location");

        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
        (medium, first, second)
    }

    #[test]
    fn a_medium_cannot_be_built_without_naming_the_policy_that_produced_it() {
        // The strongest clause in the layer, made a property of the
        // code: there is no constructor that does not state a
        // reduction, so nothing can arrive here carrying provenance
        // that says only that something happened.
        let error = MediumBuilder::new(
            C1541,
            frame(),
            Derivation::SelectedAndProjected,
            Provenance::new(C1541),
            Vec::new(),
        )
        .expect_err("an unstated policy is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("no policy names"), "{error}");
    }

    #[test]
    fn every_medium_says_it_is_derived_and_which_way() {
        let (medium, _, _) = mastered();

        assert_eq!(medium.derivation(), Derivation::SelectedAndProjected);
        assert_eq!(
            medium.derivation().as_str(),
            "selected-and-projected"
        );
        assert_eq!(
            medium.provenance().notes,
            ["selected the second observation of each location"]
        );
    }

    #[test]
    fn the_frame_states_its_rotation_exactly_rather_than_near_enough() {
        // 3,200,000 cycles of a 16 MHz clock is one fifth of a second —
        // 300 RPM — and the ratio is exact rather than 0.2 rounded.
        assert_eq!(frame().rotation_period(), (1, 5));

        // A 359.8 RPM instrument, which is what the fixture was
        // captured on, is not a round number of anything and still
        // states itself exactly.
        let odd = RotationalFrame::new(
            C1541,
            TimeBase::new(C1541, 16_000_000, 1).expect("stated"),
            2_668_520,
            OriginStatement::new(OriginRule::Declared, Provenance::new(C1541).note("declared")),
        )
        .expect("the frame states a circle");
        let (numerator, denominator) = odd.rotation_period();
        assert_eq!((numerator, denominator), (66_713, 400_000));
    }

    #[test]
    fn a_projection_hands_back_what_did_not_divide_instead_of_rounding_it() {
        // A KryoFlux tick is 1345536000/56 Hz and a 1541's clock is
        // 16 MHz: the ratio is irrational-looking in decimal and exact
        // as a fraction, so a projection either divides or says by how
        // much it did not.
        let capture = TimeBase::new("kryoflux", 18_432_000 * 73, 56).expect("stated");
        let frame = frame();

        // 1501 capture ticks: 1501 * 16000000 * 56 / (1 * 1345536000).
        let projected = frame
            .project(C1541, capture, 1501)
            .expect("the projection is in range");
        assert_eq!(projected.cycles(), 999);
        assert!(!projected.is_exact());
        let (remainder, per_cycle) = projected.remainder();
        assert!(remainder > 0 && remainder < per_cycle);

        // And an instant that lands on a cycle boundary says so, which
        // is what lets a reduction declare loss only where there is any.
        let same_rate = frame
            .project(C1541, drive_clock(), 4096)
            .expect("the projection is in range");
        assert_eq!(same_rate.cycles(), 4096);
        assert!(same_rate.is_exact());
    }

    #[test]
    fn a_frame_of_no_rotation_is_refused() {
        let error = RotationalFrame::new(
            C1541,
            drive_clock(),
            0,
            OriginStatement::new(OriginRule::Declared, Provenance::new(C1541)),
        )
        .expect_err("a circle of zero cycles places no angle");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn the_origin_records_the_rule_that_placed_it_rather_than_implying_one() {
        // A circle has no natural start. Which rule gave it one is a
        // fact about the reduction, and it travels with the frame.
        let frame = frame();
        assert_eq!(frame.origin().rule(), OriginRule::Index);
        assert_eq!(
            frame.origin().evidence().notes,
            ["origin placed at the index datum"]
        );
    }

    #[test]
    fn a_pulse_outside_the_circle_is_refused_rather_than_wrapped() {
        let mut builder =
            MediumBuilder::new(C1541, frame(), Derivation::Synthetic, policy(), Vec::new())
                .expect("the policy is stated");
        let error = builder
            .add_location(
                LocationKey::new(C1541, 0, 0),
                &[Pulse::new(CYCLES_PER_ROTATION, Strength::certain(3))],
                &[],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect_err("a position at the rotation is off the circle");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(message.contains("3200000"), "{message}");
        assert!(message.contains("angle on this circle"), "{message}");
    }

    #[test]
    fn pulses_that_do_not_advance_are_refused_rather_than_sorted() {
        let mut builder =
            MediumBuilder::new(C1541, frame(), Derivation::Synthetic, policy(), Vec::new())
                .expect("the policy is stated");
        let error = builder
            .add_location(
                LocationKey::new(C1541, 0, 0),
                &[
                    Pulse::new(10, Strength::certain(3)),
                    Pulse::new(10, Strength::certain(3)),
                ],
                &[],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect_err("two pulses at one angle contradict each other");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_location_addressed_by_another_profile_is_refused() {
        // The frame, the addressing and the strength vocabulary are one
        // profile's declarations, so a location from another would be
        // an angle measured on a circle that is not its own.
        let mut builder =
            MediumBuilder::new(C1541, frame(), Derivation::Synthetic, policy(), Vec::new())
                .expect("the policy is stated");
        let error = builder
            .add_location(
                LocationKey::new("apple2", 0, 0),
                &[],
                &[],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect_err("another family's addressing is refused");

        assert!(error.to_string().contains("apple2"), "{error}");
    }

    #[test]
    fn an_unclaimed_location_differs_from_one_claimed_recorded_and_blank() {
        // Two different facts, and the map is sparse so that they stay
        // different: an absent key means the medium claims nothing
        // there, while a location with no pulses is one the reduction
        // claims is recorded and blank.
        let (medium, _, blank) = mastered();
        let unclaimed = LocationKey::new(C1541, 40, 0);

        assert!(medium.location(&unclaimed).is_none());
        let location = medium.location(&blank).expect("the reduction claimed it");
        assert_eq!(location.pulses(), 0);
        assert!(medium.pulses(location).expect("it reads").is_empty());
        assert_eq!(location.facts(), 1);
    }

    #[test]
    fn a_half_track_is_an_address_and_never_a_rounding_of_a_whole_one() {
        let (medium, whole, half) = mastered();

        assert_eq!(half.position(), (35, 2));
        assert_ne!(half.position(), (17, 1));
        assert_ne!(half.position(), (18, 1));
        // And ordering is by value, not by the parts: 35/2 sits after
        // 17 rather than before it.
        assert!(whole < half);
        let addressed: Vec<(u64, u64)> = medium
            .locations()
            .map(|location| location.key().position())
            .collect();
        assert_eq!(addressed, [(17, 1), (35, 2)]);
    }

    #[test]
    fn strength_round_trips_through_the_backing_in_the_profiles_own_terms() {
        let (medium, first, _) = mastered();
        let location = medium.location(&first).expect("the location was added");

        let pulses = medium.pulses(location).expect("the pulses read back");
        assert_eq!(
            pulses.iter().map(Pulse::position).collect::<Vec<_>>(),
            [0, 64, 128, 3_199_999]
        );
        assert_eq!(pulses[0].strength(), Strength::certain(3));
        // A seed domain of zero is a seeded pulse, not an unseeded one:
        // the two are different claims about reproducibility.
        assert_eq!(pulses[1].strength().seed_domain(), Some(0));
        assert_eq!(pulses[2].strength().state(), 2);
        assert_eq!(
            pulses[2].strength().seed_domain(),
            Some(0x0123_4567_89ab_cdef)
        );
        assert_eq!(pulses[3].strength().seed_domain(), None);
    }

    #[test]
    fn a_seam_is_an_angle_and_a_duplicate_is_a_stated_fact() {
        let (medium, first, _) = mastered();
        let location = medium.location(&first).expect("the location was added");

        let facts = medium.facts(location).expect("the facts read back");
        assert_eq!(facts.len(), 2);
        assert_eq!(
            facts[0].kind(),
            &MediumFactKind::Seam { angle: 1_600_000 }
        );
        assert_eq!(
            facts[0].provenance().notes,
            ["write splice located half a turn in"]
        );
        // The duplicate names what it matched and resolves nothing: a
        // capture cannot tell a real duplicate from a stepper that did
        // not move, and the medium does not guess on its behalf.
        assert_eq!(
            facts[1].kind(),
            &MediumFactKind::Duplicate {
                of: LocationKey::new(C1541, 18, 0)
            }
        );
        assert_eq!(facts[1].payload(), [0xaa, 0xbb]);
    }

    #[test]
    fn a_seam_outside_the_circle_is_refused_like_a_pulse() {
        let mut builder =
            MediumBuilder::new(C1541, frame(), Derivation::Synthetic, policy(), Vec::new())
                .expect("the policy is stated");
        let error = builder
            .add_location(
                LocationKey::new(C1541, 0, 0),
                &[],
                &[MediumFact::new(
                    MediumFactKind::Seam {
                        angle: CYCLES_PER_ROTATION + 1,
                    },
                    Provenance::new(C1541).note("mislocated"),
                )],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect_err("a seam off the circle is refused");

        assert!(error.to_string().contains("seam at cycle"), "{error}");
    }

    #[test]
    fn one_locations_pulses_load_without_touching_another() {
        let (medium, first, second) = mastered();
        let location = medium.location(&first).expect("added");

        medium.pulses(location).expect("the first location reads");

        let chunk = |key: &LocationKey| {
            MediumSectionKey::new(key.clone(), MediumSectionKind::PulseChunk, 0)
        };
        assert!(medium.holds_section(&chunk(&first)));
        // The blank location has no pulse chunk at all, so the index
        // answers about it without a section existing to touch.
        assert!(!medium.holds_section(&chunk(&second)));
        assert_eq!(medium.locate(&chunk(&second)).expect("the index answers"), None);
    }

    #[test]
    fn one_angular_span_loads_without_decoding_the_rest_of_the_location() {
        // A drive serves an arc at a time. Six pulses, two to a chunk,
        // so asking for the opening arc walks the chunk it covers and
        // the one that ends it, and never reaches the third.
        let first = LocationKey::new(C1541, 0, 0);
        let mut builder =
            MediumBuilder::new(C1541, frame(), Derivation::Synthetic, policy(), Vec::new())
                .expect("the policy is stated")
                .with_chunk_records(2);
        builder
            .add_location(
                first.clone(),
                &[0, 10, 20, 30, 40, 50]
                    .map(|position| Pulse::new(position, Strength::certain(3))),
                &[],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect("one location");
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);

        let location = medium.location(&first).expect("added");
        let arc = medium.pulses_within(location, 0, 15).expect("the arc reads");
        assert_eq!(arc.iter().map(Pulse::position).collect::<Vec<_>>(), [0, 10]);

        let chunk = |ordinal| {
            MediumSectionKey::new(first.clone(), MediumSectionKind::PulseChunk, ordinal)
        };
        assert!(medium.holds_section(&chunk(0)));
        // The second chunk is what ended the walk, and the third was
        // never touched: the rest of the location stays undecoded.
        assert!(medium.holds_section(&chunk(1)));
        assert!(
            !medium.holds_section(&chunk(2)),
            "the arc walked past the chunk that ended it"
        );
    }

    #[test]
    fn a_span_that_is_not_an_arc_of_the_circle_is_refused() {
        // An arc across the origin is two spans. Serving it as one
        // would quietly decide which way round the circle the caller
        // meant, and that is the caller's to state.
        let (medium, first, _) = mastered();
        let location = medium.location(&first).expect("added");

        let error = medium
            .pulses_within(location, 100, 100)
            .expect_err("an empty span is not an arc");
        assert!(error.to_string().contains("is not an arc"), "{error}");

        let error = medium
            .pulses_within(location, 0, CYCLES_PER_ROTATION + 1)
            .expect_err("a span past the circle is not an arc of it");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_declared_bound_evicts_and_re_reads_rather_than_refusing() {
        let first = LocationKey::new(C1541, 0, 0);
        let mut builder =
            MediumBuilder::new(C1541, frame(), Derivation::Synthetic, policy(), Vec::new())
                .expect("the policy is stated")
                .with_chunk_records(1);
        builder
            .add_location(
                first.clone(),
                &[
                    Pulse::new(1, Strength::certain(3)),
                    Pulse::new(2, Strength::certain(3)),
                    Pulse::new(3, Strength::certain(1)),
                ],
                &[],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect("one location");
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        // One byte of working set: every chunk still loads, none stays.
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1);

        let location = medium.location(&first).expect("added");
        let pulses = medium.pulses(location).expect("the pulses read");
        assert_eq!(
            pulses.iter().map(Pulse::position).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(medium.resident_bytes() <= 8, "{}", medium.resident_bytes());
        assert!(medium.backing_bytes() > 0);
    }

    #[test]
    fn locations_arriving_out_of_addressing_order_are_refused() {
        let mut builder =
            MediumBuilder::new(C1541, frame(), Derivation::Synthetic, policy(), Vec::new())
                .expect("the policy is stated");
        builder
            .add_location(
                LocationKey::new(C1541, 1, 0),
                &[],
                &[],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect("the first location");
        let error = builder
            .add_location(
                LocationKey::new(C1541, 0, 0),
                &[],
                &[],
                Provenance::new(C1541).note("synthesized"),
            )
            .expect_err("a location behind the last is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_backing_naming_a_profile_the_reader_cannot_place_is_refused() {
        // The backing is private session state, so a profile outside
        // the set this reader knows means the index is not the one this
        // layer wrote — and resolving it to something plausible would
        // address the wrong locations from then on.
        let (medium, first, _) = mastered();
        let key = MediumSectionKey::new(first, MediumSectionKind::PulseChunk, 0);
        // The same backing, read by a reader that places another name.
        let bytes = {
            let backing = medium.backing().expect("attached");
            let mut all = vec![0u8; backing.total_bytes as usize];
            backing.bytes.read_at(0, &mut all).expect("reads");
            all
        };

        let error = locate_section(&Bytes(bytes), medium.backing_bytes(), &key, &["apple2"])
            .expect_err("the index names a profile this reader cannot place");

        assert!(error.to_string().contains("c1541"), "{error}");
        assert!(error.to_string().contains("cannot place"), "{error}");
    }
}
