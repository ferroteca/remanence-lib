// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The drive-profile seam (F36, P30): where a family's recording
//! conventions are declared, and where a capture is recognized as
//! belonging to that family.
//!
//! It exists because the knowledge P22 and P23 both rest on — stepping,
//! rotation, the density map, the shape of an encoding's landmarks — is
//! precisely what a flux capture does not contain. A capture records
//! what the head saw. It does not record that the drive it will be
//! served to spins at 300 RPM, that two source positions make one track,
//! or that a run of shortest intervals is a synchronization mark. Every
//! one of those is family knowledge, and this is where it lives.
//!
//! **Recognition stops at structure.** The probe reads interval lengths
//! and the patterns they form, and reports a count, a density, an angle,
//! a location and an absence. It resolves no bit, assembles no byte,
//! names no sector and validates no checksum: those are the hardware
//! bitstream and above, and reaching them here would make every
//! recognition depend on a clock-recovery model. What leaves the probe
//! is an angle, never a byte.
//!
//! **Discovery proposes and never silently decides.** Verdicts are
//! ranked and carry the observations that produced them (P4), a caller
//! may pin a profile, and a capture no profile claims is a named
//! refusal — a lone enrolled entry never wins by being alone.
//!
//! Every declared field is a fact of the family carrying the published
//! description it came from, never a value a capture is permitted to
//! establish. The arithmetic below is integer and exact throughout: a
//! cell is the rational the interval population is self-consistent
//! with, and nothing here rounds through a float.
//!
//! A profile's **materialization half** is declared here and read by the
//! flux-to-medium reduction, which is not delivered: it is laid out
//! beside the recognition half deliberately, because these are facts
//! about the same family and splitting them across two places is how two
//! features come to hold different answers about one drive.
#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::error::{Error, ErrorCategory, Result};
use crate::flux_capture::{FluxCapture, Tick, TrackKey};

// ------------------------------------------------------- declared facts

/// How the source's step positions map onto the family's own locations.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stepping {
    /// How many source steps make one family location.
    pub(crate) steps_per_location: u64,
    /// The family location source position zero addresses.
    pub(crate) first_location: u64,
}

impl Stepping {
    /// The family location a source position addresses, or `None` where
    /// the family's addressing has no location there at all.
    fn location_of(&self, position: u64) -> Option<u64> {
        (position % self.steps_per_location == 0)
            .then(|| position / self.steps_per_location + self.first_location)
    }
}

/// The family's rotation, and the clock its served medium is timed
/// against.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rotation {
    /// Rotations per second, exactly.
    pub(crate) nominal_numerator: u64,
    pub(crate) nominal_denominator: u64,
    /// The drive's reference clock in hertz, exactly.
    pub(crate) reference_clock: u64,
    pub(crate) cycles_per_rotation: u64,
    /// Whether the family's own drive observes the index hole. A family
    /// whose drive never does cannot honestly inherit a capture's index
    /// as its medium's origin.
    pub(crate) index_observed_by_drive: bool,
}

/// Which surfaces the family records.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Surfaces {
    pub(crate) recorded: u32,
}

/// The timing shape of the family's synchronization convention, stated
/// as an interval pattern and never as a symbol.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LandmarkShape {
    /// Which cell multiple the landmark's run is made of.
    pub(crate) multiple: u32,
    /// The shortest run of them that counts as one landmark.
    pub(crate) min_run: u32,
    /// How many landmarks one record carries.
    pub(crate) per_record: u32,
}

/// What interval populations the family's encoding produces.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EncodingShape {
    /// The multiples of the cell an interval may be, ascending.
    pub(crate) cell_multiples: &'static [u32],
    /// Admissible deviation from `k * cell`, as a fraction of the cell.
    pub(crate) band_numerator: u64,
    pub(crate) band_denominator: u64,
    pub(crate) landmark: LandmarkShape,
}

/// One zone the family records at.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DensityZone {
    pub(crate) first_location: u64,
    pub(crate) last_location: u64,
    /// Bits per second, exactly.
    pub(crate) rate_numerator: u64,
    pub(crate) rate_denominator: u64,
    /// What this zone claims one location holds.
    pub(crate) records: u32,
}

impl DensityZone {
    /// The cell this zone claims, in reference-clock cycles, exactly:
    /// the clock divided by the rate.
    fn nominal_cell(&self, rotation: &Rotation) -> (u128, u128) {
        (
            u128::from(rotation.reference_clock) * u128::from(self.rate_denominator),
            u128::from(self.rate_numerator),
        )
    }
}

/// What the medium holds where the family records but the capture does
/// not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnrecordedRule {
    Absent,
    CarriedAsWeak,
}

/// What it holds where a location's content duplicates an adjacent one.
///
/// `Refuse` is not timidity. A duplicate is genuinely ambiguous from
/// flux alone — it may mean the family's head reads a neighbouring track
/// at that location, which is a real thing a drive does, or it may mean
/// the capture instrument did not move — and nothing in the evidence
/// distinguishes them, so the caller declares which it is (P29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DuplicateRule {
    Absent,
    CarriedAsObserved,
    Refuse,
}

/// Where the medium's circle begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginDefault {
    LongestGap,
    Index,
    DeclaredAngle,
}

/// How several observations of one location become one served medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionRule {
    Selected,
    Sequence,
    SeededVariation,
}

/// What a projection does with a measured span and a measured density.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanProjection {
    ScaleToNominal,
    PreserveIntervals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DensityProjection {
    SnapToZoneNominal,
    PreserveMeasured,
}

/// The half of a profile the flux-to-medium reduction reads.
///
/// It is declared here rather than beside that reduction for one
/// reason: these are facts about the same family, and splitting them
/// across two places is how two features come to hold different answers
/// about one drive. Every field is a P29 policy input — the profile's
/// value is a declaration, the caller may supply its own, and what was
/// used travels into the result as provenance.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Materialization {
    /// A source position the family's addressing does not cover is
    /// refused; there is no rule that quietly gives it a location.
    pub(crate) unmapped_source_position_refused: bool,
    pub(crate) unrecorded: UnrecordedRule,
    pub(crate) duplicate: DuplicateRule,
    pub(crate) origin: OriginDefault,
    pub(crate) selection: SelectionRule,
    pub(crate) span: SpanProjection,
    pub(crate) density: DensityProjection,
    /// The family's declared strength states, weakest first.
    pub(crate) strength_states: &'static [&'static str],
}

