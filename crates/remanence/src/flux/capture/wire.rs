// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The byte grammar the backing's records are written in.
//!
//! Varints and length-prefixed text underneath, and above them the two
//! chunk forms a location's evidence is split into: transitions
//! delta-coded against their predecessor, and markers kept at absolute
//! positions in recorded order. Every decode refuses rather than
//! partly believing a chunk — a sequence that does not ascend, a
//! length that overruns, a marker outside the span it claims.
//!
//! Splitting is deterministic, so the same evidence always lands on the
//! same chunk boundaries and a section key means one thing forever.

use crate::error::{Error, Result};
use crate::evidence::Provenance;

use super::records::{Marker, MarkerKind, Tick};

/// The most bytes a `u64` varint can occupy: ten groups of seven bits.
const MAX_VARINT_BYTES: usize = 10;

/// Appends one unsigned value, seven bits per byte, low group first,
/// with the high bit set on every byte but the last.
pub(crate) fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Reads one unsigned value, returning it and the bytes it used.
///
/// Refuses a value running off the end of the record and one encoded in
/// more groups than a `u64` holds, either of which would otherwise be
/// read as a plausible tick.
pub(crate) fn read_varint(source: &str, bytes: &[u8], at: usize) -> Result<(u64, usize)> {
    let mut value: u64 = 0;
    let mut used = 0;
    loop {
        let byte = *bytes.get(at + used).ok_or_else(|| {
            Error::invalid_image(
                source,
                format!(
                    "section ends mid-value at byte {}, so the chunk states \
                     fewer ticks than it encodes",
                    at + used
                ),
            )
        })?;
        if used == MAX_VARINT_BYTES {
            return Err(Error::invalid_image(
                source,
                format!(
                    "section encodes a value in more than {MAX_VARINT_BYTES} \
                     bytes at byte {at}, which no tick occupies"
                ),
            ));
        }
        // The last group has room for fewer than seven bits, and the
        // shift alone will not say so: shifting them out is in range
        // and would leave an ordinary-looking tick behind.
        let group = u64::from(byte & 0x7f);
        let shift = 7 * used as u32;
        if shift >= u64::BITS || group > (u64::MAX >> shift) {
            return Err(Error::invalid_image(
                source,
                format!("section encodes a value wider than a tick at byte {at}"),
            ));
        }
        value |= group << shift;
        used += 1;
        if byte & 0x80 == 0 {
            return Ok((value, used));
        }
    }
}

/// Delta-codes an observation's transitions into one chunk's payload.
///
/// The positions ascend, so every gap is unsigned and the first is
/// measured from the observation's own origin.
pub(super) fn encode_transitions(transitions: &[Tick]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut previous: Tick = 0;
    for (ordinal, &position) in transitions.iter().enumerate() {
        let delta = position.checked_sub(previous).ok_or_else(|| {
            Error::invalid_image(
                "flux-capture",
                format!(
                    "transition {ordinal} at tick {position} does not advance past \
                     the preceding tick {previous}, so it has no unsigned gap"
                ),
            )
        })?;
        write_varint(&mut out, delta);
        previous = position;
    }
    Ok(out)
}

/// Reads a chunk's payload back into the ticks it was coded from.
pub(super) fn decode_transitions(bytes: &[u8]) -> Result<Vec<Tick>> {
    let source = "flux-capture";
    let mut transitions = Vec::new();
    let mut position: Tick = 0;
    let mut at = 0;
    while at < bytes.len() {
        let (delta, used) = read_varint(source, bytes, at)?;
        at += used;
        position = position.checked_add(delta).ok_or_else(|| {
            Error::invalid_image(
                source,
                "section accumulates past the largest tick, so its deltas do \
                 not describe a run of transitions",
            )
        })?;
        transitions.push(position);
    }
    Ok(transitions)
}

/// One addressable run of an observation's transitions.
///
/// It carries the ticks it covers and how many it holds, so a reader
/// deciding from the index can skip a chunk without decoding it, and
/// its payload decodes on its own: the first value is an absolute tick
/// and only the rest are gaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransitionChunk {
    ordinal: u64,
    first: Tick,
    last: Tick,
    count: u64,
    payload: Vec<u8>,
}

