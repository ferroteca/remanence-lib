// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C1541 mastering profile (F33, P29): reducing an opened capture to
//! one circular, half-track-addressed 1541 flux medium.
//!
//! It owns the reduction and nothing else. It does not read a container,
//! does not encode one, and does not ascend to hardware bitstream, GCR,
//! sectors or files. What it produces is a
//! [`crate::flux_medium::FluxMedium`] and the account of everything the
//! destination will not carry.
//!
//! **Every reduction is a named policy input** — the side, which
//! observation of a location is used, how source positions map onto
//! half-tracks and which are admitted, how one timebase projects onto
//! another, how the evidence becomes pulse strength, and where the
//! circle begins. Each is supplied by the caller or declared by the
//! profile, each is reported in the plan, and each travels into the
//! result as provenance. **A reduction no input names is a refusal, not
//! a default.**
//!
//! **It resolves in two stages.** A plan computes the whole
//! transformation and writes nothing; an execution produces the medium.
//! The loss is declared before either — enumerated in the source's own
//! terms, because a count is not an account and loss reported after the
//! fact does not satisfy P29.
//!
//! The projection is exact rational arithmetic against both declared
//! bases. Resolution the destination cannot express is declared loss,
//! never silent rounding.

use crate::drive_profile::{self, C1541, OriginDefault};
use crate::error::{Error, ErrorCategory, Result};
use crate::evidence::{DeclaredLoss, LossAccount, Provenance};
use crate::flux_capture::{FluxCapture, SessionBacking, Tick, TimeBase, Track, TrackKey};
use crate::flux_medium::{
    Cycle, Derivation, FluxMedium, LocationKey, MediumBuilder, MediumFact, MediumFactKind,
    OriginRule, OriginStatement, Pulse, RotationalFrame, Strength,
};

/// The profile this reduction is declared by.
const PROFILE: &str = "c1541";

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image(PROFILE, reason)
}

// ------------------------------------------------------- policy inputs

/// Which observation of a location supplies the medium's pulses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationPolicy {
    /// The observation at this ordinal in the location's source-record
    /// order. A location that does not hold it is a refusal, never the
    /// nearest one instead.
    Selected { ordinal: u64 },
}

/// What to do with a location whose content its neighbour also holds.
///
/// The profile declares `Refuse`, because flux alone cannot tell a head
/// reading its neighbour from an instrument that did not move. This is
/// where the caller declares which it is (P29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicatePolicy {
    /// Take the profile's declaration, which for a 1541 is to refuse.
    Declared,
    /// The caller states the duplication is a real read of a real
    /// track, and the location is mastered as observed.
    AdmitAsObserved,
    /// The caller states it is not, and the location is left out — the
    /// medium claims nothing there.
    Omit,
}

/// How the selected evidence becomes the medium's strength vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseStrengthPolicy {
    /// Every pulse carries the declared state, and disagreement across
    /// the unselected observations is declared loss rather than
    /// expressed. Honest and reproducible; it simply claims less.
    Declared { state: u32 },
    /// A pulse the location's every observation places within
    /// `window_cycles` of it is strong; one only some of them
    /// corroborate is weak. The window is a declared input, not a
    /// threshold the profile chose on its own.
    FromAgreement { window_cycles: u64 },
}

/// What a projection does with two source transitions landing on one
/// cycle of the destination frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionPolicy {
    /// The reduction stops: the destination frame cannot express this
    /// location's timing and nothing pretends otherwise.
    Refuse,
    /// The later transition is dropped and counted in the account. The
    /// caller declares this; the profile never assumes it.
    DeclareLoss,
}

/// Where the medium's circle begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginPolicy {
    /// The track's own seam, which is what the profile declares: a 1541
    /// drive never observes index, so the capture's index is a datum of
    /// the instrument rather than of the medium.
    Declared,
    /// An angle the caller states outright, in reference-clock cycles.
    Angle { cycles: u64 },
}