/// One family's recording conventions, and the published description
/// each declared fact derives from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DriveProfile {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) version: u32,
    pub(crate) provenance: &'static str,
    pub(crate) stepping: Stepping,
    pub(crate) rotation: Rotation,
    pub(crate) surfaces: Surfaces,
    pub(crate) encoding: EncodingShape,
    pub(crate) density: &'static [DensityZone],
    pub(crate) materialization: Materialization,
}

impl DriveProfile {
    fn zone_for(&self, location: u64) -> Option<(usize, &DensityZone)> {
        self.density
            .iter()
            .enumerate()
            .find(|(_, zone)| zone.first_location <= location && location <= zone.last_location)
    }

    /// How many family locations this profile's density map covers.
    fn declared_locations(&self) -> u64 {
        self.density
            .iter()
            .map(|zone| zone.last_location - zone.first_location + 1)
            .sum()
    }
}

/// The Commodore 1541, the first and only enrolled family.
pub(crate) static C1541: DriveProfile = DriveProfile {
    id: "c1541",
    name: "Commodore 1541",
    version: 1,
    provenance: "declared from the published 1541 recording conventions: two \
                 drive steps per track, 300 RPM against a 16 MHz reference, and \
                 the four documented speed zones with their track boundaries \
                 and sector counts",
    stepping: Stepping {
        steps_per_location: 2,
        first_location: 1,
    },
    rotation: Rotation {
        nominal_numerator: 5,
        nominal_denominator: 1,
        reference_clock: 16_000_000,
        cycles_per_rotation: 3_200_000,
        // The 1541 has no index sensor at all.
        index_observed_by_drive: false,
    },
    surfaces: Surfaces { recorded: 1 },
    encoding: EncodingShape {
        // GCR admits runs of at most two zeros between ones, so an
        // interval is one, two or three cells and never more.
        cell_multiples: &[1, 2, 3],
        band_numerator: 3,
        band_denominator: 10,
        landmark: LandmarkShape {
            // A GCR sync is ten or more consecutive one bits, and a one
            // bit is a transition one cell after the last — so a sync is
            // a run of at least nine minimum-length intervals, locatable
            // without a clock, without the encoding table, and without
            // knowing what it introduces.
            multiple: 1,
            min_run: 9,
            // A header sync and a data sync to each sector.
            per_record: 2,
        },
    },
    density: &[
        DensityZone {
            first_location: 1,
            last_location: 17,
            rate_numerator: 16_000_000,
            rate_denominator: 52,
            records: 21,
        },
        DensityZone {
            first_location: 18,
            last_location: 24,
            rate_numerator: 16_000_000,
            rate_denominator: 56,
            records: 19,
        },
        DensityZone {
            first_location: 25,
            last_location: 30,
            rate_numerator: 16_000_000,
            rate_denominator: 60,
            records: 18,
        },
        DensityZone {
            first_location: 31,
            last_location: 35,
            rate_numerator: 16_000_000,
            rate_denominator: 64,
            records: 17,
        },
    ],
    materialization: Materialization {
        unmapped_source_position_refused: true,
        unrecorded: UnrecordedRule::Absent,
        duplicate: DuplicateRule::Refuse,
        // The drive never observes index, so the circle begins at the
        // seam the write splice leaves.
        origin: OriginDefault::LongestGap,
        selection: SelectionRule::Selected,
        span: SpanProjection::ScaleToNominal,
        density: DensityProjection::SnapToZoneNominal,
        strength_states: &["absent", "weak", "strong"],
    },
};

/// The enrolled families, in the order they are consulted. Adding one
/// changes its declaration, its tests, and this list.
static ENROLLED: [&DriveProfile; 1] = [&C1541];

pub(crate) fn enrolled() -> &'static [&'static DriveProfile] {
    &ENROLLED
}

// -------------------------------------------------------------- probing

/// How many intervals the cell derivation samples. Enough that the
/// population's three clusters are unmistakable, bounded so the work
/// follows the claim rather than the capture's size.
const CELL_SAMPLE: usize = 4096;

/// What one observation of one location resolved to.
#[derive(Debug, Clone)]
struct ObservationReading {
    /// The cell the interval population is self-consistent with, as an
    /// exact rational of source ticks.
    cell_numerator: u128,
    cell_denominator: u128,
    /// How much of the population classified, in parts per thousand.
    resolved_permille: u32,
    landmarks: u32,
    /// The bit distance between record starts, where it repeats.
    record_bits: Option<u64>,
    /// How far the spacing departs from that, as a median deviation.
    record_bits_deviation: u64,
    /// The angle of the one departure — the location's seam — in
    /// reference-clock cycles from the observation's origin.
    seam_cycles: Option<u64>,
    /// Counts of each admitted multiple, which is what says whether two
    /// locations read the same content without reading any of it.
    population: Vec<u64>,
    /// The observation's whole circumference in source ticks.
    span: Tick,
}

