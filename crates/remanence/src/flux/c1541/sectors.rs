// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C1541 sector layer (P4, P10, P23, P30): the recording's own
//! sectors, recognized above the encoded bytestream.
//!
//! It is the ladder's missing rung. The bytestream below holds the bytes
//! one declared group code resolved, and says of every one of them that
//! it is not a header, a data field, a sector or a file. **This is the
//! seam where that ends** — and it ends by a new layer *stating* what it
//! derives, never by the layer below quietly having meant more than it
//! said.
//!
//! Every rule the recognition applies is the family's (P30): which byte
//! opens a header block and which opens a data block, how long each is,
//! where the header states the track, the sector and the disk identity,
//! which bytes the checksum covers and how it is computed, and where the
//! payload sits. Nothing here derives any of them from what it is
//! reading, and a byte that opens neither block opens nothing.
//!
//! **Pairing is grammatical, not metric.** The family writes one sync
//! ahead of each block, so a record's data block is the block the
//! recording carries next after its header. Nothing measures a distance
//! and nothing tunes a window; a header the next block does not follow
//! with data holds no data, and says so. The circle is closed, so a
//! header at the end of the circle may pair with a data block at its
//! start — the origin is the write splice, not a boundary of the
//! recording.
//!
//! **A claim carries its evidence and an unreadable sector is a
//! refusal** (P4, P10). Every record the layer recognizes is reported
//! with where it sits, the address it states for itself, both stated
//! checksums beside both computed ones, and how many of its bytes the
//! codec left unresolved. Reading by the recording's own address answers
//! only where one readable claim, or several agreeing ones, hold it;
//! nothing is repaired, nothing is filled, and the refusal names the
//! rule it broke.
//!
//! **It is derived, and it descends nothing** (P23, P33). The
//! bytestream is untouched and stays exactly what it was. The payloads
//! this layer serves stream into private session storage as they are
//! recognized and are read back a bounded section at a time (P27), so
//! neither the sectors nor the bytes beneath them are held whole.

use std::sync::Mutex;

use std::collections::BTreeSet;

use crate::error::{Error, ErrorCategory, Result, RuleIdentity};
use crate::evidence::{DeclaredLoss, LossAccount, Provenance};
use crate::flux::bytestream::{ByteRecord, BytestreamFactKind, EncodedBytestream};
use crate::flux::c1541::presentation::C1541Bytestream;
use crate::flux::capture::{
    ByteSource, LEAF_ENTRIES, SectionAddress, SectionCache, SectionWriter, SessionBacking,
    read_varint, write_varint,
};
use crate::flux::drive_profile::{BlockShape, C1541, RecordGrammar};
use crate::flux::medium::{LocationKey, read_location_key, write_location_key};

/// The profile every rule this layer applies is declared by.
const PROFILE: &str = "c1541";

// ------------------------------------------------------------ the rules

/// The rules a sector-layer refusal names (P10).
///
/// The set belongs to this seam, as every rule set belongs to the seam
/// that defines it; the category still says how a caller should behave
/// and this says which rule was broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorRule {
    /// No record of the recording states that address at all.
    NoSuchAddress,
    /// The address is stated and no claim of it can be read. The
    /// recording holds what it holds; nothing is filled in.
    AddressNotReadable,
    /// Several readable claims state that address and hold different
    /// bytes for it. Reported, never resolved: choosing between them
    /// would be answering a question the evidence does not.
    AddressDisagrees,
    /// A block's stated checksum disagrees with the one its own bytes
    /// compute under the family's declared rule.
    BlockFailedChecksum,
    /// A block covers a group the codec left unresolved. A pattern the
    /// family's table does not assign is not a byte, so the block is
    /// not read.
    BlockUnresolved,
    /// Framing ends before the block the grammar declares does.
    BlockTruncated,
    /// A header block the recording does not follow with a data block.
    RecordUnpaired,
    /// The bytestream holds no record of the family's grammar at all.
    NoRecordRecognized,
}

impl SectorRule {
    /// Every rule in the set.
    pub const ALL: [Self; 8] = [
        Self::NoSuchAddress,
        Self::AddressNotReadable,
        Self::AddressDisagrees,
        Self::BlockFailedChecksum,
        Self::BlockUnresolved,
        Self::BlockTruncated,
        Self::RecordUnpaired,
        Self::NoRecordRecognized,
    ];

    /// The stable cross-language spelling of this rule, which is what a
    /// refusal carries.
    pub const fn as_str(self) -> RuleIdentity {
        match self {
            Self::NoSuchAddress => "no-such-address",
            Self::AddressNotReadable => "address-not-readable",
            Self::AddressDisagrees => "address-disagrees",
            Self::BlockFailedChecksum => "block-failed-checksum",
            Self::BlockUnresolved => "block-unresolved",
            Self::BlockTruncated => "block-truncated",
            Self::RecordUnpaired => "record-unpaired",
            Self::NoRecordRecognized => "no-record-recognized",
        }
    }

    /// Reads a rule identity back into this set. An identity from
    /// another seam's set is `None` rather than a nearest match.
    pub fn from_identity(identity: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rule| rule.as_str() == identity)
    }

    const fn category(self) -> ErrorCategory {
        match self {
            Self::NoSuchAddress => ErrorCategory::NotFound,
            // The recording does not yield it, and no retry or
            // permission change will: that is imperfect evidence rather
            // than a host failure or a contradiction in the artifact.
            Self::AddressNotReadable | Self::AddressDisagrees => ErrorCategory::Unavailable,
            Self::BlockFailedChecksum
            | Self::BlockUnresolved
            | Self::BlockTruncated
            | Self::RecordUnpaired
            | Self::NoRecordRecognized => ErrorCategory::InvalidImage,
        }
    }
}

impl std::fmt::Display for SectorRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn refuse(rule: SectorRule, reason: impl Into<String>) -> Error {
    Error::categorized_image(rule.category(), PROFILE, reason).broke_rule(rule.as_str())
}

// ----------------------------------------------------------- the policy

/// What a block whose stated checksum disagrees with its own bytes
/// becomes.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChecksumFailurePolicy {
    /// The recognition stops and names the location and the address.
    Refuse,
    /// The claim is kept, stated as unreadable with the two checksums
    /// beside each other, and counted. What the recording holds there
    /// is a fact; a block that reads past its own checksum would be a
    /// repair nobody asked for.
    DeclareLoss,
}