/// The complete declared policy for one reduction.
///
/// There is no `Default`, deliberately: every field below is a decision
/// about evidence, and a reduction that arrived at one by construction
/// rather than by declaration is the thing P29 forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasteringPolicy {
    /// The captured head that supplies the family's one recorded
    /// surface, named as the capture-set adapter reported it.
    pub side: u64,
    pub observation: ObservationPolicy,
    pub duplicate: DuplicatePolicy,
    pub projection: ProjectionPolicy,
    pub pulse_strength: PulseStrengthPolicy,
    pub origin: OriginPolicy,
    /// What makes any stochastic element of the reduction reproducible.
    pub seed: u64,
}

// -------------------------------------------------------- the reporting

/// One half-track the medium will hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasteredLocation {
    /// The source step position this was reduced from.
    pub source_position: crate::StepPosition,
    /// The family half-track it addresses, as an exact ratio.
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    /// Which observation supplied it.
    pub observation_ordinal: u64,
    pub pulses: u64,
    pub strong_pulses: u64,
    pub weak_pulses: u64,
    /// Where the circle was given its start, in reference-clock cycles.
    pub origin_cycles: u64,
    /// The seam this location's evidence located, where it located one.
    pub seam_cycles: Option<u64>,
}

/// The whole transformation, computed and written nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasteringPlanReport {
    pub profile_id: String,
    /// The frame the medium will be expressed in.
    pub reference_clock_hz: u64,
    pub cycles_per_rotation: u64,
    pub origin_rule: String,
    pub locations: Vec<MasteredLocation>,
    /// Everything the destination will not carry. A count is not an
    /// account, so each entry says what was lost and in what terms.
    pub declared_loss: Vec<DeclaredLoss>,
    /// The policy that produced this plan, in human-readable terms, and
    /// the observations behind it (P4).
    pub evidence: Vec<String>,
}

/// A mastered medium, held in the session.
///
/// The pulses stay behind this surface: what a container makes of them
/// is the image-format adapter's, and there is no public pulse iterator
/// here or anywhere.
pub struct MasteredMedium {
    medium: FluxMedium,
    report: MasteringPlanReport,
}

impl std::fmt::Debug for MasteredMedium {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasteredMedium")
            .field("locations", &self.report.locations.len())
            .field("backing_bytes", &self.medium.backing_bytes())
            .finish()
    }
}

impl MasteredMedium {
    /// The plan this medium was produced from, unchanged.
    pub fn plan(&self) -> &MasteringPlanReport {
        &self.report
    }

    /// How many bytes of private session storage the medium occupies.
    pub fn backing_bytes(&self) -> u64 {
        self.medium.backing_bytes()
    }

    pub fn resident_bytes(&self) -> u64 {
        self.medium.resident_bytes()
    }

    /// How many locations the medium claims.
    pub fn locations(&self) -> u64 {
        self.medium.locations().count() as u64
    }

    /// The crate's own handle on the medium, for the image-format
    /// adapter that encodes it. What a container makes of these pulses
    /// is that adapter's business; nothing outside the crate reaches
    /// them.
    pub(crate) fn medium(&self) -> &FluxMedium {
        &self.medium
    }
}

// ------------------------------------------------------------- planning

/// What the reduction decided about one location, before anything was
/// written.
struct Planned {
    location: LocationKey,
    source_position: u64,
    observation_ordinal: u64,
    pulses: Vec<Pulse>,
    facts: Vec<MediumFact>,
    origin_cycles: u64,
    seam_cycles: Option<u64>,
    strong: u64,
    weak: u64,
}

/// A plan: everything computed, nothing written.
pub struct MasteringPlan {
    planned: Vec<Planned>,
    frame: RotationalFrame,
    policy: MasteringPolicy,
    report: MasteringPlanReport,
}

impl std::fmt::Debug for MasteringPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasteringPlan")
            .field("locations", &self.planned.len())
            .field("declared_loss", &self.report.declared_loss.len())
            .finish()
    }
}

impl MasteringPlan {
    /// What the transformation will do, and everything it will not
    /// carry. Read before executing: the loss is declared here, and
    /// executing does not add to it.
    pub fn report(&self) -> &MasteringPlanReport {
        &self.report
    }

