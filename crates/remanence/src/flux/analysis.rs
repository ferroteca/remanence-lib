// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The numeric core of the gap-first reconstruction (F65), over plain
//! arrays and nothing else: the cell lattice measured from a capture's
//! own intervals, the gap correspondence that aligns revolutions, the
//! gap-first integration that turns aligned revolutions into angles,
//! the coherence rule, the fat-track comparison, and the clock
//! recovered from a finished orbit.
//!
//! Ported from the owner's flux-capture research implementation, where
//! every constant below was measured against real captures; the design
//! record is [`planning/pledged/design/remanence-flux-layer.md`]. The
//! boundary this module keeps is the lineage's own: **floats measure,
//! integers state.** Analysis internals run in `f64`; everything
//! declared or stored stays integer or exact rational.
//!
//! Angles and intervals arrive here already normalized to the
//! remanence image's angular unit — divisions of 2²⁸ per revolution —
//! as `i64`, with `-1` marking a transition a revolution did not see.
#![allow(dead_code)]

use crate::flux::remanence::image::ANGULAR_DIVISIONS;

/// The research lineage's reference revolution, in KryoFlux sample
/// ticks: the mean over every revolution of every captured position of
/// the reference capture. The measured tolerances below were stated in
/// those ticks; this converts them into the angular unit once, at
/// declaration.
const REFERENCE_REVOLUTION_TICKS: i64 = 4_006_332;

pub(crate) fn in_divisions(ticks: i64) -> i64 {
    (i128::from(ticks) * i128::from(ANGULAR_DIVISIONS) / i128::from(REFERENCE_REVOLUTION_TICKS))
        as i64
}

pub(crate) fn in_divisions_f(ticks: f64) -> f64 {
    ticks * ANGULAR_DIVISIONS as f64 / REFERENCE_REVOLUTION_TICKS as f64
}

/// How many cell multiples the lattice models.
pub(crate) const RUNS: usize = 6;
/// A context's median is believed only over this many witnesses.
const WITNESSES: usize = 64;

/// The cell lattice one orbit's intervals imply: the cell length from
/// a comb periodogram, where each run length lands on average, the
/// same conditioned on the neighbouring run lengths and the read
/// channel's alternation parity, and the comb's own confidence.
///
/// The contextual medians are what carry the reader's peak shift: an
/// interval's displacement depends on the intervals beside it, and the
/// median over every occurrence of that context measures the
/// displacement so the gap-first integration can remove it.
#[derive(Debug, Clone)]
pub(crate) struct CellLattice {
    cell_ticks: f64,
    positions: [f64; RUNS + 1],
    contextual: Vec<f64>,
    alternation: Vec<f64>,
    confidence: f64,
}

fn context_key(prev: usize, this: usize, next: usize) -> usize {
    (prev * (RUNS + 1) + this) * (RUNS + 1) + next
}

impl CellLattice {
    /// The plausible cell range, in divisions: the lineage's 30..140
    /// reference ticks. Bounds, step and window rescale together — a
    /// documented trap.
    fn shortest() -> f64 {
        in_divisions_f(30.0)
    }

    fn longest() -> f64 {
        in_divisions_f(140.0)
    }