/// Derives the cell the interval population is self-consistent with.
///
/// The cell is the exact rational `sum(intervals) / sum(multiples)` over
/// the intervals that classify, iterated to a fixed point. A solution
/// leaving its shortest multiple unpopulated is the spurious one at half
/// the true cell — where every real interval classifies one step too
/// high — so it is rejected and re-derived rather than reported as a
/// finding about the disk.
fn derive_cell(intervals: &[Tick], encoding: &EncodingShape) -> Option<(u128, u128)> {
    let shortest = *encoding.cell_multiples.first()?;
    // The cell is derived from a bounded sample taken evenly around the
    // circle, not from every interval: the population's shape is what
    // the derivation needs, and reading the whole of it to find one
    // rational is work proportional to the artifact rather than to the
    // claim (P27). Classification below still visits every interval.
    let stride = intervals.len().div_ceil(CELL_SAMPLE).max(1);
    let sample: Vec<Tick> = intervals.iter().copied().step_by(stride).collect();
    let intervals = sample.as_slice();
    let mut ordered: Vec<Tick> = intervals.to_vec();
    ordered.sort_unstable();
    // The fifth percentile sits inside the shortest population.
    let seed = u128::from(ordered[ordered.len() / 20].max(1));

    // Two candidates, because the failure this guards against is a
    // solution at half the true cell: the seed as taken, and the seed
    // doubled.
    let mut best: Option<((u128, u128), u64)> = None;
    for attempt in 1..=2u128 {
        let mut cell = (seed * attempt, 1u128);
        for _ in 0..40 {
            let mut total_interval = 0u128;
            let mut total_multiple = 0u128;
            for &interval in intervals {
                if let Some(multiple) = classify(interval, cell, encoding) {
                    total_interval += u128::from(interval);
                    total_multiple += u128::from(multiple);
                }
            }
            if total_multiple == 0 {
                break;
            }
            let next = (total_interval, total_multiple);
            if next.0 * cell.1 == cell.0 * next.1 {
                cell = next;
                break;
            }
            cell = next;
        }
        let mut resolved = 0u64;
        let mut shortest_population = 0u64;
        for &interval in intervals {
            if let Some(multiple) = classify(interval, cell, encoding) {
                resolved += 1;
                if multiple == shortest {
                    shortest_population += 1;
                }
            }
        }
        // Two declared tests, both drawn from `cell_multiples` and
        // neither a tuned threshold. A solution leaving its shortest
        // multiple unpopulated is the half-cell, where every real
        // interval classified one step too high. Past that, the
        // solution the population actually fits is the one that
        // classifies more of it — a candidate resolving two fifths of
        // the intervals is not a rival to one resolving all of them.
        if resolved == 0 || shortest_population == 0 {
            continue;
        }
        if best.is_none_or(|(_, held)| resolved > held) {
            best = Some((cell, resolved));
        }
    }
    best.map(|(cell, _)| cell)
}

/// Which multiple of `cell` this interval is, if it is an admitted one
/// within the encoding's band. All integer: `|interval - k * cell|` is
/// compared by cross-multiplication against `band * cell`.
fn classify(interval: Tick, cell: (u128, u128), encoding: &EncodingShape) -> Option<u32> {
    let (numerator, denominator) = cell;
    if numerator == 0 {
        return None;
    }
    let scaled = u128::from(interval) * denominator;
    for &multiple in encoding.cell_multiples {
        let want = u128::from(multiple) * numerator;
        let apart = scaled.abs_diff(want);
        if apart * u128::from(encoding.band_denominator)
            <= u128::from(encoding.band_numerator) * numerator
        {
            return Some(multiple);
        }
    }
    None
}

/// Reads one observation in the profile's terms.
fn read_observation(
    profile: &DriveProfile,
    span: Tick,
    transitions: &[Tick],
) -> Option<ObservationReading> {
    if transitions.len() < 100 {
        return None;
    }
    let intervals: Vec<Tick> = transitions.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let cell = derive_cell(&intervals, &profile.encoding)?;
    let encoding = &profile.encoding;

    let multiples: Vec<Option<u32>> = intervals
        .iter()
        .map(|&interval| classify(interval, cell, encoding))
        .collect();
    let resolved = multiples.iter().filter(|multiple| multiple.is_some()).count();
    let mut population = vec![0u64; encoding.cell_multiples.len()];
    for multiple in multiples.iter().flatten() {
        if let Some(at) = encoding
            .cell_multiples
            .iter()
            .position(|candidate| candidate == multiple)
        {
            population[at] += 1;
        }
    }

    // Landmarks: runs of at least min_run consecutive intervals at the
    // landmark multiple. Nothing here reads what the run introduces.
    let mut landmarks = Vec::new();
    let mut run = 0u32;
    for (at, multiple) in multiples.iter().enumerate() {
        if *multiple == Some(encoding.landmark.multiple) {
            run += 1;
        } else {
            if run >= encoding.landmark.min_run {
                landmarks.push(at - run as usize);
            }
            run = 0;
        }
    }
    if run >= encoding.landmark.min_run {
        landmarks.push(multiples.len() - run as usize);
    }

    // Record starts, and the bit distance between them. Counting bits
    // rather than dividing time is what makes the spacing repeat
    // exactly: it is a property of what was recorded, and immune to the
    // instrument's own speed within one revolution.
    let per_record = encoding.landmark.per_record.max(1) as usize;
    let starts: Vec<usize> = landmarks.iter().copied().step_by(per_record).collect();
    let bits_between = |from: usize, to: usize| -> u64 {
        let mut bits = 0u64;
        for multiple in &multiples[from..to] {
            match multiple {
                Some(multiple) => bits += u64::from(*multiple),
                None => return 0,
            }
        }
        bits
    };
    let mut spacings = Vec::new();
    for pair in starts.windows(2) {
        spacings.push(bits_between(pair[0], pair[1]));
    }
    // The circle is closed: the distance from the last record start
    // round to the first is a spacing like any other, and on a track
    // written in one pass it is the one that departs — the write splice
    // the drive left when it came back round to where it began.
    if starts.len() >= 2 {
        let last = *starts.last().expect("checked");
        let wrap = bits_between(last, multiples.len()) + bits_between(0, starts[0]);
        spacings.push(wrap);
    }

    let (record_bits, deviation) = if spacings.len() >= 2 {
        let mut sorted = spacings.clone();
        sorted.sort_unstable();
        let middle = sorted[sorted.len() / 2];
        let mut apart: Vec<u64> = spacings.iter().map(|bits| bits.abs_diff(middle)).collect();
        apart.sort_unstable();
        (Some(middle), apart[apart.len() / 2])
    } else {
        (None, u64::MAX)
    };

    // The one departure from that spacing is the location's seam. It is
    // reported as an angle in the family's own cycles, never as a byte.
    let seam_cycles = record_bits.and_then(|bits| {
        let (at, _) = spacings
            .iter()
            .enumerate()
            .max_by_key(|(_, spacing)| spacing.abs_diff(bits))?;
        // The departing spacing opens at the record start it runs from,
        // and that angle is the seam. Reported in the family’s own cycles
        // rather than in the instrument ticks it was measured in.
        (spacings[at] != bits).then(|| {
            let position = transitions[starts[at.min(starts.len() - 1)]];
            u64::try_from(
                u128::from(position) * u128::from(profile.rotation.cycles_per_rotation)
                    / u128::from(span.max(1)),
            )
            .unwrap_or(0)
        })
    });

    Some(ObservationReading {
        cell_numerator: cell.0,
        cell_denominator: cell.1,
        resolved_permille: u32::try_from(resolved as u128 * 1000 / intervals.len() as u128)
            .unwrap_or(0),
        landmarks: u32::try_from(landmarks.len()).unwrap_or(u32::MAX),
        record_bits,
        record_bits_deviation: deviation,
        seam_cycles,
        population,
        span,
    })
}

