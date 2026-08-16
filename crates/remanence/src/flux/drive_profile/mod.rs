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
//! A profile's **materialization half** is read by the flux-to-medium
//! reduction, which is not delivered. Every entry declares it beside
//! its recognition half deliberately, because these are facts about the
//! same family and splitting them across two places is how two features
//! come to hold different answers about one drive.
//!
//! **The seam is four files.** This one is the declaration vocabulary —
//! the types a family's facts are stated in, and the enrollment list —
//! and it holds no entry and no behaviour of its own. [`entries`] holds
//! the enrolled families, each declared whole in one place.
//! [`intervals`] is the measurement: the cell derived from the
//! population it is self-consistent with, and every interval classified
//! into a declared multiple by exact integer arithmetic. [`verdict`] is
//! what that measurement is reported as — `probe` over one profile,
//! `recognize`/`recognize_as` over the enrollment, and `recognition`
//! as the whole act. The dependency runs one way: verdict reads
//! intervals, and intervals reads only the vocabulary.
#![allow(dead_code)]

mod entries;
mod intervals;
mod verdict;

pub(crate) use entries::C1541;
pub(crate) use verdict::recognition;

use crate::model::media_profile::MediaProfile;

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
    pub(crate) fn nominal_cell(&self, rotation: &Rotation) -> (u128, u128) {
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

/// How the family's read channel clocks a medium's pulses into bit
/// cells.
///
/// These are the mechanics and read-channel rules the presentation
/// above the medium is materialized under. They belong to the family
/// exactly as the recording conventions above them do: a medium records
/// pulses and says nothing about the counter that turns them into bits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReadChannel {
    /// A detected transition restarts the cell counter, so the next cell
    /// boundary falls one whole cell after the transition rather than
    /// one whole cell after the boundary it displaced.
    pub(crate) resync_on_transition: bool,
    /// How far past a cell boundary a transition may still arrive and be
    /// admitted into the cell it opened, as a fraction of the cell.
    ///
    /// It is what makes the channel phase-locked rather than a boundary
    /// comparison: at one half, a transition is admitted into whichever
    /// cell boundary it is nearest, which is the only value that does
    /// not read a disk running slightly fast as a disk with extra bits
    /// on it. Declaring it is what keeps the number out of the code.
    pub(crate) window_numerator: u64,
    pub(crate) window_denominator: u64,
    /// How many consecutive one bits the family's byte-framing landmark
    /// is. `EncodingShape::landmark` states the same convention one
    /// layer down, as a run of shortest intervals; a run of `n`
    /// intervals is `n + 1` one bits, and the two are checked against
    /// each other rather than one being derived from the other.
    pub(crate) alignment_one_bits: u32,
}

/// The family's declared group code: how many bits of the recording
/// carry how many bits of a byte, and which symbol each value is
/// recorded as.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupCodec {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) symbol_bits: u32,
    pub(crate) data_bits: u32,
    /// The symbol each data value is recorded as, indexed by the value.
    /// A bit pattern this table does not hold is not a symbol, and the
    /// codec says so rather than choosing the nearest entry.
    pub(crate) symbols: &'static [u16],
    pub(crate) provenance: &'static str,
}

impl GroupCodec {
    /// How many symbols make one byte. Derived rather than declared, so
    /// the two cannot disagree; a code whose symbols do not divide a
    /// byte states no byte at all.
    pub(crate) fn symbols_per_byte(&self) -> Option<u32> {
        (self.data_bits > 0 && 8 % self.data_bits == 0).then(|| 8 / self.data_bits)
    }

    /// The value a symbol records, or `None` where the pattern is not
    /// one the family assigns.
    pub(crate) fn value_of(&self, symbol: u16) -> Option<u8> {
        self.symbols
            .iter()
            .position(|candidate| *candidate == symbol)
            .map(|value| value as u8)
    }
}

/// How the family computes the checksum a block states over its own
/// bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChecksumRule {
    /// Every checked byte exclusive-ored together.
    Xor,
}

impl ChecksumRule {
    pub(crate) fn over(self, bytes: &[u8]) -> u8 {
        match self {
            Self::Xor => bytes.iter().fold(0u8, |sum, byte| sum ^ byte),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Xor => "the exclusive-or of every checked byte",
        }
    }
}

/// One block of the family's record, as it is written.
///
/// The mark is read like any other byte and introduces nothing on its
/// own: the layer below located a framing landmark and said nothing
/// whatever about what follows it, so what makes these bytes a header
/// is this declaration and the reading above it, never the landmark.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockShape {
    pub(crate) id: &'static str,
    /// The byte the family opens the block with, which is what says
    /// which of its blocks this is.
    pub(crate) mark: u8,
    /// The whole block, the mark included.
    pub(crate) bytes: u32,
    /// Which byte states the checksum, and the half-open span of the
    /// block the family takes it over.
    pub(crate) checksum_at: u32,
    pub(crate) checked_from: u32,
    pub(crate) checked_to: u32,
}

