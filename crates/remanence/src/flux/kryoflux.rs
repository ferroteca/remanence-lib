// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The KryoFlux capture-set adapter: one logical capture assembled from
//! a declared collection of members, not one image-like member.
//!
//! A KryoFlux capture of a disk is a stream file per head per drive-step
//! position — a hundred and sixty-eight of them for a two-head pass over
//! eighty-four positions — kept together as the artifact the capture
//! produced. This adapter owns what that set is: the member grammar and
//! its completeness, the track and side identity a member's *name* is
//! the only record of, the stream grammar each member is decoded by, the
//! index and control records inside it, the transfer result, and the
//! provenance tying every fact back to the member it was read from.
//!
//! It owns nothing above that. It does not merge the two heads into an
//! ideal disk, choose between passes, average a timing, or materialize a
//! medium, bitstream, sector, or file. What leaves it is a
//! [`crate::flux::capture::FluxCapture`] — evidence, ordered, attributed,
//! and refused by name where the set does not hold together.
//!
//! The members arrive as a **declared collection** — the source shape
//! this format reads: the caller's own opened files, or files gathered
//! from another medium's namespace. Whatever produced their bytes knows
//! none of this grammar, and the grammar below is the whole of what says
//! which of them are a capture set (P12, P19).
//!
//! Nothing stays resident whole (P27). The members decode once, one at
//! a time, into the flux layer's section-addressable backing in private
//! session storage; what stays in memory is identity and shape, and the
//! pulses load a bounded section at a time from there.

use crate::error::{Error, ErrorCategory, Result};
use crate::evidence::{Issue, Provenance};
use crate::flux::capture::{
    CaptureBuilder, CaptureRun, FluxCapture, ForeignRecord, Marker, MarkerKind, MetadataRecord,
    SessionBacking, SourceDescriptor, SourceRange, Tick, TimeBase, TrackKey,
};

/// The namespace every fact this adapter states is declared under.
pub(crate) const KRYOFLUX: &str = "kryoflux";

/// The KryoFlux sample clock, exactly: `((18432000 * 73) / 14) / 2 / 2`
/// hertz, which is `1345536000 / 56` and is not representable as a
/// decimal or a float.
///
/// The adapter declares it because the adapter is what knows the device;
/// the stream's own `sck` is a rounded decimal, retained as the declared
/// fact it is and checked against this rather than believed in place of
/// it.
const SAMPLE_CLOCK_NUMERATOR: u64 = 18_432_000 * 73;
const SAMPLE_CLOCK_DENOMINATOR: u64 = 56;

/// The largest member this adapter decodes (P27). A KryoFlux stream
/// holds a handful of revolutions of one track; anything larger is
/// refused by size before a byte of it is read.
const MEMBER_BOUND: u64 = 64 * 1024 * 1024;

/// The out-of-band record types this adapter names. Everything else is
/// retained verbatim as a foreign record rather than discarded.
const OOB_STREAM_INFO: u8 = 0x01;
const OOB_INDEX: u8 = 0x02;
const OOB_STREAM_END: u8 = 0x03;
const OOB_KF_INFO: u8 = 0x04;
const OOB_END_OF_STREAM: u8 = 0x0d;

/// The byte that introduces an out-of-band record.
const OOB_SIGN: u8 = 0x0d;

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image(KRYOFLUX, reason)
}

fn refuse_as(category: ErrorCategory, reason: impl Into<String>) -> Error {
    Error::categorized_image(category, KRYOFLUX, reason)
}

// ---------------------------------------------------------------- names

/// A member name split into what the capture-set grammar reads out of
/// it: the capture it belongs to, the drive-step position, and the head.
///
/// The name is the only place a KryoFlux capture's position exists — a
/// stream declares no track or side in its own out-of-band data — so
/// this parse is load-bearing evidence rather than a convenience.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MemberName {
    capture: String,
    step: u64,
    head: u64,
}