/// What a header block the recording does not follow with a data block
/// becomes.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnpairedRecordPolicy {
    /// The recognition stops and names the location and the address.
    Refuse,
    /// The claim is kept, stated as holding no data, and counted.
    DeclareLoss,
}

/// The complete declared policy for one bytestream-to-sector
/// recognition.
///
/// There is no `Default`: both fields are decisions about evidence, and
/// a recognition that arrived at one by construction rather than by
/// declaration is what P30 exists to prevent. The one every caller
/// reaches is the profile's declared `sector_policy` (P30 reached
/// through the type), which is why nothing here is public.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SectorPolicy {
    pub(crate) checksum_failure: ChecksumFailurePolicy,
    pub(crate) unpaired_record: UnpairedRecordPolicy,
}

// -------------------------------------------------------- the reporting

/// One record the recognition read, and the evidence for every claim it
/// makes.
///
/// Nothing here is a conclusion about the disk. The address is what the
/// header states for itself, the checksums are stated beside computed,
/// and `readable` is the conjunction of the tests below rather than a
/// verdict of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorClaim {
    /// The family half-track this record sits on, as an exact ratio.
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    /// The bit of the location's own bitstream the header block opens
    /// at.
    pub at_bit: u64,
    /// The address the header states for itself, as recorded.
    pub track: u8,
    pub sector: u8,
    /// The two disk-identity bytes the header carries, as recorded and
    /// unread: this layer states what a sector is addressed as, never
    /// what a disk is called.
    pub id_high: u8,
    pub id_low: u8,
    /// The checksum the header states, and the one its own bytes
    /// compute under the family's declared rule.
    pub header_checksum_stated: u8,
    pub header_checksum_computed: u8,
    /// Whether a data block followed the header, and where it opened.
    pub has_data: bool,
    pub data_at_bit: u64,
    pub data_checksum_stated: u8,
    pub data_checksum_computed: u8,
    /// How many bytes of either block the codec left unresolved. A
    /// pattern the family's table does not assign is not a byte, so a
    /// record covering one is not read.
    pub unresolved_bytes: u64,
    /// Whether the address the family's own declaration covers this
    /// one at all — the track within its density map, the sector within
    /// what that track's zone claims. Stated, never corrected.
    pub within_declaration: bool,
    /// Whether the payload is served: both blocks present and whole,
    /// both checksums agreeing, and no unresolved byte in either.
    pub readable: bool,
    /// Which rule of [`SectorRule`] stands in the way, where one does.
    pub rule: Option<String>,
    /// Why it is not readable, in the layer's own terms.
    pub refusal: Option<String>,
}

/// One location the sector layer read, and what it found there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorLocation {
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub surface: Option<u64>,
    /// What the family's density map claims one location in this zone
    /// holds, where a declared zone covers it at all.
    pub records_declared: Option<u32>,
    /// Header blocks the grammar recognized here.
    pub headers: u64,
    /// Those a data block followed.
    pub records: u64,
    pub readable: u64,
    pub failed_checksum: u64,
    /// Framed runs holding no block the grammar names — gaps, tails,
    /// and whatever else the recording carries between its records.
    pub runs_without_a_record: u64,
}

/// What one bytestream-to-sector recognition produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorReport {
    pub profile_id: String,
    pub grammar_id: String,
    pub grammar_name: String,
    /// How many bytes of payload one data block carries.
    pub payload_bytes: u32,
    pub locations: Vec<SectorLocation>,
    /// Every record recognized, in recording order within each
    /// location and by location thereafter.
    pub claims: Vec<SectorClaim>,
    /// Addresses more than one readable claim states. Reported rather
    /// than resolved; whether those claims agree is decided when one is
    /// read.
    pub contested: Vec<ContestedAddress>,
    /// Everything the sector layer does not carry of the bytestream
    /// beneath it (P29).
    pub declared_loss: Vec<DeclaredLoss>,
    /// The grammar and policy that produced it, and everything the
    /// bytestream said beneath it, in that order (P4).
    pub evidence: Vec<String>,
}

/// One address the recording states more than once, readably.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContestedAddress {
    pub track: u8,
    pub sector: u8,
    pub readable_claims: u32,
}

// ------------------------------------------------------- the backing

/// The complete address of one payload this layer holds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PayloadKey {
    location: LocationKey,
    ordinal: u64,
}

impl SectionAddress for PayloadKey {
    fn namespace(&self) -> &'static str {
        self.location.profile()
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_location_key(out, &self.location);
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
        let (ordinal, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        Ok((Self { location, ordinal }, cursor - at))
    }
}

/// Where the recognized payloads are read back from, and the bounded
/// working set they are served through (P27).
struct PayloadBacking {
    bytes: Box<dyn ByteSource + Send + Sync>,
    total_bytes: u64,
    cache: Mutex<SectionCache<PayloadKey>>,
    known: Vec<&'static str>,
}

/// The recording's own sectors, held in the session.
///
/// The payloads stay behind this surface and are read by the address
/// the recording states for them, one at a time.
pub struct C1541Sectors {
    report: SectorReport,
    /// Where each claim's payload sits, aligned with `report.claims`;
    /// `None` for a claim that is not readable and has none.
    payloads: Vec<Option<PayloadKey>>,
    backing: PayloadBacking,
}

impl std::fmt::Debug for C1541Sectors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("C1541Sectors")
            .field("locations", &self.report.locations.len())
            .field("claims", &self.report.claims.len())
            .field("backing_bytes", &self.backing.total_bytes)
            .finish()
    }
}

impl C1541Sectors {
    /// The recognition that produced these sectors, every claim's
    /// evidence, and everything the layer does not carry of the
    /// bytestream beneath it.
    pub fn inspect(&self) -> &SectorReport {
        &self.report
    }

    /// How many records the recognition read.
    pub fn claim_count(&self) -> u64 {
        self.report.claims.len() as u64
    }

    /// How many locations held at least one recognized record.
    pub fn location_count(&self) -> u64 {
        self.report.locations.len() as u64
    }

    /// How many bytes of private session storage the payloads occupy.
    pub fn backing_bytes(&self) -> u64 {
        self.backing.total_bytes
    }

    pub fn resident_bytes(&self) -> u64 {
        self.backing
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .resident_bytes()
    }

