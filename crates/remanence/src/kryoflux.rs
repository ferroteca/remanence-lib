// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The KryoFlux capture-set adapter: one logical capture assembled from
//! a catalog subtree, not one image-like member.
//!
//! A KryoFlux capture of a disk is a stream file per head per drive-step
//! position — a hundred and sixty-eight of them for a two-head pass over
//! eighty-four positions — archived together as the artifact the capture
//! produced. This adapter owns what that set is: the member grammar and
//! its completeness, the track and side identity a member's *name* is
//! the only record of, the stream grammar each member is decoded by, the
//! index and control records inside it, the transfer result, and the
//! provenance tying every fact back to the member it was read from.
//!
//! It owns nothing above that. It does not merge the two heads into an
//! ideal disk, choose between passes, average a timing, or materialize a
//! medium, bitstream, sector, or file. What leaves it is a
//! [`crate::flux_capture::FluxCapture`] — evidence, ordered, attributed,
//! and refused by name where the set does not hold together.
//!
//! The archive catalog does the reading and knows none of this: it
//! reports the members and produces their bytes, and the grammar below
//! is the whole of what says which of them are a capture set (P12, P19).
//!
//! Nothing is resident whole (P27). The members decode once, one at a
//! time, into the flux layer's section-addressable backing in private
//! session storage; what stays in memory is identity and shape, and the
//! pulses load a bounded section at a time from there.

use std::path::{Path, PathBuf};

