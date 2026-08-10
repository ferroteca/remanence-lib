// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The remanence image (F63): the flux family's physical stratum — what
//! a disk's surfaces actually hold, distinct from any capture of them
//! and beneath the served medium a drive reads.
//!
//! The model is a form factor, the index holes as angular data, and per
//! surface the **orbits** — one recorded band at one radius. "Orbit",
//! not "track": both the flux community and the recording formats use
//! "track" and mean different radii by it, and an unrelated word needs
//! no guard. An orbit is located by its center radius in whole microns
//! — a fact about the disk, never the step index of whichever
//! instrument found it — and holds an ordered circular array of points.
//!
//! **Every point states the state in force from it, and nothing else
//! appears.** An orbit is a closed loop: the state at any angle is
//! whatever the nearest preceding point declared, wrapping past the
//! origin. A reversal is an implication, sitting wherever consecutive
//! points differ. Adjacent coherent points must therefore alternate —
//! a repeated sense asserts nothing — except where a point states new
//! write widths, which is a rewrite splice and asserts a change of
//! geometry even on the same sense. The wrap is deliberately not
//! checked: real captures deliver odd transition counts, and that is a
//! fact about the evidence, not a reason to refuse the artifact.
//!
//! Angles are fixed point over [`ANGULAR_DIVISIONS`] of one turn — a
//! unit, not a measurement, so equality is exact and results are
//! bit-deterministic. Each point packs into one `u32`, angle in the
//! high 28 bits and flags low, so wraparound arithmetic on the word is
//! wraparound arithmetic on the angle. Amplitude is not modelled — a
//! domain invariant — and confidence is not a field: uncertainty is an
//! output of reconstruction, reported beside the artifact, never
//! inside it. Genuine indeterminacy is an `Unaligned` span.
//!
//! Each orbit's points are a cache-backed array of transition data:
//! packed words chunked into the family's section-addressable backing
//! (P27), LRU-resident under a declared bound, exactly as the capture
//! and medium layers stream their own records.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::evidence::Provenance;
use crate::flux_capture::{
    ByteSink, ByteSource, CHUNK_RECORDS, LEAF_ENTRIES, SectionAddress, SectionCache,
    SectionWriter, greatest_common_divisor, read_varint, write_varint,
};

/// The refusal namespace and format identity of this stratum.
pub(crate) const REMANENCE: &str = "remanence";

/// One revolution, in the disk-level angular unit every orbit shares.
///
/// 2²⁸ divisions per turn: ~670× finer than the tightest write quantum
/// in the device class and ~30× finer than an instrument sample tick.
/// A `double` in turns was weighed in the model's research lineage and
/// declined — it wastes exponent bits and breaks exact equality.
pub(crate) const ANGULAR_DIVISIONS: u64 = 1 << 28;

/// Microns per mil, exactly: the one place imperial datasheet facts
/// become this layer's metric ones.
pub(crate) const MICRONS_PER_MIL: (u64, u64) = (127, 5);

/// The passive shape of the medium the image describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaFormFactor {
    Inch8,
    Inch525,
    Inch35,
}

impl MediaFormFactor {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            MediaFormFactor::Inch8 => "8-inch",
            MediaFormFactor::Inch525 => "5.25-inch",
            MediaFormFactor::Inch35 => "3.5-inch",
        }
    }
}

/// The four states a stretch of surface can hold.
///
/// The split inside the low-amplitude pair is **writability**, not
/// amplitude: `Unaligned` is fillable by a later write, `Inert` cannot
/// hold a state at all — damage, a burn, a hole's shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Magnetization {
    Positive,
    Negative,
    Unaligned,
    Inert,
}

impl Magnetization {
    /// Whether this state carries a sense a reversal can be drawn from.
    pub(crate) fn is_coherent(&self) -> bool {
        matches!(self, Magnetization::Positive | Magnetization::Negative)
    }

    /// The opposite sense, for the two states that have one.
    pub(crate) fn opposite(&self) -> Option<Magnetization> {
        match self {
            Magnetization::Positive => Some(Magnetization::Negative),
            Magnetization::Negative => Some(Magnetization::Positive),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Magnetization::Positive => "positive",
            Magnetization::Negative => "negative",
            Magnetization::Unaligned => "unaligned",
            Magnetization::Inert => "inert",
        }
    }

    /// The stable two-bit code this state packs and serializes as.
    /// Explicit, never a language ordinal.
    pub(crate) fn code(&self) -> u32 {
        match self {
            Magnetization::Positive => 0,
            Magnetization::Negative => 1,
            Magnetization::Unaligned => 2,
            Magnetization::Inert => 3,
        }
    }

    pub(crate) fn from_code(code: u32) -> Option<Magnetization> {
        match code {
            0 => Some(Magnetization::Positive),
            1 => Some(Magnetization::Negative),
            2 => Some(Magnetization::Unaligned),
            3 => Some(Magnetization::Inert),
            _ => None,
        }
    }
}

/// A write's geometry: two full widths symmetric about the orbit's
/// center line, in whole microns. The plateau is where the write is at
/// full strength; the guard is where its influence ends. Two
/// boundaries, deliberately no shape — the falloff curve is a serving
/// concern, not a stored fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WriteWidths {
    plateau_microns: u64,
    guard_microns: u64,
}

impl WriteWidths {
    pub(crate) fn new(plateau_microns: u64, guard_microns: u64) -> Result<Self> {
        if plateau_microns == 0 {
            return Err(Error::invalid_image(
                REMANENCE,
                "a write plateau must be a positive width",
            ));
        }
        // Guard >= plateau: influence cannot end inside the region that
        // is at full strength. Equal is legal and is the cliff.
        if guard_microns < plateau_microns {
            return Err(Error::invalid_image(
                REMANENCE,
                format!(
                    "a guard of {guard_microns} microns cannot be narrower than its \
                     plateau of {plateau_microns}"
                ),
            ));
        }
        Ok(Self {
            plateau_microns,
            guard_microns,
        })
    }

