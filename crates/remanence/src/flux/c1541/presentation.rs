// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C1541 presentation (F32, P23, P33): materializing a hardware bitstream
//! from a flux medium under declared mechanics and read-channel rules,
//! and then the family's encoded bytestream from that bitstream.
//!
//! Two transitions, in one direction. A medium holds pulses; the read
//! channel clocks them into bit cells; the declared group code resolves
//! those bits into bytes. Each transition is atomic — the whole layer
//! materializes or none of it does — and each carries what produced it:
//! the medium's own selection policy, then the channel, then the codec,
//! in that order and with nothing dropped on the way up.
//!
//! **The profile owns every rule either transition applies** (P30). The
//! cell comes from the family's declared density map, the resync
//! behavior from its declared read channel, the framing landmark from
//! its declared bit convention, and the byte values from its declared
//! group-code table. Nothing here derives any of them from what it is
//! reading, and neither a capture adapter nor either flux model has an
//! opinion about what a drive observes.
//!
//! **It assigns nothing above a byte.** The bytestream is a circular,
//! track-relative byte sequence. No byte here is a header, a data field,
//! a sector or a file; the codec locates the family's alignment landmark
//! because framing has to begin somewhere the family declares, and
//! having located one it says nothing whatever about what follows it.
//!
//! **There is no way back down.** Returning to a medium is a separate,
//! explicit mastering operation governed by P33, P13 and P29, and this
//! feature does not perform one: a bitstream is not lowered into pulses
//! and a bytestream is not lowered into bits.
//!
//! What the presentation must never do is normalize several passes over
//! one location into a preferred revolution. It cannot: a medium already
//! holds exactly one pulse stream per location, chosen by a rule the P29
//! policy named, and that rule travels into everything below.

use crate::error::{Error, Result};
use crate::evidence::{DeclaredLoss, LossAccount, Provenance};
use crate::flux::bitstream::{
    BitCell, BitEvidence, BitstreamBuilder, BitstreamFact, BitstreamFactKind, HardwareBitstream,
};
use crate::flux::bytestream::{
    ByteOutcome, ByteRecord, BytestreamBuilder, BytestreamFact, BytestreamFactKind,
    EncodedBytestream,
};
use crate::flux::capture::SessionBacking;
use crate::flux::drive_profile::{C1541, DriveProfile, GroupCodec};
use crate::flux::medium::{FluxMedium, LocationKey, MediumFactKind, Pulse};

/// The profile both transitions are declared by.
const PROFILE: &str = "c1541";

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image(PROFILE, reason)
}

// ------------------------------------------------- read-channel policy

/// Which declared density the channel clocks a location at.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DensityPolicy {
    /// The zone the family's density map declares for the location. A
    /// location no zone covers — a half-track between two of them — is
    /// answered by [`UnzonedPolicy`], because no published rate reaches
    /// it and a neighbour's would be an undeclared number.
    Declared,
    /// One zone for every location, which is what a drive does when its
    /// controller selects a density and steps without changing it.
    Fixed { zone: u32 },
}

/// What a location no declared zone covers becomes.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnzonedPolicy {
    /// The materialization stops and names the location.
    Refuse,
    /// It is left out and counted; the bitstream claims nothing there.
    Omit,
}

/// How a pulse the medium states does not read the same every time
/// becomes a definite bit.
///
/// The medium's strength vocabulary cannot survive into a bit — a cell
/// reads one or zero — so this is where that is decided, and the answer
/// travels with every bit it produced.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WeakPulsePolicy {
    /// Every such pulse is taken as detected, or as not, uniformly.
    Declared { detected: bool },
    /// Each is resolved reproducibly from the policy's seed and the
    /// pulse's own angle, so the same medium and seed give the same
    /// bitstream (P29).
    Seeded,
}

/// The complete declared policy for one medium-to-bitstream transition.
///
/// There is no `Default`: every field is a decision about evidence, and
/// a channel that arrived at one by construction rather than by
/// declaration is what P30 exists to prevent. The one every caller
/// reaches is the family's own — the profile's declared
/// `channel_policy` (P30 reached through the type) — which is why
/// nothing here is public: the argument-free presentation verbs read
/// the declaration off the profile, and what was used travels into the
/// result as provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadChannelPolicy {
    pub(crate) density: DensityPolicy,
    pub(crate) unzoned: UnzonedPolicy,
    pub(crate) weak_pulse: WeakPulsePolicy,
    /// What makes any stochastic element of the channel reproducible.
    pub(crate) seed: u64,
}

// ------------------------------------------------------- codec policy

/// Where byte framing begins.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignmentPolicy {
    /// At the family's declared landmark, and nowhere else: bits before
    /// the first one are stated as unframed rather than guessed into
    /// bytes.
    Landmark,
    /// At the circle's own origin as well, the caller declaring that the
    /// medium's start is a byte boundary. Landmarks still reframe.
    Origin,
}

/// What a group holding a pattern the family's table does not assign
/// becomes.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnassignedSymbolPolicy {
    /// The materialization stops and names the location and the bit.
    Refuse,
    /// The group keeps its own bits, stated as unresolved, and is
    /// counted. There is no nearest entry in the table.
    DeclareLoss,
}

/// The complete declared policy for one bitstream-to-bytestream
/// transition — the profile's declared `codec_policy` in every
/// delivered journey, exactly as the channel policy above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GcrCodecPolicy {
    pub(crate) alignment: AlignmentPolicy,
    pub(crate) unassigned_symbol: UnassignedSymbolPolicy,
}

// -------------------------------------------------------- the reporting

/// One location the bitstream holds, and what the channel resolved
/// there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitstreamLocation {
    /// The family half-track this addresses, as an exact ratio.
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    /// Which declared density zone supplied the cell.
    pub zone: u32,
    /// The cell, in reference-clock cycles, exactly.
    pub cell_cycles_numerator: u64,
    pub cell_cycles_denominator: u64,
    pub cells: u64,
    pub one_bits: u64,
    /// Bits that are what the medium recorded, and bits a declared rule
    /// resolved. They sum to `cells`.
    pub recorded_bits: u64,
    pub resolved_bits: u64,
    /// Cells the channel closed shorter than one cell, two transitions
    /// having arrived closer together than the encoding admits.
    pub short_cells: u64,
    /// The longest run of zero cells, in cells. The encoding admits
    /// runs up to one less than its longest interval; anything past
    /// that is where the recording departed from it.
    pub longest_zero_run: u64,
    /// What is left of the circle after the last whole cell, over
    /// `cell_cycles_denominator`. A circle does not divide into cells,
    /// and this states the remainder rather than rounding it into a bit.
    pub wrap_slack_numerator: u64,
}

/// What one medium-to-bitstream transition produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitstreamReport {
    pub profile_id: String,
    pub profile_name: String,
    pub profile_version: u32,
    pub reference_clock_hz: u64,
    pub cycles_per_rotation: u64,
    pub locations: Vec<BitstreamLocation>,
    /// Everything the bitstream does not carry of the medium beneath it.
    pub declared_loss: Vec<DeclaredLoss>,
    /// The channel that produced it and the policy that produced the
    /// medium, in that order (P4).
    pub evidence: Vec<String>,
}