use crate::archive::{ClaimedArchive, EntrySource, normalize_entry_name, open_archive, split_archive_path};
use crate::device::{AccessMode, read_exact_at};
use crate::error::{Error, ErrorCategory, Result};
use crate::evidence::{Issue, Provenance};
use crate::flux_capture::{
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

/// One member the grammar admitted, and where the catalog holds it.
#[derive(Debug, Clone)]
struct AdmittedMember {
    /// The catalog's own identity for this member.
    entry_name: String,
    entry_index: usize,
    entry_bytes: u64,
    step: u64,
    head: u64,
}

/// Reads the catalog subtree as one capture set, or names why it is not
/// one.
///
/// Every refusal states the discovered catalog evidence — the member,
/// the position, the head — because "not a capture set" without it is a
/// verdict with nothing behind it (P4).
fn admit_capture_set(
    entries: &[crate::archive::ArchiveEntry],
    subtree: Option<&str>,
    archive: &Path,
) -> Result<Vec<AdmittedMember>> {
    let mut members: Vec<AdmittedMember> = Vec::new();
    let mut capture: Option<String> = None;

    for (entry_index, entry) in entries.iter().enumerate() {
        if entry.is_dir {
            continue;
        }
        let Some(relative) = within(&entry.name, subtree) else {
            continue;
        };
        let parsed = parse_member_name(relative).ok_or_else(|| {
            refuse(format!(
                "'{}' is not a KryoFlux stream member: this version admits \
                 '<capture><SS>.<H>.raw' and nothing else, so the subtree holds \
                 something that is not part of one capture set",
                entry.name
            ))
        })?;
        match &capture {
            None => capture = Some(parsed.capture.clone()),
            Some(named) if *named != parsed.capture => {
                return Err(refuse(format!(
                    "'{}' names the capture '{}' where an earlier member named \
                     '{named}', so the subtree holds more than one capture set",
                    entry.name, parsed.capture
                )));
            }
            Some(_) => {}
        }
        members.push(AdmittedMember {
            entry_name: entry.name.clone(),
            entry_index,
            entry_bytes: entry.uncompressed_size,
            step: parsed.step,
            head: parsed.head,
        });
    }

    if members.is_empty() {
        return Err(refuse_as(
            ErrorCategory::NotFound,
            format!(
                "'{}' holds no KryoFlux stream members{}",
                archive.display(),
                subtree.map_or(String::new(), |subtree| format!(" under '{subtree}'"))
            ),
        ));
    }

    members.sort_by_key(|member| (member.step, member.head));
    for pair in members.windows(2) {
        if pair[0].step == pair[1].step && pair[0].head == pair[1].head {
            return Err(refuse(format!(
                "step position {} head {} is captured twice, by '{}' and '{}', so \
                 the set states two different things about one location",
                pair[0].step, pair[0].head, pair[0].entry_name, pair[1].entry_name
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

/// The part of `name` inside `subtree`, or `None` when it sits outside.
fn within<'a>(name: &'a str, subtree: Option<&str>) -> Option<&'a str> {
    match subtree {
        None => Some(name),
        Some(subtree) => name
            .strip_prefix(subtree)
            .and_then(|rest| rest.strip_prefix('/')),
    }
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
                let low = *bytes.get(at + 1).ok_or_else(|| {
                    refuse(format!("'{name}' ends inside a two-byte flux value"))
                })?;
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
    let stated: u64 = format!("{whole}{fraction}")
        .parse()
        .map_err(|_| refuse(format!("'{name}' declares a sample clock past what a rate can count")))?;
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

// -------------------------------------------------------------- backing

// --------------------------------------------------------------- report

/// A capture's declared timing basis: an exact count of ticks per
/// second, as a ratio, because the common capture clocks are not exactly
/// representable any other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeBaseReport {
    pub ticks_per_second_numerator: u64,
    pub ticks_per_second_denominator: u64,
}

/// A source's own drive-step position, held exactly. Sources step in
/// fractions, so this is a ratio and never a rounded whole number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepPosition {
    pub numerator: u64,
    pub denominator: u64,
}

/// Something qualified about a member, recorded rather than repaired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureIssue {
    /// The adapter's stable spelling for this kind of issue.
    pub code: String,
    pub detail: String,
}

/// One circular observation bounded out of a capture run, as the set
/// holds it.
///
/// It reports the observation's shape, never its pulses: what a drive or
/// a mastering policy makes of the evidence is a later journey, and the
/// evidence stays behind this surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationReport {
    /// Its place in this location's source-record order — not a rank,
    /// and no claim that it is a good or complete revolution.
    pub ordinal: u64,
    /// The declared circumference, in the capture's own ticks.
    pub span_ticks: u64,
    pub transitions: u64,
    pub markers: u64,
}

/// One source transfer, as the set holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRunReport {
    pub ordinal: u64,
    pub transitions: u64,
    /// The last transition's tick: the extent of what was recorded, not
    /// a circumference. A run states no period.
    pub extent_ticks: u64,
    pub markers: u64,
    pub index_markers: u64,
    /// The result the capture tool declared for this transfer, if it
    /// declared one. Zero is a clean read.
    pub transfer_result: Option<u32>,
    /// Transitions recorded before the first index and after the last:
    /// evidence bounding into circular observations does not consume.
    pub transitions_before_first_index: u64,
    pub transitions_after_last_index: u64,
    pub observations: Vec<ObservationReport>,
}

/// One member of the set, and everything read out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSetMember {
    /// The catalog's own identity for this member.
    pub entry_name: String,
    pub entry_bytes: u64,
    pub position: StepPosition,
    /// The head that captured this position. A capture set records the
    /// disk's sides as its heads; which of them carries a recording is
    /// not this layer's to decide.
    pub head: Option<u64>,
    pub runs: Vec<CaptureRunReport>,
    pub issues: Vec<CaptureIssue>,
}

/// The set as the adapter recognized it: its members, their catalog
/// identities, their positions and heads, and the evidence behind the
/// recognition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSetReport {
    pub format_id: String,
    pub format_name: String,
    pub time_base: TimeBaseReport,
    pub members: Vec<CaptureSetMember>,
    /// How the set was recognized, in human-readable terms (P4).
    pub evidence: Vec<String>,
}

// ------------------------------------------------------------------ set

/// One KryoFlux capture set, opened from a catalog subtree.
///
/// Opening claims the archive (P7) — writes denied to every other
/// process — decodes every member of the set once into private session
/// storage, and holds the claim until the set is dropped. An incomplete,
/// duplicate, contradictory, or unrelated member refuses the whole set
/// by name: a member is never quietly treated as a disk of its own when
/// the logical capture is all of them together.
pub struct CaptureSet {
    path: PathBuf,
    subtree: Option<String>,
    claimed: ClaimedArchive,
    capture: FluxCapture,
    report: CaptureSetReport,
}