    pub(crate) fn measure(intervals: &[i64]) -> CellLattice {
        // Coarse then fine: the comb's peak is broad enough that a
        // half-tick step cannot step over it.
        let coarse = in_divisions_f(0.5);
        let window = in_divisions_f(1.0);
        let fine = in_divisions_f(0.02);
        let mut best_cell = Self::peak(intervals, Self::shortest(), Self::longest(), coarse);
        best_cell = Self::peak(intervals, best_cell - window, best_cell + window, fine);
        let best = Self::score(intervals, best_cell);

        // First pass: where each run length lands on average, ignoring
        // context — the classifier, and the fallback for contexts too
        // rare to measure. Median, not mean: adjacent clusters' tails
        // overlap, and a mean is dragged toward its neighbour.
        let mut by_run: Vec<Vec<i64>> = vec![Vec::new(); RUNS + 1];
        for &interval in intervals {
            let run = (interval as f64 / best_cell).round() as i64;
            if (1..=RUNS as i64).contains(&run) {
                by_run[run as usize].push(interval);
            }
        }
        let mut positions = [0.0; RUNS + 1];
        for run in 1..=RUNS {
            if by_run[run].is_empty() {
                // A run nobody wrote falls back to the uniform
                // prediction; nothing will ever select it.
                positions[run] = run as f64 * best_cell;
                continue;
            }
            by_run[run].sort_unstable();
            positions[run] = by_run[run][by_run[run].len() / 2] as f64;
        }

        // Second pass: the same medians, conditioned on the
        // neighbouring run lengths and on this stream's own index
        // parity — which means nothing outside this measurement; see
        // `fit_phase`.
        let cells = (RUNS + 1) * (RUNS + 1) * (RUNS + 1);
        let mut by_context: Vec<Vec<i64>> = vec![Vec::new(); cells * 2];
        for i in 1..intervals.len().saturating_sub(1) {
            let prev = run_of(intervals[i - 1] as f64, &positions);
            let this = run_of(intervals[i] as f64, &positions);
            let next = run_of(intervals[i + 1] as f64, &positions);
            if prev < 1 || this < 1 || next < 1 {
                continue;
            }
            by_context[context_key(prev as usize, this as usize, next as usize) * 2 + i % 2]
                .push(intervals[i]);
        }
        let mut contextual = vec![f64::NAN; cells];
        let mut alternation = vec![f64::NAN; cells];
        for prev in 1..=RUNS {
            for this in 1..=RUNS {
                for next in 1..=RUNS {
                    let key = context_key(prev, this, next);
                    let even = median_of(&mut by_context[key * 2]);
                    let odd = median_of(&mut by_context[key * 2 + 1]);
                    match (even, odd) {
                        (Some(even), Some(odd)) => {
                            contextual[key] = (even + odd) / 2.0;
                            alternation[key] = (odd - even) / 2.0;
                        }
                        // One phase alone still locates the context; it
                        // just cannot say how far the other sits from it.
                        (Some(one), None) | (None, Some(one)) => contextual[key] = one,
                        (None, None) => {}
                    }
                }
            }
        }

        CellLattice {
            cell_ticks: best_cell,
            positions,
            contextual,
            alternation,
            confidence: best / (intervals.len().max(1) as f64),
        }
    }

    fn peak(intervals: &[i64], from: f64, to: f64, step: f64) -> f64 {
        let mut best = -f64::MAX;
        let mut best_cell = Self::shortest().max(from);
        let mut cell = Self::shortest().max(from);
        let limit = Self::longest().min(to);
        while cell <= limit {
            let score = Self::score(intervals, cell);
            if score > best {
                best = score;
                best_cell = cell;
            }
            cell += step;
        }
        best_cell
    }

    /// The comb: an interval on the lattice contributes +1, one half
    /// way off contributes −1, and everything else lands in between.
    fn score(intervals: &[i64], cell: f64) -> f64 {
        intervals
            .iter()
            .map(|&interval| (2.0 * std::f64::consts::PI * interval as f64 / cell).cos())
            .sum()
    }

    pub(crate) fn cell_ticks(&self) -> f64 {
        self.cell_ticks
    }

    pub(crate) fn confidence(&self) -> f64 {
        self.confidence
    }

    /// Which run length covers `gap` — the nearest measured position.
    pub(crate) fn cells_in(&self, gap: f64) -> i64 {
        run_of(gap, &self.positions)
    }

    /// What the medium holds for a run of `cells`: the uniform lattice
    /// prediction.
    pub(crate) fn ideal_of(&self, cells: i64) -> f64 {
        cells as f64 * self.cell_ticks
    }

    /// What a capture should report for a run in this context — the
    /// medium's position plus the reader's displacement.
    pub(crate) fn expected_in(&self, prev: i64, this: i64, next: i64) -> f64 {
        if !(1..=RUNS as i64).contains(&this) {
            return self.ideal_of(this);
        }
        if (1..=RUNS as i64).contains(&prev) && (1..=RUNS as i64).contains(&next) {
            let measured =
                self.contextual[context_key(prev as usize, this as usize, next as usize)];
            if !measured.is_nan() {
                return measured;
            }
        }
        self.positions[this as usize]
    }

    /// [`Self::expected_in`] with the alternation phase applied.
    pub(crate) fn expected_in_phase(&self, prev: i64, this: i64, next: i64, phase: usize) -> f64 {
        let pooled = self.expected_in(prev, this, next);
        if !(1..=RUNS as i64).contains(&this)
            || !(1..=RUNS as i64).contains(&prev)
            || !(1..=RUNS as i64).contains(&next)
        {
            return pooled;
        }
        let swing = self.alternation[context_key(prev as usize, this as usize, next as usize)];
        if swing.is_nan() {
            pooled
        } else if phase == 1 {
            pooled + swing
        } else {
            pooled - swing
        }
    }