    /// Produces the medium. The sources are untouched: they are read
    /// and nothing else, and the result is separate state with its own
    /// derived provenance (P13, P29).
    pub fn execute(self, cache_bytes: u64) -> Result<MasteredMedium> {
        let policy = describe_policy(&self.policy);
        let mut builder = MediumBuilder::new(
            "c1541",
            C1541.media,
            self.frame,
            Derivation::SelectedAndProjected,
            policy,
            SessionBacking::create()?,
        )?;
        for planned in &self.planned {
            builder.add_location(
                planned.location.clone(),
                &planned.pulses,
                &planned.facts,
                Provenance::new("c1541").note(format!(
                    "reduced from source step position {} observation {}, projected \
                     onto the drive's 16 MHz reference across one 300 RPM rotation",
                    planned.source_position, planned.observation_ordinal
                )),
            )?;
        }
        let (mut medium, sink, total) = builder.seal()?;
        medium.attach_backing(Box::new(sink.into_source()), total, cache_bytes);
        Ok(MasteredMedium {
            medium,
            report: self.report,
        })
    }
}

fn describe_policy(policy: &MasteringPolicy) -> Provenance {
    let mut provenance = Provenance::new("c1541")
        .note(format!(
            "side {}: the captured head declared to hold the family's one recorded \
             surface",
            policy.side
        ))
        .note(match policy.observation {
            ObservationPolicy::Selected { ordinal } => {
                format!("observation {ordinal} of each location was selected")
            }
        })
        .note(match policy.duplicate {
            DuplicatePolicy::Declared => {
                "a location duplicating its neighbour takes the profile's \
                 declaration, which refuses it"
                    .to_owned()
            }
            DuplicatePolicy::AdmitAsObserved => {
                "the caller declared a duplicated location a real read, and it is \
                 mastered as observed"
                    .to_owned()
            }
            DuplicatePolicy::Omit => {
                "the caller declared a duplicated location an instrument that did \
                 not move, and it is left out"
                    .to_owned()
            }
        })
        .note(match policy.pulse_strength {
            PulseStrengthPolicy::Declared { state } => format!(
                "every pulse carries the declared strength {state}; disagreement \
                 across the unselected observations is declared loss rather than \
                 expressed"
            ),
            PulseStrengthPolicy::FromAgreement { window_cycles } => format!(
                "a pulse every observation places within {window_cycles} cycles of \
                 it is strong, and one only some corroborate is weak"
            ),
        })
        .note(match policy.origin {
            OriginPolicy::Declared => {
                "the circle begins at the track's own seam, the 1541 drive never \
                 observing index"
                    .to_owned()
            }
            OriginPolicy::Angle { cycles } => {
                format!("the circle begins at the declared angle {cycles}")
            }
        });
    provenance = provenance.note(format!(
        "seed {}, which is what makes any stochastic element repeatable",
        policy.seed
    ));
    provenance
}