    /// Reads one sector by the address the recording states for it.
    ///
    /// It answers only where the recording is unambiguous: one readable
    /// claim, or several holding the same bytes. Every other outcome is
    /// a refusal naming its rule (P10) — an address no record states, an
    /// address every claim of which failed its own checksum or covered a
    /// pattern the codec could not resolve, or an address several
    /// readable claims disagree about. Nothing is repaired and no block
    /// is filled in.
    pub fn read_sector(&self, track: u8, sector: u8) -> Result<Vec<u8>> {
        let mut readable = Vec::new();
        let mut unreadable = Vec::new();
        for (at, claim) in self.report.claims.iter().enumerate() {
            if claim.track != track || claim.sector != sector {
                continue;
            }
            if claim.readable {
                readable.push(at);
            } else {
                unreadable.push(claim);
            }
        }

        let Some(&first) = readable.first() else {
            if let Some(claim) = unreadable.first() {
                return Err(refuse(
                    SectorRule::AddressNotReadable,
                    format!(
                        "the recording states track {track} sector {sector} at {} \
                         place(s) and no claim of it reads: {}",
                        unreadable.len(),
                        claim
                            .refusal
                            .clone()
                            .unwrap_or_else(|| "the claim is not readable".to_owned())
                    ),
                ));
            }
            return Err(refuse(
                SectorRule::NoSuchAddress,
                format!(
                    "no record of this recording states track {track} sector {sector}; \
                     the family declares {}",
                    declared_extent(track)
                ),
            ));
        };

        let payload = self.payload(first)?;
        for &other in &readable[1..] {
            if self.payload(other)? != payload {
                let places: Vec<String> = readable
                    .iter()
                    .map(|at| location_text(&self.report.claims[*at]))
                    .collect();
                return Err(refuse(
                    SectorRule::AddressDisagrees,
                    format!(
                        "track {track} sector {sector} is recorded readably at {} and \
                         they do not hold the same bytes; which one the disk means is \
                         not something this evidence answers",
                        places.join(", ")
                    ),
                ));
            }
        }
        Ok(payload)
    }

    /// One claim's payload, read back through the bounded cache.
    fn payload(&self, at: usize) -> Result<Vec<u8>> {
        let key = self
            .payloads
            .get(at)
            .and_then(Option::as_ref)
            .ok_or_else(|| {
                Error::invalid_image(
                    PROFILE,
                    "a readable claim holds no payload, which is this layer contradicting \
                 itself rather than a fact about the recording",
                )
            })?;
        let mut cache = self
            .backing
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .section(
                self.backing.bytes.as_ref(),
                self.backing.total_bytes,
                key,
                &self.backing.known,
            )
            .map(<[u8]>::to_vec)
    }
}

/// What the family's declaration covers at one track, in words a
/// refusal can carry.
fn declared_extent(track: u8) -> String {
    match C1541.zone_for_ratio(u64::from(track), 1) {
        Some((_, zone)) => format!(
            "{} sectors on track {track}, numbered from zero",
            zone.records
        ),
        None => format!("no track {track} at all: its density map stops short of it"),
    }
}

fn location_text(claim: &SectorClaim) -> String {
    if claim.half_track_denominator == 1 {
        format!("half-track {}", claim.half_track_numerator)
    } else {
        format!(
            "half-track {}/{}",
            claim.half_track_numerator, claim.half_track_denominator
        )
    }
}

// ---------------------------------------------------- the recognition

/// One run of framed bytes: what the layer below resolved from one
/// framing landmark onward, in the order it resolved them.
///
/// Only the head of a run is buffered — as many bytes as the longest
/// block the grammar declares — because a record opens a run or the run
/// holds no record, and the rest of it is gap.
struct Run {
    at_bit: u64,
    head: Vec<Option<u8>>,
    /// How many framed bytes the run holds altogether, buffered or not.
    length: u64,
}

/// Walks a location's byte records into runs, buffering no more of any
/// one of them than a block can need.
///
/// A run opens where the layer below said framing began — the bit after
/// one of its declared landmarks — or wherever the byte positions stop
/// being one group apart. Both are the same statement seen twice, and
/// asking the layer below is what keeps a landmark whose length happens
/// to complete a group from joining two framings into one.
struct Runs {
    keep: usize,
    stride: u64,
    starts: BTreeSet<u64>,
    expected: Option<u64>,
    current: Option<Run>,
    closed: Vec<Run>,
}

impl Runs {
    fn new(keep: usize, stride: u64, starts: BTreeSet<u64>) -> Self {
        Self {
            keep,
            stride,
            starts,
            expected: None,
            current: None,
            closed: Vec::new(),
        }
    }

    fn push(&mut self, record: &ByteRecord) {
        if (self.expected != Some(record.at_bit()) || self.starts.contains(&record.at_bit()))
            && let Some(run) = self.current.take()
        {
            self.closed.push(run);
        }
        let run = self.current.get_or_insert_with(|| Run {
            at_bit: record.at_bit(),
            head: Vec::new(),
            length: 0,
        });
        if run.head.len() < self.keep {
            run.head.push(record.value());
        }
        run.length += 1;
        self.expected = Some(record.at_bit() + self.stride);
    }

    fn finish(mut self) -> Vec<Run> {
        if let Some(run) = self.current.take() {
            self.closed.push(run);
        }
        self.closed
    }
}

/// One block as the grammar reads it out of a run.
struct BlockReading {
    at_bit: u64,
    bytes: Vec<Option<u8>>,
    unresolved: u64,
    truncated: bool,
    /// How many framed bytes of the run the block actually covers.
    covered: u64,
    stated: u8,
    computed: u8,
}

impl BlockReading {
    fn read(grammar: &RecordGrammar, shape: &BlockShape, run: &Run) -> Self {
        let want = shape.bytes as usize;
        let truncated = run.length < u64::from(shape.bytes);
        let bytes: Vec<Option<u8>> = run.head.iter().copied().take(want).collect();
        let at = |index: u32| bytes.get(index as usize).copied().flatten();
        let checked: Vec<u8> = (shape.checked_from..shape.checked_to)
            .map(|index| at(index).unwrap_or(0))
            .collect();
        Self {
            at_bit: run.at_bit,
            unresolved: bytes.iter().filter(|byte| byte.is_none()).count() as u64
                + (want - bytes.len()) as u64,
            truncated,
            covered: run.length.min(u64::from(shape.bytes)),
            stated: at(shape.checksum_at).unwrap_or(0),
            computed: grammar.checksum.over(&checked),
            bytes,
        }
    }
}