    pub(crate) fn plateau_microns(&self) -> u64 {
        self.plateau_microns
    }

    pub(crate) fn guard_microns(&self) -> u64 {
        self.guard_microns
    }
}

/// An exact fraction of one revolution, held reduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TurnFraction {
    numerator: u64,
    denominator: u64,
}

impl TurnFraction {
    pub(crate) fn new(numerator: u64, denominator: u64) -> Result<Self> {
        if denominator == 0 {
            return Err(Error::invalid_image(
                REMANENCE,
                "a fraction of a turn cannot be stated over zero",
            ));
        }
        let divisor = greatest_common_divisor(numerator.max(1), denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    pub(crate) fn numerator(&self) -> u64 {
        self.numerator
    }

    pub(crate) fn denominator(&self) -> u64 {
        self.denominator
    }

    fn compare(&self, other: &TurnFraction) -> std::cmp::Ordering {
        (u128::from(self.numerator) * u128::from(other.denominator))
            .cmp(&(u128::from(other.numerator) * u128::from(self.denominator)))
    }

    fn is_at_least_one_turn(&self) -> bool {
        self.numerator >= self.denominator
    }

    fn is_more_than_one_turn(&self) -> bool {
        self.numerator > self.denominator
    }
}

/// An index hole: an angular datum and nothing else.
///
/// Nothing radial is stored, deliberately: what serves as an angular
/// reference is the hole's angle and extent. Damage that once shared
/// this type is `Inert` spans on the orbits it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hole {
    center_angle: TurnFraction,
    angular_extent: TurnFraction,
}

impl Hole {
    pub(crate) fn new(center_angle: TurnFraction, angular_extent: TurnFraction) -> Result<Self> {
        if center_angle.is_at_least_one_turn() {
            return Err(Error::invalid_image(
                REMANENCE,
                "a hole's centre angle must fall inside one revolution",
            ));
        }
        if angular_extent.numerator == 0 || angular_extent.is_more_than_one_turn() {
            return Err(Error::invalid_image(
                REMANENCE,
                "a hole's angular extent must fall in (0, 1] of one revolution",
            ));
        }
        Ok(Self {
            center_angle,
            angular_extent,
        })
    }

    pub(crate) fn center_angle(&self) -> TurnFraction {
        self.center_angle
    }

    pub(crate) fn angular_extent(&self) -> TurnFraction {
        self.angular_extent
    }
}

/// One point of an orbit: the angle it sits at, the state in force
/// from it, and the write geometry it states, if any.
///
/// Widths ride only on coherent points: a point stating that no
/// coherent write is in evidence cannot also state one's geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OrbitPoint {
    angle: u64,
    magnetization: Magnetization,
    widths: Option<WriteWidths>,
}

impl OrbitPoint {
    pub(crate) fn new(angle: u64, magnetization: Magnetization) -> Result<Self> {
        Self::stating(angle, magnetization, None)
    }

    pub(crate) fn stating(
        angle: u64,
        magnetization: Magnetization,
        widths: Option<WriteWidths>,
    ) -> Result<Self> {
        if angle >= ANGULAR_DIVISIONS {
            return Err(Error::invalid_image(
                REMANENCE,
                format!(
                    "angle {angle} is not inside a revolution of {ANGULAR_DIVISIONS} \
                     divisions"
                ),
            ));
        }
        if widths.is_some() && !magnetization.is_coherent() {
            return Err(Error::invalid_image(
                REMANENCE,
                format!(
                    "write widths belong on coherent points, not {}",
                    magnetization.as_str()
                ),
            ));
        }
        Ok(Self {
            angle,
            magnetization,
            widths,
        })
    }

    pub(crate) fn angle(&self) -> u64 {
        self.angle
    }

    pub(crate) fn magnetization(&self) -> Magnetization {
        self.magnetization
    }

    pub(crate) fn widths(&self) -> Option<WriteWidths> {
        self.widths
    }

    pub(crate) fn states_widths(&self) -> bool {
        self.widths.is_some()
    }

    /// The packed word: angle in the high 28 bits, the states-widths
    /// flag on bit 2, the magnetization code on bits 0–1. Bit 3 is
    /// reserved and zero. Angle-high is the load-bearing choice: `u32`
    /// wraparound is angle wraparound.
    fn packed(&self) -> u32 {
        let flags = if self.states_widths() { 0b100 } else { 0 } | self.magnetization.code();
        ((self.angle as u32) << 4) | flags
    }
}

/// Where one orbit sits: its surface, and its center radius. One
/// radius, one recording — the key is the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OrbitKey {
    surface: u64,
    radius_microns: u64,
}

impl OrbitKey {
    pub(crate) fn new(surface: u64, radius_microns: u64) -> Result<Self> {
        if radius_microns == 0 {
            return Err(Error::invalid_image(
                REMANENCE,
                "an orbit sits at a positive radius from centre",
            ));
        }
        Ok(Self {
            surface,
            radius_microns,
        })
    }

    pub(crate) fn surface(&self) -> u64 {
        self.surface
    }

    pub(crate) fn radius_microns(&self) -> u64 {
        self.radius_microns
    }
}

/// The resident identity and shape of one orbit — counts and chunk
/// counts only, never the points, which live in the backing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Orbit {
    key: OrbitKey,
    points: u64,
    coherent_points: u64,
    unaligned_spans: u64,
    point_chunks: u64,
}

impl Orbit {
    pub(crate) fn key(&self) -> OrbitKey {
        self.key
    }

    pub(crate) fn points(&self) -> u64 {
        self.points
    }

    pub(crate) fn coherent_points(&self) -> u64 {
        self.coherent_points
    }

    pub(crate) fn unaligned_spans(&self) -> u64 {
        self.unaligned_spans
    }
}

/// What one section of the backing holds for this layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RemanenceSectionKind {
    OrbitMetadata,
    PointChunk,
}