/// Plans the reduction of `capture` under `policy`.
pub(crate) fn plan(capture: &FluxCapture, policy: MasteringPolicy) -> Result<MasteringPlan> {
    let profile = &C1541;
    // The family records one surface, and the policy names which
    // captured side supplies it. A side the capture does not hold is
    // refused rather than substituted, and the sides are never merged
    // or averaged into one.
    let sides: Vec<u64> = {
        let mut sides: Vec<u64> = capture
            .tracks()
            .filter_map(|track| track.key().head())
            .collect();
        sides.sort_unstable();
        sides.dedup();
        sides
    };
    if !sides.contains(&policy.side) {
        return Err(refuse(format!(
            "the policy names side {} where the capture holds {sides:?}; the C1541 \
             records {} surface, and which captured side supplies it is declared \
             rather than guessed at",
            policy.side, profile.surfaces.recorded
        )));
    }
    let verdict = drive_profile::probe(profile, capture)?;
    let frame = RotationalFrame::new(
        PROFILE,
        TimeBase::new(PROFILE, profile.rotation.reference_clock, 1)?,
        profile.rotation.cycles_per_rotation,
        OriginStatement::new(
            match (profile.materialization.origin, policy.origin) {
                (_, OriginPolicy::Angle { .. }) => OriginRule::Declared,
                (OriginDefault::LongestGap, _) => OriginRule::Seam,
                (OriginDefault::Index, _) => OriginRule::Index,
                (OriginDefault::DeclaredAngle, _) => OriginRule::Declared,
            },
            Provenance::new(PROFILE).note(match policy.origin {
                OriginPolicy::Declared =>
                    "located at each track's own seam, the one departure from a \
                     record spacing that otherwise repeats to the bit"
                        .to_owned(),
                OriginPolicy::Angle { cycles } =>
                    format!("declared outright by the caller at cycle {cycles}"),
            }),
        ),
    )?;

    let mut planned = Vec::new();
    let mut loss = LossAccount::new();
    let cycles_per_rotation = profile.rotation.cycles_per_rotation;

    for outcome in verdict.locations() {
        let key = outcome.key();
        if key.head() != Some(policy.side) {
            // Counted whether or not the probe claimed it: the side was
            // captured, and none of its evidence enters the medium.
            // Sides are never merged or averaged, so all of it is loss.
            loss.add(
                "unselected-side",
                "positions on the side the policy did not select, whose evidence does \
                 not enter the medium; sides are never merged or averaged",
                1,
            );
            continue;
        }
        if !outcome.claimed() {
            loss.add(
                "unadmitted-location",
                "source positions the profile did not admit as recorded",
                1,
            );
            continue;
        }
        if outcome.duplicate_of().is_some() {
            match policy.duplicate {
                DuplicatePolicy::Declared => {
                    return Err(refuse(format!(
                        "source step position {} holds content its neighbour also \
                         holds, and flux alone cannot tell a head reading its \
                         neighbour from an instrument that did not move; the profile \
                         refuses it and the caller must declare which it is",
                        position_of(key)
                    )));
                }
                DuplicatePolicy::Omit => {
                    loss.add(
                        "omitted-duplicate",
                        "locations the caller declared an unmoved instrument, left \
                         out of the medium",
                        1,
                    );
                    continue;
                }
                DuplicatePolicy::AdmitAsObserved => {}
            }
        }

        planned.push(plan_location(
            capture,
            key,
            outcome,
            &policy,
            cycles_per_rotation,
            &mut loss,
        )?);
    }

    if planned.is_empty() {
        return Err(Error::categorized_image(
            ErrorCategory::InvalidImage,
            PROFILE,
            format!(
                "no location of side {} was admitted, so there is nothing to master",
                policy.side
            ),
        ));
    }
    planned.sort_by(|left, right| left.location.cmp(&right.location));

    account_for_the_envelope(capture, &mut loss);

    let report = MasteringPlanReport {
        profile_id: PROFILE.to_owned(),
        reference_clock_hz: profile.rotation.reference_clock,
        cycles_per_rotation,
        origin_rule: match policy.origin {
            OriginPolicy::Declared => "seam".to_owned(),
            OriginPolicy::Angle { .. } => "declared-angle".to_owned(),
        },
        locations: planned
            .iter()
            .map(|planned| {
                let (numerator, denominator) = planned.location.position();
                MasteredLocation {
                    source_position: crate::StepPosition {
                        numerator: planned.source_position,
                        denominator: 1,
                    },
                    half_track_numerator: numerator,
                    half_track_denominator: denominator,
                    observation_ordinal: planned.observation_ordinal,
                    pulses: planned.pulses.len() as u64,
                    strong_pulses: planned.strong,
                    weak_pulses: planned.weak,
                    origin_cycles: planned.origin_cycles,
                    seam_cycles: planned.seam_cycles,
                }
            })
            .collect(),
        declared_loss: loss.into_entries(),
        evidence: describe_policy(&policy).notes,
    };

    Ok(MasteringPlan {
        planned,
        frame,
        policy,
        report,
    })
}

fn position_of(key: &TrackKey) -> String {
    let (numerator, denominator) = key.position().parts();
    if denominator == 1 {
        numerator.to_string()
    } else {
        format!("{numerator}/{denominator}")
    }
}