/// One location the bytestream holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytestreamLocation {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    pub bytes: u64,
    pub resolved_bytes: u64,
    /// Groups holding a pattern the family's table does not assign.
    pub unassigned_groups: u64,
    /// How many times framing was established on the family's landmark.
    /// It says where bytes begin and nothing about what they are.
    pub alignments: u64,
    pub longest_landmark_bits: u64,
    pub unframed_bits: u64,
}

/// What one bitstream-to-bytestream transition produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytestreamReport {
    pub profile_id: String,
    pub codec_id: String,
    pub codec_name: String,
    pub symbol_bits: u32,
    pub data_bits: u32,
    pub symbols_per_byte: u32,
    pub locations: Vec<BytestreamLocation>,
    pub declared_loss: Vec<DeclaredLoss>,
    pub evidence: Vec<String>,
}

/// A hardware bitstream, held in the session.
///
/// The bits stay behind this surface. What a hardware presentation makes
/// of them is that seam's business, and there is no public bit iterator
/// here or anywhere.
pub struct C1541Bitstream {
    bitstream: HardwareBitstream,
    report: BitstreamReport,
}

impl std::fmt::Debug for C1541Bitstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("C1541Bitstream")
            .field("locations", &self.report.locations.len())
            .field("backing_bytes", &self.bitstream.backing_bytes())
            .finish()
    }
}

impl C1541Bitstream {
    /// The transition that produced this bitstream, and everything it
    /// does not carry of the medium beneath it.
    pub fn inspect(&self) -> &BitstreamReport {
        &self.report
    }

    /// How many locations the bitstream claims.
    pub fn location_count(&self) -> u64 {
        self.bitstream.locations().count() as u64
    }

    /// How many bytes of private session storage the bitstream occupies.
    pub fn backing_bytes(&self) -> u64 {
        self.bitstream.backing_bytes()
    }

    pub fn resident_bytes(&self) -> u64 {
        self.bitstream.resident_bytes()
    }

    /// Materializes the family's encoded bytestream from this bitstream
    /// under the family's declared group code and codec reading (P23,
    /// P30 reached through the type, P33).
    ///
    /// It takes no policy because the type carries one: being a
    /// bitstream of this family *means* resolving through the profile's
    /// declared codec policy, and what was used travels into the result
    /// as provenance. The bitstream is untouched and stays exactly what
    /// it was; the bytestream is separate session state with its own
    /// provenance, which is this bitstream's with the codec added to it.
    pub fn materialize_c1541_bytestream(&self, cache_bytes: u64) -> Result<C1541Bytestream> {
        materialize_bytestream(
            &self.bitstream,
            C1541.presentation.codec_policy,
            cache_bytes,
        )
    }
}

/// An encoded bytestream, held in the session.
pub struct C1541Bytestream {
    bytestream: EncodedBytestream,
    report: BytestreamReport,
}

impl std::fmt::Debug for C1541Bytestream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("C1541Bytestream")
            .field("locations", &self.report.locations.len())
            .field("backing_bytes", &self.bytestream.backing_bytes())
            .finish()
    }
}

impl C1541Bytestream {
    pub fn inspect(&self) -> &BytestreamReport {
        &self.report
    }

    /// How many locations the bytestream resolves.
    pub fn location_count(&self) -> u64 {
        self.bytestream.locations().count() as u64
    }

    pub fn backing_bytes(&self) -> u64 {
        self.bytestream.backing_bytes()
    }

    pub fn resident_bytes(&self) -> u64 {
        self.bytestream.resident_bytes()
    }

    /// The framed bytes one location holds, addressed in the family's
    /// own terms.
    ///
    /// A location the stream does not hold is refused naming what it
    /// does hold: the stream's locations are what the medium carried,
    /// and nothing is manufactured to answer for a track that is not
    /// there.
    pub fn location(&self, at: Location) -> Result<LocationBytes<'_>> {
        let location = self
            .bytestream
            .locations()
            .find(|location| {
                let (numerator, denominator) = location.key().position();
                u128::from(numerator) * u128::from(at.denominator)
                    == u128::from(at.numerator) * u128::from(denominator)
            })
            .ok_or_else(|| {
                Error::not_found(format!(
                    "this bytestream holds no {at}: the medium beneath it carried \
                     {} locations, and a track it does not hold is absent rather \
                     than blank",
                    self.report.locations.len()
                ))
            })?;
        Ok(LocationBytes {
            stream: &self.bytestream,
            location,
            at,
        })
    }

    /// The bytestream itself. The unframed bits stay behind the public
    /// surface: the sector layer reads the records out of these bytes,
    /// and [`location`](Self::location) serves the framed bytes alone.
    pub(crate) fn inner(&self) -> &EncodedBytestream {
        &self.bytestream
    }
}

/// One location of the family's addressing, spelled the family's own
/// way: the Commodore 1541 numbers its tracks from 1.
///
/// This is the argument of the kind-declared read verbs (P30 reached
/// through the type): the channel, the codec and the framing are the
/// profile's declarations, so a location is the whole of what a caller
/// states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    numerator: u64,
    denominator: u64,
}

impl Location {
    /// The family location one whole track addresses.
    pub fn track(track: u32) -> Self {
        Self {
            numerator: u64::from(track),
            denominator: 1,
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.denominator == 1 {
            write!(f, "track {}", self.numerator)
        } else {
            write!(f, "track {}/{}", self.numerator, self.denominator)
        }
    }
}

/// The framed bytes one location holds — the byte sequence the declared
/// group code resolved there, and nothing beneath it.
///
/// Bytes number from the first framed byte, because nothing before sync
/// is a byte at all; the unframed bits and the bit-level state stay
/// behind the surface, reported in the stream's own account.
#[derive(Debug)]
pub struct LocationBytes<'a> {
    stream: &'a EncodedBytestream,
    location: &'a crate::flux::bytestream::Location,
    at: Location,
}

impl LocationBytes<'_> {
    /// How many framed bytes this location holds.
    pub fn bytes(&self) -> u64 {
        self.location.bytes()
    }

    /// Reads framed bytes at `offset` into `buf`, whole or not at all.
    ///
    /// A byte whose recorded pattern the family's table does not assign
    /// has no value to serve: it is stated as unresolved in the
    /// stream's account, and a read that touches one is refused naming
    /// it rather than answered with an invented value.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let held = self.location.bytes();
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or_else(|| refuse("the read's extent does not fit in an offset"))?;
        if end > held {
            return Err(refuse(format!(
                "{} holds {held} framed bytes and the read wants {offset}..{end}: \
                 nothing before the first sync is a byte at all, and nothing past \
                 the circle's last frame is either",
                self.at
            )));
        }
        let records = self.stream.bytes(self.location)?;
        for (slot, record) in buf.iter_mut().zip(&records[offset as usize..end as usize]) {
            *slot = record.value().ok_or_else(|| {
                refuse(format!(
                    "a byte in {offset}..{end} of {} holds a pattern the family's \
                     table does not assign: it is stated as unresolved in the \
                     stream's account, and serving a value for it would invent one",
                    self.at
                ))
            })?;
        }
        Ok(())
    }
}

