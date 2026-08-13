// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The gap-first reconstruction: the P29 reduction from an opened
//! capture to a remanence image, on the strength of all the evidence
//! rather than the choice of one revolution.
//!
//! Per location: every revolution is aligned to the first by gap
//! correspondence; the cell lattice is measured from the transfer's
//! own intervals; angles are produced gap-first — the reader's
//! contextual displacement removed, runs snapped to the lattice,
//! intervals the medium holds off-lattice kept and reported, the whole
//! integrated so closure solves the cell exactly; coherence is decided
//! per transition and incoherent runs become `Unaligned` spans; and
//! adjacent steps carrying the same recording merge under measured
//! agreement — the fat track measured, never asserted.
//!
//! The reduction keeps the family's plan/execute discipline: a policy
//! declared with no defaults invented, a plan that computes everything
//! and writes nothing, a declared-loss account naming what the image
//! cannot carry, and the survey's facts riding provenance with their
//! basis — evidenced, measured, assumed — stated per fact.
//!
//! **The reduction answers with the image itself**, not a second root
//! beside it: what a caller holds afterwards is the same
//! [`FluxImage`] a `.remanence` artifact opens to, carrying this
//! reduction's policy and evidence as its provenance. The account of
//! how it came to be belongs to the *plan*, which computed it before
//! anything was written.

use crate::DeclaredLoss;
use crate::error::{Error, Result};
use crate::evidence::{LossAccount, Provenance};
use crate::flux::analysis::{
    CellLattice, Coherence, FatTrackMerge, GapFirstAngles, correspondence, in_divisions,
};
use crate::flux::capture::{FluxCapture, TrackKey};
use crate::flux::remanence::image::{
    ANGULAR_DIVISIONS, Hole, Magnetization, MediaFormFactor, OrbitKey, OrbitPoint, REMANENCE,
    FluxImage, FluxImageBuilder, WriteWidths,
};

/// The capture rig's radial lattice: the reference 5.25-inch rig steps
/// at 96 tpi from a first-step radius of 2250 mil. Declared facts of
/// the rig, not arithmetic a capture justifies — 2250 mil is exactly
/// 57,150 µm, and 96 tpi is exactly 3175/12 µm per step.
const FIRST_STEP_RADIUS_MICRONS: u64 = 57_150;
const STEP_PITCH_MICRONS: (u64, u64) = (3175, 12);

/// The written geometry assumed for a 48 tpi 5.25-inch recording:
/// ISO 6596-1 / ECMA-70 fix the written track at 0,330 mm; the guard
/// is the lineage's unmeasured working figure.
const WRITE_PLATEAU_MICRONS: u64 = 330;
const WRITE_GUARD_MICRONS: u64 = 432;

/// The recording discriminator: a position resolves a recording when
/// its revolutions' transition counts spread by less than this many
/// permille of the largest.
const RESOLVES_A_RECORDING_PERMILLE: u32 = 1;

/// How the reduction decides which positions hold recordings.
///
/// Nothing here is public: the reduction runs inside the declared
/// collection load, under the profile's declared `Materialization`
/// defaults, and its account rides the medium as provenance. A
/// caller-facing plan preview belongs to the question tier, argued
/// separately.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordingSelection {
    /// Measured from the evidence: a position records where its
    /// revolutions resolve the same transitions — the count-spread
    /// discriminator, a measured fact carried as such.
    Measured,
    /// The caller's own assertion, checked to exist and honoured.
    Declared(Vec<u64>),
}

/// The declared inputs of the reconstruction. No `Default`,
/// deliberately: a reduction the policy does not name is a refusal
/// rather than a default (P29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReconstructionPolicy {
    /// Which recorded side the image is reconstructed from. Sides are
    /// never merged or averaged.
    pub(crate) side: u64,
    pub(crate) recordings: RecordingSelection,
}