/// Reads `<capture><SS>.<H>.raw`, the layout the capture tool writes and
/// the only one this version admits.
///
/// Exactly two digits of step and one of head, deliberately: a looser
/// match would be a filename heuristic wearing a grammar's clothes, and
/// widening it is later adapter work with its own evidence behind it.
fn parse_member_name(name: &str) -> Option<MemberName> {
    // Read backwards in bytes rather than characters: a name may hold
    // anything at all before the fixed tail, and splitting a string
    // blindly two bytes from its end would land inside a character.
    let bytes = name.as_bytes();
    let extension = bytes.len().checked_sub(4)?;
    if !bytes[extension..].eq_ignore_ascii_case(b".raw") {
        return None;
    }
    let head_at = extension.checked_sub(2)?;
    if bytes[head_at] != b'.' || !bytes[head_at + 1].is_ascii_digit() {
        return None;
    }
    let step_at = head_at.checked_sub(2)?;
    if !bytes[step_at..head_at].iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(MemberName {
        // A digit is one byte in UTF-8, so the step's first byte is a
        // character boundary and the capture name splits cleanly there.
        capture: name[..step_at].to_owned(),
        step: u64::from(bytes[step_at] - b'0') * 10 + u64::from(bytes[step_at + 1] - b'0'),
        head: u64::from(bytes[head_at + 1] - b'0'),
    })
}

/// One member the grammar admitted, and which member of the collection
/// it is.
#[derive(Debug, Clone)]
struct AdmittedMember {
    /// The member's grammar-bearing name.
    name: String,
    /// Its place in the declared collection, which is where its bytes
    /// are.
    index: usize,
    step: u64,
    head: u64,
}

/// Reads the declared collection's names as one capture set, or names
/// why they are not one.
///
/// Every refusal states the discovered evidence — the member, the
/// position, the head — because "not a capture set" without it is a
/// verdict with nothing behind it (P4).
fn admit_capture_set(names: &[String], collection: &str) -> Result<Vec<AdmittedMember>> {
    let mut members: Vec<AdmittedMember> = Vec::new();
    let mut capture: Option<String> = None;

    for (index, name) in names.iter().enumerate() {
        let parsed = parse_member_name(name).ok_or_else(|| {
            refuse(format!(
                "'{name}' is not a KryoFlux stream member: this version admits \
                 '<capture><SS>.<H>.raw' and nothing else, so {collection} holds \
                 something that is not part of one capture set"
            ))
        })?;
        match &capture {
            None => capture = Some(parsed.capture.clone()),
            Some(named) if *named != parsed.capture => {
                return Err(refuse(format!(
                    "'{name}' names the capture '{}' where an earlier member named \
                     '{named}', so {collection} holds more than one capture set",
                    parsed.capture
                )));
            }
            Some(_) => {}
        }
        members.push(AdmittedMember {
            name: name.clone(),
            index,
            step: parsed.step,
            head: parsed.head,
        });
    }

    if members.is_empty() {
        return Err(refuse_as(
            ErrorCategory::NotFound,
            format!("{collection} holds no KryoFlux stream members"),
        ));
    }

    members.sort_by_key(|member| (member.step, member.head));
    for pair in members.windows(2) {
        if pair[0].step == pair[1].step && pair[0].head == pair[1].head {
            return Err(refuse(format!(
                "step position {} head {} is captured twice, by '{}' and '{}', so \
                 the set states two different things about one location",
                pair[0].step, pair[0].head, pair[0].name, pair[1].name
            )));
        }
    }

    // The heads the first position was captured by are the heads the
    // whole set must carry: a position short of one is an incomplete
    // capture, not a location that happens to hold less.
    let first_step = members[0].step;
    let heads: Vec<u64> = members
        .iter()
        .filter(|member| member.step == first_step)
        .map(|member| member.head)
        .collect();
    for (expected, head) in heads.iter().enumerate() {
        if *head != expected as u64 {
            return Err(refuse(format!(
                "step position {first_step} is captured by head {head} where head \
                 {expected} is absent, so the set numbers its heads from something \
                 other than zero"
            )));
        }
    }
    if first_step != 0 {
        return Err(refuse(format!(
            "the set's lowest step position is {first_step}, so every position \
             below it is absent from a capture that should begin at zero"
        )));
    }

    let last_step = members.last().expect("the set is not empty").step;
    for step in 0..=last_step {
        for head in 0..heads.len() as u64 {
            if !members
                .iter()
                .any(|member| member.step == step && member.head == head)
            {
                return Err(refuse(format!(
                    "step position {step} head {head} is absent, so the set is \
                     incomplete: {} members cover positions 0 to {last_step} by \
                     {} heads, which wants {}",
                    members.len(),
                    heads.len(),
                    (last_step + 1) * heads.len() as u64
                )));
            }
        }
    }

    Ok(members)
}

// --------------------------------------------------------------- stream