/// Whether the family's own declaration covers the address a header
/// states — the track within its density map, the sector within what
/// that track's zone claims.
fn within_declaration(track: u8, sector: u8) -> bool {
    C1541
        .zone_for_ratio(u64::from(track), 1)
        .is_some_and(|(_, zone)| u32::from(sector) < zone.records)
}

/// One header block and the data block the recording followed it with.
struct Paired {
    header: BlockReading,
    data: Option<BlockReading>,
}

/// Reads one location's runs into records, in recording order.
///
/// The circle is closed: a header at the end of a location pairs with a
/// data block at its start where nothing else claimed one, because the
/// origin is the write splice rather than a boundary of the recording.
fn pair(grammar: &RecordGrammar, runs: Vec<Run>) -> (Vec<Paired>, u64) {
    let mut records: Vec<Paired> = Vec::new();
    let mut pending: Option<BlockReading> = None;
    let mut leading_data: Option<BlockReading> = None;
    let mut without_a_record = 0u64;

    for run in runs {
        let head = run.head.first().copied().flatten();
        match head.and_then(|mark| grammar.block_of(mark)) {
            Some(shape) if shape.mark == grammar.header.mark => {
                if let Some(header) = pending.take() {
                    records.push(Paired { header, data: None });
                }
                pending = Some(BlockReading::read(grammar, &grammar.header, &run));
            }
            Some(_) => {
                let data = BlockReading::read(grammar, &grammar.data, &run);
                match pending.take() {
                    Some(header) => records.push(Paired {
                        header,
                        data: Some(data),
                    }),
                    // A data block nothing opened. It may be the tail of
                    // a record the origin cut, which the closing pass
                    // below decides.
                    None if leading_data.is_none() && records.is_empty() => {
                        leading_data = Some(data);
                    }
                    None => without_a_record += 1,
                }
            }
            None => without_a_record += 1,
        }
    }
    if let Some(header) = pending.take() {
        records.push(Paired {
            header,
            data: leading_data.take(),
        });
    }
    if leading_data.is_some() {
        without_a_record += 1;
    }
    (records, without_a_record)
}

/// Recognizes the family's sectors out of an encoded bytestream.
fn recognize(
    bytestream: &EncodedBytestream,
    policy: SectorPolicy,
    cache_bytes: u64,
) -> Result<C1541Sectors> {
    let profile = &C1541;
    let grammar = &profile.presentation.record;
    let codec = &profile.presentation.codec;
    if bytestream.profile() != profile.id {
        return Err(Error::invalid_image(
            PROFILE,
            format!(
                "the bytestream is addressed by the profile '{}' and this grammar is \
                 declared by '{}'; a recording is read by the family it is",
                bytestream.profile(),
                profile.id
            ),
        ));
    }
    let symbols_per_byte = codec.symbols_per_byte().ok_or_else(|| {
        Error::invalid_image(
            PROFILE,
            format!(
                "the family's group code records {} bits to a symbol, which do not \
                 divide a byte; there is no byte for a block to be made of",
                codec.data_bits
            ),
        )
    })?;
    let stride = u64::from(symbols_per_byte * codec.symbol_bits);
    let keep = grammar.header.bytes.max(grammar.data.bytes) as usize;

    let described = describe(grammar, &policy, bytestream.provenance());
    let mut writer: SectionWriter<PayloadKey, SessionBacking> =
        SectionWriter::to_sink(SessionBacking::create()?, LEAF_ENTRIES);
    let mut loss = LossAccount::new();
    let mut reported = Vec::new();
    let mut claims: Vec<SectorClaim> = Vec::new();
    let mut payloads: Vec<Option<PayloadKey>> = Vec::new();

    for location in bytestream.locations() {
        let key = location.key();
        let (numerator, denominator) = key.position();

        // Where the layer below said framing began, which is where a
        // record may open.
        let starts: BTreeSet<u64> = bytestream
            .facts(location)?
            .iter()
            .filter_map(|fact| match fact.kind() {
                BytestreamFactKind::Alignment { at_bit, run_bits } => Some(at_bit + run_bits),
                BytestreamFactKind::Unframed { .. } => None,
            })
            .collect();

        // The location's bytes, gathered a chunk at a time so that no
        // other location is touched on the way to this one's records,
        // and no more of any run than a block can need.
        let mut runs = Runs::new(keep, stride, starts);
        let mut framed_bytes = 0u64;
        for ordinal in 0..bytestream.byte_chunks(location) {
            for record in bytestream.byte_chunk(location, ordinal)? {
                runs.push(&record);
                framed_bytes += 1;
            }
        }
        let (records, without_a_record) = pair(grammar, runs.finish());

        let mut headers_here = 0u64;
        let mut readable_here = 0u64;
        let mut failed_here = 0u64;
        let mut paired_here = 0u64;
        let mut covered_bytes = 0u64;
        let mut ordinal = 0u64;
        for record in records {
            let header = record.header;
            let track = header
                .bytes
                .get(grammar.track_at as usize)
                .copied()
                .flatten();
            let sector = header
                .bytes
                .get(grammar.sector_at as usize)
                .copied()
                .flatten();
            headers_here += 1;
            covered_bytes += header.covered;
            if let Some(data) = &record.data {
                paired_here += 1;
                covered_bytes += data.covered;
            }

            let (rule, refusal) = unreadable_because(grammar, &header, record.data.as_ref());
            if let Some(rule) = rule {
                match rule {
                    SectorRule::BlockFailedChecksum
                        if policy.checksum_failure == ChecksumFailurePolicy::Refuse =>
                    {
                        return Err(refuse(
                            rule,
                            format!(
                                "a block of the record at bit {} of half-track \
                                 {numerator}/{denominator} states a checksum its own \
                                 bytes do not compute, and the policy refuses rather \
                                 than reporting a sector the recording does not hold",
                                header.at_bit
                            ),
                        ));
                    }
                    SectorRule::RecordUnpaired
                        if policy.unpaired_record == UnpairedRecordPolicy::Refuse =>
                    {
                        return Err(refuse(
                            rule,
                            format!(
                                "the header at bit {} of half-track \
                                 {numerator}/{denominator} is followed by no data block, \
                                 and the policy refuses rather than reporting a sector \
                                 with nothing in it",
                                header.at_bit
                            ),
                        ));
                    }
                    _ => {}
                }
            }

            let data = record.data;
            let readable = rule.is_none();
            let unresolved = header.unresolved + data.as_ref().map_or(0, |block| block.unresolved);
            if readable {
                readable_here += 1;
            }
            if rule == Some(SectorRule::BlockFailedChecksum) {
                failed_here += 1;
            }

            let track = track.unwrap_or(0);
            let sector = sector.unwrap_or(0);
            let payload = if readable {
                let block = data.as_ref().expect("a readable record holds its data");
                let bytes: Vec<u8> = (grammar.payload_from..grammar.payload_to)
                    .map(|index| block.bytes[index as usize].expect("resolved"))
                    .collect();
                let payload_key = PayloadKey {
                    location: key.clone(),
                    ordinal,
                };
                writer.append(payload_key.clone(), bytes)?;
                ordinal += 1;
                Some(payload_key)
            } else {
                None
            };

            claims.push(SectorClaim {
                half_track_numerator: numerator,
                half_track_denominator: denominator,
                surface: key.surface(),
                at_bit: header.at_bit,
                track,
                sector,
                id_high: header
                    .bytes
                    .get(grammar.id_high_at as usize)
                    .copied()
                    .flatten()
                    .unwrap_or(0),
                id_low: header
                    .bytes
                    .get(grammar.id_low_at as usize)
                    .copied()
                    .flatten()
                    .unwrap_or(0),
                header_checksum_stated: header.stated,
                header_checksum_computed: header.computed,
                has_data: data.is_some(),
                data_at_bit: data.as_ref().map_or(0, |block| block.at_bit),
                data_checksum_stated: data.as_ref().map_or(0, |block| block.stated),
                data_checksum_computed: data.as_ref().map_or(0, |block| block.computed),
                unresolved_bytes: unresolved,
                within_declaration: within_declaration(track, sector),
                readable,
                rule: rule.map(|rule| rule.as_str().to_owned()),
                refusal,
            });
            payloads.push(payload);
        }

        loss.add(
            "byte-outside-a-record",
            "bytes the recording carries that no block of the family's grammar covers \
             — the gaps and tails between records; the sector layer states the blocks \
             and says nothing about what lies between them",
            framed_bytes.saturating_sub(covered_bytes),
        );
        if without_a_record > 0 {
            loss.add(
                "run-without-a-record",
                "framed runs opening with a byte that opens neither of the family's \
                 blocks, so no record was read from them",
                without_a_record,
            );
        }
        reported.push(SectorLocation {
            half_track_numerator: numerator,
            half_track_denominator: denominator,
            surface: key.surface(),
            records_declared: profile
                .zone_for_ratio(numerator, denominator)
                .map(|(_, zone)| zone.records),
            headers: headers_here,
            records: paired_here,
            readable: readable_here,
            failed_checksum: failed_here,
            runs_without_a_record: without_a_record,
        });
    }

    if claims.is_empty() {
        return Err(refuse(
            SectorRule::NoRecordRecognized,
            "no run of this bytestream opens with a byte the family's record grammar \
             names, so there is no sector layer to hold",
        ));
    }

    account_for(&mut loss, &claims);
    let contested = contested_addresses(&claims);
    let (sink, total_bytes) = writer.seal()?;

    let report = SectorReport {
        profile_id: profile.id.to_owned(),
        grammar_id: grammar.id.to_owned(),
        grammar_name: grammar.name.to_owned(),
        payload_bytes: grammar.payload_bytes(),
        locations: reported,
        claims,
        contested,
        declared_loss: loss.into_entries(),
        evidence: described.notes,
    };
    Ok(C1541Sectors {
        report,
        payloads,
        backing: PayloadBacking {
            bytes: Box::new(sink.into_source()),
            total_bytes,
            cache: Mutex::new(SectionCache::with_bytes(cache_bytes)),
            known: vec![profile.id],
        },
    })
}