/// One reconstructed orbit, as the plan reports it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReconstructedOrbit {
    /// The instrument position the orbit was read at — capture
    /// provenance, not a fact of the medium.
    pub position: u64,
    /// Where the orbit is: the rig's radius at that position.
    pub radius_microns: u64,
    pub revolutions: u32,
    /// Raw transitions per revolution, before alignment.
    pub transition_counts: Vec<u32>,
    /// The count-spread discriminator, in permille of the largest.
    pub count_spread_permille: u32,
    pub points: u64,
    pub coherent_points: u64,
    pub unaligned_spans: u64,
    /// The cell the closed revolution implies, in millidivisions.
    pub implied_cell_millidivisions: u64,
    /// Intervals kept off the lattice: the medium holding what the
    /// crystal did not write.
    pub off_lattice: u32,
    /// Whether the fat-track merge admitted this orbit into the image.
    pub admitted: bool,
}

/// What the reconstruction will produce, computed whole before
/// anything is written.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReconstructionReport {
    pub format_id: &'static str,
    pub side: u64,
    /// Every position the capture holds on the side.
    pub swept_positions: u32,
    /// The positions the policy's selection names as recordings.
    pub recorded_positions: Vec<u64>,
    pub orbits: Vec<ReconstructedOrbit>,
    pub declared_loss: Vec<DeclaredLoss>,
    pub evidence: Vec<String>,
}

#[derive(Debug)]
struct PlannedOrbit {
    radius_microns: u64,
    points: Vec<OrbitPoint>,
}

/// The computed reduction: everything decided, nothing written.
#[derive(Debug)]
pub(crate) struct ReconstructionPlan {
    planned: Vec<PlannedOrbit>,
    policy: Provenance,
    report: ReconstructionReport,
}

impl ReconstructionPlan {
    /// What the reduction will produce, and what it will leave behind —
    /// computed whole, before anything is written. Read it and then
    /// decide: executing adds nothing to the account (P29).
    pub(crate) fn report(&self) -> &ReconstructionReport {
        &self.report
    }

    /// Produces the remanence image, streaming each admitted orbit's
    /// packed points into private session storage under `cache_bytes`
    /// of working set (P27).
    ///
    /// The image is the family's ordinary physical stratum — the same
    /// root a `.remanence` artifact opens to — and carries this
    /// reduction's declared policy and evidence as its provenance.
    pub(crate) fn execute(&self, cache_bytes: u64) -> Result<FluxImage> {
        let sink = crate::flux::capture::SessionBacking::create()?;
        let mut builder = FluxImageBuilder::to_sink(
            MediaFormFactor::Inch525,
            Vec::<Hole>::new(),
            self.policy.clone(),
            sink,
            crate::flux::capture::CHUNK_RECORDS,
        )?;
        // Ascending radius is ascending key order; the planned orbits
        // run outermost-first (position order), so they add reversed.
        for planned in self.planned.iter().rev() {
            builder.add_orbit(
                OrbitKey::new(self.report.side, planned.radius_microns)?,
                &planned.points,
            )?;
        }
        let (mut image, sink, total) = builder.seal()?;
        image.attach_backing(sink.into_source(), total, cache_bytes);
        Ok(image)
    }
}

/// The rig's radius at an instrument position, rounded once where a
/// derived lattice position becomes an asserted whole-micron fact.
fn radius_microns_at(position: u64) -> u64 {
    let (pitch_numerator, pitch_denominator) = STEP_PITCH_MICRONS;
    let offset_scaled = position as i128 * pitch_numerator as i128;
    let radius_scaled =
        FIRST_STEP_RADIUS_MICRONS as i128 * pitch_denominator as i128 - offset_scaled;
    // Round to nearest whole micron.
    let rounded = (radius_scaled + pitch_denominator as i128 / 2) / pitch_denominator as i128;
    rounded.max(0) as u64
}