    /// Recovers the one alternation-parity bit for a gap sequence in
    /// its own numbering: the lattice measured the swing against its
    /// own stream, and nothing relates that to where alignment put
    /// transition zero, so the bit is fitted rather than assumed —
    /// applied backwards it would double the error it removes.
    pub(crate) fn fit_phase(&self, gaps: &[f64], runs: &[i64]) -> usize {
        let n = gaps.len();
        let mut cost = [0.0f64; 2];
        for (phase, total) in cost.iter_mut().enumerate() {
            for i in 0..n {
                let prev = runs[(i + n - 1) % n];
                let next = runs[(i + 1) % n];
                *total +=
                    (gaps[i] - self.expected_in_phase(prev, runs[i], next, (i + phase) % 2)).abs();
            }
        }
        if cost[0] <= cost[1] { 0 } else { 1 }
    }
}

fn run_of(gap: f64, positions: &[f64; RUNS + 1]) -> i64 {
    let mut best = -1i64;
    let mut best_distance = f64::MAX;
    for run in 1..=RUNS {
        let distance = (gap - positions[run]).abs();
        if distance < best_distance {
            best_distance = distance;
            best = run as i64;
        }
    }
    best
}

fn median_of(bucket: &mut Vec<i64>) -> Option<f64> {
    if bucket.len() < WITNESSES {
        return None;
    }
    bucket.sort_unstable();
    Some(bucket[bucket.len() / 2] as f64)
}

/// The correspondence between two circular transition sequences,
/// decided in the gap domain: identity lives in the interval sequence,
/// angles only position it.
pub(crate) mod correspondence {
    use super::in_divisions;

    /// Two gaps are the same gap within this much.
    pub(crate) fn tolerance() -> i64 {
        in_divisions(6)
    }

    /// The resynchronising walk's looser budget.
    pub(crate) fn walk_tolerance() -> i64 {
        in_divisions(24)
    }

    const WINDOW: usize = 96;
    const SAMPLES: usize = 9;
    const MINIMUM_SYMBOLS: usize = 3;
    const RESYNC_CONFIRM: usize = 6;
    const CANDIDATE_LIMIT: usize = 64;
    const CANDIDATES: usize = 4;

    /// The circular gap sequence of a set of angles, the last gap
    /// wrapping to the first angle one revolution on.
    pub(crate) fn gaps(angles: &[i64], divisions: i64) -> Vec<i64> {
        (0..angles.len())
            .map(|i| {
                let next = if i + 1 < angles.len() {
                    angles[i + 1]
                } else {
                    angles[0] + divisions
                };
                next - angles[i]
            })
            .collect()
    }

    /// Candidate rotations of `b` against `a`, voted by sampled
    /// windows. A window must be informative — enough distinct gap
    /// values to mean something — and every full match votes for the
    /// global start it implies; a candidate needs half the windows.
    pub(crate) fn candidate_starts(a: &[i64], b: &[i64], tolerance: i64) -> Vec<usize> {
        if a.len() < 500 || b.len() < 500 {
            return Vec::new();
        }
        let mut votes: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
        let mut windows = 0;
        for s in 0..SAMPLES {
            let from = (s + 1) * (a.len() - WINDOW - 1) / (SAMPLES + 1);
            if !informative(a, from, tolerance) {
                continue;
            }
            let offsets = full_matches(a, from, b, tolerance);
            if offsets.is_empty() || offsets.len() > CANDIDATE_LIMIT {
                continue;
            }
            windows += 1;
            let starts: std::collections::BTreeSet<usize> = offsets
                .into_iter()
                .map(|offset| (offset + b.len() - from % b.len()) % b.len())
                .collect();
            for start in starts {
                *votes.entry(start).or_insert(0) += 1;
            }
        }
        if windows == 0 {
            return Vec::new();
        }
        let mut ranked: Vec<(usize, usize)> = votes
            .into_iter()
            .map(|(start, count)| (count, start))
            .collect();
        // Most votes first; ties resolve by start so the outcome is a
        // pure function of the input.
        ranked.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        ranked
            .into_iter()
            .take_while(|(count, _)| count * 2 >= windows)
            .take(CANDIDATES)
            .map(|(_, start)| start)
            .collect()
    }

    fn full_matches(a: &[i64], from: usize, b: &[i64], tolerance: i64) -> Vec<usize> {
        let mut found = Vec::new();
        for offset in 0..b.len() {
            let mut hits = 0;
            for i in 0..WINDOW {
                if (a[from + i] - b[(offset + i) % b.len()]).abs() <= tolerance {
                    hits += 1;
                } else {
                    break;
                }
            }
            if hits >= WINDOW {
                found.push(offset);
                if found.len() > CANDIDATE_LIMIT {
                    return found;
                }
            }
        }
        found
    }