// -------------------------------------------------------- the channel

/// Reduces an exact ratio, so one cell has one spelling.
fn reduce(numerator: u128, denominator: u128) -> (u128, u128) {
    let (mut a, mut b) = (numerator, denominator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let divisor = a.max(1);
    (numerator / divisor, denominator / divisor)
}

/// Mixes a seed, a location and an angle into one reproducible bit.
///
/// It is a hash and nothing more: the point is that the same medium,
/// seed and pulse always resolve the same way, so a seeded reading is
/// repeatable rather than merely arbitrary (P29).
fn seeded_bit(seed: u64, key: &LocationKey, position: u64) -> bool {
    let (numerator, denominator) = key.position();
    let mut state = seed
        ^ numerator.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ denominator.wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ key
            .surface()
            .unwrap_or(u64::MAX)
            .wrapping_mul(0x1656_67b1_9e37_79f9)
        ^ position.wrapping_mul(0xff51_afd7_ed55_8ccd);
    state ^= state >> 30;
    state = state.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    state ^= state >> 27;
    state = state.wrapping_mul(0x94d0_49bb_1331_11eb);
    state ^= state >> 31;
    state & 1 == 1
}

/// Whether the channel detects a pulse the medium states this way, and
/// what the resulting bit's evidence is.
///
/// The vocabulary is the profile's and is ordered weakest first, so its
/// first state never triggers, its last always does, and anything
/// between is a pulse that sometimes does. A state outside it is refused
/// rather than placed somewhere plausible.
fn detect(
    profile: &DriveProfile,
    key: &LocationKey,
    pulse: &Pulse,
    policy: &ReadChannelPolicy,
) -> Result<(bool, BitEvidence)> {
    let states = profile.materialization.strength_states;
    if states.len() < 2 {
        return Err(refuse(format!(
            "the profile declares {} strength state(s), which cannot distinguish a \
             pulse that triggers from one that does not",
            states.len()
        )));
    }
    let state = pulse.strength().state() as usize;
    if state >= states.len() {
        return Err(refuse(format!(
            "a pulse carries the strength state {state} and the family declares \
             {states:?}; the channel will not read a state the profile does not name"
        )));
    }
    if state == 0 {
        return Ok((false, BitEvidence::Recorded));
    }
    if state == states.len() - 1 {
        return Ok((true, BitEvidence::Recorded));
    }
    Ok(match policy.weak_pulse {
        WeakPulsePolicy::Declared { detected } => (detected, BitEvidence::Declared),
        WeakPulsePolicy::Seeded => (
            seeded_bit(policy.seed, key, pulse.position()),
            BitEvidence::Seeded {
                domain: policy.seed,
            },
        ),
    })
}

/// What the channel resolved at one location.
struct Clocked {
    cells: Vec<BitCell>,
    facts: Vec<BitstreamFact>,
    longest_zero_run: u64,
    short_cells: u64,
    wrap_slack: u64,
}

/// Clocks one location's pulses into bit cells.
///
/// The counter runs at the zone's declared cell and restarts at every
/// detected transition, which is the declared read channel and not a
/// model chosen here. A cell holding a transition reads one and closes
/// where the transition arrived; every cell that passes without one
/// reads zero and closes a whole cell later.
///
/// How many cells a transition is away is decided by the channel's own
/// declared window rather than by comparing against the next boundary:
/// at half a cell the channel locks onto the recording's phase, which is
/// what stops a disk running a little fast from reading as a disk with
/// extra bits on it.
fn clock(
    profile: &DriveProfile,
    key: &LocationKey,
    pulses: &[Pulse],
    (cell, scale): (u64, u64),
    rotation_scaled: u64,
    policy: &ReadChannelPolicy,
    loss: &mut LossAccount,
) -> Result<Clocked> {
    // The longest run of zeros the encoding admits between transitions:
    // an interval of k cells is k - 1 zeros and then a one.
    let admitted_zeros = u64::from(
        profile
            .encoding
            .cell_multiples
            .iter()
            .copied()
            .max()
            .unwrap_or(1)
            .saturating_sub(1),
    );

    let mut cells: Vec<BitCell> = Vec::new();
    let mut facts: Vec<BitstreamFact> = Vec::new();
    let mut end = 0u64;
    let mut zero_run = 0u64;
    let mut longest_zero_run = 0u64;
    let mut short_cells = 0u64;
    let mut undetected = 0u64;
    let mut resolved = 0u64;

    let close_run = |cells: &Vec<BitCell>,
                     facts: &mut Vec<BitstreamFact>,
                     zero_run: &mut u64,
                     longest: &mut u64| {
        *longest = (*longest).max(*zero_run);
        if *zero_run > admitted_zeros {
            facts.push(BitstreamFact::new(
                BitstreamFactKind::LongZeroRun {
                    at_bit: cells.len() as u64 - *zero_run,
                    cells: *zero_run,
                },
                Provenance::new(PROFILE).note(format!(
                    "a run of {} cells without a transition, where the family's \
                     encoding admits {admitted_zeros}",
                    *zero_run
                )),
            ));
        }
        *zero_run = 0;
    };

    let channel = &profile.presentation.read_channel;
    for pulse in pulses {
        let (detected, evidence) = detect(profile, key, pulse, policy)?;
        if !detected {
            undetected += 1;
            continue;
        }
        if !evidence.is_recorded() {
            resolved += 1;
        }
        let position = pulse
            .position()
            .checked_mul(scale)
            .ok_or_else(|| refuse("a pulse angle scales past what exact arithmetic holds here"))?;
        // How many cells this transition sits away, by the declared
        // window: the cell whose boundary it is nearest.
        let span = u128::from(position - end);
        let mut carried = u64::try_from(
            (span * u128::from(channel.window_denominator)
                + u128::from(cell) * u128::from(channel.window_numerator))
                / (u128::from(cell) * u128::from(channel.window_denominator)),
        )
        .unwrap_or(u64::MAX);
        if carried == 0 {
            // Two transitions inside one window. The channel cannot hold
            // them a cell apart and neither is discarded: the cell it
            // closes is shorter than the encoding's shortest interval,
            // and that is stated.
            carried = 1;
            short_cells += 1;
            facts.push(BitstreamFact::new(
                BitstreamFactKind::ShortCell {
                    at_bit: cells.len() as u64,
                },
                Provenance::new(PROFILE).note(
                    "two transitions arrived inside one window, closer together than \
                     the channel can hold a cell apart",
                ),
            ));
        }
        for step in 1..carried {
            cells.push(BitCell::new(
                end + cell * step,
                false,
                BitEvidence::Recorded,
            ));
            zero_run += 1;
        }
        close_run(&cells, &mut facts, &mut zero_run, &mut longest_zero_run);
        cells.push(BitCell::new(position, true, evidence));
        end = position;
    }
    // The circle closes: cells keep coming until one would run past it.
    while end + cell <= rotation_scaled {
        end += cell;
        cells.push(BitCell::new(end, false, BitEvidence::Recorded));
        zero_run += 1;
    }
    close_run(&cells, &mut facts, &mut zero_run, &mut longest_zero_run);

    if undetected > 0 {
        loss.add(
            "never-triggering-pulse",
            "pulses the medium states never trigger, which the channel does not detect \
             and no cell records",
            undetected,
        );
    }
    if resolved > 0 {
        loss.add(
            "unexpressed-pulse-strength",
            "pulses the medium states do not read the same every time, resolved to a \
             definite bit by the declared rule; a cell reads one or zero and carries no \
             strength of its own",
            resolved,
        );
    }

    Ok(Clocked {
        cells,
        facts,
        longest_zero_run,
        short_cells,
        wrap_slack: rotation_scaled - end,
    })
}

/// Materializes a hardware bitstream from a flux medium.
pub(crate) fn materialize_bitstream(
    medium: &FluxMedium,
    policy: ReadChannelPolicy,
    cache_bytes: u64,
) -> Result<C1541Bitstream> {
    let profile = &C1541;
    if medium.profile() != profile.id {
        return Err(refuse(format!(
            "the medium is addressed by the profile '{}' and this presentation is \
             declared by '{}'; a drive reads the family it is",
            medium.profile(),
            profile.id
        )));
    }
    let frame = medium.frame();
    let cycles_per_rotation = frame.cycles_per_rotation();
    if cycles_per_rotation != profile.rotation.cycles_per_rotation {
        return Err(refuse(format!(
            "the medium places its pulses on a circle of {cycles_per_rotation} cycles \
             and the profile declares {}; the cell this channel clocks at is the \
             profile's, so the two must describe the same circle",
            profile.rotation.cycles_per_rotation
        )));
    }

    // The channel this module clocks is the phase-locked one: a family
    // whose counter free-runs is a different channel, and reading its
    // medium with this one would put a rule nobody declared into the
    // bits. It is refused by name rather than approximated (P3).
    if !profile.presentation.read_channel.resync_on_transition {
        return Err(refuse(format!(
            "the '{}' profile declares a read channel whose cell counter does not \
             restart at a transition, and this presentation clocks only one that does",
            profile.id
        )));
    }
    if profile.presentation.read_channel.window_denominator == 0 {
        return Err(refuse(
            "the profile declares a window over zero cells, which admits a transition \
             into no cell at all",
        ));
    }

    let channel = describe_channel(profile, &policy, medium.provenance());
    let mut builder = BitstreamBuilder::new(
        profile.id,
        profile.rotation.reference_clock,
        cycles_per_rotation,
        channel.clone(),
        SessionBacking::create()?,
    )?;
    let mut loss = LossAccount::new();
    let mut reported = Vec::new();

    for location in medium.locations() {
        let key = location.key();
        let (numerator, denominator) = key.position();
        let Some((zone_at, zone)) = zone_of(profile, numerator, denominator, &policy)? else {
            loss.add(
                "unzoned-location",
                "locations the family's density map does not cover, which no declared \
                 rate reaches",
                1,
            );
            continue;
        };
        let (cell_numerator, cell_denominator) = reduce_cell(zone.nominal_cell(&profile.rotation))?;
        let rotation_scaled = cycles_per_rotation
            .checked_mul(cell_denominator)
            .ok_or_else(|| refuse("the circle scales past what exact arithmetic holds here"))?;

        let pulses = medium.pulses(location)?;
        let clocked = clock(
            profile,
            key,
            &pulses,
            (cell_numerator, cell_denominator),
            rotation_scaled,
            &policy,
            &mut loss,
        )?;

        let mut facts = clocked.facts;
        for fact in medium.facts(location)? {
            match fact.kind() {
                MediumFactKind::Seam { angle } => facts.push(BitstreamFact::new(
                    BitstreamFactKind::Seam { angle: *angle },
                    Provenance::new(PROFILE)
                        .note("carried from the medium as the angle it is; it frames nothing"),
                )),
                MediumFactKind::Unformatted => facts.push(BitstreamFact::new(
                    BitstreamFactKind::Unformatted,
                    Provenance::new(PROFILE).note("carried from the medium"),
                )),
                MediumFactKind::WriteProtect
                | MediumFactKind::Duplicate { .. }
                | MediumFactKind::SourceFact { .. } => loss.add(
                    "medium-fact",
                    "facts the medium states about a location that a clocked bit \
                     sequence has no place for",
                    1,
                ),
            }
        }

        let cells = clocked.cells.len() as u64;
        let one_bits = clocked.cells.iter().filter(|cell| cell.one()).count() as u64;
        let resolved_bits = clocked
            .cells
            .iter()
            .filter(|cell| !cell.evidence().is_recorded())
            .count() as u64;
        reported.push(BitstreamLocation {
            half_track_numerator: numerator,
            half_track_denominator: denominator,
            surface: key.surface(),
            zone: zone_at as u32,
            cell_cycles_numerator: cell_numerator,
            cell_cycles_denominator: cell_denominator,
            cells,
            one_bits,
            recorded_bits: cells - resolved_bits,
            resolved_bits,
            short_cells: clocked.short_cells,
            longest_zero_run: clocked.longest_zero_run,
            wrap_slack_numerator: clocked.wrap_slack,
        });

        builder.add_location(
            key.clone(),
            zone_at as u32,
            (cell_numerator, cell_denominator),
            clocked.wrap_slack,
            &clocked.cells,
            &facts,
            Provenance::new(PROFILE).note(format!(
                "clocked from the medium's own pulses at the zone {zone_at} cell of \
                 {cell_numerator}/{cell_denominator} reference cycles"
            )),
        )?;
    }

    if reported.is_empty() {
        return Err(refuse(
            "no location of the medium was clocked, so there is no bitstream to hold",
        ));
    }

    let (mut bitstream, sink, total) = builder.seal()?;
    bitstream.attach_backing(Box::new(sink.into_source()), total, cache_bytes);

    let report = BitstreamReport {
        profile_id: profile.id.to_owned(),
        profile_name: profile.name.to_owned(),
        profile_version: profile.version,
        reference_clock_hz: profile.rotation.reference_clock,
        cycles_per_rotation,
        locations: reported,
        declared_loss: loss.into_entries(),
        evidence: channel.notes,
    };
    Ok(C1541Bitstream { bitstream, report })
}

/// The zone a location is clocked at, or `None` where the policy leaves
/// it out.
fn zone_of<'a>(
    profile: &'a DriveProfile,
    numerator: u64,
    denominator: u64,
    policy: &ReadChannelPolicy,
) -> Result<Option<(usize, &'a crate::flux::drive_profile::DensityZone)>> {
    match policy.density {
        DensityPolicy::Fixed { zone } => {
            let at = zone as usize;
            profile
                .density
                .get(at)
                .map(|zone| Some((at, zone)))
                .ok_or_else(|| {
                    refuse(format!(
                        "the policy pins density zone {zone} and the family declares {}; \
                         a zone the profile does not name has no rate to clock at",
                        profile.density.len()
                    ))
                })
        }
        DensityPolicy::Declared => match profile.zone_for_ratio(numerator, denominator) {
            Some(found) => Ok(Some(found)),
            None => match policy.unzoned {
                UnzonedPolicy::Omit => Ok(None),
                UnzonedPolicy::Refuse => Err(refuse(format!(
                    "the family's density map covers no location at {numerator}/{denominator}, \
                     so no declared rate reaches it; a neighbouring zone's would be a \
                     number the family never stated"
                ))),
            },
        },
    }
}