/// One location's evidence, gathered from the capture's own records.
struct LocationEvidence {
    position: u64,
    /// Raw normalized angles per revolution, in capture order.
    revolutions: Vec<Vec<i64>>,
    /// The whole transfer's intervals, scaled to divisions.
    intervals: Vec<i64>,
}

fn gather(capture: &FluxCapture, key: &TrackKey) -> Result<Option<LocationEvidence>> {
    let (numerator, denominator) = key.position().parts();
    if denominator != 1 {
        return Err(Error::invalid_image(
            REMANENCE,
            format!(
                "the capture addresses a location at {numerator}/{denominator} \
                 steps, which the rig's whole-step lattice cannot place"
            ),
        ));
    }
    let track = capture
        .track(key)
        .ok_or_else(|| Error::invalid_image(REMANENCE, "a gathered location vanished"))?;

    let divisions = ANGULAR_DIVISIONS as i64;
    let mut revolutions: Vec<Vec<i64>> = Vec::new();
    let mut intervals: Vec<i64> = Vec::new();
    for run in track.runs() {
        // Every circular observation of this run is one revolution.
        let entries: Vec<_> = track
            .observations()
            .iter()
            .filter(|entry| entry.source().run_ordinal() == run.ordinal())
            .collect();
        if entries.is_empty() {
            continue;
        }
        let mut span_sum: u128 = 0;
        for entry in &entries {
            let observation = capture.observation(key, entry)?;
            span_sum += u128::from(observation.span());
            let mut angles = Vec::with_capacity(observation.transitions().len());
            let mut previous = -1i64;
            for &tick in observation.transitions() {
                if tick == 0 {
                    continue;
                }
                let angle = (u128::from(tick) * u128::from(ANGULAR_DIVISIONS)
                    / u128::from(observation.span())) as i64;
                if angle > previous {
                    angles.push(angle);
                    previous = angle;
                }
            }
            revolutions.push(angles);
        }
        // The lattice is measured over the whole transfer's intervals,
        // scaled by the run's own mean revolution.
        let mean_span = (span_sum / entries.len() as u128).max(1);
        let transitions = capture.run_transitions(key, run)?;
        for pair in transitions.windows(2) {
            let interval = pair[1] - pair[0];
            if interval > 0 {
                intervals.push(
                    (u128::from(interval) * u128::from(ANGULAR_DIVISIONS) / mean_span) as i64,
                );
            }
        }
    }
    if revolutions.is_empty() {
        return Ok(None);
    }
    let _ = divisions;
    Ok(Some(LocationEvidence {
        position: numerator,
        revolutions,
        intervals,
    }))
}

/// Exactly the reversals observed, and nothing else: coherent angles
/// become alternating points, runs of incoherent angles wide enough
/// become one `Unaligned` span point each, and the write geometry is
/// stated on the first coherent point.
fn points_from(angles: &[i64], coheres: &[bool], coherence: &Coherence) -> Result<Vec<OrbitPoint>> {
    let divisions = ANGULAR_DIVISIONS as i64;
    let mut in_span = vec![false; angles.len()];
    let mut scan = 0;
    while scan < angles.len() {
        if coheres[scan] {
            scan += 1;
            continue;
        }
        let mut end = scan;
        while end < angles.len() && !coheres[end] {
            end += 1;
        }
        if coherence.span_is_indeterminate(end - scan) {
            for flag in &mut in_span[scan..end] {
                *flag = true;
            }
        }
        scan = end;
    }

    let written = WriteWidths::new(WRITE_PLATEAU_MICRONS, WRITE_GUARD_MICRONS)?;
    let mut points = Vec::with_capacity(angles.len());
    let mut widths_stated = false;
    let mut within_span = false;
    // Polarity is a gauge: no capture of this class supplies it, so
    // the run opens POSITIVE and alternates. What the capture does
    // establish is the alternation structure itself.
    let mut sense = Magnetization::Positive;
    for i in 0..angles.len() {
        let angle = angles[i];
        if angle <= 0 || angle >= divisions {
            continue;
        }
        if !in_span[i] {
            points.push(if widths_stated {
                OrbitPoint::new(angle as u64, sense)?
            } else {
                OrbitPoint::stating(angle as u64, sense, Some(written))?
            });
            widths_stated = true;
            sense = sense
                .opposite()
                .expect("the gauge alternates coherent senses");
            within_span = false;
        } else if !within_span {
            // One point opens a span that runs to the next transition;
            // restating it per failed reversal would claim thousands
            // of times what one claim already covers.
            points.push(OrbitPoint::new(angle as u64, Magnetization::Unaligned)?);
            within_span = true;
            // Alternation does not propagate across a span carrying no
            // sense: the run resuming afterward takes a fresh gauge.
            sense = Magnetization::Positive;
        }
    }
    Ok(points)
}