impl TransitionChunk {
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// The first and last tick this chunk holds, inclusive.
    pub(crate) fn bounds(&self) -> (Tick, Tick) {
        (self.first, self.last)
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Splits an observation's transitions into chunks of at most
/// `records` each.
///
/// The boundary is a record count rather than a byte count or a tick
/// span, so the same transitions always split the same way — the
/// backing may be rebuilt and still address what an index says it does.
pub(super) fn split_transitions(
    transitions: &[Tick],
    records: usize,
) -> Result<Vec<TransitionChunk>> {
    if records == 0 {
        return Err(Error::invalid_image(
            "flux-capture",
            "a transition chunk of zero records states no boundary to split on",
        ));
    }
    transitions
        .chunks(records)
        .enumerate()
        .map(|(ordinal, run)| {
            Ok(TransitionChunk {
                ordinal: ordinal as u64,
                first: *run.first().expect("chunks are never empty"),
                last: *run.last().expect("chunks are never empty"),
                count: run.len() as u64,
                payload: encode_transitions(run)?,
            })
        })
        .collect()
}

/// Writes a marker channel's records into one chunk's payload.
///
/// Positions are absolute. A marker channel is not a run of ascending
/// ticks — two markers may share a position, and a later one may sit
/// earlier on the circle — so there is no unsigned gap to code against,
/// and the recorded sequence is what is written.
pub(super) fn encode_markers(markers: &[Marker]) -> Vec<u8> {
    let mut out = Vec::new();
    for marker in markers {
        write_varint(&mut out, marker.position);
        match &marker.kind {
            MarkerKind::Index => write_varint(&mut out, 0),
            MarkerKind::HardSector => write_varint(&mut out, 1),
            MarkerKind::WriteSplice => write_varint(&mut out, 2),
            MarkerKind::SourceMarker { namespace, code } => {
                write_varint(&mut out, 3);
                write_varint(&mut out, namespace.len() as u64);
                out.extend_from_slice(namespace.as_bytes());
                write_varint(&mut out, u64::from(*code));
            }
        }
        write_varint(&mut out, marker.payload.len() as u64);
        out.extend_from_slice(&marker.payload);
        write_text(&mut out, marker.provenance.source);
        write_varint(&mut out, marker.provenance.notes.len() as u64);
        for note in &marker.provenance.notes {
            write_text(&mut out, note);
        }
    }
    out
}

/// Writes one length-prefixed UTF-8 string.
pub(crate) fn write_text(out: &mut Vec<u8>, text: &str) {
    write_varint(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// Reads a marker chunk's payload back into the records it was coded
/// from, in the order they were recorded.
pub(super) fn decode_markers(
    source: &str,
    bytes: &[u8],
    known: &[&'static str],
) -> Result<Vec<Marker>> {
    let mut markers = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let (position, used) = read_varint(source, bytes, at)?;
        at += used;
        let (tag, used) = read_varint(source, bytes, at)?;
        at += used;
        let kind = match tag {
            0 => MarkerKind::Index,
            1 => MarkerKind::HardSector,
            2 => MarkerKind::WriteSplice,
            3 => {
                let (namespace, used) = read_text(source, bytes, at)?;
                at += used;
                let (code, used) = read_varint(source, bytes, at)?;
                at += used;
                let code = u32::try_from(code).map_err(|_| {
                    Error::invalid_image(
                        source,
                        "marker states a source code wider than the channel carries",
                    )
                })?;
                MarkerKind::SourceMarker { namespace, code }
            }
            other => {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "marker states a kind {other}, which this version has no \
                         reading of"
                    ),
                ));
            }
        };
        let (length, used) = read_varint(source, bytes, at)?;
        at += used;
        let length = usize::try_from(length).map_err(|_| {
            Error::invalid_image(
                source,
                "marker states a payload longer than this host can address",
            )
        })?;
        let payload = bytes
            .get(at..at + length)
            .ok_or_else(|| {
                Error::invalid_image(
                    source,
                    "marker chunk ends inside the payload it states, so it holds \
                     fewer records than it claims",
                )
            })?
            .to_vec();
        at += length;
        let (spelling, used) = read_text(source, bytes, at)?;
        at += used;
        let namespace = known
            .iter()
            .find(|candidate| **candidate == spelling)
            .copied()
            .ok_or_else(|| {
                Error::invalid_image(
                    source,
                    format!(
                        "marker states the namespace {spelling:?}, which this reader \
                         cannot place"
                    ),
                )
            })?;
        let (notes, used) = read_varint(source, bytes, at)?;
        at += used;
        let mut provenance = Provenance::new(namespace);
        for _ in 0..notes {
            let (note, used) = read_text(source, bytes, at)?;
            at += used;
            provenance = provenance.note(note);
        }
        markers.push(Marker {
            position,
            kind,
            payload,
            provenance,
        });
    }
    Ok(markers)
}

/// Reads one length-prefixed UTF-8 string, and the bytes it used.
pub(crate) fn read_text(source: &str, bytes: &[u8], at: usize) -> Result<(String, usize)> {
    let (length, used) = read_varint(source, bytes, at)?;
    let length = usize::try_from(length).map_err(|_| {
        Error::invalid_image(
            source,
            "record states text longer than this host can address",
        )
    })?;
    let raw = bytes
        .get(at + used..at + used + length)
        .ok_or_else(|| Error::invalid_image(source, "record ends inside the text it states"))?;
    let text = std::str::from_utf8(raw)
        .map_err(|_| Error::invalid_image(source, "record states text that is not text"))?;
    Ok((text.to_owned(), used + length))
}

/// One addressable run of a marker channel.
///
/// Its bounds are the lowest and highest position it holds, not its
/// first and last: markers are not ordered by position, so the ends
/// would let a reader skipping by tick range skip a chunk covering the
/// tick it wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkerChunk {
    ordinal: u64,
    lowest: Tick,
    highest: Tick,
    count: u64,
    payload: Vec<u8>,
}