impl std::fmt::Debug for CaptureSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureSet")
            .field("path", &self.path)
            .field("members", &self.report.members.len())
            .field("backing_bytes", &self.capture.backing_bytes())
            .finish()
    }
}

impl CaptureSet {
    /// Opens the capture set held by `path` — an archive this library
    /// reads, optionally followed by the subtree inside it that holds
    /// the members — with the stated default session cache bound
    /// ([`crate::DEFAULT_CACHE_BYTES`]).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_cache(path, crate::DEFAULT_CACHE_BYTES)
    }

    /// Opens the capture set under a caller-declared cache bound (P27):
    /// at most `cache_bytes` of the decoded capture stays resident. The
    /// bound narrows the working set; it never refuses service.
    pub fn open_with_cache(path: impl AsRef<Path>, cache_bytes: u64) -> Result<Self> {
        let path = path.as_ref();
        let (archive_path, entry_path) = split_archive_path(path).ok_or_else(|| {
            Error::unsupported(format!(
                "'{}' names no archive this library reads; a KryoFlux capture set \
                 is one disk spread over a stream per head per step position, and \
                 is read from a catalog rather than from a single file",
                path.display()
            ))
        })?;
        let subtree = entry_path.as_deref().map(normalize_entry_name);
        let claimed = open_archive(&archive_path)?;

        let members = admit_capture_set(
            claimed.catalog.entries(),
            subtree.as_deref(),
            &archive_path,
        )?;
        let mut evidence = vec![format!(
            "'{}' holds {} KryoFlux stream members{}, covering step positions 0 to \
             {} by {} heads",
            archive_path.display(),
            members.len(),
            subtree
                .as_deref()
                .map_or(String::new(), |subtree| format!(" under '{subtree}'")),
            members.last().expect("the set is not empty").step,
            members.iter().filter(|member| member.step == 0).count(),
        )];

        let indices: Vec<usize> = members.iter().map(|member| member.entry_index).collect();
        let sources = claimed.catalog.entry_group(&indices)?;

        let time_base = TimeBase::new(KRYOFLUX, SAMPLE_CLOCK_NUMERATOR, SAMPLE_CLOCK_DENOMINATOR)?;
        let mut builder = CaptureBuilder::new(time_base, SessionBacking::create()?);

        let mut reported = Vec::with_capacity(members.len());
        for (member, source) in members.iter().zip(sources.iter()) {
            let bytes = member_bytes(&claimed, source, &member.entry_name)?;
            let facts = decode_stream(&member.entry_name, &bytes)?;
            if let Some(declared) = &facts.declared_sample_clock {
                check_sample_clock(&member.entry_name, declared)?;
            }

            let key = TrackKey::new(KRYOFLUX, member.step, member.head);
            let envelope = builder.envelope_mut();
            let id = envelope.declare_source(SourceDescriptor::new(
                KRYOFLUX,
                member.entry_name.clone(),
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

            let transfer_result = facts.transfer_result;
            builder.add_location(
                key.clone(),
                id,
                std::slice::from_ref(&facts.run),
                facts.issues,
            )?;
            reported.push((member.clone(), key, transfer_result));
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

        let report = CaptureSetReport {
            format_id: KRYOFLUX.to_owned(),
            format_name: "KryoFlux capture set".to_owned(),
            time_base: TimeBaseReport {
                ticks_per_second_numerator: SAMPLE_CLOCK_NUMERATOR,
                ticks_per_second_denominator: SAMPLE_CLOCK_DENOMINATOR,
            },
            members: reported
                .into_iter()
                .map(|(member, key, transfer_result)| {
                    report_member(&capture, &member, &key, transfer_result)
                })
                .collect(),
            evidence,
        };

        Ok(Self {
            path: path.to_path_buf(),
            subtree,
            claimed,
            capture,
            report,
        })
    }

    /// The path the set was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The subtree inside the archive the members were read from, when
    /// one was named.
    pub fn subtree(&self) -> Option<&str> {
        self.subtree.as_deref()
    }

    /// The capture format's stable identifier.
    pub fn format_id(&self) -> &'static str {
        KRYOFLUX
    }

    /// The capture format's human-readable name.
    pub fn format_name(&self) -> &'static str {
        "KryoFlux capture set"
    }

    /// The archive grammar the members were read through.
    pub fn archive_format_id(&self) -> &'static str {
        self.claimed.catalog.descriptor().id
    }

    /// Which P7 mode the open obtained on the archive file.
    pub fn access_mode(&self) -> AccessMode {
        self.claimed.mode
    }

    /// How many bytes of private session storage the decoded capture
    /// occupies.
    pub fn backing_bytes(&self) -> u64 {
        self.capture.backing_bytes()
    }

    /// How much of that backing is currently resident. The capture is
    /// never held whole (P27); this is what says so.
    pub fn resident_bytes(&self) -> u64 {
        self.capture.resident_bytes()
    }

    /// The set as the adapter recognized it.
    pub fn inspect(&self) -> &CaptureSetReport {
        &self.report
    }

    /// Recognizes the drive family this capture belongs to (P30).
    ///
    /// Every enrolled profile is consulted and what claims the capture
    /// is ranked, never resolved by catalog order; a capture no profile
    /// claims is a named refusal, and a lone enrolled profile never wins
    /// by being the only one. The verdict carries the observations that
    /// produced its confidence, because a confidence figure on its own
    /// is not an answer (P4).
    pub fn recognize(&self) -> Result<crate::Recognition> {
        crate::drive_profile::recognition(&self.capture, None)
    }

    /// Recognizes the capture against one named profile, whether or not
    /// it would have won the ranking. What the caller pinned travels
    /// into the result.
    pub fn recognize_as(&self, profile_id: &str) -> Result<crate::Recognition> {
        crate::drive_profile::recognition(&self.capture, Some(profile_id))
    }

    /// Plans the reduction of this capture to one 1541 flux medium
    /// under a declared policy (P29).
    ///
    /// Nothing is written and nothing is mutated: the plan computes the
    /// whole transformation, reports the medium it will produce, and
    /// enumerates everything the destination will not carry in the
    /// source own terms. A reduction the policy does not name is a
    /// refusal rather than a default, so the plan either accounts for
    /// the whole capture or does not exist.
    pub fn plan_c1541_mastering(
        &self,
        policy: crate::MasteringPolicy,
    ) -> Result<crate::MasteringPlan> {
        crate::c1541_mastering::plan(&self.capture, policy)
    }
}