/// Computes the whole reduction: every location of the declared side
/// reconstructed, the recordings selected, the fat track merged, and
/// the account drawn up. Writes nothing.
pub(crate) fn plan(
    capture: &FluxCapture,
    policy: &ReconstructionPolicy,
) -> Result<ReconstructionPlan> {
    let divisions = ANGULAR_DIVISIONS as i64;
    let coherence = Coherence::measured();
    let merge = FatTrackMerge::measured();
    let mut loss = LossAccount::new();

    // The declared side must exist; sides are never merged.
    let mut sides: Vec<u64> = capture
        .tracks()
        .filter_map(|track| track.key().head())
        .collect();
    sides.sort_unstable();
    sides.dedup();
    if !sides.contains(&policy.side) {
        return Err(Error::invalid_image(
            REMANENCE,
            format!(
                "the capture records no side {}; it holds sides {:?}",
                policy.side, sides
            ),
        ));
    }
    let mut unselected_locations = 0u64;
    let mut keys: Vec<TrackKey> = Vec::new();
    for track in capture.tracks() {
        if track.key().head() == Some(policy.side) {
            keys.push(track.key().clone());
        } else {
            unselected_locations += 1;
        }
    }
    if unselected_locations > 0 {
        loss.add(
            "unselected-side",
            "locations recorded on sides the policy did not select",
            unselected_locations,
        );
    }

    // Reconstruct every held position: a recording wider than a head
    // spans steps nobody would think to name, and deciding that has
    // to happen while the evidence is still in hand.
    struct Built {
        position: u64,
        points: Vec<OrbitPoint>,
        coherent_angles: Vec<i64>,
        raw_counts: Vec<u32>,
        revolutions: u32,
        implied_cell: f64,
        off_lattice: u32,
    }
    let mut built: Vec<Built> = Vec::new();
    for key in &keys {
        let Some(evidence) = gather(capture, key)? else {
            loss.add(
                "unbounded-location",
                "locations whose transfers carried too few index events to bound \
                 one whole revolution",
                1,
            );
            continue;
        };

        let raw_counts: Vec<u32> = evidence
            .revolutions
            .iter()
            .map(|revolution| revolution.len() as u32)
            .collect();
        let reference = evidence.revolutions[0].clone();
        if reference.is_empty() {
            loss.add(
                "empty-revolution",
                "locations whose reference revolution resolved no transitions",
                1,
            );
            continue;
        }
        let mut matched: Vec<Vec<i64>> = vec![reference.clone()];
        for revolution in evidence.revolutions.iter().skip(1) {
            let indices = correspondence::match_indices(&reference, revolution, in_divisions(24));
            matched.push(
                indices
                    .iter()
                    .map(|&index| {
                        if index < 0 {
                            -1
                        } else {
                            revolution[index as usize]
                        }
                    })
                    .collect(),
            );
        }

        let lattice = CellLattice::measure(&evidence.intervals);
        let gap_first = GapFirstAngles::from(&matched, divisions, &lattice, reference[0]);

        // Coherence per transition, over the one capture group.
        let n = reference.len();
        let revolutions = matched.len();
        let mut coheres = vec![false; n];
        for i in 0..n {
            let mut low = i64::MAX;
            let mut high = i64::MIN;
            let mut seen = 0usize;
            for row in &matched {
                if row[i] >= 0 {
                    low = low.min(row[i]);
                    high = high.max(row[i]);
                    seen += 1;
                }
            }
            if seen > 0 {
                coheres[i] = coherence.coheres(seen, high - low, revolutions);
            }
        }

        let points = points_from(&gap_first.angles, &coheres, &coherence)?;
        let coherent_angles: Vec<i64> = points
            .iter()
            .filter(|point| point.magnetization().is_coherent())
            .map(|point| point.angle() as i64)
            .collect();
        built.push(Built {
            position: evidence.position,
            points,
            coherent_angles,
            raw_counts,
            revolutions: revolutions as u32,
            implied_cell: gap_first.implied_cell,
            off_lattice: gap_first.off_lattice.len() as u32,
        });
    }
    if built.is_empty() {
        return Err(Error::invalid_image(
            REMANENCE,
            "no location of the selected side could be reconstructed",
        ));
    }
    built.sort_by_key(|orbit| orbit.position);

    // The recording set, by the policy's own selection.
    let count_spread_permille = |counts: &[u32]| -> u32 {
        let high = counts.iter().copied().max().unwrap_or(0);
        let low = counts.iter().copied().min().unwrap_or(0);
        if high == 0 {
            0
        } else {
            ((u64::from(high - low) * 1000).div_ceil(u64::from(high))) as u32
        }
    };
    let recorded: Vec<u64> = match &policy.recordings {
        RecordingSelection::Measured => built
            .iter()
            .filter(|orbit| {
                let high = orbit.raw_counts.iter().copied().max().unwrap_or(0);
                let low = orbit.raw_counts.iter().copied().min().unwrap_or(0);
                // Strictly below the threshold fraction, as measured.
                u64::from(high - low) * 1000
                    < u64::from(RESOLVES_A_RECORDING_PERMILLE) * u64::from(high)
            })
            .map(|orbit| orbit.position)
            .collect(),
        RecordingSelection::Declared(positions) => {
            for position in positions {
                if !built.iter().any(|orbit| orbit.position == *position) {
                    return Err(Error::invalid_image(
                        REMANENCE,
                        format!(
                            "the policy declares a recording at position {position}, \
                             which the capture does not hold on side {}",
                            policy.side
                        ),
                    ));
                }
            }
            let mut positions = positions.clone();
            positions.sort_unstable();
            positions.dedup();
            positions
        }
    };
    if recorded.is_empty() {
        return Err(Error::invalid_image(
            REMANENCE,
            "no position of the selected side resolves a recording",
        ));
    }

    // The fat-track merge: group consecutive steps reading the same
    // recording; a group is admitted only where the selection names
    // one of its steps — the merge widens recordings, it never
    // decides what the disk holds.
    let mut admitted = vec![false; built.len()];
    let mut i = 0usize;
    while i < built.len() {
        let head = &built[i];
        if !merge.is_recording(
            head.coherent_angles.len(),
            head.raw_counts.first().copied().unwrap_or(0) as usize,
        ) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end + 1 < built.len()
            && built[end + 1].position == built[end].position + 1
            && merge.is_recording(
                built[end + 1].coherent_angles.len(),
                built[end + 1].raw_counts.first().copied().unwrap_or(0) as usize,
            )
            && merge.same_recording(
                &head.coherent_angles,
                &built[end + 1].coherent_angles,
                divisions,
            )
        {
            end += 1;
        }
        let wanted = (i..=end).any(|k| recorded.contains(&built[k].position));
        if wanted {
            for flag in &mut admitted[i..=end] {
                *flag = true;
            }
        }
        i = end + 1;
    }
    let dropped = admitted.iter().filter(|&&flag| !flag).count() as u64;
    if dropped > 0 {
        loss.add(
            "unadmitted-position",
            "positions whose evidence names no recording — noise floor, guard \
             band, or a fringe no recording group claims",
            dropped,
        );
    }

    account_for_the_envelope(capture, &mut loss);

    // The report, and the plan's own working set.
    let mut orbits = Vec::with_capacity(built.len());
    let mut planned = Vec::new();
    for (orbit, &keep) in built.iter().zip(&admitted) {
        let unaligned = orbit
            .points
            .iter()
            .filter(|point| point.magnetization() == Magnetization::Unaligned)
            .count() as u64;
        orbits.push(ReconstructedOrbit {
            position: orbit.position,
            radius_microns: radius_microns_at(orbit.position),
            revolutions: orbit.revolutions,
            transition_counts: orbit.raw_counts.clone(),
            count_spread_permille: count_spread_permille(&orbit.raw_counts),
            points: orbit.points.len() as u64,
            coherent_points: orbit.coherent_angles.len() as u64,
            unaligned_spans: unaligned,
            implied_cell_millidivisions: (orbit.implied_cell * 1000.0).round() as u64,
            off_lattice: orbit.off_lattice,
            admitted: keep,
        });
        if keep {
            planned.push(PlannedOrbit {
                radius_microns: radius_microns_at(orbit.position),
                points: orbit.points.clone(),
            });
        }
    }

    let evidence = vec![
        format!(
            "declared: side {} of the capture, reduced under the gap-first \
             reconstruction",
            policy.side
        ),
        format!(
            "declared: the rig's radial lattice — first step at \
             {FIRST_STEP_RADIUS_MICRONS} microns, {}/{} microns per step",
            STEP_PITCH_MICRONS.0, STEP_PITCH_MICRONS.1
        ),
        format!(
            "assumed: written geometry {WRITE_PLATEAU_MICRONS}/{WRITE_GUARD_MICRONS} \
             microns plateau/guard — ISO 6596-1 and ECMA-70 fix the written track \
             at 0,330 mm; the guard is a working figure"
        ),
        match &policy.recordings {
            RecordingSelection::Measured => format!(
                "measured: {} of {} swept positions resolve a recording under the \
                 count-spread discriminator",
                recorded.len(),
                built.len()
            ),
            RecordingSelection::Declared(_) => format!(
                "declared: {} positions named as recordings by the policy",
                recorded.len()
            ),
        },
    ];

    let report = ReconstructionReport {
        format_id: REMANENCE,
        side: policy.side,
        swept_positions: built.len() as u32,
        recorded_positions: recorded,
        orbits,
        declared_loss: loss.into_entries(),
        evidence,
    };

    let mut policy_provenance = Provenance::new(REMANENCE)
        .note(format!("side: {}", policy.side))
        .note(match &policy.recordings {
            RecordingSelection::Measured => "recordings: measured".to_owned(),
            RecordingSelection::Declared(positions) => {
                format!("recordings: declared at {positions:?}")
            }
        });
    for note in &report.evidence {
        policy_provenance = policy_provenance.note(note.clone());
    }

    Ok(ReconstructionPlan {
        planned,
        policy: policy_provenance,
        report,
    })
}