fn reduce_cell((numerator, denominator): (u128, u128)) -> Result<(u64, u64)> {
    let (numerator, denominator) = reduce(numerator, denominator);
    let too_wide = || refuse("a declared cell is wider than exact arithmetic holds here");
    Ok((
        u64::try_from(numerator).map_err(|_| too_wide())?,
        u64::try_from(denominator).map_err(|_| too_wide())?,
    ))
}

fn describe_channel(
    profile: &DriveProfile,
    policy: &ReadChannelPolicy,
    medium: &Provenance,
) -> Provenance {
    let mut provenance = Provenance::new(PROFILE)
        .note(format!(
            "{} version {}: {}",
            profile.name, profile.version, profile.provenance
        ))
        .note(match policy.density {
            DensityPolicy::Declared => {
                "each location is clocked at the cell its declared density zone states".to_owned()
            }
            DensityPolicy::Fixed { zone } => format!(
                "every location is clocked at zone {zone}'s cell, the caller declaring \
                 one density for the whole medium"
            ),
        })
        .note(match policy.unzoned {
            UnzonedPolicy::Refuse => {
                "a location no declared zone covers stops the materialization".to_owned()
            }
            UnzonedPolicy::Omit => {
                "a location no declared zone covers is left out and counted".to_owned()
            }
        })
        .note(format!(
            "the cell counter {} at every detected transition, and a transition is \
             admitted into the cell whose boundary it is within {}/{} of a cell of",
            if profile.presentation.read_channel.resync_on_transition {
                "restarts"
            } else {
                "runs on"
            },
            profile.presentation.read_channel.window_numerator,
            profile.presentation.read_channel.window_denominator
        ))
        .note(match policy.weak_pulse {
            WeakPulsePolicy::Declared { detected } => format!(
                "a pulse the medium states does not read the same every time is taken \
                 as {} detected",
                if detected { "always" } else { "never" }
            ),
            WeakPulsePolicy::Seeded => format!(
                "a pulse the medium states does not read the same every time is \
                 resolved reproducibly from seed {} and its own angle",
                policy.seed
            ),
        });
    for note in &medium.notes {
        provenance = provenance.note(format!("the medium beneath it: {note}"));
    }
    provenance
}