/// Plans one location: selects its observation, projects it, and gives
/// every pulse its strength.
fn plan_location(
    capture: &FluxCapture,
    key: &TrackKey,
    outcome: &drive_profile::LocationOutcome,
    policy: &MasteringPolicy,
    cycles_per_rotation: u64,
    loss: &mut LossAccount,
) -> Result<Planned> {
    let track = capture
        .track(key)
        .ok_or_else(|| refuse("the capture no longer holds a location it reported"))?;
    let ObservationPolicy::Selected { ordinal } = policy.observation;
    let selected = track
        .observations()
        .iter()
        .find(|entry| entry.ordinal() == ordinal)
        .ok_or_else(|| {
            refuse(format!(
                "source step position {} holds {} observations, and the policy \
                 selects observation {ordinal}; the nearest one is not a substitute",
                position_of(key),
                track.observations().len()
            ))
        })?;

    // The source's step positions map onto half-tracks: two steps to a
    // track, which is a declared fact of the family rather than
    // arithmetic performed unasked.
    let (source_position, denominator) = key.position().parts();
    if denominator != 1 {
        return Err(refuse(format!(
            "source step position {} is not a whole step, and no declared map covers it",
            position_of(key)
        )));
    }
    let location = LocationKey::fraction(
        "c1541",
        source_position + 2,
        C1541.stepping.steps_per_location,
        Some(0),
    )?;

    let observation = capture.observation(key, selected)?;
    let span = observation.span();
    let origin_cycles = match policy.origin {
        OriginPolicy::Angle { cycles } => cycles % cycles_per_rotation,
        OriginPolicy::Declared => outcome.seam_cycles().unwrap_or(0),
    };

    // Project each transition onto the frame: the observation's own
    // measured span becomes one nominal rotation, exactly, and what
    // does not divide is declared rather than rounded away.
    let mut projected: Vec<Cycle> = Vec::with_capacity(observation.transitions().len());
    let mut inexact = 0u64;
    for &tick in observation.transitions() {
        let numerator = u128::from(tick) * u128::from(cycles_per_rotation);
        let denominator = u128::from(span.max(1));
        if numerator % denominator != 0 {
            inexact += 1;
        }
        let cycles = u64::try_from(numerator / denominator).map_err(|_| {
            refuse("a transition projects past one rotation of the destination frame")
        })?;
        // Rotate so the declared origin sits at zero.
        projected.push((cycles + cycles_per_rotation - origin_cycles) % cycles_per_rotation);
    }
    if inexact > 0 {
        loss.add(
            "timing-resolution",
            "transitions whose source instant falls between two cycles of the \
             destination frame, projected onto the cycle below",
            inexact,
        );
    }
    projected.sort_unstable();

    // Two source transitions landing on one cycle is timing the frame
    // cannot express. What happens then is the caller's to declare.
    let mut pulses_at: Vec<Cycle> = Vec::with_capacity(projected.len());
    let mut collided = 0u64;
    for cycles in projected {
        if pulses_at.last() == Some(&cycles) {
            collided += 1;
            if policy.projection == ProjectionPolicy::Refuse {
                return Err(refuse(format!(
                    "two transitions of source step position {} project onto cycle \
                     {cycles} of the destination frame, which cannot express them \
                     apart; the policy refuses rather than dropping one",
                    position_of(key)
                )));
            }
            continue;
        }
        pulses_at.push(cycles);
    }
    if collided > 0 {
        loss.add(
            "unexpressible-timing",
            "transitions dropped because the destination frame placed them on a \
             cycle already occupied",
            collided,
        );
    }

    let (pulses, strong, weak) = give_strength(
        capture,
        key,
        track,
        selected.ordinal(),
        &pulses_at,
        policy,
        cycles_per_rotation,
        origin_cycles,
        span,
        loss,
    )?;

    let mut facts = Vec::new();
    if let Some(seam) = outcome.seam_cycles() {
        facts.push(MediumFact::new(
            MediumFactKind::Seam {
                angle: (seam + cycles_per_rotation - origin_cycles) % cycles_per_rotation,
            },
            Provenance::new("c1541").note(
                "the one departure from a record spacing that otherwise repeats to \
                 the bit",
            ),
        ));
    }
    if let Some(other) = outcome.duplicate_of() {
        facts.push(MediumFact::new(
            MediumFactKind::Duplicate {
                of: LocationKey::fraction(
                    "c1541",
                    other.position().parts().0 + 2,
                    C1541.stepping.steps_per_location,
                    Some(0),
                )?,
            },
            Provenance::new("c1541").note(
                "the caller declared this a real read of a real track; the evidence \
                 is that its content matched its neighbour's",
            ),
        ));
    }

    Ok(Planned {
        location,
        source_position,
        observation_ordinal: selected.ordinal(),
        pulses,
        facts,
        origin_cycles,
        seam_cycles: outcome.seam_cycles(),
        strong,
        weak,
    })
}