/// What the envelope holds that a remanence image has no place for.
fn account_for_the_envelope(capture: &FluxCapture, loss: &mut LossAccount) {
    let envelope = capture.envelope();
    if !envelope.metadata().is_empty() {
        loss.add(
            "capture-metadata",
            "device and host facts the capture stated, which the image has no \
             place for",
            envelope.metadata().len() as u64,
        );
    }
    if !envelope.foreign_records().is_empty() {
        loss.add(
            "foreign-record",
            "source records the capture layer retained verbatim, which do not \
             cross into the image",
            envelope.foreign_records().len() as u64,
        );
    }
    let mut markers = 0u64;
    let mut before = 0u64;
    let mut after = 0u64;
    for track in capture.tracks() {
        for run in track.runs() {
            markers += run.markers();
            before += run.before_first_index();
            after += run.after_last_index();
        }
    }
    if markers > 0 {
        loss.add(
            "marker-channel",
            "index and other timed markers the capture recorded, which the image \
             carries only as the angular frame they established",
            markers,
        );
    }
    if before + after > 0 {
        loss.add(
            "outside-the-revolution",
            "transitions recorded before a transfer's first index and after its \
             last, which no circular observation bounded",
            before + after,
        );
    }
}

/// Opening the capture fixture takes the library's mandatory
/// write-denial claim (P7), so the whole-capture tests in this crate
/// take this gate first rather than colliding on the archive when the
/// test harness runs them on parallel threads.
#[cfg(test)]
pub(crate) static CAPTURE_FIXTURE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Assembles the capture fixture the way a declared collection load
/// does: the archive's namespace gathered whole, each member's bytes
/// produced once.
#[cfg(test)]
pub(crate) fn fixture_capture(capture_path: &std::path::Path) -> crate::flux::capture::FluxCapture {
    struct Member(crate::io::source::FileSource);
    impl crate::flux::kryoflux::MemberSource for Member {
        fn name(&self) -> &str {
            self.0.name()
        }
        fn size(&self) -> u64 {
            self.0.size()
        }
        fn bytes(&self) -> Result<Vec<u8>> {
            self.0.read_whole()
        }
    }
    let file = std::fs::File::open(capture_path).expect("the fixture opens");
    let archive = crate::archive::ArchiveRecognition::load(file, "7z")
        .expect("the fixture is a 7z")
        .into_medium(crate::io::cache::DEFAULT_CACHE_BYTES);
    let members: Vec<Member> = archive
        .entry_group_sources("")
        .expect("the members gather")
        .into_iter()
        .map(Member)
        .collect();
    crate::flux::kryoflux::assemble(
        "the fixture capture",
        &members,
        crate::io::cache::DEFAULT_CACHE_BYTES,
    )
    .expect("the capture assembles")
    .capture
}