/// What the probe concluded about one source position, before the
/// neighbours were compared.
#[derive(Debug, Clone)]
struct LocationReading {
    key: TrackKey,
    artifact: String,
    family_location: Option<u64>,
    zone: Option<usize>,
    observations: u32,
    agreeing: u32,
    records: u32,
    record_bits: Option<u64>,
    record_bits_deviation: u64,
    seam_cycles: Option<u64>,
    /// The derived cell projected onto the family's nominal rotation, in
    /// thousandths of a reference cycle. Projecting removes the capture
    /// instrument's own speed: one captured revolution is one rotation.
    cell_millicycles: Option<u64>,
    nominal_cell_millicycles: Option<u64>,
    resolved_permille: u32,
    /// Counts of each admitted multiple, the median across observations.
    fingerprint: Vec<u64>,
    /// How far this location's own observations vary from one another.
    self_spread: u64,
    refusal: Option<String>,
}

fn median(values: &mut Vec<u64>) -> u64 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

fn read_location(
    profile: &DriveProfile,
    capture: &FluxCapture,
    key: &TrackKey,
) -> Result<LocationReading> {
    let track = capture
        .track(key)
        .ok_or_else(|| Error::invalid_image(profile.id, "capture supplied no such location"))?;
    let artifact = track
        .runs()
        .first()
        .and_then(|run| capture.envelope().source(run.source()))
        .map_or_else(String::new, |source| source.artifact().to_owned());

    let (numerator, denominator) = key.position().parts();
    let family_location = (denominator == 1)
        .then(|| profile.stepping.location_of(numerator))
        .flatten();
    let zone = family_location.and_then(|location| profile.zone_for(location).map(|(at, _)| at));

    // Each transfer is read back once and its observations sliced out
    // of it, rather than once per observation: the observations of one
    // location are cuts of the same evidence, and reading it five times
    // to make five cuts is work the claim does not need.
    let mut readings = Vec::new();
    for run in track.runs() {
        let transitions = capture.run_transitions(key, run)?;
        for entry in track
            .observations()
            .iter()
            .filter(|entry| entry.source().run_ordinal() == run.ordinal())
        {
            let (start, end) = (entry.source().start(), entry.source().end());
            let inside: Vec<Tick> = transitions
                .iter()
                .copied()
                .filter(|position| *position >= start && *position < end)
                .map(|position| position - start)
                .collect();
            if let Some(reading) = read_observation(profile, entry.span(), &inside) {
                readings.push(reading);
            }
        }
    }

    let observations = u32::try_from(track.observations().len()).unwrap_or(u32::MAX);
    if readings.is_empty() {
        return Ok(LocationReading {
            key: key.clone(),
            artifact,
            family_location,
            zone,
            observations,
            agreeing: 0,
            records: 0,
            record_bits: None,
            record_bits_deviation: u64::MAX,
            seam_cycles: None,
            cell_millicycles: None,
            nominal_cell_millicycles: None,
            resolved_permille: 0,
            fingerprint: Vec::new(),
            self_spread: u64::MAX,
            refusal: Some(
                "no interval population resolves into this family's cell multiples"
                    .to_owned(),
            ),
        });
    }

    let per_record = u64::from(profile.encoding.landmark.per_record.max(1));
    let mut records: Vec<u64> = readings
        .iter()
        .map(|reading| u64::from(reading.landmarks) / per_record)
        .collect();
    let mut bits: Vec<u64> = readings.iter().filter_map(|reading| reading.record_bits).collect();
    let mut deviations: Vec<u64> = readings
        .iter()
        .map(|reading| reading.record_bits_deviation)
        .collect();
    let mut resolved: Vec<u64> = readings
        .iter()
        .map(|reading| u64::from(reading.resolved_permille))
        .collect();

    // The projected cell, in thousandths of a reference cycle:
    // cell_ticks * cycles_per_rotation / span, exactly.
    let mut projected: Vec<u64> = readings
        .iter()
        .map(|reading| {
            let numerator = reading.cell_numerator
                * u128::from(profile.rotation.cycles_per_rotation)
                * 1000;
            let denominator = reading.cell_denominator * u128::from(reading.span.max(1));
            u64::try_from(numerator / denominator.max(1)).unwrap_or(u64::MAX)
        })
        .collect();

    let width = profile.encoding.cell_multiples.len();
    let mut fingerprint = Vec::with_capacity(width);
    let mut self_spread = 0u64;
    for at in 0..width {
        let mut counts: Vec<u64> = readings
            .iter()
            .filter_map(|reading| reading.population.get(at).copied())
            .collect();
        if counts.is_empty() {
            fingerprint.push(0);
            continue;
        }
        let high = counts.iter().max().copied().unwrap_or(0);
        let low = counts.iter().min().copied().unwrap_or(0);
        self_spread += high - low;
        fingerprint.push(median(&mut counts));
    }

    let record_count = median(&mut records);
    // An observation agrees when it produced the same record count and
    // the same bit spacing as the location's median.
    let record_bits = (!bits.is_empty()).then(|| median(&mut bits));
    let agreeing = readings
        .iter()
        .filter(|reading| {
            u64::from(reading.landmarks) / per_record == record_count
                && reading.record_bits == record_bits
        })
        .count();

    let nominal = zone.map(|at| {
        let (numerator, denominator) = profile.density[at].nominal_cell(&profile.rotation);
        u64::try_from(numerator * 1000 / denominator).unwrap_or(u64::MAX)
    });

    Ok(LocationReading {
        key: key.clone(),
        artifact,
        family_location,
        zone,
        observations,
        agreeing: u32::try_from(agreeing).unwrap_or(u32::MAX),
        records: u32::try_from(record_count).unwrap_or(u32::MAX),
        record_bits,
        record_bits_deviation: median(&mut deviations),
        seam_cycles: readings.iter().find_map(|reading| reading.seam_cycles),
        cell_millicycles: Some(median(&mut projected)),
        nominal_cell_millicycles: nominal,
        resolved_permille: u32::try_from(median(&mut resolved)).unwrap_or(0),
        fingerprint,
        self_spread,
        refusal: None,
    })
}