fn report_member(
    capture: &FluxCapture,
    member: &AdmittedMember,
    key: &TrackKey,
    transfer_result: Option<u32>,
) -> CaptureSetMember {
    let track = capture.track(key).expect("every admitted member was added");
    let runs = track
        .runs()
        .iter()
        .map(|run| CaptureRunReport {
            ordinal: run.ordinal(),
            transitions: run.transitions(),
            extent_ticks: run.extent(),
            markers: run.markers(),
            index_markers: run.indices(),
            transfer_result,
            transitions_before_first_index: run.before_first_index(),
            transitions_after_last_index: run.after_last_index(),
            observations: track
                .observations()
                .iter()
                .filter(|observation| observation.source().run_ordinal() == run.ordinal())
                .map(|observation| ObservationReport {
                    ordinal: observation.ordinal(),
                    span_ticks: observation.span(),
                    transitions: observation.transitions(),
                    markers: observation.markers(),
                })
                .collect(),
        })
        .collect();
    CaptureSetMember {
        entry_name: member.entry_name.clone(),
        entry_bytes: member.entry_bytes,
        position: StepPosition {
            numerator: member.step,
            denominator: 1,
        },
        head: key.head(),
        runs,
        issues: track
            .issues()
            .iter()
            .map(|issue| CaptureIssue {
                code: issue.code.to_owned(),
                detail: issue.detail.clone(),
            })
            .collect(),
    }
}

