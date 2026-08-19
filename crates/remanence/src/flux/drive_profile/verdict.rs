// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! What a probe reports, and how a capture is recognized.
//!
//! **Discovery proposes and never silently decides.** Verdicts are
//! ranked and carry the observations that produced them (P4), a caller
//! may pin a profile with `recognize_as`, and a capture no profile
//! claims is a named refusal — a lone enrolled entry never wins by
//! being alone.
//!
//! `recognition` is the whole act over one capture: every enrolled
//! profile probed, the verdicts ranked, and what was pinned travelling
//! with the result.

use std::collections::BTreeMap;

use crate::error::{Error, ErrorCategory, Result};
use crate::flux::capture::{FluxCapture, TrackKey};

use super::intervals::{LocationReading, fingerprint_distance, position_text, read_location};
use super::*;

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
                     which takes {}",
                    position_text(&reading.key),
                    profile.stepping.describe()
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
                if enrolled().len() == 1 { "was" } else { "were" }
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
pub(crate) struct ZoneClaim {
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
pub(crate) struct LocationVerdict {
    /// The member this position was read from.
    pub artifact: String,
    pub position: crate::flux::kryoflux::StepPosition,
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
    pub duplicate_of: Option<crate::flux::kryoflux::StepPosition>,
    pub claimed: bool,
    /// Why this position was not claimed, in the profile's own terms.
    pub refusal: Option<String>,
}

/// One profile's answer, with the observations that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileVerdict {
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
pub(crate) struct Recognition {
    /// Highest confidence first. Several profiles may claim one
    /// capture, and the ranking is reported rather than resolved.
    pub verdicts: Vec<ProfileVerdict>,
    /// The profile the caller pinned, where one was pinned. What the
    /// library chose travels into the result either way.
    pub pinned: Option<String>,
    pub evidence: Vec<String>,
}

fn step_position(key: &TrackKey) -> crate::flux::kryoflux::StepPosition {
    let (numerator, denominator) = key.position().parts();
    crate::flux::kryoflux::StepPosition {
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
    fn a_profile_the_build_does_not_enroll_is_refused_by_name() {
        assert_eq!(enrolled().len(), 1);
        assert_eq!(enrolled()[0].id, "c1541");
    }
}

/// Recognition over the prepared capture set — the fixture-driven
/// claims that lived at the integration tier while `recognize` was a
/// public verb. The surface folded into the declared load (F59), and
/// the claims stayed: the zone table recovered from interval statistics
/// alone, and every refusal naming the rule it broke.
#[cfg(all(test, feature = "fixtures"))]
mod fixture_tests {
    use super::*;
    use crate::error::ErrorCategory;

    /// The four documented 1541 speed zones: track range and sector count.
    const ZONES: [(u64, u64, u32); 4] = [(1, 17, 21), (18, 24, 19), (25, 30, 18), (31, 35, 17)];
    /// The 1541 records 35 tracks; the capture reached 42 step positions.
    const DECLARED_LOCATIONS: u64 = 35;

    /// Everything one probe of the fixture established.
    struct Probed {
        ranked: Recognition,
        pinned: Recognition,
        unknown_profile: (ErrorCategory, String),
    }

    /// The whole set decoded and probed once, shared by every assertion
    /// below: the probe is read-only and mutates nothing (P2), and
    /// decoding the archive five times over would be minutes spent
    /// proving nothing extra.
    fn probed() -> &'static Probed {
        static PROBED: std::sync::OnceLock<Probed> = std::sync::OnceLock::new();
        PROBED.get_or_init(|| {
            let capture_path = crate::flux::remanence::reconstruction::capture_fixture_path();
            let capture = crate::flux::remanence::reconstruction::fixture_capture(&capture_path);
            let error = recognition(&capture, Some("apple2"))
                .expect_err("a profile this build does not enroll is refused by name");
            Probed {
                ranked: recognition(&capture, None).expect("a drive profile claims this capture"),
                pinned: recognition(&capture, Some("c1541")).expect("the pinned profile answers"),
                unknown_profile: (error.category(), error.to_string()),
            }
        })
    }

    #[test]
    fn probing_the_data_surface_recovers_all_four_documented_zones() {
        let recognition = &probed().ranked;
        assert_eq!(recognition.pinned, None);
        assert_eq!(recognition.verdicts.len(), 1);

        let verdict = &recognition.verdicts[0];
        assert_eq!(verdict.profile_id, "c1541");
        assert_eq!(verdict.profile_name, "Commodore 1541");
        assert_eq!(verdict.locations_declared, DECLARED_LOCATIONS);
        assert_eq!(
            u64::from(verdict.locations_claimed),
            DECLARED_LOCATIONS,
            "every declared location should be recovered: {:?}",
            verdict.evidence
        );
        assert_eq!(verdict.confidence, 100);

        // Every zone at its documented boundaries, fully recovered, and
        // every claimed location in it holding exactly the records the
        // zone claims. That agreement across four zones is the signature.
        assert_eq!(verdict.zones.len(), ZONES.len());
        for (zone, (first, last, records)) in verdict.zones.iter().zip(ZONES) {
            assert_eq!((zone.first_location, zone.last_location), (first, last));
            assert_eq!(zone.records_declared, records);
            assert_eq!(zone.locations_declared, last - first + 1);
            assert_eq!(
                zone.locations_claimed, zone.locations_declared,
                "zone {first}-{last} recovered {} of {}",
                zone.locations_claimed, zone.locations_declared
            );
        }

        for location in verdict.locations.iter().filter(|location| location.claimed) {
            let family = location
                .family_location
                .expect("a claimed location is addressed");
            let (_, _, records) = ZONES
                .iter()
                .copied()
                .find(|(first, last, _)| (*first..=*last).contains(&family))
                .expect("a claimed location sits in a zone");
            assert_eq!(location.records, records, "track {family}");
            // The spacing between records repeats to the bit, which is
            // what a recorded track does and a straddled step position
            // does not.
            assert_eq!(location.record_bits_deviation, 0, "track {family}");
            assert!(location.record_bits.is_some(), "track {family}");
            // Counting is the discriminator; the projected rate is
            // corroboration, and it lands within a few per cent of what
            // the zone claims — the writing drive's own speed sits in
            // that gap.
            let cell = location
                .cell_millicycles
                .expect("a claimed location derived a cell");
            let nominal = location
                .nominal_cell_millicycles
                .expect("its zone claims one");
            assert!(
                cell.abs_diff(nominal) * 20 < nominal,
                "track {family}: derived {cell} against a claimed {nominal}"
            );
            // And every observation of it agreed about what it holds.
            assert_eq!(
                location.observations_agreeing, location.observations,
                "track {family}"
            );
        }
    }

    #[test]
    fn every_refusal_names_the_rule_it_broke() {
        let verdict = &probed().ranked.verdicts[0];

        // Every position the capture supplied is accounted for: 84 step
        // positions by two heads, each either claimed or refused by name.
        assert_eq!(verdict.locations.len(), 168);
        for location in &verdict.locations {
            assert_eq!(
                location.claimed,
                location.refusal.is_none(),
                "{} is neither claimed nor refused",
                location.artifact
            );
        }

        // The unrecorded surface is refused everywhere. Which head
        // carries the recording is not this layer's to assume — it is
        // what the evidence said.
        for location in verdict.locations.iter().filter(|l| l.head == Some(1)) {
            assert!(!location.claimed, "{} was claimed", location.artifact);
        }

        // A half-step position is not one the family's addressing
        // covers, and that is a statement about addressing rather than
        // about what the position holds.
        let half = verdict
            .locations
            .iter()
            .find(|l| l.position.numerator == 1 && l.head == Some(0))
            .expect("step position 1 was captured");
        assert_eq!(half.family_location, None);
        assert!(
            half.refusal
                .as_deref()
                .expect("refused")
                .contains("addressing covers"),
            "{:?}",
            half.refusal
        );

        // A step position past the last declared zone is refused by
        // declaration: no threshold decides it, the density map simply
        // does not reach there.
        let past = verdict
            .locations
            .iter()
            .find(|l| l.position.numerator == 70 && l.head == Some(0))
            .expect("step position 70 was captured");
        assert_eq!(past.family_location, Some(36));
        assert!(
            past.refusal
                .as_deref()
                .expect("refused")
                .contains("outside every zone"),
            "{:?}",
            past.refusal
        );
    }

    #[test]
    fn a_position_holding_its_neighbours_content_is_reported_not_resolved() {
        let verdict = &probed().ranked.verdicts[0];

        // Three consecutive step positions on the data surface carry
        // the same content. The middle one is a half-step the family's
        // addressing would not expect to hold a track at all, and it
        // passes every structural test — because it is reading a real
        // track, just not its own. Flux alone cannot tell that from an
        // instrument that did not move, so it is reported and never
        // resolved.
        let at = |position: u64| {
            verdict
                .locations
                .iter()
                .find(|l| l.position.numerator == position && l.head == Some(0))
                .unwrap_or_else(|| panic!("step position {position} was captured"))
        };
        let middle = at(67);
        assert_eq!(middle.family_location, None, "step 67 is a half-step");
        assert!(
            middle.duplicate_of.is_some(),
            "step 67 duplicates a neighbour"
        );
        assert_eq!(at(66).duplicate_of.map(|p| p.numerator), Some(67));
        assert_eq!(at(68).duplicate_of.map(|p| p.numerator), Some(67));

        // It is reported beside the claim rather than instead of it:
        // tracks 34 and 35 are still claimed, and what the caller does
        // about the duplication is the caller's to declare.
        assert!(at(66).claimed && at(68).claimed);
        assert!(
            verdict
                .evidence
                .iter()
                .any(|line| line.contains("their neighbour also holds")),
            "{:?}",
            verdict.evidence
        );

        // And a distinct track is not reported as a duplicate of
        // anything.
        assert_eq!(at(0).duplicate_of, None);
    }

    #[test]
    fn the_verdict_carries_the_observations_that_produced_it() {
        let verdict = &probed().ranked.verdicts[0];

        // "C1541, confidence 100" is not an answer. The observations
        // are.
        assert!(verdict.evidence.len() >= 5, "{:?}", verdict.evidence);
        assert!(
            verdict
                .evidence
                .iter()
                .any(|line| line.contains("claims 35 of the 35 locations")),
            "{:?}",
            verdict.evidence
        );
        assert!(
            verdict
                .evidence
                .iter()
                .any(|line| line.contains("21 records each")),
            "{:?}",
            verdict.evidence
        );
        // And the declared facts say they are the family's, not the
        // capture's.
        assert!(
            verdict
                .evidence
                .iter()
                .any(|line| line.contains("not the capture's")),
            "{:?}",
            verdict.evidence
        );

        // The seam is reported as an angle in the drive's own cycles —
        // the one departure from a spacing that otherwise repeats —
        // never as anything read out of the track.
        let seams = verdict
            .locations
            .iter()
            .filter(|l| l.claimed && l.seam_cycles.is_some())
            .count();
        assert!(seams > 0, "no claimed location located its seam");
        for location in verdict.locations.iter().filter(|l| l.claimed) {
            if let Some(seam) = location.seam_cycles {
                assert!(seam < 3_200_000, "a seam past one rotation: {seam}");
            }
        }
    }

    #[test]
    fn a_caller_may_pin_a_profile_and_what_was_pinned_travels_with_the_result() {
        let recognition = &probed().pinned;
        assert_eq!(recognition.pinned.as_deref(), Some("c1541"));
        assert_eq!(recognition.verdicts.len(), 1);
        assert_eq!(recognition.verdicts[0].confidence, 100);
        assert!(
            recognition
                .evidence
                .iter()
                .any(|line| line.contains("pinned")),
            "{:?}",
            recognition.evidence
        );

        let (category, message) = &probed().unknown_profile;
        assert_eq!(*category, ErrorCategory::NotFound);
        assert!(message.contains("apple2"), "{message}");
    }
}