    fn informative(gaps: &[i64], from: usize, tolerance: i64) -> bool {
        let mut symbols: Vec<i64> = Vec::new();
        for &gap in gaps.iter().skip(from).take(WINDOW) {
            if !symbols
                .iter()
                .any(|&symbol| (gap - symbol).abs() <= tolerance)
            {
                symbols.push(gap);
            }
        }
        symbols.len() >= MINIMUM_SYMBOLS
    }

    /// Walks `other` along `reference`, pairing transitions by gap
    /// agreement and resynchronising where one sequence resolved a
    /// reversal the other did not. A candidate repair must prove
    /// itself over [`RESYNC_CONFIRM`] following gaps: GCR gaps come
    /// from so small an alphabet that a single coincidence would
    /// otherwise look like a resync, and one wrong shift
    /// desynchronises everything after it. Returns, per reference
    /// transition, the index in `other` or `-1`.
    pub(crate) fn match_indices(reference: &[i64], other: &[i64], tolerance: i64) -> Vec<i64> {
        let mut matched = vec![-1i64; reference.len()];
        let mut i = 0usize;
        let mut j = 0usize;
        while i < reference.len() && j < other.len() {
            matched[i] = j as i64;
            if i + 1 >= reference.len() || j + 1 >= other.len() {
                break;
            }
            if gaps_agree(reference, i, other, j, tolerance) {
                i += 1;
                j += 1;
                continue;
            }
            let skip_other = resync_score(reference, i + 1, other, j + 2, tolerance);
            let skip_reference = resync_score(reference, i + 2, other, j + 1, tolerance);
            let neither = resync_score(reference, i + 1, other, j + 1, tolerance);
            let best = neither.max(skip_other).max(skip_reference);
            if best < RESYNC_CONFIRM - 1 {
                i += 1;
                j += 1;
            } else if skip_other == best {
                // The other sequence resolved a reversal the reference
                // did not; its extra one at j+1 has no counterpart.
                i += 1;
                j += 2;
            } else if skip_reference == best {
                // The reference resolved one the other missed, so
                // reference[i+1] stays unmatched — the instability.
                i += 2;
                j += 1;
            } else {
                i += 1;
                j += 1;
            }
        }
        matched
    }

    fn resync_score(reference: &[i64], i: usize, other: &[i64], j: usize, tolerance: i64) -> usize {
        let mut agreed = 0;
        while agreed < RESYNC_CONFIRM
            && i + agreed + 1 < reference.len()
            && j + agreed + 1 < other.len()
            && gaps_agree(reference, i + agreed, other, j + agreed, tolerance)
        {
            agreed += 1;
        }
        agreed
    }

    fn gaps_agree(reference: &[i64], i: usize, other: &[i64], j: usize, tolerance: i64) -> bool {
        ((reference[i + 1] - reference[i]) - (other[j + 1] - other[j])).abs() <= tolerance
    }
}

/// The coherence rule: when does a set of revolutions assert a
/// transition, and how long a run of failures is indeterminate medium
/// rather than marginal readings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Coherence {
    /// Sightings of one transition must sit within this many divisions.
    pub(crate) angular_tolerance: i64,
    /// The fraction of revolutions that must have seen it.
    pub(crate) minimum_agreement: (u32, u32),
    /// This many consecutive failures is the background, not noise.
    pub(crate) indeterminate_run: usize,
}

impl Coherence {
    /// The lineage's measured calibration: intra-revolution wander
    /// reaches hundreds of ticks and noise tens of thousands, so 2000
    /// ticks separates them with a decade of margin either side; 3/4
    /// of revolutions; a run of 32.
    pub(crate) fn measured() -> Coherence {
        Coherence {
            angular_tolerance: in_divisions(2000),
            minimum_agreement: (3, 4),
            indeterminate_run: 32,
        }
    }

    pub(crate) fn revolutions_required(&self, revolutions: usize) -> usize {
        let (numerator, denominator) = self.minimum_agreement;
        let required = (revolutions as u64 * u64::from(numerator)).div_ceil(u64::from(denominator));
        revolutions.min(required as usize)
    }

    pub(crate) fn coheres(&self, seen: usize, spread: i64, revolutions: usize) -> bool {
        seen >= self.revolutions_required(revolutions) && spread <= self.angular_tolerance
    }