/// How far apart two locations' content is, in transitions.
fn fingerprint_distance(left: &[u64], right: &[u64]) -> u64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left.abs_diff(*right))
        .sum()
}

/// Probes one capture against one profile.
///
/// Every location is read on its own terms first, and only then compared
/// with its neighbours: a location's content matching an adjacent one is
/// an observation in its own right, never something inferred from how
/// many steps the family says make a location. Parity is a statement
/// about addressing and never a statement about which source positions
/// hold distinct content.
pub(crate) fn probe(profile: &'static DriveProfile, capture: &FluxCapture) -> Result<Verdict> {
    let keys: Vec<TrackKey> = capture.tracks().map(|track| track.key().clone()).collect();
    let mut readings = Vec::with_capacity(keys.len());
    for key in &keys {
        readings.push(read_location(profile, capture, key)?);
    }

    // Neighbours are the locations either side on the same surface, in
    // the source's own addressing order.
    let mut by_surface: BTreeMap<Option<u64>, Vec<usize>> = BTreeMap::new();
    for (at, reading) in readings.iter().enumerate() {
        by_surface.entry(reading.key.head()).or_default().push(at);
    }
    let mut duplicate_of: Vec<Option<usize>> = vec![None; readings.len()];
    for group in by_surface.values() {
        for window in group.windows(2) {
            let (left, right) = (window[0], window[1]);
            if readings[left].fingerprint.is_empty() || readings[right].fingerprint.is_empty() {
                continue;
            }
            let apart =
                fingerprint_distance(&readings[left].fingerprint, &readings[right].fingerprint);
            // Two reads of one surface may differ by a transition or two
            // at the cut the bounding made, so the location's own
            // variation is the scale distinctness is judged against —
            // not a threshold picked to make an answer come out.
            let tolerance = readings[left]
                .self_spread
                .max(readings[right].self_spread)
                .saturating_add(2);
            if apart <= tolerance {
                duplicate_of[left] = Some(right);
                duplicate_of[right] = Some(left);
            }
        }
    }

    let mut locations = Vec::with_capacity(readings.len());
    let mut claimed = 0u32;
    for (at, reading) in readings.iter().enumerate() {
        let refusal = reading.refusal.clone().or_else(|| {
            let Some(location) = reading.family_location else {
                return Some(format!(
                    "source position {} is not one this family's addressing covers, \
                     which takes {} steps to a location",
                    position_text(&reading.key),
                    profile.stepping.steps_per_location
                ));
            };
            let Some(zone) = reading.zone else {
                return Some(format!(
                    "location {location} lies outside every zone this family declares"
                ));
            };
            let want = profile.density[zone].records;
            if reading.records != want {
                return Some(format!(
                    "location {location} holds {} records where its zone claims {want}",
                    reading.records
                ));
            }
            if reading.record_bits_deviation != 0 {
                return Some(format!(
                    "the spacing between records at location {location} does not repeat: \
                     it departs from its own median by {} bits",
                    reading.record_bits_deviation
                ));
            }
            if reading.agreeing != reading.observations {
                return Some(format!(
                    "only {} of {} observations of location {location} agree on what it \
                     holds",
                    reading.agreeing, reading.observations
                ));
            }
            None
        });
        if refusal.is_none() {
            claimed += 1;
        }
        locations.push(LocationOutcome {
            reading: reading.clone(),
            duplicate_of: duplicate_of[at].map(|other| readings[other].key.clone()),
            claimed: refusal.is_none(),
            refusal,
        });
    }

    let expected = profile.declared_locations();
    let confidence = if expected == 0 {
        0
    } else {
        u8::try_from((u64::from(claimed) * 100 / expected).min(100)).unwrap_or(100)
    };

    Ok(Verdict {
        profile,
        confidence,
        claimed,
        expected,
        locations,
    })
}

/// One location as the probe left it.
#[derive(Debug, Clone)]
pub(crate) struct LocationOutcome {
    reading: LocationReading,
    duplicate_of: Option<TrackKey>,
    claimed: bool,
    refusal: Option<String>,
}

/// One profile's answer over one capture.
#[derive(Debug, Clone)]
pub(crate) struct Verdict {
    profile: &'static DriveProfile,
    confidence: u8,
    claimed: u32,
    expected: u64,
    locations: Vec<LocationOutcome>,
}

fn position_text(key: &TrackKey) -> String {
    let (numerator, denominator) = key.position().parts();
    if denominator == 1 {
        numerator.to_string()
    } else {
        format!("{numerator}/{denominator}")
    }
}