/// An index record read out of the stream, waiting for the flux it
/// names.
///
/// The device reports an index out of band and *ahead* of the cell it
/// belongs to, so the record cannot be placed in time until the whole
/// transfer has been decoded.
struct PendingIndex {
    position: u64,
    counter: u64,
    index_counter: u64,
    /// Where the record sat in the member, for its provenance.
    at: usize,
    payload: Vec<u8>,
}

/// What decoding one member's stream established.
#[derive(Debug)]
struct StreamFacts {
    run: CaptureRun,
    /// The `KFInfo` key and value pairs, in the source's own spelling
    /// and order.
    metadata: Vec<(String, String)>,
    /// Records this version has no named home for, retained verbatim.
    foreign: Vec<(String, SourceRange, Vec<u8>)>,
    issues: Vec<Issue>,
    /// The transfer result the stream declared, if it declared one.
    /// Read by the unit tests alone today: the run's issues already
    /// carry an unclean result, and reporting the clean ones is the
    /// capture-inspection surface, which stays out with the question
    /// tier (F59).
    #[cfg_attr(not(test), allow(dead_code))]
    transfer_result: Option<u32>,
    /// The sample clock the stream declared, in its own decimal.
    declared_sample_clock: Option<String>,
}

/// Decodes one KryoFlux stream into the run it recorded.
///
/// The flux cells, the asynchronous index observations, the transport
/// control records and the device information keep their separate
/// meanings: only the index records become timed markers, and everything
/// the transport said about itself stays beside the run as provenance,
/// an issue, or a retained record. Nothing before the first index or
/// after the last is dropped — bounding into circular observations
/// happens later and does not consume the run.
fn decode_stream(name: &str, bytes: &[u8]) -> Result<StreamFacts> {
    let mut transitions: Vec<Tick> = Vec::new();
    // Where each cell's first byte sat in the stream, so an index
    // naming a stream position can be placed against the cell it was
    // measured in.
    let mut cell_starts: Vec<u64> = Vec::new();
    let mut metadata = Vec::new();
    let mut foreign = Vec::new();
    let mut issues = Vec::new();
    let mut pending: Vec<PendingIndex> = Vec::new();
    let mut transfer_result = None;
    let mut declared_sample_clock = None;
    let mut notes = Vec::new();
    let mut stream_reads = 0u64;
    let mut nops = 0u64;
    let mut ended = false;

    let mut at = 0usize;
    // The position of the next byte in the stream proper, which is the
    // file with every out-of-band block taken out of it. That is the
    // frame an index record names, and it is not the file offset.
    let mut stream_position = 0u64;
    let mut cell_start = 0u64;
    let mut total: Tick = 0;
    let mut overflow: u64 = 0;

    while at < bytes.len() {
        let head = bytes[at];
        let (value, width) = match head {
            0x00..=0x07 => {
                let low = *bytes
                    .get(at + 1)
                    .ok_or_else(|| refuse(format!("'{name}' ends inside a two-byte flux value")))?;
                (Some((u64::from(head) << 8) | u64::from(low)), 2usize)
            }
            0x08 => (None, 1),
            0x09 => (None, 2),
            0x0a => (None, 3),
            0x0b => {
                overflow += 0x1_0000;
                stream_position += 1;
                at += 1;
                continue;
            }
            0x0c => {
                let (Some(&high), Some(&low)) = (bytes.get(at + 1), bytes.get(at + 2)) else {
                    return Err(refuse(format!(
                        "'{name}' ends inside a three-byte flux value"
                    )));
                };
                (Some((u64::from(high) << 8) | u64::from(low)), 3)
            }
            OOB_SIGN => {
                let kind = *bytes.get(at + 1).ok_or_else(|| {
                    refuse(format!("'{name}' ends inside an out-of-band record header"))
                })?;
                if kind == OOB_END_OF_STREAM {
                    ended = true;
                    at += 4.min(bytes.len() - at);
                    break;
                }
                let size = bytes
                    .get(at + 2..at + 4)
                    .map(|size| usize::from(u16::from_le_bytes([size[0], size[1]])))
                    .ok_or_else(|| {
                        refuse(format!("'{name}' ends inside an out-of-band record header"))
                    })?;
                let payload = bytes.get(at + 4..at + 4 + size).ok_or_else(|| {
                    refuse(format!(
                        "'{name}' declares an out-of-band record of {size} bytes at byte \
                         {at}, which reaches past the {} bytes the member holds",
                        bytes.len()
                    ))
                })?;
                match kind {
                    OOB_STREAM_INFO => stream_reads += 1,
                    OOB_INDEX => {
                        let [position, counter, index_counter] = read_u32_triple(name, payload)?;
                        pending.push(PendingIndex {
                            position,
                            counter,
                            index_counter,
                            at,
                            payload: payload.to_vec(),
                        });
                    }
                    OOB_STREAM_END => {
                        let [position, result] = read_u32_pair(name, payload)?;
                        transfer_result = Some(result as u32);
                        notes.push(format!(
                            "transfer ended at stream position {position} with result {result}"
                        ));
                        if result != 0 {
                            issues.push(Issue::new(
                                "kryoflux-transfer-result",
                                format!(
                                    "the capture tool declared transfer result {result}, so \
                                     this stream is not a clean read of the position"
                                ),
                            ));
                        }
                    }
                    OOB_KF_INFO => {
                        for (key, value) in parse_kf_info(payload) {
                            if key == "sck" {
                                declared_sample_clock = Some(value.clone());
                            }
                            metadata.push((key, value));
                        }
                    }
                    other => foreign.push((
                        format!("oob-{other:02x}"),
                        SourceRange::new(at as u64, (size + 4) as u64),
                        payload.to_vec(),
                    )),
                }
                at += 4 + size;
                continue;
            }
            other => (Some(u64::from(other)), 1),
        };

        match value {
            Some(value) => {
                cell_starts.push(cell_start);
                total = total.checked_add(overflow + value).ok_or_else(|| {
                    refuse(format!("'{name}' accumulates past what a tick can count"))
                })?;
                transitions.push(total);
                overflow = 0;
            }
            None => nops += 1,
        }
        at += width;
        stream_position += width as u64;
        cell_start = stream_position;
    }

    if !ended {
        issues.push(Issue::new(
            "kryoflux-no-end-of-stream",
            "the member ends without the end-of-stream record the capture tool \
             writes, so what it holds may be less than what was transferred",
        ));
    }
    if transfer_result.is_none() {
        issues.push(Issue::new(
            "kryoflux-no-transfer-result",
            "the member declares no transfer result, so nothing in it says the \
             read completed",
        ));
    }
    if at < bytes.len() {
        foreign.push((
            "trailing".to_owned(),
            SourceRange::new(at as u64, (bytes.len() - at) as u64),
            bytes[at..].to_vec(),
        ));
    }

    let mut markers = Vec::with_capacity(pending.len());
    for index in pending {
        if index.position > stream_position {
            return Err(refuse(format!(
                "'{name}' records an index at stream position {}, which lies past \
                 the {stream_position} bytes of flux the member holds",
                index.position
            )));
        }
        let tick = index_tick(&cell_starts, &transitions, index.position, index.counter);
        markers.push(
            Marker::new(
                tick,
                MarkerKind::Index,
                Provenance::new(KRYOFLUX).note(format!(
                    "index record at byte {}: stream position {}, {} sample ticks \
                     into the flux cell measured there, index counter {}",
                    index.at, index.position, index.counter, index.index_counter
                )),
            )
            // The source record's own bytes, kept beside the placement
            // this adapter read out of them.
            .with_payload(index.payload),
        );
    }

    notes.insert(
        0,
        format!(
            "read from KryoFlux stream member '{name}': {} flux transitions over \
             {stream_position} stream bytes, {} index records, {stream_reads} \
             transport reads, {nops} padding records",
            transitions.len(),
            markers.len()
        ),
    );

    let mut provenance = Provenance::new(KRYOFLUX);
    for note in notes {
        provenance = provenance.note(note);
    }
    let run = CaptureRun::new(KRYOFLUX, 0, provenance, transitions, markers)?;
    Ok(StreamFacts {
        run,
        metadata,
        foreign,
        issues,
        transfer_result,
        declared_sample_clock,
    })
}