impl MarkerChunk {
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// The lowest and highest tick this chunk holds, inclusive.
    pub(crate) fn bounds(&self) -> (Tick, Tick) {
        (self.lowest, self.highest)
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Splits a marker channel into chunks of at most `records` each.
///
/// Infallible where the transition split is not: any sequence of
/// markers is a valid one, there being no ordering for it to violate.
pub(super) fn split_markers(markers: &[Marker], records: usize) -> Vec<MarkerChunk> {
    markers
        .chunks(records.max(1))
        .enumerate()
        .map(|(ordinal, run)| MarkerChunk {
            ordinal: ordinal as u64,
            lowest: run.iter().map(Marker::position).min().unwrap_or(0),
            highest: run.iter().map(Marker::position).max().unwrap_or(0),
            count: run.len() as u64,
            payload: encode_markers(run),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;
    use crate::evidence::Provenance;

    /// The namespaces a decoding test lets the reader place.
    const KNOWN: &[&str] = &["kryoflux", "c1541", "a2r", "scp"];

    fn index_at(position: Tick) -> Marker {
        Marker::new(
            position,
            MarkerKind::Index,
            Provenance::new("kryoflux").note("index OOB record"),
        )
    }

    #[test]
    fn a_transition_chunk_delta_codes_and_decodes_back_to_the_exact_ticks() {
        // Flux is what the layer exists to carry, so this coding is
        // exact or it is worthless: one delta misread shifts every
        // transition after it, and nothing downstream could tell.
        //
        // The boundary rows are the point. A varint goes wrong at its
        // byte boundaries, so 127/128 and 16383/16384 are pinned, and
        // the table takes a new row whenever another input turns up.
        for original in [
            vec![],
            vec![0],
            vec![0, 1],
            vec![10, 400, 790],
            vec![127, 128, 16_383, 16_384],
            vec![0, u64::MAX],
            vec![u64::MAX],
        ] {
            let encoded = encode_transitions(&original).expect("the ticks ascend");
            assert_eq!(
                decode_transitions(&encoded).expect("what was encoded decodes"),
                original,
                "round trip for {original:?}"
            );
        }
    }

    #[test]
    fn transitions_that_do_not_ascend_are_refused_rather_than_wrapped() {
        // The delta is unsigned, so a descending pair has no encoding.
        // Refusing beats the wrap that would otherwise write a gap of
        // eighteen quintillion ticks and read back as valid flux.
        let error = encode_transitions(&[400, 10]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_chunk_that_does_not_decode_is_refused_rather_than_partly_believed() {
        // P6: a section yields the flux it claims or it yields nothing.
        // Handing back the transitions decoded before the damage would
        // be the worst outcome available — a short revolution that
        // reads as a real one.
        let intact = encode_transitions(&[10, 400, 790]).expect("the ticks ascend");

        // Truncated mid-value: a multi-byte delta loses its last byte.
        let truncated = &intact[..intact.len() - 1];
        assert_eq!(
            decode_transitions(truncated).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );

        // More groups than a tick occupies.
        let overlong = vec![0x80; MAX_VARINT_BYTES + 1];
        assert_eq!(
            decode_transitions(&overlong).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );

        // Deltas accumulating past the largest tick.
        let mut accumulating = Vec::new();
        write_varint(&mut accumulating, u64::MAX);
        write_varint(&mut accumulating, 1);
        assert_eq!(
            decode_transitions(&accumulating).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );

        // A full-width value whose last group carries bits no tick has
        // room for. The shift is in range, so only the value itself
        // says this is malformed — and silently dropping the overflow
        // would turn it into an ordinary-looking tick.
        let mut overflowing = vec![0x80; MAX_VARINT_BYTES - 1];
        overflowing.push(0x7f);
        assert_eq!(
            decode_transitions(&overflowing).unwrap_err().category(),
            ErrorCategory::InvalidImage
        );
    }

    #[test]
    fn a_long_transition_sequence_splits_into_independently_decodable_chunks() {
        // A real revolution is tens of thousands of transitions — the
        // fixture's outermost track carries 33,396 — so chunking is
        // what lets a reader take a span without the whole revolution.
        //
        // The split is by record count, so it is reproducible; each
        // chunk states the ticks it covers, so a reader can skip it
        // from the index without decoding it; and each decodes alone,
        // which is the property the whole backing rests on.
        let transitions: Vec<Tick> = (0..2500).map(|n| n * 3).collect();

        let chunks = split_transitions(&transitions, 1000).expect("the ticks ascend");

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks
                .iter()
                .map(TransitionChunk::ordinal)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(chunks[0].bounds(), (0, 2997));
        assert_eq!(chunks[1].bounds(), (3000, 5997));
        assert_eq!(chunks[2].bounds(), (6000, 7497));
        assert_eq!(chunks[2].count(), 500);

        // Each chunk decodes to its own absolute ticks with no
        // neighbour present — a reader that wants the middle of a
        // revolution decodes one chunk, not the ones before it.
        assert_eq!(
            decode_transitions(chunks[1].payload()).unwrap(),
            transitions[1000..2000]
        );

        // And the chunks together are the original, in order.
        let rejoined: Vec<Tick> = chunks
            .iter()
            .flat_map(|chunk| decode_transitions(chunk.payload()).unwrap())
            .collect();
        assert_eq!(rejoined, transitions);
    }

    #[test]
    fn splitting_into_chunks_of_no_records_is_refused() {
        let error = split_transitions(&[10, 20], 0).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_marker_chunk_keeps_absolute_positions_and_the_recorded_order() {
        // Markers are parallel timed evidence, not transitions. Two may
        // share a position and a later one may sit earlier on the
        // circle, so there is no unsigned gap to delta-code and no
        // order to restore by sorting: positions stay absolute and the
        // sequence is kept exactly as recorded.
        let markers = vec![
            index_at(700),
            Marker::new(700, MarkerKind::WriteSplice, Provenance::new("a2r")),
            index_at(100),
            Marker::new(
                42,
                MarkerKind::SourceMarker {
                    namespace: "kryoflux".into(),
                    code: 0x0b,
                },
                Provenance::new("kryoflux"),
            )
            .with_payload(vec![1, 2, 3]),
            Marker::new(0, MarkerKind::HardSector, Provenance::new("kryoflux")),
        ];

        let encoded = encode_markers(&markers);
        let decoded =
            decode_markers("kryoflux", &encoded, KNOWN).expect("what was encoded decodes");

        assert_eq!(decoded, markers);
    }

    #[test]
    fn a_marker_chunk_states_the_span_it_covers_not_its_ends() {
        // A transition chunk's bounds are its first and last tick,
        // which works only because transitions ascend. Markers do not,
        // so a chunk states the lowest and highest it holds — take the
        // ends instead and a reader skipping by tick range would skip
        // a chunk that covers the tick it wanted.
        let markers = vec![index_at(700), index_at(100), index_at(400)];

        let chunks = split_markers(&markers, 8);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].bounds(), (100, 700));
        assert_eq!(chunks[0].count(), 3);
        assert_eq!(
            decode_markers("kryoflux", chunks[0].payload(), KNOWN).unwrap(),
            markers
        );
    }

    #[test]
    fn markers_split_at_deterministic_boundaries_preserving_order() {
        let markers: Vec<Marker> = (0..5).map(|n| index_at(100 - n * 10)).collect();

        let chunks = split_markers(&markers, 2);

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks.iter().map(MarkerChunk::ordinal).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        let rejoined: Vec<Marker> = chunks
            .iter()
            .flat_map(|chunk| decode_markers("kryoflux", chunk.payload(), KNOWN).unwrap())
            .collect();
        assert_eq!(rejoined, markers);
    }

    #[test]
    fn a_marker_chunk_that_does_not_decode_is_refused() {
        // Same rule as flux: a chunk yields the markers it claims or
        // it yields nothing.
        let intact = encode_markers(&[Marker::new(
            42,
            MarkerKind::SourceMarker {
                namespace: "kryoflux".into(),
                code: 0x0b,
            },
            Provenance::new("kryoflux"),
        )
        .with_payload(vec![1, 2, 3])]);

        // Truncated before the payload it promised.
        assert_eq!(
            decode_markers("kryoflux", &intact[..intact.len() - 2], KNOWN)
                .unwrap_err()
                .category(),
            ErrorCategory::InvalidImage
        );

        // A kind this version has no reading of.
        let mut unknown = Vec::new();
        write_varint(&mut unknown, 10);
        write_varint(&mut unknown, 99);
        assert_eq!(
            decode_markers("kryoflux", &unknown, KNOWN)
                .unwrap_err()
                .category(),
            ErrorCategory::InvalidImage
        );
    }
}