/// Probes every enrolled profile and ranks what claimed the capture.
///
/// A capture no profile claims is a named refusal, and a lone enrolled
/// entry never wins by being the only one: it must claim the capture on
/// its own evidence.
pub(crate) fn recognize(capture: &FluxCapture) -> Result<Vec<Verdict>> {
    let mut verdicts = Vec::new();
    for profile in enrolled() {
        let verdict = probe(profile, capture)?;
        if verdict.claimed > 0 {
            verdicts.push(verdict);
        }
    }
    if verdicts.is_empty() {
        return Err(Error::categorized_image(
            ErrorCategory::Unsupported,
            "drive-profile",
            format!(
                "no enrolled drive profile claims this capture; {} {} consulted and \
                 none recognized a location it declares",
                enrolled().len(),
                if enrolled().len() == 1 {
                    "was"
                } else {
                    "were"
                }
            ),
        ));
    }
    verdicts.sort_by(|left, right| right.confidence.cmp(&left.confidence));
    Ok(verdicts)
}

/// Probes one named profile, whether or not it would have won.
pub(crate) fn recognize_as(capture: &FluxCapture, id: &str) -> Result<Verdict> {
    let profile = enrolled()
        .iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| {
            Error::categorized_image(
                ErrorCategory::NotFound,
                "drive-profile",
                format!(
                    "'{id}' names no enrolled drive profile; this build enrolls {}",
                    enrolled()
                        .iter()
                        .map(|profile| profile.id)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        })?;
    probe(profile, capture)
}

// ------------------------------------------------------- the reporting

impl Verdict {
    pub(crate) fn profile(&self) -> &'static DriveProfile {
        self.profile
    }

    pub(crate) fn confidence(&self) -> u8 {
        self.confidence
    }

    pub(crate) fn claimed(&self) -> u32 {
        self.claimed
    }

    pub(crate) fn expected(&self) -> u64 {
        self.expected
    }

    pub(crate) fn locations(&self) -> &[LocationOutcome] {
        &self.locations
    }

    /// The observations behind the confidence, in human-readable terms
    /// (P4). A confidence figure with none of these beside it is not an
    /// answer.
    pub(crate) fn evidence(&self) -> Vec<String> {
        let mut evidence = vec![format!(
            "{} claims {} of the {} locations its density map declares",
            self.profile.name, self.claimed, self.expected
        )];
        for (at, zone) in self.profile.density.iter().enumerate() {
            let claimed = self
                .locations
                .iter()
                .filter(|outcome| outcome.claimed && outcome.reading.zone == Some(at))
                .count();
            let records: Vec<u32> = {
                let mut records: Vec<u32> = self
                    .locations
                    .iter()
                    .filter(|outcome| outcome.claimed && outcome.reading.zone == Some(at))
                    .map(|outcome| outcome.reading.records)
                    .collect();
                records.sort_unstable();
                records.dedup();
                records
            };
            evidence.push(format!(
                "zone {at}: locations {}-{} claim {} records each; {claimed} of {} \
                 recovered, holding {}",
                zone.first_location,
                zone.last_location,
                zone.records,
                zone.last_location - zone.first_location + 1,
                match records.as_slice() {
                    [] => "nothing".to_owned(),
                    [one] => format!("{one} records each"),
                    many => format!("{many:?} records"),
                }
            ));
        }
        let duplicates = self
            .locations
            .iter()
            .filter(|outcome| outcome.duplicate_of.is_some())
            .count();
        if duplicates > 0 {
            evidence.push(format!(
                "{duplicates} source positions hold content their neighbour also holds, \
                 which flux alone cannot tell from an instrument that did not move"
            ));
        }
        evidence.push(format!(
            "every declared fact above is the family's, not the capture's: {}",
            self.profile.provenance
        ));
        evidence
    }
}

impl LocationOutcome {
    pub(crate) fn key(&self) -> &TrackKey {
        &self.reading.key
    }

    pub(crate) fn artifact(&self) -> &str {
        &self.reading.artifact
    }

    pub(crate) fn family_location(&self) -> Option<u64> {
        self.reading.family_location
    }

    pub(crate) fn zone(&self) -> Option<usize> {
        self.reading.zone
    }

    pub(crate) fn records(&self) -> u32 {
        self.reading.records
    }

    pub(crate) fn record_bits(&self) -> Option<u64> {
        self.reading.record_bits
    }

    pub(crate) fn record_bits_deviation(&self) -> u64 {
        self.reading.record_bits_deviation
    }

    pub(crate) fn seam_cycles(&self) -> Option<u64> {
        self.reading.seam_cycles
    }

    pub(crate) fn cell_millicycles(&self) -> Option<u64> {
        self.reading.cell_millicycles
    }

    pub(crate) fn nominal_cell_millicycles(&self) -> Option<u64> {
        self.reading.nominal_cell_millicycles
    }

    pub(crate) fn resolved_permille(&self) -> u32 {
        self.reading.resolved_permille
    }

    pub(crate) fn observations(&self) -> u32 {
        self.reading.observations
    }

    pub(crate) fn observations_agreeing(&self) -> u32 {
        self.reading.agreeing
    }

    pub(crate) fn duplicate_of(&self) -> Option<&TrackKey> {
        self.duplicate_of.as_ref()
    }

    pub(crate) fn claimed(&self) -> bool {
        self.claimed
    }

    pub(crate) fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }
}

// ----------------------------------------------------- the public shape

/// One zone as the profile declares it, and what the capture recovered
/// of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneClaim {
    pub first_location: u64,
    pub last_location: u64,
    /// What the family claims one location in this zone holds.
    pub records_declared: u32,
    pub locations_declared: u64,
    pub locations_claimed: u64,
    /// The cell this zone claims, in thousandths of a reference cycle.
    pub nominal_cell_millicycles: u64,
}