/// Why a record does not read, in the order the tests apply: what is
/// missing first, then what is not a byte, then what does not check.
fn unreadable_because(
    grammar: &RecordGrammar,
    header: &BlockReading,
    data: Option<&BlockReading>,
) -> (Option<SectorRule>, Option<String>) {
    let reason = |rule: SectorRule, detail: String| (Some(rule), Some(detail));
    if header.truncated {
        return reason(
            SectorRule::BlockTruncated,
            format!(
                "framing ends before the {}-byte header block does",
                grammar.header.bytes
            ),
        );
    }
    if header.unresolved > 0 {
        return reason(
            SectorRule::BlockUnresolved,
            format!(
                "{} byte(s) of the header hold a pattern the family's table does not \
                 assign",
                header.unresolved
            ),
        );
    }
    if header.stated != header.computed {
        return reason(
            SectorRule::BlockFailedChecksum,
            format!(
                "the header states the checksum {:#04x} and its own bytes compute \
                 {:#04x}",
                header.stated, header.computed
            ),
        );
    }
    let Some(data) = data else {
        return reason(
            SectorRule::RecordUnpaired,
            "the recording follows this header with no data block".to_owned(),
        );
    };
    if data.truncated {
        return reason(
            SectorRule::BlockTruncated,
            format!(
                "framing ends before the {}-byte data block does",
                grammar.data.bytes
            ),
        );
    }
    if data.unresolved > 0 {
        return reason(
            SectorRule::BlockUnresolved,
            format!(
                "{} byte(s) of the data block hold a pattern the family's table does \
                 not assign",
                data.unresolved
            ),
        );
    }
    if data.stated != data.computed {
        return reason(
            SectorRule::BlockFailedChecksum,
            format!(
                "the data block states the checksum {:#04x} and its own bytes compute \
                 {:#04x}",
                data.stated, data.computed
            ),
        );
    }
    (None, None)
}

/// The addresses more than one readable claim states.
fn contested_addresses(claims: &[SectorClaim]) -> Vec<ContestedAddress> {
    let mut counts: std::collections::BTreeMap<(u8, u8), u32> = std::collections::BTreeMap::new();
    for claim in claims.iter().filter(|claim| claim.readable) {
        *counts.entry((claim.track, claim.sector)).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, held)| *held > 1)
        .map(|((track, sector), readable_claims)| ContestedAddress {
            track,
            sector,
            readable_claims,
        })
        .collect()
}