    pub(crate) fn span_is_indeterminate(&self, consecutive_failures: usize) -> bool {
        consecutive_failures >= self.indeterminate_run
    }
}

/// What the gap-first integration produced for one orbit: the angles,
/// which intervals were kept off the lattice, the cell the closed
/// revolution implies, and the warp curve's harmonics as a report.
#[derive(Debug, Clone)]
pub(crate) struct GapFirstAngles {
    pub(crate) angles: Vec<i64>,
    pub(crate) off_lattice: Vec<usize>,
    pub(crate) cells: i64,
    pub(crate) implied_cell: f64,
    pub(crate) warp_harmonics: Vec<f64>,
}

/// How many harmonics of the revolution the warp report carries.
const WARP_HARMONICS: usize = 5;

impl GapFirstAngles {
    /// An interval is kept off the lattice only when it departs it by
    /// more than this *and* is consistent across revolutions within it.
    fn consistent() -> f64 {
        in_divisions_f(4.0)
    }

    /// The gap-first integration over aligned revolutions: mean each
    /// gap across the revolutions that saw both ends, remove the
    /// reader's contextual displacement, classify to whole cells,
    /// keep what the medium holds off-lattice, re-solve the cell so
    /// closure is exact, and integrate from `origin`.
    pub(crate) fn from(
        matched: &[Vec<i64>],
        divisions: i64,
        lattice: &CellLattice,
        origin: i64,
    ) -> GapFirstAngles {
        let n = matched[0].len();
        let mut mean_gap = vec![0.0f64; n];
        let mut spread = vec![0.0f64; n];
        for i in 0..n {
            let mut sum = 0.0;
            let mut low = f64::MAX;
            let mut high = -f64::MAX;
            let mut seen = 0;
            for revolution in matched {
                let next = (i + 1) % n;
                if revolution[i] < 0 || revolution[next] < 0 {
                    continue;
                }
                let mut gap = (revolution[next] - revolution[i]) as f64;
                if gap < 0.0 {
                    gap += divisions as f64;
                }
                sum += gap;
                low = low.min(gap);
                high = high.max(gap);
                seen += 1;
            }
            mean_gap[i] = if seen > 0 {
                sum / seen as f64
            } else {
                lattice.cell_ticks()
            };
            spread[i] = if seen > 1 { high - low } else { f64::MAX };
        }

        // Classification first, for the whole orbit: which multiple an
        // interval is survives a few ticks of displacement easily,
        // since the decision is made at half-cell resolution and peak
        // shift is a tenth of a cell.
        let run: Vec<i64> = mean_gap.iter().map(|&gap| lattice.cells_in(gap)).collect();
        let phase = lattice.fit_phase(&mean_gap, &run);

        // The reader's contribution, removed from every interval
        // whether or not it ends up on the grid. What is left is the
        // medium speaking.
        let mut corrected = vec![0.0f64; n];
        let mut cells: i64 = 0;
        for i in 0..n {
            let prev = run[(i + n - 1) % n];
            let next = run[(i + 1) % n];
            corrected[i] = mean_gap[i]
                - (lattice.expected_in_phase(prev, run[i], next, (i + phase) % 2)
                    - lattice.ideal_of(run[i]));
            cells += run[i];
        }

        // The closed revolution solves the cell exactly, in the right
        // unit: the periodogram's hundredth-of-a-tick error, integrated
        // over a revolution, would be hundreds of cells of drift.
        let mut implied = if cells > 0 {
            divisions as f64 / cells as f64
        } else {
            lattice.cell_ticks()
        };
        let mut refused = Vec::new();
        for i in 0..n {
            // Consistent across revolutions but off the lattice: the
            // medium holds something a crystal clock did not write.
            // Keep it where it is and say so.
            if (corrected[i] - run[i] as f64 * implied).abs() > Self::consistent()
                && spread[i] <= Self::consistent()
            {
                refused.push(i);
            }
        }
        // Off-lattice intervals occupy angle the crystal did not
        // allocate, so the cell is solved again over what remains and
        // integration accumulates nothing at all.
        let off: std::collections::BTreeSet<usize> = refused.iter().copied().collect();
        let mut off_span = 0.0f64;
        let mut snapped_cells: i64 = 0;
        for i in 0..n {
            if off.contains(&i) {
                off_span += corrected[i];
            } else {
                snapped_cells += run[i];
            }
        }
        if snapped_cells > 0 {
            implied = (divisions as f64 - off_span) / snapped_cells as f64;
        }
        let resolved: Vec<f64> = (0..n)
            .map(|i| {
                if off.contains(&i) {
                    corrected[i]
                } else {
                    run[i] as f64 * implied
                }
            })
            .collect();

        // The warp curve — measured minus crystal, integrated — is the
        // timebase wander the snapping corrected: capture spindle plus
        // writer spindle together, reported and never attributed.
        let mut angles = vec![0i64; n];
        let mut running = origin as f64;
        let mut warp = 0.0f64;
        let mut harmonic_sums = vec![0.0f64; 2 * WARP_HARMONICS];
        for i in 0..n {
            let wrapped = (running.round() as i64).rem_euclid(divisions);
            angles[i] = wrapped;
            let theta = 2.0 * std::f64::consts::PI * wrapped as f64 / divisions as f64;
            for h in 1..=WARP_HARMONICS {
                harmonic_sums[2 * (h - 1)] += warp * (h as f64 * theta).cos();
                harmonic_sums[2 * (h - 1) + 1] += warp * (h as f64 * theta).sin();
            }
            running += resolved[i];
            warp += corrected[i] - resolved[i];
        }
        let mut warp_harmonics = vec![0.0f64; 2 * WARP_HARMONICS];
        if n > 0 {
            for h in 0..WARP_HARMONICS {
                let re = 2.0 * harmonic_sums[2 * h] / n as f64;
                let im = 2.0 * harmonic_sums[2 * h + 1] / n as f64;
                warp_harmonics[2 * h] = re.hypot(im);
                warp_harmonics[2 * h + 1] = im.atan2(re);
            }
        }

        // Rounding can tie two neighbours a fraction of a division
        // apart; an orbit's angles must strictly ascend.
        for i in 1..n {
            if angles[i] <= angles[i - 1] {
                angles[i] = angles[i - 1] + 1;
            }
        }

        GapFirstAngles {
            angles,
            off_lattice: refused,
            cells,
            implied_cell: implied,
            warp_harmonics,
        }
    }
}