/// What the probe found at one source position.
///
/// Every field is an observation, not a conclusion: a count, a density,
/// an angle, an absence. Nothing here names a sector or reads a byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationVerdict {
    /// The member this position was read from.
    pub artifact: String,
    pub position: crate::StepPosition,
    pub head: Option<u64>,
    /// The family location this position addresses, where the family's
    /// addressing covers it at all.
    pub family_location: Option<u64>,
    /// Which declared zone covers that location.
    pub zone: Option<u32>,
    pub records: u32,
    /// The bit distance between record starts, where it repeats.
    pub record_bits: Option<u64>,
    /// How far that spacing departs from its own median. Zero is a
    /// spacing that repeats to the bit.
    pub record_bits_deviation: u64,
    /// The one departure from it, as an angle in reference-clock cycles
    /// — the location's seam, where a reduction may begin its circle.
    pub seam_cycles: Option<u64>,
    /// The derived cell projected onto the family's nominal rotation, in
    /// thousandths of a reference cycle, beside what the zone claims.
    pub cell_millicycles: Option<u64>,
    pub nominal_cell_millicycles: Option<u64>,
    /// How much of the interval population classified, per thousand.
    pub resolved_permille: u32,
    pub observations: u32,
    pub observations_agreeing: u32,
    /// The adjacent position holding the same content, where one does.
    /// Reported, never resolved: flux alone cannot tell a head reading
    /// its neighbour from an instrument that did not move.
    pub duplicate_of: Option<crate::StepPosition>,
    pub claimed: bool,
    /// Why this position was not claimed, in the profile's own terms.
    pub refusal: Option<String>,
}

/// One profile's answer, with the observations that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileVerdict {
    pub profile_id: String,
    pub profile_name: String,
    pub profile_version: u32,
    /// Bounded and comparable, 0 to 100. Never an answer on its own.
    pub confidence: u8,
    pub locations_claimed: u32,
    pub locations_declared: u64,
    pub zones: Vec<ZoneClaim>,
    pub locations: Vec<LocationVerdict>,
    pub evidence: Vec<String>,
}

/// What the enrolled profiles made of one capture, ranked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recognition {
    /// Highest confidence first. Several profiles may claim one
    /// capture, and the ranking is reported rather than resolved.
    pub verdicts: Vec<ProfileVerdict>,
    /// The profile the caller pinned, where one was pinned. What the
    /// library chose travels into the result either way.
    pub pinned: Option<String>,
    pub evidence: Vec<String>,
}

fn step_position(key: &TrackKey) -> crate::StepPosition {
    let (numerator, denominator) = key.position().parts();
    crate::StepPosition {
        numerator,
        denominator,
    }
}

impl Verdict {
    /// The verdict in the shape all three surfaces present it.
    pub(crate) fn report(&self) -> ProfileVerdict {
        let zones = self
            .profile
            .density
            .iter()
            .enumerate()
            .map(|(at, zone)| {
                let (numerator, denominator) = zone.nominal_cell(&self.profile.rotation);
                ZoneClaim {
                    first_location: zone.first_location,
                    last_location: zone.last_location,
                    records_declared: zone.records,
                    locations_declared: zone.last_location - zone.first_location + 1,
                    locations_claimed: self
                        .locations
                        .iter()
                        .filter(|outcome| outcome.claimed && outcome.reading.zone == Some(at))
                        .count() as u64,
                    nominal_cell_millicycles: u64::try_from(numerator * 1000 / denominator)
                        .unwrap_or(u64::MAX),
                }
            })
            .collect();
        ProfileVerdict {
            profile_id: self.profile.id.to_owned(),
            profile_name: self.profile.name.to_owned(),
            profile_version: self.profile.version,
            confidence: self.confidence,
            locations_claimed: self.claimed,
            locations_declared: self.expected,
            zones,
            locations: self
                .locations
                .iter()
                .map(|outcome| LocationVerdict {
                    artifact: outcome.reading.artifact.clone(),
                    position: step_position(&outcome.reading.key),
                    head: outcome.reading.key.head(),
                    family_location: outcome.reading.family_location,
                    zone: outcome.reading.zone.map(|at| at as u32),
                    records: outcome.reading.records,
                    record_bits: outcome.reading.record_bits,
                    record_bits_deviation: outcome.reading.record_bits_deviation,
                    seam_cycles: outcome.reading.seam_cycles,
                    cell_millicycles: outcome.reading.cell_millicycles,
                    nominal_cell_millicycles: outcome.reading.nominal_cell_millicycles,
                    resolved_permille: outcome.reading.resolved_permille,
                    observations: outcome.reading.observations,
                    observations_agreeing: outcome.reading.agreeing,
                    duplicate_of: outcome.duplicate_of.as_ref().map(step_position),
                    claimed: outcome.claimed,
                    refusal: outcome.refusal.clone(),
                })
                .collect(),
            evidence: self.evidence(),
        }
    }
}