/// What the sector layer does not carry of the bytestream beneath it.
fn account_for(loss: &mut LossAccount, claims: &[SectorClaim]) {
    let count = |rule: SectorRule| {
        claims
            .iter()
            .filter(|claim| claim.rule.as_deref() == Some(rule.as_str()))
            .count() as u64
    };
    for (rule, detail) in [
        (
            SectorRule::BlockFailedChecksum,
            "records a block of which states a checksum its own bytes do not compute; \
             recorded here, never repaired, and never served as a sector",
        ),
        (
            SectorRule::BlockUnresolved,
            "records a block of which covers a group the codec left unresolved; a \
             pattern the family's table does not assign is not a byte, so the block is \
             not read",
        ),
        (
            SectorRule::BlockTruncated,
            "records whose framing ends before the block the grammar declares does",
        ),
        (
            SectorRule::RecordUnpaired,
            "header blocks the recording follows with no data block, so the address is \
             stated and its contents are not",
        ),
    ] {
        let held = count(rule);
        if held > 0 {
            loss.add(rule.as_str(), detail, held);
        }
    }
    let outside = claims
        .iter()
        .filter(|claim| !claim.within_declaration)
        .count() as u64;
    if outside > 0 {
        loss.add(
            "address-outside-declaration",
            "records stating a track or sector the family's own density map does not \
             cover; stated as recorded and never corrected onto the grid",
            outside,
        );
    }
}

fn describe(grammar: &RecordGrammar, policy: &SectorPolicy, bytestream: &Provenance) -> Provenance {
    let mut provenance = Provenance::new(PROFILE)
        .note(format!("{}: {}", grammar.name, grammar.provenance))
        .note(format!(
            "a header block opens {:#04x} and a data block {:#04x}; a framed byte that \
             opens neither opens nothing",
            grammar.header.mark, grammar.data.mark
        ))
        .note(format!(
            "each block's checksum is {}, over the bytes the family declares: the \
             header's {}..{} and the data block's {}..{}",
            grammar.checksum.name(),
            grammar.header.checked_from,
            grammar.header.checked_to,
            grammar.data.checked_from,
            grammar.data.checked_to
        ))
        .note(
            "a record's data block is the block the recording carries next after its \
             header, the family writing one framing landmark ahead of each; no distance \
             is measured and no window is tuned"
                .to_owned(),
        )
        .note(match policy.checksum_failure {
            ChecksumFailurePolicy::Refuse => {
                "a block stating a checksum its own bytes do not compute stops the \
                 recognition"
                    .to_owned()
            }
            ChecksumFailurePolicy::DeclareLoss => {
                "a block stating a checksum its own bytes do not compute is kept as a \
                 claim, stated unreadable, and counted"
                    .to_owned()
            }
        })
        .note(match policy.unpaired_record {
            UnpairedRecordPolicy::Refuse => {
                "a header block no data block follows stops the recognition".to_owned()
            }
            UnpairedRecordPolicy::DeclareLoss => {
                "a header block no data block follows is kept as a claim, stated as \
                 holding no data, and counted"
                    .to_owned()
            }
        })
        .note(
            "this is where the bytestream's silence ends: the layer below assigns no \
             byte to a header or a sector, and this one states which bytes it read as \
             both, under the family's declared grammar and nothing else"
                .to_owned(),
        );
    for note in &bytestream.notes {
        provenance = provenance.note(format!("the bytestream beneath it: {note}"));
    }
    provenance
}

// ------------------------------------------------- the partition door

/// The sector layer as a source of the blocks a filesystem addresses.
///
/// It is the whole of what a filesystem above may ask of this layer: one
/// block by the address the recording states for it, or the refusal this
/// layer already made about it. Nothing of the recording — a header, a
/// checksum, a half-track, a claim — crosses that seam, which is what
/// keeps a filesystem adapter from knowing what it is standing on (P18).
impl crate::filesystem::cbm_dos::BlockSource for C1541Sectors {
    fn block(&self, track: u8, sector: u8) -> Result<Vec<u8>> {
        self.read_sector(track, sector)
    }
}

impl C1541Sectors {
    /// The **direct partition** over this recording — the library's own
    /// composition of the whole content, which is what a namespace above
    /// is reached through (P19).
    ///
    /// A recording records no partition scheme, so there is one member
    /// and it is synthetic: its account is provenance and never
    /// evidence, and it composes no addressed extent, because a
    /// recording's blocks are addressed by the recording rather than by
    /// position. The addressable vantage is therefore absent and the
    /// namespace vantage is *declared* —
    /// [`filesystem_as`](crate::PartitionView::filesystem_as) with
    /// `"cbmdos"` — because nothing here determines a reading and this
    /// layer will not pick one.
    ///
    /// **The sector layer carries no file verbs of its own.** It may be
    /// asked what it composes — this — and may not be told to act as a
    /// namespace it is not: the node the file verbs live on is the
    /// namespace itself.
    ///
    /// The declaration's refusal is the seam that ran out of answers
    /// stating it: a disk whose directory track does not read says so
    /// with the sector layer's own rule identity, and one that reads but
    /// claims no CBM DOS says *that*. Everything beneath stays readable
    /// either way — a disk with no filesystem is still a recording,
    /// still a sector layer, and still every claim this layer made about
    /// it.
    pub fn partition(&self) -> crate::PartitionView<'_> {
        crate::PartitionView::over_blocks(self)
    }
}

// ------------------------------------------------------ the entry point