// ---------------------------------------------------------- the codec

/// Materializes an encoded bytestream from a hardware bitstream.
fn materialize_bytestream(
    bitstream: &HardwareBitstream,
    policy: GcrCodecPolicy,
    cache_bytes: u64,
) -> Result<C1541Bytestream> {
    let profile = &C1541;
    let codec = &profile.presentation.codec;
    let symbols_per_byte = codec.symbols_per_byte().ok_or_else(|| {
        refuse(format!(
            "the family's group code records {} bits to a symbol, which do not divide a \
             byte; there is no byte for it to state",
            codec.data_bits
        ))
    })?;
    let group_bits = symbols_per_byte * codec.symbol_bits;
    let landmark = u64::from(profile.presentation.read_channel.alignment_one_bits);
    if landmark == 0 {
        return Err(refuse(
            "the family declares no alignment landmark, so byte framing has nowhere to \
             begin",
        ));
    }

    let described = describe_codec(codec, &policy, bitstream.provenance());
    let mut builder = BytestreamBuilder::new(
        profile.id,
        codec.id,
        described.clone(),
        SessionBacking::create()?,
    )?;
    let mut loss = LossAccount::new();
    let mut reported = Vec::new();

    for location in bitstream.locations() {
        let key = location.key();
        let (numerator, denominator) = key.position();
        // The location's bits, gathered a chunk at a time so that no
        // other location is touched on the way to this one's bytes.
        let mut bits: Vec<bool> = Vec::with_capacity(location.cells() as usize);
        let mut unrecorded = 0u64;
        for ordinal in 0..bitstream.cell_chunks(location) {
            for cell in bitstream.cell_chunk(location, ordinal)? {
                if !cell.evidence().is_recorded() {
                    unrecorded += 1;
                }
                bits.push(cell.one());
            }
        }
        let framed = Framed::resolve(&bits, landmark, policy.alignment);

        let mut records = Vec::new();
        let mut facts = Vec::new();
        let mut unassigned = 0u64;
        for (at, run) in &framed.landmarks {
            facts.push(BytestreamFact::new(
                BytestreamFactKind::Alignment {
                    at_bit: *at as u64,
                    run_bits: *run as u64,
                },
                Provenance::new(PROFILE).note(format!(
                    "a run of {run} one bits, which is the family's declared framing \
                     landmark; byte framing resumes after it and nothing is claimed \
                     about what follows"
                )),
            ));
        }
        for (start, end) in &framed.segments {
            let mut at = *start;
            while at + group_bits as usize <= *end {
                let mut value = 0u64;
                for offset in 0..group_bits as usize {
                    value = value << 1 | u64::from(bits[at + offset]);
                }
                let outcome = resolve_group(codec, symbols_per_byte, value);
                if outcome.is_none() {
                    if policy.unassigned_symbol == UnassignedSymbolPolicy::Refuse {
                        return Err(refuse(format!(
                            "the group at bit {at} of location {numerator}/{denominator} \
                             holds a pattern the family's table does not assign, and the \
                             policy refuses rather than reporting a value it never \
                             recorded"
                        )));
                    }
                    unassigned += 1;
                }
                records.push(ByteRecord::new(
                    at as u64,
                    outcome.map_or(
                        ByteOutcome::Unresolved { bits: value },
                        ByteOutcome::Resolved,
                    ),
                ));
                at += group_bits as usize;
            }
            if at < *end {
                facts.push(BytestreamFact::new(
                    BytestreamFactKind::Unframed {
                        at_bit: at as u64,
                        bits: (*end - at) as u64,
                    },
                    Provenance::new(PROFILE).note(
                        "bits no framed group covers, stated rather than padded into a \
                         byte",
                    ),
                ));
            }
        }
        for (at, span) in &framed.unframed {
            facts.push(BytestreamFact::new(
                BytestreamFactKind::Unframed {
                    at_bit: *at as u64,
                    bits: *span as u64,
                },
                Provenance::new(PROFILE).note(
                    "bits framing does not reach, no landmark having placed a byte \
                           boundary ahead of them",
                ),
            ));
        }

        let unframed_bits: u64 = facts
            .iter()
            .filter_map(|fact| match fact.kind() {
                BytestreamFactKind::Unframed { bits, .. } => Some(*bits),
                BytestreamFactKind::Alignment { .. } => None,
            })
            .sum();
        let landmark_bits: u64 = framed.landmarks.iter().map(|(_, run)| *run as u64).sum();
        if landmark_bits > 0 {
            loss.add(
                "alignment-landmark",
                "bits forming the family's framing landmark, which say where bytes \
                 begin rather than becoming one",
                landmark_bits,
            );
        }
        if unframed_bits > 0 {
            loss.add(
                "unframed-bits",
                "bits no framed group covers, which no byte carries",
                unframed_bits,
            );
        }
        if unassigned > 0 {
            loss.add(
                "unassigned-symbol",
                "groups holding a pattern the family's table does not assign, kept as \
                 their own bits because there is no nearest entry",
                unassigned,
            );
        }
        if unrecorded > 0 {
            loss.add(
                "resolved-bit-evidence",
                "bits a declared rule resolved rather than the medium recording them, \
                 whose rule a byte does not carry",
                unrecorded,
            );
        }

        reported.push(BytestreamLocation {
            half_track_numerator: numerator,
            half_track_denominator: denominator,
            surface: key.surface(),
            bytes: records.len() as u64,
            resolved_bytes: records.len() as u64 - unassigned,
            unassigned_groups: unassigned,
            alignments: framed.landmarks.len() as u64,
            longest_landmark_bits: framed
                .landmarks
                .iter()
                .map(|(_, run)| *run as u64)
                .max()
                .unwrap_or(0),
            unframed_bits,
        });

        builder.add_location(
            key.clone(),
            &records,
            &facts,
            Provenance::new(PROFILE).note(format!(
                "resolved from the location's {} clocked bits by the family's declared \
                 group code",
                bits.len()
            )),
        )?;
    }

    let (mut bytestream, sink, total) = builder.seal()?;
    bytestream.attach_backing(Box::new(sink.into_source()), total, cache_bytes);

    let report = BytestreamReport {
        profile_id: profile.id.to_owned(),
        codec_id: codec.id.to_owned(),
        codec_name: codec.name.to_owned(),
        symbol_bits: codec.symbol_bits,
        data_bits: codec.data_bits,
        symbols_per_byte,
        locations: reported,
        declared_loss: loss.into_entries(),
        evidence: described.notes,
    };
    Ok(C1541Bytestream { bytestream, report })
}

