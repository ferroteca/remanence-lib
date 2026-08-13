// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Recognition from interval statistics alone.
//!
//! The cell is derived as the rational the interval population is
//! self-consistent with — a comb periodogram over the sample, scored by
//! how cleanly the intervals fall onto its multiples — and every
//! interval is then classified into a declared multiple by exact
//! integer arithmetic. Nothing here rounds through a float, and a
//! solution that leaves its shortest multiple empty is rejected and
//! re-derived rather than accepted as the best available.
//!
//! **Recognition stops at structure.** What leaves this file is a
//! count, a density, an angle, a location and an absence — never a
//! resolved bit, an assembled byte, a named sector or a validated
//! checksum. Reaching those would make every recognition depend on a
//! clock-recovery model.

use crate::error::{Error, Result};
use crate::flux::capture::{FluxCapture, Tick, TrackKey};

use super::*;

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
    let intervals: Vec<Tick> = transitions
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect();
    let cell = derive_cell(&intervals, &profile.encoding)?;
    let encoding = &profile.encoding;

    let multiples: Vec<Option<u32>> = intervals
        .iter()
        .map(|&interval| classify(interval, cell, encoding))
        .collect();
    let resolved = multiples
        .iter()
        .filter(|multiple| multiple.is_some())
        .count();
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
pub(super) struct LocationReading {
    pub(super) key: TrackKey,
    pub(super) artifact: String,
    pub(super) family_location: Option<u64>,
    pub(super) zone: Option<usize>,
    pub(super) observations: u32,
    pub(super) agreeing: u32,
    pub(super) records: u32,
    pub(super) record_bits: Option<u64>,
    pub(super) record_bits_deviation: u64,
    pub(super) seam_cycles: Option<u64>,
    /// The derived cell projected onto the family's nominal rotation, in
    /// thousandths of a reference cycle. Projecting removes the capture
    /// instrument's own speed: one captured revolution is one rotation.
    pub(super) cell_millicycles: Option<u64>,
    pub(super) nominal_cell_millicycles: Option<u64>,
    pub(super) resolved_permille: u32,
    /// Counts of each admitted multiple, the median across observations.
    pub(super) fingerprint: Vec<u64>,
    /// How far this location's own observations vary from one another.
    pub(super) self_spread: u64,
    pub(super) refusal: Option<String>,
}

fn median(values: &mut Vec<u64>) -> u64 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

pub(super) fn read_location(
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
                "no interval population resolves into this family's cell multiples".to_owned(),
            ),
        });
    }

    let per_record = u64::from(profile.encoding.landmark.per_record.max(1));
    let mut records: Vec<u64> = readings
        .iter()
        .map(|reading| u64::from(reading.landmarks) / per_record)
        .collect();
    let mut bits: Vec<u64> = readings
        .iter()
        .filter_map(|reading| reading.record_bits)
        .collect();
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
            let numerator =
                reading.cell_numerator * u128::from(profile.rotation.cycles_per_rotation) * 1000;
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
pub(super) fn fingerprint_distance(left: &[u64], right: &[u64]) -> u64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left.abs_diff(*right))
        .sum()
}

pub(super) fn position_text(key: &TrackKey) -> String {
    let (numerator, denominator) = key.position().parts();
    if denominator == 1 {
        numerator.to_string()
    } else {
        format!("{numerator}/{denominator}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