/// Turns capture facts into medium facts: how the evidence across the
/// location's observations becomes each pulse's strength.
///
/// This is the conversion P22 names, and it happens here or nowhere.
#[allow(clippy::too_many_arguments)]
fn give_strength(
    capture: &FluxCapture,
    key: &TrackKey,
    track: &Track,
    selected: u64,
    at: &[Cycle],
    policy: &MasteringPolicy,
    cycles_per_rotation: u64,
    origin_cycles: u64,
    _span: Tick,
    loss: &mut LossAccount,
) -> Result<(Vec<Pulse>, u64, u64)> {
    match policy.pulse_strength {
        PulseStrengthPolicy::Declared { state } => {
            let unselected = track.observations().len().saturating_sub(1) as u64;
            if unselected > 0 {
                loss.add(
                    "unexpressed-disagreement",
                    "observations whose agreement or disagreement with the selected \
                     one is not expressed, the policy declaring one strength for \
                     every pulse",
                    unselected,
                );
            }
            let pulses = at
                .iter()
                .map(|cycles| Pulse::new(*cycles, Strength::certain(state)))
                .collect::<Vec<_>>();
            let strong = pulses.len() as u64;
            Ok((pulses, strong, 0))
        }
        PulseStrengthPolicy::FromAgreement { window_cycles } => {
            // Every other observation of this location, projected onto
            // the same frame, so a pulse can be looked for near an
            // angle rather than at a tick.
            let mut corroborators = Vec::new();
            for entry in track.observations() {
                if entry.ordinal() == selected {
                    continue;
                }
                let observation = capture.observation(key, entry)?;
                let span = observation.span().max(1);
                let mut angles: Vec<Cycle> = observation
                    .transitions()
                    .iter()
                    .map(|&tick| {
                        let cycles = u64::try_from(
                            u128::from(tick) * u128::from(cycles_per_rotation)
                                / u128::from(span),
                        )
                        .unwrap_or(0);
                        (cycles + cycles_per_rotation - origin_cycles) % cycles_per_rotation
                    })
                    .collect();
                angles.sort_unstable();
                corroborators.push(angles);
            }

            let mut pulses = Vec::with_capacity(at.len());
            let (mut strong, mut weak) = (0u64, 0u64);
            for &cycles in at {
                let agreeing = corroborators
                    .iter()
                    .filter(|angles| within(angles, cycles, window_cycles, cycles_per_rotation))
                    .count();
                let all = agreeing == corroborators.len();
                if all {
                    strong += 1;
                    pulses.push(Pulse::new(cycles, Strength::certain(2)));
                } else {
                    weak += 1;
                    pulses.push(Pulse::new(cycles, Strength::seeded(1, policy.seed)));
                }
            }
            if weak > 0 {
                loss.add(
                    "weakened-pulse",
                    "pulses the location's every observation did not corroborate, \
                     carried as weak rather than as recorded evidence",
                    weak,
                );
            }
            Ok((pulses, strong, weak))
        }
    }
}

/// Whether `angles` holds one within `window` of `cycles`, the circle's
/// wrap included.
fn within(angles: &[Cycle], cycles: Cycle, window: u64, rotation: u64) -> bool {
    let near = |candidate: Cycle| {
        let apart = candidate.abs_diff(cycles);
        apart.min(rotation - apart) <= window
    };
    let at = angles.partition_point(|angle| *angle < cycles);
    angles.get(at).copied().is_some_and(near)
        || at.checked_sub(1).and_then(|at| angles.get(at)).copied().is_some_and(near)
        // The wrap: the circle has no ends, so the far side counts.
        || angles.first().copied().is_some_and(near)
        || angles.last().copied().is_some_and(near)
}