/// Places one index observation in the run's own ticks.
///
/// The device reports an index out of band and ahead of the flux it
/// belongs to: the record names the stream position of the flux cell
/// that was being measured when the pulse arrived, and how many sample
/// ticks into that cell it arrived. So the index sits that many ticks
/// after the transition the cell *began* at — the preceding one, not the
/// one the cell ends in.
fn index_tick(cell_starts: &[u64], transitions: &[Tick], position: u64, counter: u64) -> Tick {
    let cell = cell_starts.partition_point(|start| *start <= position);
    // `partition_point` counts the cells at or below the position, so
    // the cell in progress is the last of them and the transition it
    // began at is the one before that.
    match cell.checked_sub(2) {
        Some(previous) => transitions[previous] + counter,
        None => counter,
    }
}

fn read_u32_pair(name: &str, payload: &[u8]) -> Result<[u64; 2]> {
    let values = read_u32s(payload, 2).ok_or_else(|| {
        refuse(format!(
            "'{name}' states an out-of-band record of {} bytes where two 32-bit \
             values were declared",
            payload.len()
        ))
    })?;
    Ok([values[0], values[1]])
}

fn read_u32_triple(name: &str, payload: &[u8]) -> Result<[u64; 3]> {
    let values = read_u32s(payload, 3).ok_or_else(|| {
        refuse(format!(
            "'{name}' states an out-of-band record of {} bytes where three 32-bit \
             values were declared",
            payload.len()
        ))
    })?;
    Ok([values[0], values[1], values[2]])
}