/// Recognizes a capture against every enrolled profile, or against one
/// the caller pinned.
pub(crate) fn recognition(capture: &FluxCapture, pinned: Option<&str>) -> Result<Recognition> {
    match pinned {
        Some(id) => {
            let verdict = recognize_as(capture, id)?;
            Ok(Recognition {
                evidence: vec![format!(
                    "profile '{id}' was pinned by the caller, so it was consulted \
                     whether or not it would have won the ranking"
                )],
                verdicts: vec![verdict.report()],
                pinned: Some(id.to_owned()),
            })
        }
        None => {
            let verdicts = recognize(capture)?;
            let evidence = vec![format!(
                "{} enrolled drive {} consulted; {} claimed the capture, ranked by \
                 confidence",
                enrolled().len(),
                if enrolled().len() == 1 {
                    "profile was"
                } else {
                    "profiles were"
                },
                verdicts.len()
            )];
            Ok(Recognition {
                verdicts: verdicts.iter().map(Verdict::report).collect(),
                pinned: None,
                evidence,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_c1541_entry_declares_the_family_and_derives_nothing_from_a_capture() {
        // Every value here is a published fact of the drive. None is a
        // number a capture is permitted to establish.
        assert_eq!(C1541.stepping.steps_per_location, 2);
        assert_eq!(C1541.stepping.first_location, 1);
        assert_eq!(C1541.rotation.cycles_per_rotation, 3_200_000);
        assert!(!C1541.rotation.index_observed_by_drive);
        assert_eq!(C1541.surfaces.recorded, 1);
        assert_eq!(C1541.encoding.cell_multiples, &[1, 2, 3]);
        assert_eq!(C1541.encoding.landmark.min_run, 9);
        assert_eq!(C1541.encoding.landmark.per_record, 2);
        assert_eq!(C1541.declared_locations(), 35);

        // The four documented zones, at their documented boundaries.
        let zones: Vec<(u64, u64, u32)> = C1541
            .density
            .iter()
            .map(|zone| (zone.first_location, zone.last_location, zone.records))
            .collect();
        assert_eq!(zones, [(1, 17, 21), (18, 24, 19), (25, 30, 18), (31, 35, 17)]);

        // And each zone's cell follows from its rate and the clock,
        // exactly: 52, 56, 60 and 64 cycles.
        let cells: Vec<u128> = C1541
            .density
            .iter()
            .map(|zone| {
                let (numerator, denominator) = zone.nominal_cell(&C1541.rotation);
                numerator / denominator
            })
            .collect();
        assert_eq!(cells, [52, 56, 60, 64]);
    }

    #[test]
    fn the_drive_that_observes_no_index_does_not_inherit_the_captures() {
        // A 1541 has no index sensor, so a medium reduced for one cannot
        // honestly begin its circle at a datum the drive never sees.
        assert!(!C1541.rotation.index_observed_by_drive);
        assert_eq!(C1541.materialization.origin, OriginDefault::LongestGap);
    }

    #[test]
    fn a_duplicate_is_refused_by_declaration_rather_than_resolved() {
        assert_eq!(C1541.materialization.duplicate, DuplicateRule::Refuse);
        assert!(C1541.materialization.unmapped_source_position_refused);
    }

    #[test]
    fn addressing_maps_whole_steps_and_covers_no_half_step() {
        // Parity is a statement about the family's addressing. It is
        // never a statement about which source positions hold content.
        assert_eq!(C1541.stepping.location_of(0), Some(1));
        assert_eq!(C1541.stepping.location_of(2), Some(2));
        assert_eq!(C1541.stepping.location_of(68), Some(35));
        assert_eq!(C1541.stepping.location_of(1), None);
        assert_eq!(C1541.stepping.location_of(67), None);
    }

    #[test]
    fn a_zone_covers_what_it_declares_and_nothing_past_it() {
        assert_eq!(C1541.zone_for(1).map(|(at, _)| at), Some(0));
        assert_eq!(C1541.zone_for(17).map(|(at, _)| at), Some(0));
        assert_eq!(C1541.zone_for(18).map(|(at, _)| at), Some(1));
        assert_eq!(C1541.zone_for(35).map(|(at, _)| at), Some(3));
        // Tracks past 35 are outside every zone the family declares, so
        // a capture of them is refused by declaration rather than by a
        // threshold.
        assert!(C1541.zone_for(36).is_none());
    }

    /// Intervals of one, two and three cells of `cell` ticks, in a
    /// pattern with `syncs` runs of `run` shortest intervals.
    fn synthetic(cell: u64, count: usize, syncs: usize, run: usize) -> Vec<Tick> {
        let mut intervals = Vec::new();
        let every = count / syncs.max(1);
        for index in 0..count {
            if index % every < run {
                intervals.push(cell);
            } else {
                intervals.push(cell * (1 + (index % 3) as u64));
            }
        }
        let mut transitions = Vec::with_capacity(intervals.len() + 1);
        let mut at = 0;
        for interval in intervals {
            at += interval;
            transitions.push(at);
        }
        transitions
    }

    #[test]
    fn a_cell_is_derived_from_the_population_it_is_self_consistent_with() {
        let transitions = synthetic(64, 4000, 40, 12);
        let intervals: Vec<Tick> = transitions.windows(2).map(|p| p[1] - p[0]).collect();
        let (numerator, denominator) =
            derive_cell(&intervals, &C1541.encoding).expect("the population resolves");

        // Exactly 64, as a rational, with nothing rounded through a
        // float on the way.
        assert_eq!(numerator / denominator, 64);
        assert_eq!(numerator % denominator, 0);
    }

    #[test]
    fn a_solution_leaving_its_shortest_multiple_empty_is_rejected_and_re_derived() {
        // Short outliers drag the seed down to half the true cell. In
        // that solution every real interval classifies one step too
        // high, so the shortest multiple is left unpopulated — which is
        // what the declared multiples make detectable. Without the
        // guard the half-cell would be reported as a finding about the
        // disk rather than as one about the probe.
        let mut intervals: Vec<Tick> = Vec::new();
        for index in 0..2000u64 {
            intervals.push(64 * (1 + index % 3));
        }
        // A tenth of the population at half a cell, ahead of everything
        // real once sorted.
        for _ in 0..220 {
            intervals.push(32);
        }
        let mut ordered = intervals.clone();
        ordered.sort_unstable();
        assert_eq!(ordered[ordered.len() / 20], 32, "the seed is the half-cell");

        let (numerator, denominator) =
            derive_cell(&intervals, &C1541.encoding).expect("the population resolves");
        assert_eq!(
            numerator / denominator,
            64,
            "the half-cell was reported as real"
        );
    }

    #[test]
    fn classification_is_exact_integer_arithmetic_inside_the_declared_band() {
        let cell = (64u128, 1u128);
        // Three tenths of a cell either side, and nothing past it.
        assert_eq!(classify(64, cell, &C1541.encoding), Some(1));
        assert_eq!(classify(83, cell, &C1541.encoding), Some(1));
        assert_eq!(classify(109, cell, &C1541.encoding), Some(2));
        assert_eq!(classify(192, cell, &C1541.encoding), Some(3));
        assert_eq!(classify(96, cell, &C1541.encoding), None);
        // Four cells is past what GCR produces, so it is unresolved
        // rather than admitted as a fourth multiple.
        assert_eq!(classify(256, cell, &C1541.encoding), None);
    }

    #[test]
    fn a_profile_the_build_does_not_enroll_is_refused_by_name() {
        assert_eq!(enrolled().len(), 1);
        assert_eq!(enrolled()[0].id, "c1541");
    }
}