/// Reads one member's bytes, refusing one past the bound before
/// anything is allocated for it.
fn member_bytes(claimed: &ClaimedArchive, source: &EntrySource, name: &str) -> Result<Vec<u8>> {
    let (file, offset, length) = match source {
        EntrySource::InPlace { offset, length } => (&claimed.file, *offset, *length),
        EntrySource::Spooled {
            spool,
            offset,
            length,
        } => (spool, *offset, *length),
    };
    if length > MEMBER_BOUND {
        return Err(refuse(format!(
            "'{name}' is {length} bytes; a KryoFlux stream member is bounded at \
             {MEMBER_BOUND} bytes"
        )));
    }
    let mut bytes = vec![0u8; length as usize];
    read_exact_at(file, offset, &mut bytes)
        .map_err(|error| Error::io(format!("failed to read '{name}': {error}")))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::ArchiveEntry;

    fn entry(name: &str) -> ArchiveEntry {
        ArchiveEntry {
            name: name.to_owned(),
            is_dir: false,
            compressed_size: None,
            uncompressed_size: 16,
        }
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
        let entries = vec![
            entry("cap00.0.raw"),
            entry("cap00.1.raw"),
            entry("cap01.0.raw"),
        ];
        let error = admit_capture_set(&entries, None, Path::new("c.7z"))
            .expect_err("an incomplete set is refused");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(
            error.to_string().contains("step position 1 head 1 is absent"),
            "{error}"
        );
    }

    #[test]
    fn a_duplicated_position_refuses_the_set_naming_both_members() {
        let entries = vec![
            entry("cap00.0.raw"),
            entry("cap00.0.raw"),
            entry("cap00.1.raw"),
        ];
        let error = admit_capture_set(&entries, None, Path::new("c.7z"))
            .expect_err("a duplicate is refused");
        assert!(error.to_string().contains("captured twice"), "{error}");
    }

    #[test]
    fn a_second_capture_in_the_same_subtree_refuses_the_set() {
        let entries = vec![entry("one00.0.raw"), entry("two00.0.raw")];
        let error = admit_capture_set(&entries, None, Path::new("c.7z"))
            .expect_err("two captures are not one set");
        assert!(
            error.to_string().contains("more than one capture set"),
            "{error}"
        );
    }

    #[test]
    fn an_unrelated_member_refuses_the_set_rather_than_being_skipped() {
        // Skipping it would let a set look complete while the subtree
        // held something nobody accounted for.
        let entries = vec![entry("cap00.0.raw"), entry("readme.txt")];
        let error = admit_capture_set(&entries, None, Path::new("c.7z"))
            .expect_err("an unrelated member is refused");
        assert!(
            error.to_string().contains("is not a KryoFlux stream member"),
            "{error}"
        );
    }

    #[test]
    fn a_subtree_scopes_the_set_to_what_sits_under_it() {
        let entries = vec![
            entry("notes.txt"),
            entry("disk1/cap00.0.raw"),
            entry("disk1/cap00.1.raw"),
        ];
        let members = admit_capture_set(&entries, Some("disk1"), Path::new("c.7z"))
            .expect("the subtree holds one whole set");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].entry_name, "disk1/cap00.0.raw");
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
        assert_eq!(facts.metadata[0], ("sck".to_owned(), "24027428.5714285".to_owned()));
        assert_eq!(facts.declared_sample_clock.as_deref(), Some("24027428.5714285"));
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
        assert!(
            error.to_string().contains("lies past"),
            "{error}"
        );
    }

    #[test]
    fn the_declared_sample_clock_is_checked_to_its_own_last_digit() {
        // The stream's decimal is a truncation of a rate with no exact
        // decimal, so it agrees to within its own last place and no
        // closer.
        check_sample_clock("cap00.0.raw", "24027428.5714285").expect("truncated, as the tool writes it");
        check_sample_clock("cap00.0.raw", "24027428.5714286").expect("rounded, which is no further out");
        // A hertz out where the stream stated seven decimals is ten
        // million units of its own precision, not a rounding.
        check_sample_clock("cap00.0.raw", "24027429.5714285")
            .expect_err("a hertz out at that precision is another instrument");
        check_sample_clock("cap00.0.raw", "8000000").expect_err("a different clock entirely");
        check_sample_clock("cap00.0.raw", "not-a-rate").expect_err("no rate at all");
    }
}