/// One group of bits as the table reads it, or nothing where it holds a
/// pattern the family does not assign.
fn resolve_group(codec: &GroupCodec, symbols_per_byte: u32, group: u64) -> Option<u8> {
    let mask = (1u64 << codec.symbol_bits) - 1;
    let mut value = 0u8;
    for at in 0..symbols_per_byte {
        let shift = codec.symbol_bits * (symbols_per_byte - 1 - at);
        let symbol = u16::try_from(group >> shift & mask).ok()?;
        value = value << codec.data_bits | codec.value_of(symbol)?;
    }
    Some(value)
}

/// Where framing begins, what it covers, and what it does not.
///
/// Framing does not cross the circle's origin, and that is a stated rule
/// rather than an oversight: the origin is where the medium's own
/// reduction placed the circle's start — for a 1541 the write splice —
/// which is the one angle at which the recording is genuinely
/// discontinuous. A landmark spanning it is therefore two runs, and a
/// caller who wants framing to begin at the origin declares it.
struct Framed {
    /// Landmark runs, as (bit, run length).
    landmarks: Vec<(usize, usize)>,
    /// Framed spans, half-open.
    segments: Vec<(usize, usize)>,
    /// Spans framing does not reach, as (bit, length).
    unframed: Vec<(usize, usize)>,
}

impl Framed {
    fn resolve(bits: &[bool], landmark: u64, alignment: AlignmentPolicy) -> Self {
        let length = bits.len();
        let mut landmarks = Vec::new();
        let mut segments = Vec::new();
        let mut unframed = Vec::new();

        // Every landmark-length run of ones.
        let mut at = 0usize;
        while at < length {
            if bits[at] {
                let start = at;
                while at < length && bits[at] {
                    at += 1;
                }
                if (at - start) as u64 >= landmark {
                    landmarks.push((start, at - start));
                }
            } else {
                at += 1;
            }
        }

        let mut cursor = 0usize;
        let mut before_the_first = true;
        for (start, run) in &landmarks {
            if *start > cursor {
                if before_the_first && alignment == AlignmentPolicy::Landmark {
                    unframed.push((cursor, *start - cursor));
                } else {
                    segments.push((cursor, *start));
                }
            }
            cursor = start + run;
            before_the_first = false;
        }
        if cursor < length {
            if before_the_first && alignment == AlignmentPolicy::Landmark {
                unframed.push((cursor, length - cursor));
            } else {
                segments.push((cursor, length));
            }
        }

        Self {
            landmarks,
            segments,
            unframed,
        }
    }
}

fn describe_codec(
    codec: &GroupCodec,
    policy: &GcrCodecPolicy,
    bitstream: &Provenance,
) -> Provenance {
    let mut provenance = Provenance::new(PROFILE)
        .note(format!("{}: {}", codec.name, codec.provenance))
        .note(format!(
            "{} bits of the recording carry {} bits of a byte",
            codec.symbol_bits, codec.data_bits
        ))
        .note(match policy.alignment {
            AlignmentPolicy::Landmark => {
                "byte framing begins at the family's declared landmark and nowhere else".to_owned()
            }
            AlignmentPolicy::Origin => {
                "byte framing begins at the circle's origin as well, the caller \
                 declaring it a byte boundary"
                    .to_owned()
            }
        })
        .note(match policy.unassigned_symbol {
            UnassignedSymbolPolicy::Refuse => {
                "a pattern the table does not assign stops the materialization".to_owned()
            }
            UnassignedSymbolPolicy::DeclareLoss => {
                "a pattern the table does not assign keeps its own bits and is counted".to_owned()
            }
        })
        .note(
            "no byte here is a header, a data field, a sector or a file, and no \
             landmark introduces one"
                .to_owned(),
        );
    for note in &bitstream.notes {
        provenance = provenance.note(format!("the bitstream beneath it: {note}"));
    }
    provenance
}

// ------------------------------------------------------- the entry points