impl C1541Bytestream {
    /// Recognizes the recording's own sectors out of this bytestream,
    /// under the family's declared record grammar and sector reading
    /// (P23, P30 reached through the type).
    ///
    /// It takes no policy because the profile carries one, and what was
    /// used travels into the result as provenance. The bytestream is
    /// untouched and stays exactly what it was; the sector layer is
    /// separate session state, carrying this bytestream's provenance
    /// beneath the grammar and policy that produced it. There is no way
    /// back down (P33): a sector is not lowered into bytes.
    pub fn recognize_sectors(&self, cache_bytes: u64) -> Result<C1541Sectors> {
        recognize(
            self.inner(),
            crate::flux::drive_profile::C1541.presentation.sector_policy,
            cache_bytes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flux::c1541::presentation::{
        DensityPolicy, ReadChannelPolicy, UnzonedPolicy, WeakPulsePolicy, materialize_bitstream,
    };
    use crate::flux::capture::TimeBase;
    use crate::flux::medium::{
        Derivation, FluxMedium, MediumBuilder, MediumFact, MediumFactKind, OriginRule,
        OriginStatement, Pulse, RotationalFrame, Strength,
    };

    const CYCLES_PER_ROTATION: u64 = 3_200_000;

    fn policy() -> SectorPolicy {
        SectorPolicy {
            checksum_failure: ChecksumFailurePolicy::DeclareLoss,
            unpaired_record: UnpairedRecordPolicy::DeclareLoss,
        }
    }

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    /// The bits one byte is recorded as, through the family's own table.
    fn record_byte(bits: &mut Vec<bool>, byte: u8) {
        let codec = &C1541.presentation.codec;
        for nibble in [byte >> 4, byte & 0x0f] {
            let symbol = codec.symbols[nibble as usize];
            for shift in (0..codec.symbol_bits).rev() {
                bits.push(symbol >> shift & 1 == 1);
            }
        }
    }

    fn sync(bits: &mut Vec<bool>) {
        for _ in 0..12 {
            bits.push(true);
        }
    }

    /// One sector as CBM DOS writes it: sync, header, gap, sync, data,
    /// tail. `corrupt_data` flips one payload byte after the checksum is
    /// taken, which is exactly what a failing block looks like.
    fn recorded_sector(track: u8, sector: u8, fill: u8, corrupt_data: bool) -> Vec<bool> {
        let grammar = &C1541.presentation.record;
        let mut bits = Vec::new();
        let id = (0x50u8, 0x43u8);
        sync(&mut bits);
        let checksum = sector ^ track ^ id.1 ^ id.0;
        for byte in [
            grammar.header.mark,
            checksum,
            sector,
            track,
            id.1,
            id.0,
            0x0f,
            0x0f,
        ] {
            record_byte(&mut bits, byte);
        }
        // The gap the family writes between the two blocks.
        for _ in 0..9 {
            record_byte(&mut bits, 0x55);
        }
        sync(&mut bits);
        record_byte(&mut bits, grammar.data.mark);
        let mut payload = vec![fill; grammar.payload_bytes() as usize];
        for (at, byte) in payload.iter_mut().enumerate() {
            *byte = byte.wrapping_add((at % 251) as u8);
        }
        let checksum = payload.iter().fold(0u8, |sum, byte| sum ^ byte);
        if corrupt_data {
            payload[7] ^= 0xff;
        }
        for byte in &payload {
            record_byte(&mut bits, *byte);
        }
        record_byte(&mut bits, checksum);
        record_byte(&mut bits, 0x00);
        record_byte(&mut bits, 0x00);
        for _ in 0..8 {
            record_byte(&mut bits, 0x55);
        }
        bits
    }

    /// The cell the family declares for a location.
    fn cell_of(location: &LocationKey) -> u64 {
        let (_, zone) = C1541
            .zone_for_ratio(location.position().0, location.position().1)
            .expect("the test writes on declared tracks");
        let (numerator, denominator) = zone.nominal_cell(&C1541.rotation);
        u64::try_from(numerator / denominator).expect("the cell is whole")
    }

    /// A medium holding one location whose pulses are `bits` laid down
    /// at that location's declared cell.
    fn medium_of(location: LocationKey, bits: &[bool]) -> FluxMedium {
        let cell = cell_of(&location);
        let mut pulses = Vec::new();
        let mut at = 0u64;
        for bit in bits {
            at += cell;
            if *bit {
                pulses.push(Pulse::new(at, Strength::certain(2)));
            }
        }
        let frame = RotationalFrame::new(
            C1541.id,
            TimeBase::new(C1541.id, 16_000_000, 1).expect("the rate is stated"),
            CYCLES_PER_ROTATION,
            OriginStatement::new(
                OriginRule::Seam,
                Provenance::new(C1541.id).note("placed at the seam for the test"),
            ),
        )
        .expect("the frame states a circle");
        let mut builder = MediumBuilder::new(
            C1541.id,
            C1541.media,
            frame,
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
                    MediumFactKind::Seam { angle: 0 },
                    Provenance::new(C1541.id).note("located for the test"),
                )],
                Provenance::new(C1541.id).note("reduced for the test"),
            )
            .expect("the location");
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
        medium
    }

    fn read(location: LocationKey, bits: &[bool], policy: SectorPolicy) -> Result<C1541Sectors> {
        let medium = medium_of(location, bits);
        let bitstream = materialize_bitstream(
            &medium,
            ReadChannelPolicy {
                density: DensityPolicy::Declared,
                unzoned: UnzonedPolicy::Refuse,
                weak_pulse: WeakPulsePolicy::Seeded,
                seed: 0x0123_4567_89ab_cdef,
            },
            1 << 20,
        )
        .expect("the channel clocks it");
        let bytestream = bitstream
            .materialize_bytestream(1 << 20)
            .expect("the codec resolves it");
        recognize(bytestream.inner(), policy, 1 << 20)
    }

    #[test]
    fn a_recorded_sector_is_read_by_the_address_it_states_for_itself() {
        let location = LocationKey::new(C1541.id, 18, 0);
        let mut bits = recorded_sector(18, 3, 0x10, false);
        bits.extend(recorded_sector(18, 4, 0x80, false));
        let sectors = read(location, &bits, policy()).expect("the grammar reads it");

        let report = sectors.inspect();
        assert_eq!(report.profile_id, "c1541");
        assert_eq!(report.grammar_id, "cbm-dos-record");
        assert_eq!(report.payload_bytes, 256);
        assert_eq!(report.claims.len(), 2);
        assert_eq!(report.locations.len(), 1);
        assert_eq!(report.locations[0].records_declared, Some(19));
        assert_eq!(report.locations[0].readable, 2);

        // The address is what the header states, and the checksums are
        // stated beside computed rather than collapsed into a verdict.
        let claim = &report.claims[0];
        assert_eq!((claim.track, claim.sector), (18, 3));
        assert_eq!((claim.id_high, claim.id_low), (0x50, 0x43));
        assert_eq!(claim.header_checksum_stated, claim.header_checksum_computed);
        assert_eq!(claim.data_checksum_stated, claim.data_checksum_computed);
        assert!(claim.has_data && claim.readable && claim.within_declaration);
        assert_eq!(claim.unresolved_bytes, 0);
        assert!(claim.rule.is_none() && claim.refusal.is_none());

        // And the payload is the recording's, byte for byte.
        let payload = sectors.read_sector(18, 3).expect("the sector reads");
        assert_eq!(payload.len(), 256);
        let expected: Vec<u8> = (0..256u32)
            .map(|at| 0x10u8.wrapping_add((at as usize % 251) as u8))
            .collect();
        assert_eq!(payload, expected);
        assert_ne!(sectors.read_sector(18, 4).expect("reads"), payload);
    }