/// The clock a finished orbit implies, recovered from its own
/// coherent angles: circular intervals, the lattice they imply, and
/// the revolution divided by the total run count.
pub(crate) fn cell_of(angles: &[i64], divisions: i64) -> f64 {
    if angles.len() < 2 {
        return 0.0;
    }
    let intervals: Vec<i64> = (0..angles.len())
        .map(|i| {
            let interval = angles[(i + 1) % angles.len()] - angles[i];
            if interval > 0 {
                interval
            } else {
                interval + divisions
            }
        })
        .collect();
    let lattice = CellLattice::measure(&intervals);
    let cells: i64 = intervals
        .iter()
        .map(|&interval| lattice.cells_in(interval as f64).max(1))
        .sum();
    if cells > 0 {
        divisions as f64 / cells as f64
    } else {
        0.0
    }
}

/// The fat-track comparison: whether two adjacent steps read the same
/// recording, decided in the gap domain, and whether a step carries a
/// recording at all, decided by how much of its raw transition count
/// survived coherence.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FatTrackMerge {
    /// A recording keeps at least this fraction of its raw count.
    pub(crate) retention_floor: f64,
    /// Two gaps are the same gap within this much.
    pub(crate) gap_tolerance: i64,
    /// The window's hit rate that says "same recording".
    pub(crate) required_agreement: f64,
}

impl FatTrackMerge {
    /// The lineage's measured calibration: fringe steps retain a few
    /// percent, the fat track's middle retains essentially all.
    pub(crate) fn measured() -> FatTrackMerge {
        FatTrackMerge {
            retention_floor: 0.5,
            gap_tolerance: in_divisions(24),
            required_agreement: 0.9,
        }
    }

    pub(crate) fn is_recording(&self, coherent_points: usize, raw_count: usize) -> bool {
        raw_count > 0 && coherent_points as f64 / raw_count as f64 >= self.retention_floor
    }