/// The address of one section: which orbit, which kind, which chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RemanenceSectionKey {
    orbit: OrbitKey,
    kind: RemanenceSectionKind,
    ordinal: u64,
}

impl SectionAddress for RemanenceSectionKey {
    fn namespace(&self) -> &'static str {
        REMANENCE
    }

    fn write(&self, out: &mut Vec<u8>) {
        let namespace = REMANENCE.as_bytes();
        write_varint(out, namespace.len() as u64);
        out.extend_from_slice(namespace);
        write_varint(out, self.orbit.surface);
        write_varint(out, self.orbit.radius_microns);
        write_varint(
            out,
            match self.kind {
                RemanenceSectionKind::OrbitMetadata => 0,
                RemanenceSectionKind::PointChunk => 1,
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
        let (length, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        let length = usize::try_from(length).map_err(|_| {
            Error::invalid_image(source, "index names a namespace longer than this host can address")
        })?;
        let raw = bytes.get(cursor..cursor + length).ok_or_else(|| {
            Error::invalid_image(source, "index ends inside the namespace it names")
        })?;
        cursor += length;
        let spelling = std::str::from_utf8(raw).map_err(|_| {
            Error::invalid_image(source, "index names a namespace that is not text")
        })?;
        if !known.iter().any(|candidate| *candidate == spelling) {
            return Err(Error::invalid_image(
                source,
                format!(
                    "index names the namespace {spelling:?}, which this reader cannot \
                     place, so the index is not the one this layer wrote"
                ),
            ));
        }
        let (surface, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        let (radius, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        if radius == 0 {
            return Err(Error::invalid_image(
                source,
                "index states an orbit at radius zero, which is no orbit",
            ));
        }
        let (kind_tag, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        let kind = match kind_tag {
            0 => RemanenceSectionKind::OrbitMetadata,
            1 => RemanenceSectionKind::PointChunk,
            other => {
                return Err(Error::invalid_image(
                    source,
                    format!("index states a section kind {other}, which this version has no reading of"),
                ));
            }
        };
        let (ordinal, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        Ok((
            Self {
                orbit: OrbitKey {
                    surface,
                    radius_microns: radius,
                },
                kind,
                ordinal,
            },
            cursor - at,
        ))
    }
}

/// A backing held whole in memory: the sink a builder filled, served
/// back as a source. For images assembled before any session storage
/// exists — a decoded artifact in a test, a small synthetic image.
pub(crate) struct MemorySource(pub(crate) std::sync::Arc<Vec<u8>>);

impl ByteSource for MemorySource {
    fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
        let at = usize::try_from(offset)
            .ok()
            .filter(|&at| at + into.len() <= self.0.len())
            .ok_or_else(|| {
                Error::invalid_image(REMANENCE, "read past the end of an in-memory backing")
            })?;
        into.copy_from_slice(&self.0[at..at + into.len()]);
        Ok(())
    }
}

struct RemanenceBacking {
    bytes: Box<dyn ByteSource + Send + Sync>,
    total_bytes: u64,
    cache: Mutex<SectionCache<RemanenceSectionKey>>,
    known: Vec<&'static str>,
}

impl std::fmt::Debug for RemanenceBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemanenceBacking")
            .field("total_bytes", &self.total_bytes)
            .finish_non_exhaustive()
    }
}

/// The remanence image: the physical facts of one disk.
#[derive(Debug)]
pub(crate) struct RemanenceImage {
    form_factor: MediaFormFactor,
    holes: Vec<Hole>,
    orbits: BTreeMap<OrbitKey, Orbit>,
    provenance: Provenance,
    backing: Option<RemanenceBacking>,
}

impl RemanenceImage {
    pub(crate) fn form_factor(&self) -> MediaFormFactor {
        self.form_factor
    }

    pub(crate) fn holes(&self) -> &[Hole] {
        &self.holes
    }

    pub(crate) fn orbits(&self) -> impl Iterator<Item = &Orbit> {
        self.orbits.values()
    }

    pub(crate) fn orbit(&self, key: &OrbitKey) -> Option<&Orbit> {
        self.orbits.get(key)
    }

    pub(crate) fn orbit_count(&self) -> usize {
        self.orbits.len()
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Hands this image its sealed backing and the bound its working
    /// set may occupy.
    pub(crate) fn attach_backing(
        &mut self,
        bytes: impl ByteSource + Send + Sync + 'static,
        total_bytes: u64,
        cache_bytes: u64,
    ) {
        self.backing = Some(RemanenceBacking {
            bytes: Box::new(bytes),
            total_bytes,
            cache: Mutex::new(SectionCache::with_bytes(cache_bytes)),
            known: vec![REMANENCE],
        });
    }

    pub(crate) fn backing_bytes(&self) -> u64 {
        self.backing
            .as_ref()
            .map(|backing| backing.total_bytes)
            .unwrap_or(0)
    }

    pub(crate) fn resident_bytes(&self) -> u64 {
        self.backing
            .as_ref()
            .map(|backing| {
                backing
                    .cache
                    .lock()
                    .expect("no holder of the cache lock panics while holding it")
                    .resident_bytes()
            })
            .unwrap_or(0)
    }

    /// One orbit's points, decoded chunk by chunk through the working
    /// set. The whole orbit is bounded — the largest real orbit is a
    /// few hundred kilobytes packed — and a consumer that needs less
    /// than all of it still pays only the chunks it touches staying
    /// resident.
    pub(crate) fn points(&self, orbit: &Orbit) -> Result<Vec<OrbitPoint>> {
        let backing = self.backing.as_ref().ok_or_else(|| {
            Error::invalid_image(
                REMANENCE,
                "image holds no backing, so its points cannot be served",
            )
        })?;
        let mut cache = backing
            .cache
            .lock()
            .expect("no holder of the cache lock panics while holding it");
        let mut points = Vec::with_capacity(usize::try_from(orbit.points).unwrap_or(0));
        for ordinal in 0..orbit.point_chunks {
            let key = RemanenceSectionKey {
                orbit: orbit.key,
                kind: RemanenceSectionKind::PointChunk,
                ordinal,
            };
            let payload = cache.section(
                backing.bytes.as_ref(),
                backing.total_bytes,
                &key,
                &backing.known,
            )?;
            decode_point_chunk(payload, &mut points)?;
        }
        if points.len() as u64 != orbit.points {
            return Err(Error::invalid_image(
                REMANENCE,
                format!(
                    "orbit at surface {} radius {} states {} points but its chunks \
                     decode {}",
                    orbit.key.surface,
                    orbit.key.radius_microns,
                    orbit.points,
                    points.len()
                ),
            ));
        }
        Ok(points)
    }
}

/// Validates one orbit's points as the model requires: strictly
/// ascending angles, the first coherent point stating widths, and
/// adjacent coherent points alternating unless the second states
/// widths (the rewrite splice).
pub(crate) fn validate_orbit_points(points: &[OrbitPoint]) -> Result<()> {
    for point in points {
        if point.magnetization.is_coherent() {
            if !point.states_widths() {
                return Err(Error::invalid_image(
                    REMANENCE,
                    format!(
                        "the first coherent point must state write widths; found none \
                         at angle {}",
                        point.angle
                    ),
                ));
            }
            break;
        }
    }
    let mut previous: Option<&OrbitPoint> = None;
    for point in points {
        if let Some(before) = previous {
            if point.angle <= before.angle {
                return Err(Error::invalid_image(
                    REMANENCE,
                    format!(
                        "point angles must be strictly increasing: {} then {}",
                        before.angle, point.angle
                    ),
                ));
            }
            if before.magnetization.is_coherent()
                && before.magnetization == point.magnetization
                && !point.states_widths()
            {
                return Err(Error::invalid_image(
                    REMANENCE,
                    format!(
                        "adjacent coherent points must alternate, but angles {} and {} \
                         are both {}; the second states a state already in force",
                        before.angle,
                        point.angle,
                        point.magnetization.as_str()
                    ),
                ));
            }
        }
        previous = Some(point);
    }
    Ok(())
}

/// Whether an orbit asserts anything at all: a wholly-`Unaligned`
/// orbit said "no reliable information here", and absence says exactly
/// that, so the model drops it rather than keeping a claim of nothing.
pub(crate) fn orbit_asserts(points: &[OrbitPoint]) -> bool {
    points
        .iter()
        .any(|point| point.magnetization != Magnetization::Unaligned)
}

/// The state in force at an angle: whatever the nearest preceding
/// point declared, wrapping past the origin. `None` only for an empty
/// orbit, which declares nothing anywhere.
pub(crate) fn state_at(points: &[OrbitPoint], angle: u64) -> Option<Magnetization> {
    let mut in_force = None;
    for point in points {
        if point.angle <= angle {
            in_force = Some(point.magnetization);
        } else {
            break;
        }
    }
    in_force.or_else(|| points.last().map(|point| point.magnetization))
}

/// The reversals an orbit holds strictly inside `[from, to)`, or
/// `None` if it does not read that whole stretch.
///
/// "Reads" is all-or-nothing on purpose: the state in force where the
/// stretch opens must carry a sense, and nothing inside it may decline
/// to — one `Unaligned` or `Inert` point anywhere within disqualifies
/// the lot. A partial answer would leave a consumer unable to say
/// where the reading stopped being an account of the medium and
/// started being a gap. `to` may exceed one revolution, meaning the
/// stretch wraps past the origin; returned angles are always inside
/// one.
pub(crate) fn transitions_across(
    points: &[OrbitPoint],
    from: u64,
    to: u64,
) -> Option<Vec<u64>> {
    if points.is_empty() || to <= from {
        return None;
    }
    let opening = state_at(points, from % ANGULAR_DIVISIONS)?;
    if !opening.is_coherent() {
        return None;
    }
    let mut inside = Vec::new();
    for point in points {
        // A point sits in the stretch under its own angle or one
        // revolution on, since the stretch may run past the origin.
        for candidate in [point.angle, point.angle + ANGULAR_DIVISIONS] {
            if candidate > from && candidate < to {
                if !point.magnetization.is_coherent() {
                    return None;
                }
                inside.push(candidate % ANGULAR_DIVISIONS);
            }
        }
    }
    Some(inside)
}

/// The angle at which the span opened by point `index` ends, wrapping
/// if it is the last.
pub(crate) fn span_end_after(points: &[OrbitPoint], index: usize) -> u64 {
    if index + 1 < points.len() {
        points[index + 1].angle
    } else {
        points[0].angle + ANGULAR_DIVISIONS
    }
}

/// An orbit as the other orientation of a flippy medium holds it.
///
/// **Spans reverse, not points.** A point declares the state in force
/// *from* it, so the span is the real object and the point only its
/// leading edge: reversing the revolution turns `[p(i), p(i+1))` into
/// `[D - p(i+1), D - p(i))`, whose leading edge is the image of the
/// span's *trailing* point — so the assignment shifts by one against
/// the angles, including across the wrap. Polarity is re-laid from the
/// start, which discards nothing (alternation makes the opening pole
/// the only information, and it was an arbitrary gauge) and is
/// required: reversing a cycle changes which adjacent pair falls at
/// the wrap, the one place a non-alternating pair is tolerated. The
/// angular origin is *not* corrected, because no capture records it:
/// the result is a true image of the orbit, rotated by an unknown
/// constant against any read taken the normal way round.
pub(crate) fn reversed_points(points: &[OrbitPoint]) -> Result<Vec<OrbitPoint>> {
    let n = points.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    // The write geometry in force over each span, resolved forward
    // with the wrap, so the turn can restate it wherever it changes
    // rather than relying on an order that is about to reverse.
    let in_force = widths_in_force(points);

    struct Turned {
        angle: u64,
        was: Magnetization,
        widths: Option<WriteWidths>,
    }
    let mut turned = Vec::with_capacity(n);
    for i in 0..n {
        let edge = (ANGULAR_DIVISIONS - points[(i + 1) % n].angle) % ANGULAR_DIVISIONS;
        turned.push(Turned {
            angle: edge,
            was: points[i].magnetization,
            widths: in_force[i],
        });
    }
    turned.sort_by_key(|point| point.angle);

    relay_polarity(
        turned
            .into_iter()
            .map(|point| (point.angle, point.was, point.widths)),
    )
}

/// The geometry in force over each span, resolved forward with the
/// wrap: absence on a point always means unchanged, never unknown.
fn widths_in_force(points: &[OrbitPoint]) -> Vec<Option<WriteWidths>> {
    let mut carried = points
        .iter()
        .rev()
        .find_map(|point| point.widths);
    points
        .iter()
        .map(|point| {
            if point.states_widths() {
                carried = point.widths;
            }
            carried
        })
        .collect()
}

/// Lays polarity fresh over a sorted sequence of spans: alternate
/// across coherent spans, reset after any span with no sense, restate
/// geometry only where it changes and always on the first coherent
/// point, so "absent" can always mean "unchanged".
fn relay_polarity(
    spans: impl Iterator<Item = (u64, Magnetization, Option<WriteWidths>)>,
) -> Result<Vec<OrbitPoint>> {
    let mut rebuilt = Vec::new();
    let mut sense: Option<Magnetization> = None;
    let mut standing: Option<WriteWidths> = None;
    let mut opened = false;
    for (angle, was, widths) in spans {
        if !was.is_coherent() {
            rebuilt.push(OrbitPoint::new(angle, was)?);
            sense = None;
            continue;
        }
        let next = match sense {
            None => Magnetization::Positive,
            Some(current) => current
                .opposite()
                .expect("only coherent senses are carried here"),
        };
        sense = Some(next);
        let restate = !opened || standing != widths;
        rebuilt.push(if restate {
            OrbitPoint::stating(angle, next, widths)?
        } else {
            OrbitPoint::new(angle, next)?
        });
        standing = widths;
        opened = true;
    }
    Ok(rebuilt)
}

/// What refining one image with another gained, and what it could not
/// settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refinement {
    pub(crate) orbits_gained: u64,
    pub(crate) spans_read: u64,
    pub(crate) transitions_gained: u64,
    pub(crate) spans_still_unread: u64,
    pub(crate) contested: u64,
}

impl Refinement {
    pub(crate) fn gained_anything(&self) -> bool {
        self.orbits_gained > 0 || self.spans_read > 0
    }
}

/// Fills one orbit's `Unaligned` spans from a donor's reading of the
/// same stretches. Only ignorance may be overwritten: `Unaligned` is
/// fillable, `Inert` is contested — this image says the medium cannot
/// hold a state there, the donor says it read one, and that is not
/// ours to settle from the images alone, so it is counted and left.
pub(crate) fn fill_from(
    mine: &[OrbitPoint],
    donor: &[OrbitPoint],
) -> Result<(Vec<OrbitPoint>, Refinement)> {
    let n = mine.len();
    let in_force = widths_in_force(mine);
    let mut placed: Vec<(u64, Magnetization, Option<WriteWidths>)> = Vec::new();
    let mut refinement = Refinement {
        orbits_gained: 0,
        spans_read: 0,
        transitions_gained: 0,
        spans_still_unread: 0,
        contested: 0,
    };
    for i in 0..n {
        let point = &mine[i];
        if point.magnetization != Magnetization::Unaligned {
            placed.push((point.angle, point.magnetization, in_force[i]));
            if point.magnetization == Magnetization::Inert
                && transitions_across(donor, point.angle, span_end_after(mine, i)).is_some()
            {
                refinement.contested += 1;
            }
            continue;
        }
        let from = point.angle;
        let to = span_end_after(mine, i);
        match transitions_across(donor, from, to) {
            None => {
                // Neither image can read this stretch; it stays unread,
                // which is the honest outcome and worth counting.
                placed.push((from, Magnetization::Unaligned, None));
                refinement.spans_still_unread += 1;
            }
            Some(recovered) => {
                // The donor resolves it. Its content opens at the
                // span's start and runs to the next point this image
                // already has. Senses are placeholders here; polarity
                // is re-laid below.
                placed.push((from, Magnetization::Positive, in_force[i]));
                refinement.transitions_gained += recovered.len() as u64;
                for angle in recovered {
                    placed.push((angle, Magnetization::Positive, in_force[i]));
                }
                refinement.spans_read += 1;
            }
        }
    }
    placed.sort_by_key(|(angle, _, _)| *angle);
    let rebuilt = relay_polarity(placed.into_iter())?;
    Ok((rebuilt, refinement))
}

/// The metadata records' own version, gating their reading (P8).
const METADATA_VERSION: u64 = 1;

/// Builds a remanence image by streaming each orbit's packed points
/// into the backing, orbits in key order.
pub(crate) struct RemanenceImageBuilder<S: ByteSink = Vec<u8>> {
    writer: SectionWriter<RemanenceSectionKey, S>,
    image: RemanenceImage,
    chunk_records: usize,
}

impl RemanenceImageBuilder<Vec<u8>> {
    /// A builder whose backing is an in-memory vector, for tests and
    /// for images small enough to assemble before a home exists.
    pub(crate) fn in_memory(
        form_factor: MediaFormFactor,
        holes: Vec<Hole>,
        policy: Provenance,
    ) -> Result<Self> {
        Self::to_sink(form_factor, holes, policy, Vec::new(), CHUNK_RECORDS)
    }
}

impl<S: ByteSink> RemanenceImageBuilder<S> {
    /// A builder streaming into `sink`. Refuses a policy with no
    /// notes: an image is reconstructed from evidence or read from an
    /// artifact, and the account of which travels with it — it is
    /// never conjured.
    pub(crate) fn to_sink(
        form_factor: MediaFormFactor,
        holes: Vec<Hole>,
        policy: Provenance,
        sink: S,
        chunk_records: usize,
    ) -> Result<Self> {
        if policy.notes.is_empty() {
            return Err(Error::invalid_image(
                REMANENCE,
                "an image carries the account of what produced it, so a policy with \
                 nothing to say cannot build one",
            ));
        }
        let mut previous: Option<&Hole> = None;
        for hole in &holes {
            if let Some(before) = previous
                && before.center_angle.compare(&hole.center_angle) != std::cmp::Ordering::Less
            {
                return Err(Error::invalid_image(
                    REMANENCE,
                    "holes must be ordered by centre angle, with no two sharing one",
                ));
            }
            previous = Some(hole);
        }
        Ok(Self {
            writer: SectionWriter::to_sink(sink, LEAF_ENTRIES),
            image: RemanenceImage {
                form_factor,
                holes,
                orbits: BTreeMap::new(),
                provenance: policy,
                backing: None,
            },
            chunk_records: chunk_records.max(1),
        })
    }

    /// Adds one orbit, which must sort after every orbit already
    /// added. Returns whether it was kept: an orbit asserting nothing
    /// is dropped — absence of an orbit says exactly what a
    /// wholly-`Unaligned` one said — and dropping it here keeps the
    /// model and its backing saying the same thing.
    pub(crate) fn add_orbit(&mut self, key: OrbitKey, points: &[OrbitPoint]) -> Result<bool> {
        validate_orbit_points(points)?;
        if !orbit_asserts(points) {
            return Ok(false);
        }
        if self.image.orbits.contains_key(&key) {
            return Err(Error::invalid_image(
                REMANENCE,
                format!(
                    "two orbits claim surface {} radius {}; one radius, one recording",
                    key.surface, key.radius_microns
                ),
            ));
        }

        let coherent = points
            .iter()
            .filter(|point| point.magnetization.is_coherent())
            .count() as u64;
        // Each `Unaligned` point opens one unread span, so the count of
        // points is the count of spans.
        let unaligned = points
            .iter()
            .filter(|point| point.magnetization == Magnetization::Unaligned)
            .count() as u64;
        let chunks = points.chunks(self.chunk_records).len() as u64;

        // Metadata first: its kind sorts before the chunks under one
        // orbit, so append order and key order agree.
        let mut metadata = Vec::new();
        write_varint(&mut metadata, METADATA_VERSION);
        write_varint(&mut metadata, points.len() as u64);
        write_varint(&mut metadata, coherent);
        write_varint(&mut metadata, unaligned);
        write_varint(&mut metadata, chunks);
        self.writer.append(
            RemanenceSectionKey {
                orbit: key,
                kind: RemanenceSectionKind::OrbitMetadata,
                ordinal: 0,
            },
            metadata,
        )?;
        for (ordinal, run) in points.chunks(self.chunk_records).enumerate() {
            self.writer.append(
                RemanenceSectionKey {
                    orbit: key,
                    kind: RemanenceSectionKind::PointChunk,
                    ordinal: ordinal as u64,
                },
                encode_point_chunk(run),
            )?;
        }

        self.image.orbits.insert(
            key,
            Orbit {
                key,
                points: points.len() as u64,
                coherent_points: coherent,
                unaligned_spans: unaligned,
                point_chunks: chunks,
            },
        );
        Ok(true)
    }

    /// Closes the backing and hands back the image, the sink, and the
    /// backing's total length; the caller attaches the backing.
    pub(crate) fn seal(self) -> Result<(RemanenceImage, S, u64)> {
        let (sink, total) = self.writer.seal()?;
        Ok((self.image, sink, total))
    }
}

/// One point chunk's payload: per point the packed little-endian word,
/// then the two width varints where the flag elects them.
fn encode_point_chunk(points: &[OrbitPoint]) -> Vec<u8> {
    let mut out = Vec::with_capacity(points.len() * 4);
    for point in points {
        out.extend_from_slice(&point.packed().to_le_bytes());
        if let Some(widths) = point.widths {
            write_varint(&mut out, widths.plateau_microns);
            write_varint(&mut out, widths.guard_microns);
        }
    }
    out
}

fn decode_point_chunk(payload: &[u8], into: &mut Vec<OrbitPoint>) -> Result<()> {
    let mut at = 0;
    while at < payload.len() {
        let raw = payload.get(at..at + 4).ok_or_else(|| {
            Error::invalid_image(REMANENCE, "point chunk ends inside a packed word")
        })?;
        at += 4;
        let word = u32::from_le_bytes(raw.try_into().expect("four bytes"));
        let angle = u64::from(word >> 4);
        let magnetization = Magnetization::from_code(word & 0b11)
            .expect("two bits always name one of the four states");
        let widths = if word & 0b100 != 0 {
            let (plateau, used) = read_varint(REMANENCE, payload, at)?;
            at += used;
            let (guard, used) = read_varint(REMANENCE, payload, at)?;
            at += used;
            Some(WriteWidths::new(plateau, guard)?)
        } else {
            None
        };
        into.push(OrbitPoint::stating(angle, magnetization, widths)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths() -> WriteWidths {
        WriteWidths::new(330, 432).expect("the standard geometry is valid")
    }

    fn policy() -> Provenance {
        Provenance::new(REMANENCE).note("built by a unit test")
    }

    /// An alternating orbit of `count` points starting at `first`,
    /// spaced `step` apart, widths stated on the first point.
    fn alternating(first: u64, step: u64, count: usize) -> Vec<OrbitPoint> {
        (0..count)
            .map(|i| {
                let angle = first + step * i as u64;
                let sense = if i % 2 == 0 {
                    Magnetization::Positive
                } else {
                    Magnetization::Negative
                };
                if i == 0 {
                    OrbitPoint::stating(angle, sense, Some(widths()))
                } else {
                    OrbitPoint::new(angle, sense)
                }
                .expect("test points are inside the revolution")
            })
            .collect()
    }

    #[test]
    fn widths_refuse_a_guard_inside_the_plateau() {
        assert!(WriteWidths::new(330, 200).is_err());
        assert!(WriteWidths::new(0, 432).is_err());
        assert!(WriteWidths::new(330, 330).is_ok(), "equal is the cliff");
    }

    #[test]
    fn a_point_refuses_widths_on_an_incoherent_state() {
        assert!(OrbitPoint::stating(0, Magnetization::Unaligned, Some(widths())).is_err());
        assert!(OrbitPoint::stating(0, Magnetization::Inert, Some(widths())).is_err());
    }

    #[test]
    fn a_point_refuses_an_angle_outside_the_revolution() {
        assert!(OrbitPoint::new(ANGULAR_DIVISIONS, Magnetization::Positive).is_err());
        assert!(OrbitPoint::new(ANGULAR_DIVISIONS - 1, Magnetization::Unaligned).is_ok());
    }

    #[test]
    fn the_first_coherent_point_must_state_widths() {
        let bare = vec![OrbitPoint::new(10, Magnetization::Positive).unwrap()];
        assert!(validate_orbit_points(&bare).is_err());
        let stated = vec![
            OrbitPoint::new(5, Magnetization::Unaligned).unwrap(),
            OrbitPoint::stating(10, Magnetization::Positive, Some(widths())).unwrap(),
        ];
        assert!(validate_orbit_points(&stated).is_ok());
    }

    #[test]
    fn adjacent_coherent_points_must_alternate_unless_widths_are_stated() {
        let repeated = vec![
            OrbitPoint::stating(10, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::new(20, Magnetization::Positive).unwrap(),
        ];
        assert!(validate_orbit_points(&repeated).is_err());

        // The rewrite splice: the same sense, admitted because the
        // point asserts a change of geometry.
        let splice = vec![
            OrbitPoint::stating(10, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::stating(
                20,
                Magnetization::Positive,
                Some(WriteWidths::new(300, 400).unwrap()),
            )
            .unwrap(),
        ];
        assert!(validate_orbit_points(&splice).is_ok());

        // Polarity does not propagate across an unaligned span: a
        // coherent point after one may take either sense.
        let across = vec![
            OrbitPoint::stating(10, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::new(20, Magnetization::Unaligned).unwrap(),
            OrbitPoint::new(30, Magnetization::Positive).unwrap(),
        ];
        assert!(validate_orbit_points(&across).is_ok());
    }

    #[test]
    fn angles_must_strictly_ascend() {
        let tied = vec![
            OrbitPoint::stating(10, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::new(10, Magnetization::Negative).unwrap(),
        ];
        assert!(validate_orbit_points(&tied).is_err());
    }

    #[test]
    fn state_wraps_past_the_origin() {
        let points = alternating(1000, 1000, 3);
        // Before the first point is after the last one, going round.
        assert_eq!(state_at(&points, 0), Some(Magnetization::Positive));
        assert_eq!(state_at(&points, 1000), Some(Magnetization::Positive));
        assert_eq!(state_at(&points, 2500), Some(Magnetization::Negative));
        assert_eq!(
            state_at(&points, ANGULAR_DIVISIONS - 1),
            Some(Magnetization::Positive)
        );
        assert_eq!(state_at(&[], 0), None);
    }

    #[test]
    fn transitions_across_reads_all_or_nothing() {
        let points = alternating(1000, 1000, 4);
        assert_eq!(
            transitions_across(&points, 1000, 4001),
            Some(vec![2000, 3000, 4000])
        );

        // A stretch wrapping the origin finds the early points one
        // revolution on. Angles come back in point order, not stretch
        // order, and always inside one revolution.
        let wrapped = transitions_across(&points, 3500, ANGULAR_DIVISIONS + 1500)
            .expect("the stretch opens on a coherent state");
        assert_eq!(wrapped, vec![1000, 4000]);

        // One unaligned point inside disqualifies the lot.
        let broken = vec![
            OrbitPoint::stating(1000, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::new(2000, Magnetization::Unaligned).unwrap(),
            OrbitPoint::new(3000, Magnetization::Positive).unwrap(),
        ];
        assert_eq!(transitions_across(&broken, 1000, 3500), None);
        // And a stretch opening on no sense is not read at all.
        assert_eq!(transitions_across(&broken, 2100, 2900), None);
    }

    #[test]
    fn reversal_shifts_spans_not_points() {
        // Three spans: [1000,2000) positive, [2000,3000) negative,
        // [3000,1000+D) positive. Reversed, the span that opened at
        // angle a(i) opens at D - a(i+1), carrying span i's state.
        let points = alternating(1000, 1000, 3);
        let turned = reversed_points(&points).expect("a valid orbit reverses");
        let angles: Vec<u64> = turned.iter().map(|point| point.angle()).collect();
        assert_eq!(
            angles,
            vec![
                ANGULAR_DIVISIONS - 3000,
                ANGULAR_DIVISIONS - 2000,
                ANGULAR_DIVISIONS - 1000
            ]
        );
        // The gauge is re-laid from POSITIVE; alternation holds.
        assert!(validate_orbit_points(&turned).is_ok());
        // Reversing twice restores the original angles.
        let back = reversed_points(&turned).expect("a reversed orbit reverses back");
        let restored: Vec<u64> = back.iter().map(|point| point.angle()).collect();
        assert_eq!(restored, vec![1000, 2000, 3000]);
    }

    #[test]
    fn reversal_keeps_an_unaligned_span() {
        let points = vec![
            OrbitPoint::stating(1000, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::new(2000, Magnetization::Unaligned).unwrap(),
            OrbitPoint::new(3000, Magnetization::Positive).unwrap(),
        ];
        let turned = reversed_points(&points).expect("a valid orbit reverses");
        assert!(
            turned
                .iter()
                .any(|point| point.magnetization() == Magnetization::Unaligned),
            "the record that a stretch could not be read must survive the turn"
        );
        assert!(validate_orbit_points(&turned).is_ok());
    }

    #[test]
    fn refinement_fills_only_ignorance() {
        // Mine reads [1000,2000) then declares the rest unreadable.
        let mine = vec![
            OrbitPoint::stating(1000, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::new(2000, Magnetization::Unaligned).unwrap(),
        ];
        // The donor reads the whole revolution with transitions inside
        // the span mine could not read.
        let donor = alternating(1000, 1000, 4);
        let (filled, refinement) = fill_from(&mine, &donor).expect("refinement composes");
        assert_eq!(refinement.spans_read, 1);
        assert!(refinement.transitions_gained >= 2);
        assert_eq!(refinement.contested, 0);
        assert!(validate_orbit_points(&filled).is_ok());
        assert!(orbit_asserts(&filled));

        // An inert claim against a donor reading is contested, and the
        // inert claim stands.
        let inert = vec![
            OrbitPoint::stating(1000, Magnetization::Positive, Some(widths())).unwrap(),
            OrbitPoint::new(2000, Magnetization::Inert).unwrap(),
        ];
        let (kept, refinement) = fill_from(&inert, &donor).expect("refinement composes");
        assert_eq!(refinement.contested, 1);
        assert!(
            kept.iter()
                .any(|point| point.magnetization() == Magnetization::Inert)
        );
    }

    #[test]
    fn the_builder_drops_an_orbit_asserting_nothing() {
        let mut builder =
            RemanenceImageBuilder::in_memory(MediaFormFactor::Inch525, Vec::new(), policy())
                .expect("a noted policy builds");
        let silent = vec![OrbitPoint::new(0, Magnetization::Unaligned).unwrap()];
        let kept = builder
            .add_orbit(OrbitKey::new(0, 57150).unwrap(), &silent)
            .expect("a valid orbit is judged");
        assert!(!kept, "a wholly-unaligned orbit asserts nothing and is dropped");
        let (image, _, _) = builder.seal().expect("an empty image seals");
        assert_eq!(image.orbit_count(), 0);
    }

    #[test]
    fn the_builder_refuses_a_second_recording_at_one_radius() {
        let mut builder =
            RemanenceImageBuilder::in_memory(MediaFormFactor::Inch525, Vec::new(), policy())
                .expect("a noted policy builds");
        let key = OrbitKey::new(0, 57150).unwrap();
        builder
            .add_orbit(key, &alternating(0, 1000, 4))
            .expect("the first recording is admitted");
        assert!(builder.add_orbit(key, &alternating(0, 1000, 4)).is_err());
    }

    #[test]
    fn the_builder_refuses_a_policy_with_nothing_to_say() {
        assert!(
            RemanenceImageBuilder::in_memory(
                MediaFormFactor::Inch525,
                Vec::new(),
                Provenance::new(REMANENCE),
            )
            .is_err()
        );
    }

    #[test]
    fn holes_must_be_ordered_and_bounded() {
        let quarter = TurnFraction::new(1, 4).unwrap();
        let eighth = TurnFraction::new(1, 8).unwrap();
        let extent = TurnFraction::new(1, 50).unwrap();
        let disordered = vec![
            Hole::new(quarter, extent).unwrap(),
            Hole::new(eighth, extent).unwrap(),
        ];
        assert!(
            RemanenceImageBuilder::in_memory(MediaFormFactor::Inch525, disordered, policy())
                .is_err()
        );
        assert!(Hole::new(TurnFraction::new(5, 4).unwrap(), extent).is_err());
        assert!(Hole::new(quarter, TurnFraction::new(0, 1).unwrap()).is_err());
        assert!(Hole::new(quarter, TurnFraction::new(3, 2).unwrap()).is_err());
    }

    #[test]
    fn points_stream_through_the_backing_and_come_back_whole() {
        let mut builder = RemanenceImageBuilder::to_sink(
            MediaFormFactor::Inch525,
            vec![
                Hole::new(
                    TurnFraction::new(3, 8).unwrap(),
                    TurnFraction::new(1, 50).unwrap(),
                )
                .unwrap(),
            ],
            policy(),
            Vec::new(),
            // A tiny chunk bound forces several chunks out of a
            // handful of points, the way the layers beside this one
            // test their own splitting.
            3,
        )
        .expect("a noted policy builds");

        let key = OrbitKey::new(0, 57150).unwrap();
        let mut points = alternating(1000, 1000, 10);
        // A splice mid-orbit exercises the widths side-stream.
        points.push(
            OrbitPoint::stating(
                20_000,
                points.last().unwrap().magnetization(),
                Some(WriteWidths::new(300, 400).unwrap()),
            )
            .unwrap(),
        );
        let second = OrbitKey::new(1, 40000).unwrap();
        builder.add_orbit(key, &points).expect("the orbit is admitted");
        builder
            .add_orbit(second, &alternating(500, 2000, 5))
            .expect("the second orbit is admitted");

        let (mut image, sink, total) = builder.seal().expect("the backing seals");
        let backing = std::sync::Arc::new(sink);
        struct Mem(std::sync::Arc<Vec<u8>>);
        impl ByteSource for Mem {
            fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
                let at = usize::try_from(offset).expect("test offsets fit");
                into.copy_from_slice(&self.0[at..at + into.len()]);
                Ok(())
            }
        }
        image.attach_backing(Mem(backing), total, 1 << 16);

        assert_eq!(image.orbit_count(), 2);
        let orbit = image
            .orbit(&key)
            .expect("the admitted orbit is resident")
            .clone();
        assert_eq!(orbit.points(), 11);
        assert_eq!(orbit.point_chunks, 4, "eleven points over chunks of three");
        let served = image.points(&orbit).expect("the chunks decode");
        assert_eq!(served, points);

        let second = image.orbit(&second).expect("the second orbit is resident").clone();
        let served = image.points(&second).expect("the chunks decode");
        assert_eq!(served.len(), 5);
    }
}