fn read_u32s(payload: &[u8], count: usize) -> Option<Vec<u64>> {
    if payload.len() < count * 4 {
        return None;
    }
    Some(
        payload
            .chunks_exact(4)
            .take(count)
            .map(|four| u64::from(u32::from_le_bytes([four[0], four[1], four[2], four[3]])))
            .collect(),
    )
}

/// Splits a device-information record into its stated keys and values,
/// each in the source's own spelling.
///
/// Nothing here interprets a value: the record is a comma-separated list
/// of `key=value` pairs terminated by a NUL, and what any of them means
/// belongs to the namespace that wrote them.
fn parse_kf_info(payload: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(payload);
    let text = text.split('\0').next().unwrap_or_default();
    text.split(',')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((key.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

/// Checks the stream's declared sample clock against the one this
/// adapter claims, to the precision the stream itself stated.
///
/// The declared value is a rounded decimal of a rate that has no exact
/// decimal, so it is never adopted in place of the exact rational: it is
/// a declared fact, retained as written, and its only job here is to say
/// whether this stream came off the device the adapter claims. A
/// disagreement past its own last digit is another instrument, and is
/// refused by name rather than timed as if it were this one.
fn check_sample_clock(name: &str, declared: &str) -> Result<()> {
    let (whole, fraction) = declared.split_once('.').unwrap_or((declared, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 18
    {
        return Err(refuse(format!(
            "'{name}' declares the sample clock as '{declared}', which states no rate"
        )));
    }
    let scale = 10u64.pow(fraction.len() as u32);
    let stated: u64 = format!("{whole}{fraction}").parse().map_err(|_| {
        refuse(format!(
            "'{name}' declares a sample clock past what a rate can count"
        ))
    })?;
    // |stated/scale - claimed| < one unit in the stream's last place,
    // by exact cross-multiplication: the decimal is a truncation, so it
    // is never a whole unit away from the rate it was truncated from.
    let left = u128::from(stated) * u128::from(SAMPLE_CLOCK_DENOMINATOR);
    let right = u128::from(SAMPLE_CLOCK_NUMERATOR) * u128::from(scale);
    let difference = left.abs_diff(right);
    if difference >= u128::from(SAMPLE_CLOCK_DENOMINATOR) {
        return Err(refuse(format!(
            "'{name}' declares a sample clock of {declared} Hz, which disagrees with \
             the {SAMPLE_CLOCK_NUMERATOR}/{SAMPLE_CLOCK_DENOMINATOR} Hz this adapter \
             claims past the precision the stream itself stated"
        )));
    }
    Ok(())
}

// ------------------------------------------------------------------ set

/// A source's own drive-step position, held exactly. Sources step in
/// fractions, so this is a ratio and never a rounded whole number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StepPosition {
    pub(crate) numerator: u64,
    pub(crate) denominator: u64,
}

/// One member of a declared collection, as the assembler reads it: a
/// name the grammar places, and the bytes behind it, produced only
/// after the name and the size have both been admitted.
pub(crate) trait MemberSource {
    /// The grammar-bearing name — the only record of the member's
    /// position, a stream declaring no track or side of its own.
    fn name(&self) -> &str;
    /// The member's size, checked against the bound before a byte of it
    /// is read (P27).
    fn size(&self) -> u64;
    /// The member's bytes, whole: one stream of one position, decoded
    /// once into the capture's own backing and not kept.
    fn bytes(&self) -> Result<Vec<u8>>;
}

impl<M: MemberSource + ?Sized> MemberSource for Box<M> {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn size(&self) -> u64 {
        (**self).size()
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        (**self).bytes()
    }
}

/// One capture assembled from a declared collection, and the evidence
/// of how.
pub(crate) struct AssembledCapture {
    pub(crate) capture: FluxCapture,
    /// How the set was recognized, in human-readable terms (P4).
    pub(crate) evidence: Vec<String>,
    /// The collection's own bytes, summed — the raw plane's extent.
    pub(crate) source_bytes: u64,
}

/// Assembles the declared collection into one capture, or names why it
/// is not one (P29's "checked whole": the member grammar, the set's
/// completeness, and every stream's own grammar, before anything else
/// runs).
///
/// `collection` is how refusals name the set — the caller's collection
/// has no one path, so the refusal says what the caller declared.
/// An incomplete, duplicate, contradictory, or unrelated member refuses
/// the whole set by name: a member is never quietly treated as a disk
/// of its own when the logical capture is all of them together.
pub(crate) fn assemble<M: MemberSource>(
    collection: &str,
    members: &[M],
    cache_bytes: u64,
) -> Result<AssembledCapture> {
    let names: Vec<String> = members
        .iter()
        .map(|member| member.name().to_owned())
        .collect();
    let admitted = admit_capture_set(&names, collection)?;
    let last_step = admitted.last().expect("the set is not empty").step;
    let heads = admitted.iter().filter(|member| member.step == 0).count() as u64;
    let mut evidence = vec![format!(
        "{collection} holds {} KryoFlux stream members, covering step positions \
         0 to {last_step} by {heads} heads",
        admitted.len(),
    )];

    let time_base = TimeBase::new(KRYOFLUX, SAMPLE_CLOCK_NUMERATOR, SAMPLE_CLOCK_DENOMINATOR)?;
    let mut builder = CaptureBuilder::new(time_base, SessionBacking::create()?);

    let mut source_bytes = 0u64;
    for member in &admitted {
        let source = &members[member.index];
        if source.size() > MEMBER_BOUND {
            return Err(refuse(format!(
                "'{}' is {} bytes; a KryoFlux stream member is bounded at \
                 {MEMBER_BOUND} bytes",
                member.name,
                source.size()
            )));
        }
        let bytes = source.bytes()?;
        source_bytes += bytes.len() as u64;
        let facts = decode_stream(&member.name, &bytes)?;
        if let Some(declared) = &facts.declared_sample_clock {
            check_sample_clock(&member.name, declared)?;
        }

        let key = TrackKey::new(KRYOFLUX, member.step, member.head);
        let envelope = builder.envelope_mut();
        let id = envelope.declare_source(SourceDescriptor::new(
            KRYOFLUX,
            member.name.clone(),
            SourceRange::new(0, bytes.len() as u64),
        ));
        for (key, value) in &facts.metadata {
            envelope.record_metadata(MetadataRecord::new(KRYOFLUX, id, key, value));
        }
        for (type_id, range, payload) in &facts.foreign {
            envelope.retain_foreign(ForeignRecord::new(
                id,
                KRYOFLUX,
                type_id,
                *range,
                payload.clone(),
            ));
        }

        builder.add_location(key, id, std::slice::from_ref(&facts.run), facts.issues)?;
    }

    let (mut capture, backing, total_bytes) = builder.seal()?;
    capture.attach_backing(
        Box::new(backing.into_source()),
        total_bytes,
        cache_bytes,
        vec![KRYOFLUX],
    );

    evidence.push(format!(
        "every member decoded once into a {total_bytes}-byte section backing in \
         private session storage, addressed by location"
    ));
    evidence.push(format!(
        "capture timed against a declared {SAMPLE_CLOCK_NUMERATOR}/\
         {SAMPLE_CLOCK_DENOMINATOR} Hz sample clock, checked against each \
         member's own declaration"
    ));

    Ok(AssembledCapture {
        capture,
        evidence,
        source_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// A minimal stream: the device information, some flux, two index
    /// records, the transfer result, and the end marker.
    fn stream(cells: &[u8], indices: &[(u32, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let info = b"sck=24027428.5714285, ick=3003428.5714285625\0";
        out.push(OOB_SIGN);
        out.push(OOB_KF_INFO);
        out.extend_from_slice(&(info.len() as u16).to_le_bytes());
        out.extend_from_slice(info);
        for (position, counter) in indices {
            out.push(OOB_SIGN);
            out.push(OOB_INDEX);
            out.extend_from_slice(&12u16.to_le_bytes());
            out.extend_from_slice(&position.to_le_bytes());
            out.extend_from_slice(&counter.to_le_bytes());
            out.extend_from_slice(&7u32.to_le_bytes());
        }
        out.extend_from_slice(cells);
        out.push(OOB_SIGN);
        out.push(OOB_STREAM_END);
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&(cells.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&[OOB_SIGN, OOB_END_OF_STREAM, OOB_SIGN, OOB_END_OF_STREAM]);
        out
    }

    #[test]
    fn a_member_name_is_the_only_record_of_where_it_was_captured() {
        let parsed = parse_member_name("Some Capture[Machine](1of2)17.1.raw")
            .expect("the tool's own layout parses");
        assert_eq!(parsed.capture, "Some Capture[Machine](1of2)");
        assert_eq!(parsed.step, 17);
        assert_eq!(parsed.head, 1);
    }

    #[test]
    fn a_name_outside_the_admitted_layout_parses_as_nothing() {
        // Widening any of these is later adapter work with evidence
        // behind it, not a looser match here.
        assert!(parse_member_name("track17.1.img").is_none());
        assert!(parse_member_name("capture7.1.raw").is_none());
        assert!(parse_member_name("capture17.raw").is_none());
        assert!(parse_member_name("capture17.10.raw").is_none());
        assert!(parse_member_name("capture17.x.raw").is_none());
    }

    #[test]
    fn an_absent_position_refuses_the_whole_set() {
        // Step 1 head 1 is missing. The set is one disk, so a hole in
        // it is not a member that reads a little short.
        let error = admit_capture_set(
            &names(&["cap00.0.raw", "cap00.1.raw", "cap01.0.raw"]),
            "the declared collection",
        )
        .expect_err("an incomplete set is refused");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(
            error
                .to_string()
                .contains("step position 1 head 1 is absent"),
            "{error}"
        );
    }

    #[test]
    fn a_duplicated_position_refuses_the_set_naming_both_members() {
        let error = admit_capture_set(
            &names(&["cap00.0.raw", "cap00.0.raw", "cap00.1.raw"]),
            "the declared collection",
        )
        .expect_err("a duplicate is refused");
        assert!(error.to_string().contains("captured twice"), "{error}");
    }

    #[test]
    fn a_second_capture_in_the_same_collection_refuses_the_set() {
        let error = admit_capture_set(
            &names(&["one00.0.raw", "two00.0.raw"]),
            "the declared collection",
        )
        .expect_err("two captures are not one set");
        assert!(
            error.to_string().contains("more than one capture set"),
            "{error}"
        );
    }

    #[test]
    fn an_unrelated_member_refuses_the_set_rather_than_being_skipped() {
        // Skipping it would let a set look complete while the declared
        // collection held something nobody accounted for.
        let error = admit_capture_set(
            &names(&["cap00.0.raw", "readme.txt"]),
            "the declared collection",
        )
        .expect_err("an unrelated member is refused");
        assert!(
            error
                .to_string()
                .contains("is not a KryoFlux stream member"),
            "{error}"
        );
    }

    #[test]
    fn members_under_one_directory_prefix_are_one_capture_set() {
        // A collection gathered from an archive's namespace carries the
        // entry names as they are, prefix included; a shared prefix is
        // part of one capture's name rather than a second grammar.
        let members = admit_capture_set(
            &names(&["disk1/cap00.0.raw", "disk1/cap00.1.raw"]),
            "the declared collection",
        )
        .expect("one whole set");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "disk1/cap00.0.raw");
    }

    #[test]
    fn the_stream_grammar_keeps_flux_index_and_transport_apart() {
        // Cells 0x14 and 0x28 are one-byte flux values; the index
        // records name stream positions inside them.
        let bytes = stream(&[0x14, 0x28, 0x1e, 0x32], &[(0, 3), (2, 5)]);
        let facts = decode_stream("cap00.0.raw", &bytes).expect("the stream decodes");

        assert_eq!(facts.run.transitions(), [0x14, 0x3c, 0x5a, 0x8c]);
        assert_eq!(facts.run.markers().len(), 2);
        assert_eq!(facts.transfer_result, Some(0));
        assert!(facts.issues.is_empty(), "{:?}", facts.issues);
        // The device information is retained in the source's own
        // spelling, unparsed and uninterpreted.
        assert_eq!(
            facts.metadata[0],
            ("sck".to_owned(), "24027428.5714285".to_owned())
        );
        assert_eq!(
            facts.declared_sample_clock.as_deref(),
            Some("24027428.5714285")
        );
    }

    #[test]
    fn an_index_sits_where_the_counter_says_inside_the_cell_it_names() {
        // The record names the cell in progress and how far into it the
        // pulse arrived, so the index is that far past the transition
        // the cell began at — not past the one it ends in.
        let bytes = stream(&[0x14, 0x28, 0x1e], &[(1, 5)]);
        let facts = decode_stream("cap00.0.raw", &bytes).expect("the stream decodes");
        // Cell 1 begins at stream byte 1 and the transition before it
        // is at tick 0x14, so the index is at 0x14 + 5.
        assert_eq!(facts.run.markers()[0].position(), 0x14 + 5);
    }

    #[test]
    fn flux_before_the_first_index_and_after_the_last_stays_in_the_run() {
        let bytes = stream(&[0x14, 0x28, 0x1e, 0x32, 0x46], &[(1, 0), (3, 0)]);
        let facts = decode_stream("cap00.0.raw", &bytes).expect("the stream decodes");
        let observations = facts.run.observations(KRYOFLUX).expect("one revolution");

        assert_eq!(facts.run.transitions().len(), 5);
        assert_eq!(observations.len(), 1);
        // The observation holds only what the two indices bracket; the
        // run still holds everything.
        assert!(observations[0].transitions().len() < facts.run.transitions().len());
    }

    #[test]
    fn a_record_this_version_cannot_name_is_retained_rather_than_dropped() {
        let mut bytes = stream(&[0x14, 0x28], &[(0, 0), (1, 0)]);
        // An out-of-band record of a type this build has no home for,
        // spliced in before the end marker.
        let at = bytes.len() - 4;
        let mut record = vec![OOB_SIGN, 0x7f, 0x02, 0x00, 0xab, 0xcd];
        record.extend_from_slice(&bytes[at..]);
        bytes.truncate(at);
        bytes.extend_from_slice(&record);

        let facts = decode_stream("cap00.0.raw", &bytes).expect("the record is kept, not refused");
        let (type_id, _, payload) = facts
            .foreign
            .iter()
            .find(|(type_id, _, _)| type_id == "oob-7f")
            .expect("the record was retained under its own type");
        assert_eq!(type_id, "oob-7f");
        assert_eq!(payload, &[0xab, 0xcd]);
    }

    #[test]
    fn a_declared_transfer_result_that_is_not_clean_is_recorded_not_repaired() {
        let mut bytes = stream(&[0x14, 0x28], &[(0, 0), (1, 0)]);
        // The result field of the stream-end record, which sits four
        // bytes before the end marker.
        let at = bytes.len() - 8;
        bytes[at..at + 4].copy_from_slice(&2u32.to_le_bytes());

        let facts = decode_stream("cap00.0.raw", &bytes).expect("the member still decodes");
        assert_eq!(facts.transfer_result, Some(2));
        assert_eq!(facts.issues[0].code, "kryoflux-transfer-result");
    }

    #[test]
    fn a_stream_ending_inside_a_record_is_refused_by_name() {
        let mut bytes = stream(&[0x14, 0x28], &[(0, 0), (1, 0)]);
        bytes.truncate(bytes.len() - 6);
        let error = decode_stream("cap00.0.raw", &bytes).expect_err("a torn record is refused");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("cap00.0.raw"), "{error}");
    }

    #[test]
    fn an_index_past_the_flux_the_member_holds_is_refused() {
        let bytes = stream(&[0x14, 0x28], &[(0, 0), (900, 0)]);
        let error = decode_stream("cap00.0.raw", &bytes).expect_err("the position is outside");
        assert!(error.to_string().contains("lies past"), "{error}");
    }

    #[test]
    fn the_declared_sample_clock_is_checked_to_its_own_last_digit() {
        // The stream's decimal is a truncation of a rate with no exact
        // decimal, so it agrees to within its own last place and no
        // closer.
        check_sample_clock("cap00.0.raw", "24027428.5714285")
            .expect("truncated, as the tool writes it");
        check_sample_clock("cap00.0.raw", "24027428.5714286")
            .expect("rounded, which is no further out");
        // A hertz out where the stream stated seven decimals is ten
        // million units of its own precision, not a rounding.
        check_sample_clock("cap00.0.raw", "24027429.5714285")
            .expect_err("a hertz out at that precision is another instrument");
        check_sample_clock("cap00.0.raw", "8000000").expect_err("a different clock entirely");
        check_sample_clock("cap00.0.raw", "not-a-rate").expect_err("no rate at all");
    }
}