    /// Whether two coherent-gap sequences read as one recording: a
    /// bounded window slid over every rotation of the second, scored
    /// by gap agreement. Counts must already be close — a fat track's
    /// passes differ by a reversal or two, not by percent.
    pub(crate) fn same_recording(&self, a: &[i64], b: &[i64], divisions: i64) -> bool {
        let ga = correspondence::gaps(a, divisions);
        let gb = correspondence::gaps(b, divisions);
        if ga.is_empty() || gb.is_empty() {
            return false;
        }
        if ga.len().abs_diff(gb.len()) > 4.max(ga.len() / 100) {
            return false;
        }
        let window = 256.min(ga.len()).min(gb.len());
        let mut best = 0usize;
        for offset in 0..gb.len() {
            let mut hits = 0usize;
            for i in 0..window {
                if (ga[i] - gb[(offset + i) % gb.len()]).abs() <= self.gap_tolerance {
                    hits += 1;
                }
            }
            best = best.max(hits);
        }
        best as f64 / window as f64 >= self.required_agreement
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic orbit: `pattern` gives each gap's run length; the
    /// cell is `cell` divisions. Returns the angle sequence starting
    /// at `origin`.
    fn synthetic_angles(pattern: &[i64], cell: i64, origin: i64) -> Vec<i64> {
        let mut angles = Vec::with_capacity(pattern.len());
        let mut at = origin;
        for &runs in pattern {
            angles.push(at);
            at += runs * cell;
        }
        angles
    }

    /// A repeating GCR-flavoured run pattern long enough for the
    /// correspondence sampler.
    fn long_pattern() -> Vec<i64> {
        let motif = [1i64, 1, 2, 1, 3, 1, 1, 2, 2, 1, 4, 1, 2, 3, 1, 1];
        let mut pattern = Vec::new();
        // Vary the motif so windows are informative and distinct.
        for round in 0..64i64 {
            for (i, &runs) in motif.iter().enumerate() {
                pattern.push(if (round + i as i64) % 7 == 0 {
                    runs + 1
                } else {
                    runs
                });
            }
        }
        pattern
    }

    /// A run pattern whose cells sum to exactly one revolution of
    /// `cell`-division cells, so the synthetic orbit closes the way a
    /// real recording does — the integration solves the cell from
    /// closure, so a test orbit that does not close tests nothing.
    fn closing_pattern(cell: i64) -> Vec<i64> {
        let total_cells = ANGULAR_DIVISIONS as i64 / cell;
        assert_eq!(
            total_cells * cell,
            ANGULAR_DIVISIONS as i64,
            "the test cell must divide the revolution exactly"
        );
        let motif = [1i64, 1, 2, 1, 3, 1, 1, 2, 2, 1, 4, 1, 2, 3, 1, 1];
        let mut pattern = Vec::new();
        let mut cells: i64 = 0;
        let mut round = 0i64;
        'fill: loop {
            for (i, &runs) in motif.iter().enumerate() {
                let varied = if (round + i as i64) % 7 == 0 {
                    runs + 1
                } else {
                    runs
                };
                if cells + varied + 4 > total_cells {
                    break 'fill;
                }
                pattern.push(varied);
                cells += varied;
                if cells == total_cells {
                    break 'fill;
                }
            }
            round += 1;
        }
        // Close the loop exactly with a final run of whatever remains,
        // capped at the lattice's largest multiple.
        let mut remaining = total_cells - cells;
        while remaining > 0 {
            let take = remaining.min(4);
            pattern.push(take);
            remaining -= take;
        }
        pattern
    }

    #[test]
    fn the_lattice_finds_a_planted_cell() {
        let cell = 4839i64; // zone 2 of the reference disk, roughly
        let pattern = long_pattern();
        let intervals: Vec<i64> = pattern.iter().map(|&runs| runs * cell).collect();
        let lattice = CellLattice::measure(&intervals);
        assert!(
            (lattice.cell_ticks() - cell as f64).abs() < cell as f64 * 0.01,
            "planted {cell}, measured {}",
            lattice.cell_ticks()
        );
        assert_eq!(lattice.cells_in(cell as f64), 1);
        assert_eq!(lattice.cells_in(2.0 * cell as f64), 2);
        assert_eq!(lattice.cells_in(3.2 * cell as f64), 3);
    }

    #[test]
    fn correspondence_finds_a_rotation() {
        let cell = 4839i64;
        let pattern = long_pattern();
        let angles = synthetic_angles(&pattern, cell, 1000);
        let divisions = ANGULAR_DIVISIONS as i64;
        let gaps_a = correspondence::gaps(&angles, divisions);

        // The same sequence rotated by 100 transitions.
        let rotated: Vec<i64> = (0..gaps_a.len())
            .map(|i| gaps_a[(100 + i) % gaps_a.len()])
            .collect();
        let candidates =
            correspondence::candidate_starts(&gaps_a, &rotated, correspondence::tolerance());
        assert!(
            candidates.contains(&(rotated.len() - 100)),
            "the true rotation is among the candidates: {candidates:?}"
        );
    }