impl crate::RemanenceImage {
    /// Materializes the family's hardware bitstream from what this image
    /// holds, under the family's declared mechanics and read-channel
    /// rules (P23, P30 reached through the type, P33).
    ///
    /// It takes no policy because the profile carries one; `cache_bytes`
    /// is the P27 working-set bound, which is a bound rather than a
    /// reading. The image is the physical stratum and carries no clock —
    /// a cell length is a property of a *recording*, recoverable from
    /// the image, never a field of it — so the ladder stands on the
    /// served projection of it, the same one multiply per point the p64
    /// rendition uses, at the family's reference frame. The image is
    /// untouched and stays exactly what it was: the bitstream is
    /// separate session state, carrying the image's own provenance
    /// beneath the channel that produced it. There is no way back down
    /// (P33).
    pub fn materialize_c1541_bitstream(&self, cache_bytes: u64) -> Result<C1541Bitstream> {
        let (medium, _) = self.served_medium()?;
        materialize_bitstream(&medium, C1541.presentation.channel_policy, cache_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;
    use crate::flux::capture::TimeBase;
    use crate::flux::medium::{
        Derivation, MediumBuilder, MediumFact, OriginRule, OriginStatement, RotationalFrame,
        Strength,
    };

    const CYCLES_PER_ROTATION: u64 = 3_200_000;

    fn channel_policy() -> ReadChannelPolicy {
        ReadChannelPolicy {
            density: DensityPolicy::Declared,
            unzoned: UnzonedPolicy::Omit,
            weak_pulse: WeakPulsePolicy::Seeded,
            seed: 0x0123_4567_89ab_cdef,
        }
    }

    fn codec_policy() -> GcrCodecPolicy {
        GcrCodecPolicy {
            alignment: AlignmentPolicy::Landmark,
            unassigned_symbol: UnassignedSymbolPolicy::DeclareLoss,
        }
    }

    struct Bytes(Vec<u8>);

    impl crate::flux::capture::ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    fn frame() -> RotationalFrame {
        RotationalFrame::new(
            C1541.id,
            TimeBase::new(C1541.id, 16_000_000, 1).expect("the rate is stated"),
            CYCLES_PER_ROTATION,
            OriginStatement::new(
                OriginRule::Seam,
                Provenance::new(C1541.id).note("placed at the seam for the test"),
            ),
        )
        .expect("the frame states a circle")
    }

    /// The cell a location is recorded at, falling back to the first
    /// declared zone where no zone covers it — which is what the pinned
    /// policy does, and lets a half-track between two zones be recorded
    /// at all.
    fn cell_of(location: &LocationKey) -> u64 {
        let (_, zone) = C1541
            .zone_for_ratio(location.position().0, location.position().1)
            .unwrap_or((0, &C1541.density[0]));
        let (numerator, denominator) = zone.nominal_cell(&C1541.rotation);
        u64::try_from(numerator / denominator).expect("the cell is whole")
    }

    /// A medium of one location whose pulses are the bit pattern `bits`
    /// laid down at the zone's declared cell: a one bit is a transition
    /// one cell after the last, and a zero bit is a cell without one.
    fn medium_of(location: LocationKey, bits: &[bool], strengths: &[u32]) -> FluxMedium {
        let cell = cell_of(&location);

        let mut pulses = Vec::new();
        let mut at = 0u64;
        for (index, bit) in bits.iter().enumerate() {
            at += cell;
            if *bit {
                pulses.push(Pulse::new(
                    at,
                    Strength::certain(strengths.get(index).copied().unwrap_or(2)),
                ));
            }
        }

        let mut builder = MediumBuilder::new(
            C1541.id,
            C1541.media,
            frame(),
            Derivation::SelectedAndProjected,
            Provenance::new(C1541.id).note("selected observation 0 of each location"),
            Vec::new(),
        )
        .expect("the policy is stated");
        builder
            .add_location(
                location,
                &pulses,
                &[MediumFact::new(
                    MediumFactKind::Seam { angle: 1000 },
                    Provenance::new(C1541.id).note("located for the test"),
                )],
                Provenance::new(C1541.id).note("reduced for the test"),
            )
            .expect("the location");
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
        medium
    }

    /// The bits a run of bytes is recorded as: a landmark of ten ones,
    /// then each byte as two five-bit symbols of the declared table.
    fn recorded(bytes: &[u8]) -> Vec<bool> {
        let codec = &C1541.presentation.codec;
        let mut bits = vec![true; C1541.presentation.read_channel.alignment_one_bits as usize];
        for byte in bytes {
            for nibble in [byte >> 4, byte & 0x0f] {
                let symbol = codec.symbols[nibble as usize];
                for shift in (0..codec.symbol_bits).rev() {
                    bits.push(symbol >> shift & 1 == 1);
                }
            }
        }
        bits
    }

    #[test]
    fn a_declared_byte_sequence_survives_the_whole_journey_unchanged() {
        // The claim that matters: what the family's table records is what
        // the codec reads back, through the read channel, the backing and
        // the group code, with nothing in between guessing.
        //
        // The track is written full, because a half-written one would
        // test the codec against a blank tail rather than against the
        // recording.
        let location = LocationKey::new(C1541.id, 18, 0);
        let cell = cell_of(&location);
        let landmark = u64::from(C1541.presentation.read_channel.alignment_one_bits);
        let fits = ((CYCLES_PER_ROTATION / cell) - landmark) / 10;
        let mut written = vec![0x00u8, 0x52, 0xff, 0x0f, 0xa5, 0x7b];
        written.resize(fits as usize, 0x55);
        let medium = medium_of(location.clone(), &recorded(&written), &[]);

        let bitstream = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect("the channel clocks it");
        let bytestream = materialize_bytestream(&bitstream.bitstream, codec_policy(), 1 << 20)
            .expect("the codec resolves it");

        let inner = bytestream.inner();
        let held = inner.location(&location).expect("the location is held");
        let read: Vec<u8> = inner
            .bytes(held)
            .expect("the bytes read back")
            .iter()
            .filter_map(crate::flux::bytestream::ByteRecord::value)
            .collect();
        assert_eq!(read, written, "{} bytes read back", read.len());

        // And the landmark was located rather than assumed: one run of
        // ten ones, and framing began after it. Every group the family's
        // table assigns resolved, and the bits the circle had left over
        // are stated rather than padded into a byte.
        let location_report = &bytestream.inspect().locations[0];
        assert_eq!(location_report.alignments, 1);
        assert_eq!(location_report.longest_landmark_bits, 10);
        assert_eq!(location_report.unassigned_groups, 0);
        assert_eq!(location_report.bytes, written.len() as u64);
        assert!(location_report.unframed_bits < 10, "{location_report:?}");
    }

    #[test]
    fn a_pattern_the_table_does_not_assign_is_refused_or_kept_as_its_own_bits() {
        // Ten ones frame the stream, then a group of ten zeros, which is
        // two symbols the family assigns nothing to.
        let mut bits = vec![true; 10];
        bits.extend(std::iter::repeat_n(false, 10));
        bits.extend(recorded(&[0x12])[10..].iter().copied());
        let location = LocationKey::new(C1541.id, 18, 0);
        let medium = medium_of(location.clone(), &bits, &[]);
        let bitstream = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect("the channel clocks it");

        let error = materialize_bytestream(
            &bitstream.bitstream,
            GcrCodecPolicy {
                unassigned_symbol: UnassignedSymbolPolicy::Refuse,
                ..codec_policy()
            },
            1 << 20,
        )
        .expect_err("a pattern the table does not assign is refused");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("does not assign"), "{error}");

        let bytestream = materialize_bytestream(&bitstream.bitstream, codec_policy(), 1 << 20)
            .expect("the other policy keeps it");
        assert!(bytestream.inspect().locations[0].unassigned_groups >= 1);
        assert!(
            bytestream
                .inspect()
                .declared_loss
                .iter()
                .any(|loss| loss.code == "unassigned-symbol")
        );
    }

    #[test]
    fn a_weak_pulse_resolves_the_same_way_every_time_under_one_seed() {
        // P29's reproducibility, at this layer: the same medium and the
        // same seed give the same bitstream, and a different seed is
        // allowed to differ.
        let bits = recorded(&[0x5a; 8]);
        let strengths: Vec<u32> = (0..bits.len())
            .map(|index| 1 + (index as u32 % 2))
            .collect();
        let location = LocationKey::new(C1541.id, 18, 0);
        let medium = medium_of(location, &bits, &strengths);

        let once = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect("the channel clocks it");
        let again = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect("the channel clocks it again");
        assert_eq!(once.inspect(), again.inspect());
        assert!(once.inspect().locations[0].resolved_bits > 0);

        // Every bit a rule resolved is accounted for as something the
        // bitstream does not carry from the medium.
        assert!(
            once.inspect()
                .declared_loss
                .iter()
                .any(|loss| loss.code == "unexpressed-pulse-strength")
        );
    }

    #[test]
    fn a_declared_weak_rule_is_not_the_seeded_one() {
        let bits = recorded(&[0x5a; 8]);
        let strengths: Vec<u32> = vec![1; bits.len()];
        let location = LocationKey::new(C1541.id, 18, 0);
        let medium = medium_of(location, &bits, &strengths);

        let never = materialize_bitstream(
            &medium,
            ReadChannelPolicy {
                weak_pulse: WeakPulsePolicy::Declared { detected: false },
                ..channel_policy()
            },
            1 << 20,
        )
        .expect("the channel clocks it");
        // Nothing triggered, so the whole circle is zero cells and the
        // longest run says so.
        assert_eq!(never.inspect().locations[0].one_bits, 0);
        assert!(never.inspect().locations[0].longest_zero_run > 100);

        let always = materialize_bitstream(
            &medium,
            ReadChannelPolicy {
                weak_pulse: WeakPulsePolicy::Declared { detected: true },
                ..channel_policy()
            },
            1 << 20,
        )
        .expect("the channel clocks it");
        assert!(always.inspect().locations[0].one_bits > 0);
    }

    #[test]
    fn a_pulse_the_medium_states_never_triggers_is_not_detected() {
        let bits = recorded(&[0xff; 4]);
        let strengths: Vec<u32> = vec![0; bits.len()];
        let location = LocationKey::new(C1541.id, 18, 0);
        let medium = medium_of(location, &bits, &strengths);

        let bitstream = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect("the channel clocks it");
        assert_eq!(bitstream.inspect().locations[0].one_bits, 0);
        assert!(
            bitstream
                .inspect()
                .declared_loss
                .iter()
                .any(|loss| loss.code == "never-triggering-pulse")
        );
    }

    #[test]
    fn a_strength_state_the_profile_does_not_name_is_refused() {
        let bits = recorded(&[0x12]);
        let strengths: Vec<u32> = vec![9; bits.len()];
        let location = LocationKey::new(C1541.id, 18, 0);
        let medium = medium_of(location, &bits, &strengths);

        let error = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect_err("a state outside the family's vocabulary is refused");
        assert!(error.to_string().contains("does not name"), "{error}");
    }

    #[test]
    fn a_location_no_declared_zone_covers_is_refused_or_omitted_and_never_guessed() {
        // A half-track between two zones: no published rate reaches it,
        // and a neighbour's would be a number the family never stated.
        let location =
            LocationKey::fraction(C1541.id, 35, 2, Some(0)).expect("a half-track is an address");
        let medium = medium_of(location, &recorded(&[0x12, 0x34]), &[]);

        let error = materialize_bitstream(
            &medium,
            ReadChannelPolicy {
                unzoned: UnzonedPolicy::Refuse,
                ..channel_policy()
            },
            1 << 20,
        )
        .expect_err("an unzoned location is refused");
        assert!(error.to_string().contains("no declared rate"), "{error}");

        // Omitted, the medium's only location is left out and there is
        // nothing to hold — which is itself a refusal rather than an
        // empty bitstream nobody asked for.
        let error = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect_err("nothing was clocked");
        assert!(error.to_string().contains("no bitstream"), "{error}");

        // And the caller may pin a zone instead, which is what a
        // controller that selects a density and steps actually does.
        let pinned = materialize_bitstream(
            &medium,
            ReadChannelPolicy {
                density: DensityPolicy::Fixed { zone: 0 },
                ..channel_policy()
            },
            1 << 20,
        )
        .expect("a pinned zone clocks it");
        assert_eq!(pinned.inspect().locations[0].zone, 0);
    }

    #[test]
    fn a_zone_the_family_does_not_declare_is_refused_by_name() {
        let medium = medium_of(LocationKey::new(C1541.id, 18, 0), &recorded(&[0x12]), &[]);
        let error = materialize_bitstream(
            &medium,
            ReadChannelPolicy {
                density: DensityPolicy::Fixed { zone: 9 },
                ..channel_policy()
            },
            1 << 20,
        )
        .expect_err("a zone the profile does not name has no rate");
        assert!(error.to_string().contains("zone 9"), "{error}");
    }

    #[test]
    fn the_channel_and_the_medium_policy_both_travel_into_the_bytestream() {
        // The transition preserves what produced its source: the medium's
        // own selection is still readable two layers up.
        let medium = medium_of(LocationKey::new(C1541.id, 18, 0), &recorded(&[0x12]), &[]);
        let bitstream = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect("the channel clocks it");
        let bytestream = materialize_bytestream(&bitstream.bitstream, codec_policy(), 1 << 20)
            .expect("the codec resolves it");

        let evidence = &bytestream.inspect().evidence;
        assert!(
            evidence
                .iter()
                .any(|line| line.contains("Commodore group-coded recording")),
            "{evidence:?}"
        );
        assert!(
            evidence.iter().any(|line| line.contains("Commodore 1541")),
            "{evidence:?}"
        );
        assert!(
            evidence
                .iter()
                .any(|line| line.contains("selected observation 0")),
            "{evidence:?}"
        );
    }

    #[test]
    fn a_medium_of_another_family_is_refused() {
        let mut builder = MediumBuilder::new(
            "apple2",
            C1541.media,
            RotationalFrame::new(
                "apple2",
                TimeBase::new("apple2", 16_000_000, 1).expect("stated"),
                CYCLES_PER_ROTATION,
                OriginStatement::new(OriginRule::Declared, Provenance::new("apple2").note("x")),
            )
            .expect("the frame states a circle"),
            Derivation::Synthetic,
            Provenance::new("apple2").note("synthesized"),
            Vec::new(),
        )
        .expect("the policy is stated");
        builder
            .add_location(
                LocationKey::new("apple2", 1, 0),
                &[],
                &[],
                Provenance::new("apple2").note("synthesized"),
            )
            .expect("the location");
        let (medium, _, _) = builder.seal().expect("the backing seals");

        let error = materialize_bitstream(&medium, channel_policy(), 1 << 20)
            .expect_err("a drive reads the family it is");
        assert!(error.to_string().contains("apple2"), "{error}");
    }
}