    #[test]
    fn an_address_the_recording_does_not_state_is_a_named_refusal() {
        let location = LocationKey::new(C1541.id, 18, 0);
        let sectors = read(location, &recorded_sector(18, 3, 0x10, false), policy())
            .expect("the grammar reads it");

        let error = sectors
            .read_sector(18, 9)
            .expect_err("nothing states that address");
        assert_eq!(error.category(), ErrorCategory::NotFound);
        assert_eq!(error.rule(), Some(SectorRule::NoSuchAddress.as_str()));
        // The refusal says what the family declares there rather than
        // only that the answer is no.
        assert!(
            error.to_string().contains("19 sectors on track 18"),
            "{error}"
        );

        // A track outside the density map is refused the same way, and
        // says why there is no such track.
        let error = sectors.read_sector(41, 0).expect_err("no such track");
        assert!(error.to_string().contains("no track 41"), "{error}");
    }

    #[test]
    fn a_block_that_fails_its_own_checksum_is_recorded_or_refused_and_never_filled() {
        let location = LocationKey::new(C1541.id, 18, 0);
        let bits = recorded_sector(18, 3, 0x10, true);

        // Declared: the claim survives with both checksums on it, the
        // address is not served, and the account says so.
        let sectors = read(location.clone(), &bits, policy()).expect("the grammar reads it");
        let claim = &sectors.inspect().claims[0];
        assert_eq!((claim.track, claim.sector), (18, 3));
        assert!(!claim.readable);
        assert_ne!(claim.data_checksum_stated, claim.data_checksum_computed);
        assert_eq!(claim.rule.as_deref(), Some("block-failed-checksum"));
        assert!(
            claim
                .refusal
                .as_ref()
                .expect("a claim that does not read says why")
                .contains("data block states the checksum"),
            "{claim:?}"
        );
        assert!(
            sectors
                .inspect()
                .declared_loss
                .iter()
                .any(|loss| loss.code == "block-failed-checksum")
        );

        let error = sectors
            .read_sector(18, 3)
            .expect_err("an unreadable address is a refusal, not a filled block");
        assert_eq!(error.category(), ErrorCategory::Unavailable);
        assert_eq!(error.rule(), Some(SectorRule::AddressNotReadable.as_str()));

        // Refusing: the recognition stops and names where.
        let error = read(
            location,
            &bits,
            SectorPolicy {
                checksum_failure: ChecksumFailurePolicy::Refuse,
                ..policy()
            },
        )
        .expect_err("the policy refuses");
        assert_eq!(error.rule(), Some(SectorRule::BlockFailedChecksum.as_str()));
        assert!(error.to_string().contains("half-track 18/1"), "{error}");
    }

    #[test]
    fn a_header_no_data_block_follows_states_its_address_and_nothing_else() {
        // A header, its gap, and then the next sector: the first header
        // is followed by another header rather than by data.
        let grammar = &C1541.presentation.record;
        let mut bits = Vec::new();
        sync(&mut bits);
        let checksum = 5u8 ^ 18 ^ 0x43 ^ 0x50;
        for byte in [grammar.header.mark, checksum, 5, 18, 0x43, 0x50, 0x0f, 0x0f] {
            record_byte(&mut bits, byte);
        }
        for _ in 0..9 {
            record_byte(&mut bits, 0x55);
        }
        bits.extend(recorded_sector(18, 6, 0x20, false));

        let location = LocationKey::new(C1541.id, 18, 0);
        let sectors = read(location.clone(), &bits, policy()).expect("the grammar reads it");
        let claim = &sectors.inspect().claims[0];
        assert_eq!((claim.track, claim.sector), (18, 5));
        assert!(!claim.has_data && !claim.readable);
        assert_eq!(claim.rule.as_deref(), Some("record-unpaired"));
        assert!(sectors.read_sector(18, 5).is_err());
        // The one that is whole still reads: an unpaired neighbour
        // costs nothing.
        assert_eq!(sectors.read_sector(18, 6).expect("reads").len(), 256);

        let error = read(
            location,
            &bits,
            SectorPolicy {
                unpaired_record: UnpairedRecordPolicy::Refuse,
                ..policy()
            },
        )
        .expect_err("the policy refuses");
        assert_eq!(error.rule(), Some(SectorRule::RecordUnpaired.as_str()));
    }

    #[test]
    fn the_grammar_and_everything_beneath_it_travel_into_the_recognition() {
        let location = LocationKey::new(C1541.id, 18, 0);
        let sectors = read(location, &recorded_sector(18, 3, 0x10, false), policy())
            .expect("the grammar reads it");
        let evidence = &sectors.inspect().evidence;

        assert!(
            evidence
                .iter()
                .any(|line| line.contains("CBM DOS sector record")),
            "{evidence:?}"
        );
        assert!(
            evidence
                .iter()
                .any(|line| line.contains("carries next after its header")),
            "{evidence:?}"
        );
        // The whole ladder is still readable from up here: the codec,
        // the channel, and the policy that produced the medium.
        assert!(
            evidence
                .iter()
                .any(|line| line.contains("Commodore group-coded recording")),
            "{evidence:?}"
        );
        assert!(
            evidence
                .iter()
                .any(|line| line.contains("selected observation 0")),
            "{evidence:?}"
        );
        // And the payloads are session-backed rather than resident.
        assert!(sectors.backing_bytes() >= 256);
    }

    #[test]
    fn a_bytestream_holding_no_record_of_the_grammar_is_refused_by_name() {
        // A track of nothing but the family's landmark and gap bytes:
        // framed, resolvable, and opening no block the grammar names.
        let mut bits = Vec::new();
        sync(&mut bits);
        for _ in 0..200 {
            record_byte(&mut bits, 0x55);
        }
        let error = read(LocationKey::new(C1541.id, 18, 0), &bits, policy())
            .expect_err("no record is a refusal rather than an empty layer");
        assert_eq!(error.rule(), Some(SectorRule::NoRecordRecognized.as_str()));
    }

    #[test]
    fn the_rule_set_is_stable_and_reads_back() {
        for rule in SectorRule::ALL {
            assert_eq!(SectorRule::from_identity(rule.as_str()), Some(rule));
            assert!(!rule.as_str().is_empty());
        }
        // An identity from another seam's set is not a nearest match.
        assert_eq!(SectorRule::from_identity("no-namespace"), None);
    }
}