    #[test]
    fn the_walk_resynchronises_over_a_missing_transition() {
        let cell = 4839i64;
        let pattern = long_pattern();
        let reference = synthetic_angles(&pattern, cell, 1000);
        // The other revolution missed transition 40: its two gaps
        // fused into one.
        let mut other = reference.clone();
        other.remove(40);
        let matched =
            correspondence::match_indices(&reference, &other, correspondence::walk_tolerance());
        // Before the loss, indices line up; after it, they are offset
        // by one; the lost transition itself is unmatched.
        assert_eq!(matched[10], 10);
        assert_eq!(
            matched[40], -1,
            "the unresolved transition has no counterpart"
        );
        assert_eq!(matched[41], 40);
        assert_eq!(matched[100], 99);
    }

    #[test]
    fn gap_first_integration_recovers_planted_angles() {
        // 4096 divides the revolution, so the synthetic orbit closes
        // exactly; it sits inside the lattice's plausible cell range.
        let cell = 4096i64;
        let pattern = closing_pattern(cell);
        let clean = synthetic_angles(&pattern, cell, 5000);
        let divisions = ANGULAR_DIVISIONS as i64;

        // Five noisy revolutions of the same recording: each angle
        // jittered deterministically within ±3 divisions.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut noisy = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 7) as i64 - 3
        };
        let revolutions: Vec<Vec<i64>> = (0..5)
            .map(|_| clean.iter().map(|&angle| angle + noisy()).collect())
            .collect();

        let intervals: Vec<i64> = pattern.iter().map(|&runs| runs * cell).collect();
        let lattice = CellLattice::measure(&intervals);
        let built = GapFirstAngles::from(&revolutions, divisions, &lattice, clean[0]);

        assert!(
            built.off_lattice.is_empty(),
            "nothing was planted off-lattice"
        );
        // The implied cell solves the closed loop; over a synthetic
        // orbit that does not close to exactly one revolution it still
        // lands within a fraction of a percent.
        assert!(
            (built.implied_cell - cell as f64).abs() < cell as f64 * 0.01,
            "planted {cell}, implied {}",
            built.implied_cell
        );
        // Every recovered angle sits within a few divisions of the
        // planted truth once integration replaces averaging.
        let mut worst = 0i64;
        for (recovered, &planted) in built.angles.iter().zip(&clean) {
            worst = worst.max((recovered - planted).abs());
        }
        assert!(
            worst <= 64,
            "integration holds the planted angles: worst departure {worst} divisions"
        );
    }

    #[test]
    fn coherence_asks_presence_and_spread() {
        let coherence = Coherence::measured();
        assert_eq!(
            coherence.revolutions_required(5),
            4,
            "3/4 of five rounds up"
        );
        assert!(coherence.coheres(4, 100, 5));
        assert!(!coherence.coheres(3, 100, 5), "too few sightings");
        assert!(
            !coherence.coheres(5, coherence.angular_tolerance + 1, 5),
            "too wide a spread"
        );
        assert!(coherence.span_is_indeterminate(32));
        assert!(!coherence.span_is_indeterminate(31));
    }

    #[test]
    fn the_fat_track_comparison_recognises_the_same_recording() {
        let cell = 4839i64;
        let divisions = ANGULAR_DIVISIONS as i64;
        let pattern = long_pattern();
        let a = synthetic_angles(&pattern, cell, 1000);
        // The neighbouring step read the same recording two divisions
        // late with one extra resolved reversal at the end.
        let mut b: Vec<i64> = a.iter().map(|&angle| angle + 2).collect();
        b.push(b.last().unwrap() + cell);
        let merge = FatTrackMerge::measured();
        assert!(merge.same_recording(&a, &b, divisions));

        // A different recording — the gaps reshuffled — does not pass.
        let mut different = a.clone();
        for pair in different.chunks_mut(2) {
            if pair.len() == 2 {
                let swap = pair[0];
                pair[0] = pair[1] - cell;
                pair[1] = swap + 3 * cell;
            }
        }
        different.sort_unstable();
        different.dedup();
        assert!(!merge.same_recording(&a, &different, divisions));

        assert!(merge.is_recording(90, 100));
        assert!(!merge.is_recording(2, 100));
    }

    #[test]
    fn the_orbit_clock_recovers_the_cell() {
        let cell = 4096i64;
        let pattern = closing_pattern(cell);
        let angles = synthetic_angles(&pattern, cell, 0);
        let recovered = cell_of(&angles, ANGULAR_DIVISIONS as i64);
        assert!(
            (recovered - cell as f64).abs() < cell as f64 * 0.01,
            "planted {cell}, recovered {recovered}"
        );
    }
}