/// Everything the capture holds beside its flux, and what becomes of it.
fn account_for_the_envelope(capture: &FluxCapture, loss: &mut LossAccount) {
    let envelope = capture.envelope();
    if !envelope.metadata().is_empty() {
        loss.add(
            "capture-metadata",
            "device and host facts the capture stated, which a medium has no place \
             for",
            envelope.metadata().len() as u64,
        );
    }
    if !envelope.foreign_records().is_empty() {
        loss.add(
            "foreign-record",
            "source records the capture layer retained verbatim, which do not cross \
             into a medium",
            envelope.foreign_records().len() as u64,
        );
    }
    let mut markers = 0u64;
    let mut runs = 0u64;
    let mut before = 0u64;
    let mut after = 0u64;
    let mut unselected = 0u64;
    for track in capture.tracks() {
        for run in track.runs() {
            runs += 1;
            markers += run.markers();
            before += run.before_first_index();
            after += run.after_last_index();
        }
        unselected += track.observations().len().saturating_sub(1) as u64;
    }
    if markers > 0 {
        loss.add(
            "marker-channel",
            "index and other timed markers the capture recorded, which the drive \
             this medium is for never observes",
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
    if unselected > 0 {
        loss.add(
            "unselected-observation",
            "observations of a location the policy did not select",
            unselected,
        );
    }
    if runs > 0 {
        loss.add(
            "transfer-result",
            "declared transfer results and transport records, which a medium does \
             not carry",
            runs,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> MasteringPolicy {
        MasteringPolicy {
            side: 0,
            observation: ObservationPolicy::Selected { ordinal: 0 },
            duplicate: DuplicatePolicy::Declared,
            projection: ProjectionPolicy::Refuse,
            pulse_strength: PulseStrengthPolicy::Declared { state: 2 },
            origin: OriginPolicy::Declared,
            seed: 0x0123_4567_89ab_cdef,
        }
    }

    #[test]
    fn every_policy_input_is_named_in_the_provenance_the_medium_carries() {
        // The medium cannot exist without the policy that produced it,
        // and the policy is not a shrug: every input appears.
        let notes = describe_policy(&policy()).notes;
        assert_eq!(notes.len(), 6, "{notes:?}");
        assert!(notes[0].contains("side 0"), "{notes:?}");
        assert!(notes[1].contains("observation 0"), "{notes:?}");
        assert!(notes[2].contains("refuses it"), "{notes:?}");
        assert!(notes[3].contains("declared strength 2"), "{notes:?}");
        assert!(notes[4].contains("seam"), "{notes:?}");
        assert!(notes[5].contains("seed"), "{notes:?}");
    }

    #[test]
    fn the_declared_origin_follows_the_profile_rather_than_the_instrument() {
        // A 1541 drive never observes index, so the capture's index is
        // a datum of the instrument and the circle begins at the seam.
        assert_eq!(C1541.materialization.origin, OriginDefault::LongestGap);
        assert!(!C1541.rotation.index_observed_by_drive);
    }

    #[test]
    fn the_account_states_what_was_lost_rather_than_only_how_much() {
        let mut loss = LossAccount::new();
        loss.add("marker-channel", "index markers", 6);
        loss.add("marker-channel", "index markers", 6);
        loss.add("capture-metadata", "device facts", 10);
        let entries = loss.into_entries();

        // One kind of loss is one entry, counted across everything that
        // contributed to it, and it says what it was in the source's
        // own terms.
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].code, "capture-metadata");
        assert_eq!(entries[1].code, "marker-channel");
        assert_eq!(entries[1].count, 12);
        assert!(!entries[1].detail.is_empty());
    }

    #[test]
    fn a_pulse_is_corroborated_across_the_circles_wrap_as_well() {
        // The circle has no ends: a pulse just past the origin and one
        // just before it are neighbours, not opposites.
        let angles = [5u64, 1000, 3_199_990];
        assert!(within(&angles, 3_199_995, 10, 3_200_000));
        assert!(within(&angles, 0, 10, 3_200_000));
        assert!(within(&angles, 1005, 10, 3_200_000));
        assert!(!within(&angles, 1_600_000, 10, 3_200_000));
    }
}