/// The family's record grammar: the blocks one sector is written as,
/// where its header states the address, and where the payload sits.
///
/// A record's data block is the block the recording carries next after
/// its header — the family writes one sync ahead of each — so pairing
/// is grammatical rather than a distance anybody tuned.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecordGrammar {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) checksum: ChecksumRule,
    pub(crate) header: BlockShape,
    pub(crate) data: BlockShape,
    /// Where the header states the address, as byte offsets into it.
    pub(crate) track_at: u32,
    pub(crate) sector_at: u32,
    /// The two identity bytes the header carries, in the order the
    /// family writes them.
    pub(crate) id_high_at: u32,
    pub(crate) id_low_at: u32,
    /// The payload's half-open span within the data block.
    pub(crate) payload_from: u32,
    pub(crate) payload_to: u32,
    pub(crate) provenance: &'static str,
}

impl RecordGrammar {
    /// The block a byte opens, or `None` for one that opens neither. A
    /// framed byte the grammar does not name opens no block, and this
    /// says so rather than choosing the nearer of the two marks.
    pub(crate) fn block_of(&self, mark: u8) -> Option<&BlockShape> {
        if mark == self.header.mark {
            Some(&self.header)
        } else if mark == self.data.mark {
            Some(&self.data)
        } else {
            None
        }
    }

    /// How many bytes of payload one data block carries.
    pub(crate) fn payload_bytes(&self) -> u32 {
        self.payload_to.saturating_sub(self.payload_from)
    }
}

/// The half of a profile the medium-to-bitstream and
/// bitstream-to-bytestream materializations read, and the record
/// grammar the sector layer above them recognizes under.
///
/// It sits beside the recognition and materialization halves for the
/// same reason they sit together: these are facts about one drive, and
/// splitting them across two places is how two features come to hold
/// different answers about it.
///
/// The three `*_policy` fields are the family's **declared defaults**
/// for the transitions above the medium (P30 reached through the type):
/// being a medium of this family *means* reading through this channel,
/// codec and record reading, so the argument-free presentation verbs
/// take their policy from here and every value used travels into the
/// result as provenance (P29). A choice no family convention could make
/// would refuse by name instead of sitting here as a quiet default.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Presentation {
    pub(crate) read_channel: ReadChannel,
    pub(crate) codec: GroupCodec,
    pub(crate) record: RecordGrammar,
    /// The declared medium-to-bitstream policy, for the one channel
    /// every enrolled family is clocked by.
    pub(crate) channel_policy: crate::flux::presentation::ReadChannelPolicy,
    /// The declared bitstream-to-bytestream policy.
    pub(crate) codec_policy: crate::flux::c1541::presentation::GcrCodecPolicy,
    /// The declared bytestream-to-sector reading.
    pub(crate) sector_policy: crate::flux::c1541::sectors::SectorPolicy,
    /// **The family's own bitstream-to-bytestream transition.** Bits
    /// become bytes by a rule that differs in kind between families, so
    /// the profile carries the transition as behavior rather than as a
    /// declaration central code would have to interpret (P12): enrolling
    /// a family enrols its codec here, and the rung above branches on
    /// nothing.
    pub(crate) bytestream: fn(
        &crate::flux::presentation::Bitstream,
        u64,
    ) -> crate::error::Result<crate::flux::presentation::Bytestream>,
}

/// One family's recording conventions, and the published description
/// each declared fact derives from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DriveProfile {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) version: u32,
    pub(crate) provenance: &'static str,
    /// The medium this family is served (P14). A drive declaring which
    /// article it accepts is a compatibility fact of the family, the
    /// same class as every other declaration here; the medium's own
    /// facts stay in the article catalog, which knows nothing about
    /// this drive and holds no behavior of its own.
    pub(crate) media: &'static MediaProfile,
    pub(crate) stepping: Stepping,
    pub(crate) rotation: Rotation,
    pub(crate) surfaces: Surfaces,
    pub(crate) encoding: EncodingShape,
    pub(crate) density: &'static [DensityZone],
    pub(crate) materialization: Materialization,
    pub(crate) presentation: Presentation,
}

impl DriveProfile {
    fn zone_for(&self, location: u64) -> Option<(usize, &DensityZone)> {
        self.density
            .iter()
            .enumerate()
            .find(|(_, zone)| zone.first_location <= location && location <= zone.last_location)
    }

    /// The zone covering a location the family addresses as an exact
    /// ratio of its steps.
    ///
    /// A half-track between two zones is covered by neither, which is a
    /// fact about the family's declaration rather than a gap to be
    /// closed: no published rate covers it, and choosing a neighbour's
    /// would put an undeclared number into the presentation.
    pub(crate) fn zone_for_ratio(
        &self,
        numerator: u64,
        denominator: u64,
    ) -> Option<(usize, &DensityZone)> {
        let denominator = u128::from(denominator.max(1));
        let position = u128::from(numerator);
        self.density.iter().enumerate().find(|(_, zone)| {
            u128::from(zone.first_location) * denominator <= position
                && position <= u128::from(zone.last_location) * denominator
        })
    }

    /// How many family locations this profile's density map covers.
    fn declared_locations(&self) -> u64 {
        self.density
            .iter()
            .map(|zone| zone.last_location - zone.first_location + 1)
            .sum()
    }
}

/// The enrolled families, in the order they are consulted. Adding one
/// changes its declaration, its tests, and this list.
static ENROLLED: [&DriveProfile; 1] = [&C1541];

pub(crate) fn enrolled() -> &'static [&'static DriveProfile] {
    &ENROLLED
}

// -------------------------------------------------------------- probing