/// The one reduction this crate's tests share.
///
/// Opening the capture fixture and reducing it costs minutes, and more
/// than one test needs the same disk, so it is computed once and handed
/// out. It is the image the reduction produced rather than a stand-in
/// for it, so what a test masters off here is what a run of its own
/// would have given it.
#[cfg(test)]
pub(crate) fn reconstructed_capture() -> &'static crate::flux::remanence::image::FluxImage {
    static SHARED: std::sync::OnceLock<crate::flux::remanence::image::FluxImage> =
        std::sync::OnceLock::new();
    SHARED.get_or_init(|| {
        // The archive is opened under a P7 claim, so every test that
        // needs this disk takes the same gate rather than racing the
        // reduction test for it.
        let _gate = CAPTURE_FIXTURE_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let capture_path =
            fixtures.join("Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z");
        if !capture_path.exists() {
            panic!(
                "missing fixture {capture_path:?}: run `uv run --group test-fixture-prep \
                 test-fixture-prep/prep_fixtures.py`"
            );
        }
        let capture = fixture_capture(&capture_path);
        let plan = plan(
            &capture,
            &ReconstructionPolicy {
                side: 0,
                recordings: RecordingSelection::Measured,
            },
        )
        .expect("the reduction plans");
        plan.execute(crate::io::cache::DEFAULT_CACHE_BYTES)
            .expect("the plan executes")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_lattice_places_the_reference_positions() {
        assert_eq!(radius_microns_at(0), 57_150);
        // 57150 - 3175/12 = 56885.42 → 56885
        assert_eq!(radius_microns_at(1), 56_885);
        // Position 66, the reference fat track's first step.
        assert_eq!(radius_microns_at(66), 39_688);
    }

    #[test]
    fn points_from_opens_spans_and_alternates() {
        let coherence = Coherence::measured();
        let run = coherence.indeterminate_run;
        // Forty coherent angles, then a wide incoherent run, then two
        // more coherent ones.
        let mut angles: Vec<i64> = (1..=40).map(|i| i * 1000).collect();
        let mut coheres = vec![true; 40];
        for i in 0..run {
            angles.push(41_000 + i as i64 * 1000);
            coheres.push(false);
        }
        angles.push(200_000);
        angles.push(201_000);
        coheres.push(true);
        coheres.push(true);

        let points = points_from(&angles, &coheres, &coherence).expect("points compose");
        // Forty alternating points, one span opener, two reopened.
        assert_eq!(points.len(), 43);
        assert!(
            points[0].states_widths(),
            "the first coherent point states widths"
        );
        assert_eq!(points[0].magnetization(), Magnetization::Positive);
        assert_eq!(points[1].magnetization(), Magnetization::Negative);
        assert_eq!(points[40].magnetization(), Magnetization::Unaligned);
        // The gauge resets after a span with no sense.
        assert_eq!(points[41].magnetization(), Magnetization::Positive);
        crate::flux::remanence::image::validate_orbit_points(&points)
            .expect("the reduction's points satisfy the model");
    }

    #[test]
    fn an_isolated_failure_is_a_marginal_reading_not_a_span() {
        let coherence = Coherence::measured();
        let angles: Vec<i64> = (1..=10).map(|i| i * 1000).collect();
        let mut coheres = vec![true; 10];
        coheres[5] = false;
        let points = points_from(&angles, &coheres, &coherence).expect("points compose");
        // The marginal reading is recorded as a reversal like any
        // other; how well it was measured is the report's business.
        assert_eq!(points.len(), 10);
        assert!(
            points
                .iter()
                .all(|point| point.magnetization().is_coherent())
        );
    }

    /// The whole pipeline over the repository's own capture fixture:
    /// the sweep, the positions the disk actually records, and an image
    /// whose orbits satisfy the model's own rules. It is checked against
    /// what the recording requires rather than against another
    /// implementation's run of it, which no threshold here could make
    /// meaningful.
    #[test]
    fn the_pinball_capture_reduces_to_a_whole_side() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let capture_path =
            fixtures.join("Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z");
        if !capture_path.exists() {
            panic!(
                "missing fixture {capture_path:?}: run `uv run --group test-fixture-prep \
                 test-fixture-prep/prep_fixtures.py`"
            );
        }
        let _gate = CAPTURE_FIXTURE_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let capture = fixture_capture(&capture_path);
        let plan = plan(
            &capture,
            &ReconstructionPolicy {
                side: 0,
                recordings: RecordingSelection::Measured,
            },
        )
        .expect("the reduction plans");

        let report = plan.report();
        assert_eq!(report.swept_positions, 84);
        assert!(
            (30..=40).contains(&report.recorded_positions.len()),
            "the reference disk records about 35 positions: {:?}",
            report.recorded_positions
        );

        let image = plan
            .execute(crate::io::cache::DEFAULT_CACHE_BYTES)
            .expect("the plan executes");
        // What the reduction produced, checked against the model's own
        // rules rather than against another program's run of it: every
        // orbit sits at a positive radius and carries transitions, and
        // the whole side reconstructs.
        let orbits: Vec<_> = image.orbits().collect();
        assert!(
            orbits.len() > 30,
            "a whole side reconstructs tens of orbits: {}",
            orbits.len()
        );
        assert!(
            orbits.iter().all(|orbit| orbit.key().radius_microns() > 0),
            "every orbit sits at a positive radius"
        );
        let points: u64 = orbits.iter().map(|orbit| orbit.points()).sum();
        assert!(
            points > 1_000_000,
            "a whole side carries over a million transitions: {points}"
        );
    }
}
