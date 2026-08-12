// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The private encoded-bytestream model (P23): the circular,
//! track-relative byte sequence a declared family codec materializes
//! from a hardware bitstream.
//!
//! It is the layer above the bitstream and below CHS. A bitstream holds
//! clocked bits and no opinion about where a byte begins; a bytestream
//! holds the bytes one declared group code resolved out of them, in the
//! family's own addressing.
//!
//! **It assigns nothing.** No byte here is a header, a data field, a
//! sector, a file, or the first of any of them. The codec locates the
//! family's alignment landmark because byte framing has to begin
//! somewhere and the family declares where — and having located one, it
//! records the angle and says nothing whatever about what follows it.
//! A landmark that introduced something would be a sector interpretation
//! wearing a byte's clothes.
//!
//! **A pattern the code does not assign is not a byte.** The group code
//! is a declared table of the family's, so a symbol outside it resolves
//! to nothing; what the bytestream then holds is the bits themselves,
//! stated as unresolved, and never the nearest entry in the table.
//!
//! The backing is the flux-capture layer's, keyed by the family's own
//! addressing (P27). Every item is crate-private: the presentation above
//! the bitstream builds one of these, and nothing outside the crate sees
//! a byte.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::evidence::Provenance;
use crate::flux::capture::{
    ByteSink, ByteSource, CHUNK_RECORDS, LEAF_ENTRIES, SectionAddress, SectionCache, SectionWriter,
    decode_provenance, encode_provenance, read_varint, write_varint,
};
use crate::flux::medium::{LocationKey, read_location_key, write_location_key};

/// What the codec made of one framed group of bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ByteOutcome {
    /// The group's symbols are all ones the family's table assigns, and
    /// this is the value they record.
    Resolved(u8),
    /// At least one is not. The group's own bits are kept, because what
    /// the medium holds there is a fact and the nearest table entry
    /// would be a guess.
    Unresolved { bits: u64 },
}

/// One byte-shaped position in the circle: where its first bit sits, and
/// what the codec made of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ByteRecord {
    at_bit: u64,
    outcome: ByteOutcome,
}

impl ByteRecord {
    pub(crate) fn new(at_bit: u64, outcome: ByteOutcome) -> Self {
        Self { at_bit, outcome }
    }

    /// The bit of the location's own bitstream this group opens at.
    pub(crate) fn at_bit(&self) -> u64 {
        self.at_bit
    }

    pub(crate) fn outcome(&self) -> ByteOutcome {
        self.outcome
    }

    pub(crate) fn value(&self) -> Option<u8> {
        match self.outcome {
            ByteOutcome::Resolved(value) => Some(value),
            ByteOutcome::Unresolved { .. } => None,
        }
    }
}

/// What one bytestream-level fact says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BytestreamFactKind {
    /// A run of the family's alignment landmark, and where byte framing
    /// resumed after it. It states where framing begins and nothing at
    /// all about what the bits after it mean.
    Alignment { at_bit: u64, run_bits: u64 },
    /// Bits no framed group covers — before the first landmark, or left
    /// over where the next one arrived mid-group. Stated rather than
    /// padded into a byte.
    Unframed { at_bit: u64, bits: u64 },
}

/// One fact about a location, with how it came to be known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BytestreamFact {
    kind: BytestreamFactKind,
    provenance: Provenance,
}

impl BytestreamFact {
    pub(crate) fn new(kind: BytestreamFactKind, provenance: Provenance) -> Self {
        Self { kind, provenance }
    }

    pub(crate) fn kind(&self) -> &BytestreamFactKind {
        &self.kind
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What one section of a bytestream's backing holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BytestreamSectionKind {
    LocationMetadata,
    ByteChunk,
    FactChunk,
}

/// The complete address of one section of a bytestream's backing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BytestreamSectionKey {
    location: LocationKey,
    kind: BytestreamSectionKind,
    ordinal: u64,
}

impl BytestreamSectionKey {
    pub(crate) fn new(location: LocationKey, kind: BytestreamSectionKind, ordinal: u64) -> Self {
        Self {
            location,
            kind,
            ordinal,
        }
    }
}

impl SectionAddress for BytestreamSectionKey {
    fn namespace(&self) -> &'static str {
        self.location.profile()
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_location_key(out, &self.location);
        write_varint(
            out,
            match self.kind {
                BytestreamSectionKind::LocationMetadata => 0,
                BytestreamSectionKind::ByteChunk => 1,
                BytestreamSectionKind::FactChunk => 2,
            },
        );
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
        let (kind, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        let kind = match kind {
            0 => BytestreamSectionKind::LocationMetadata,
            1 => BytestreamSectionKind::ByteChunk,
            2 => BytestreamSectionKind::FactChunk,
            other => {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "index states a section kind {other}, which this version has no \
                         reading of"
                    ),
                ));
            }
        };
        let (ordinal, used) = read_varint(source, bytes, cursor)?;
        cursor += used;
        Ok((
            Self {
                location,
                kind,
                ordinal,
            },
            cursor - at,
        ))
    }
}

/// What a bytestream keeps resident for one location: its identity and
/// its shape, never its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Location {
    key: LocationKey,
    bytes: u64,
    resolved: u64,
    unresolved: u64,
    alignments: u64,
    unframed_bits: u64,
    byte_chunks: u64,
    fact_chunks: u64,
    provenance: Provenance,
}

impl Location {
    pub(crate) fn key(&self) -> &LocationKey {
        &self.key
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    pub(crate) fn resolved(&self) -> u64 {
        self.resolved
    }

    /// How many groups held a pattern the family's table does not
    /// assign.
    pub(crate) fn unresolved(&self) -> u64 {
        self.unresolved
    }

    /// How many times byte framing was established on a landmark.
    pub(crate) fn alignments(&self) -> u64 {
        self.alignments
    }

    pub(crate) fn unframed_bits(&self) -> u64 {
        self.unframed_bits
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Where a bytestream's sections are read back from, and the bounded
/// working set they are served through (P27).
struct BytestreamBacking {
    bytes: Box<dyn ByteSource + Send + Sync>,
    total_bytes: u64,
    cache: Mutex<SectionCache<BytestreamSectionKey>>,
    known: Vec<&'static str>,
}

impl std::fmt::Debug for BytestreamBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BytestreamBacking")
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

/// One circular, track-relative byte sequence per location the family
/// addresses: what a declared group code resolved out of the bits.
#[derive(Debug)]
pub(crate) struct EncodedBytestream {
    profile: &'static str,
    codec: &'static str,
    locations: BTreeMap<LocationKey, Location>,
    provenance: Provenance,
    backing: Option<BytestreamBacking>,
}

impl EncodedBytestream {
    pub(crate) fn profile(&self) -> &'static str {
        self.profile
    }

    /// Which declared group code resolved these bytes.
    pub(crate) fn codec(&self) -> &'static str {
        self.codec
    }

    /// The codec, the channel beneath it and the medium policy beneath
    /// that, in that order: nothing is dropped on the way up.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn location(&self, key: &LocationKey) -> Option<&Location> {
        self.locations.get(key)
    }

    pub(crate) fn locations(&self) -> impl Iterator<Item = &Location> {
        self.locations.values()
    }

    pub(crate) fn attach_backing(
        &mut self,
        bytes: Box<dyn ByteSource + Send + Sync>,
        total_bytes: u64,
        cache_bytes: u64,
    ) {
        let known = vec![self.profile];
        self.backing = Some(BytestreamBacking {
            bytes,
            total_bytes,
            cache: Mutex::new(SectionCache::with_bytes(cache_bytes)),
            known,
        });
    }

    pub(crate) fn backing_bytes(&self) -> u64 {
        self.backing
            .as_ref()
            .map_or(0, |backing| backing.total_bytes)
    }

    pub(crate) fn resident_bytes(&self) -> u64 {
        self.backing.as_ref().map_or(0, |backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .resident_bytes()
        })
    }

    fn backing(&self) -> Result<&BytestreamBacking> {
        self.backing.as_ref().ok_or_else(|| {
            Error::invalid_image(
                self.profile,
                "bytestream has no backing to read its bytes from, so nothing was ever \
                 written for it to serve",
            )
        })
    }

    fn section(&self, key: &BytestreamSectionKey) -> Result<Vec<u8>> {
        let backing = self.backing()?;
        let mut cache = backing
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .section(
                backing.bytes.as_ref(),
                backing.total_bytes,
                key,
                &backing.known,
            )
            .map(<[u8]>::to_vec)
    }

    fn holds_section(&self, key: &BytestreamSectionKey) -> bool {
        self.backing.as_ref().is_some_and(|backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .holds(key)
        })
    }

    /// One chunk of one location's byte records.
    pub(crate) fn byte_chunk(&self, location: &Location, ordinal: u64) -> Result<Vec<ByteRecord>> {
        let key = BytestreamSectionKey::new(
            location.key.clone(),
            BytestreamSectionKind::ByteChunk,
            ordinal,
        );
        decode_bytes(self.profile, &self.section(&key)?)
    }

    pub(crate) fn byte_chunks(&self, location: &Location) -> u64 {
        location.byte_chunks
    }

    /// One location's byte records, whole. The layer's own tests ask.
    pub(crate) fn bytes(&self, location: &Location) -> Result<Vec<ByteRecord>> {
        let mut records = Vec::with_capacity(location.bytes as usize);
        for ordinal in 0..location.byte_chunks {
            records.extend(self.byte_chunk(location, ordinal)?);
        }
        Ok(records)
    }

    pub(crate) fn facts(&self, location: &Location) -> Result<Vec<BytestreamFact>> {
        let known = self.backing()?.known.clone();
        let mut facts = Vec::new();
        for ordinal in 0..location.fact_chunks {
            let key = BytestreamSectionKey::new(
                location.key.clone(),
                BytestreamSectionKind::FactChunk,
                ordinal,
            );
            facts.extend(decode_facts(self.profile, &self.section(&key)?, &known)?);
        }
        Ok(facts)
    }
}

/// Builds a bytestream and its backing together.
#[derive(Debug)]
pub(crate) struct BytestreamBuilder<S: ByteSink> {
    writer: SectionWriter<BytestreamSectionKey, S>,
    bytestream: EncodedBytestream,
    chunk_records: usize,
}

impl<S: ByteSink> BytestreamBuilder<S> {
    pub(crate) fn new(
        profile: &'static str,
        codec: &'static str,
        policy: Provenance,
        sink: S,
    ) -> Result<Self> {
        if policy.notes.is_empty() {
            return Err(Error::invalid_image(
                profile,
                "bytestream states no codec for the presentation that produced it, and \
                 a code no policy names is a refusal rather than a default",
            ));
        }
        Ok(Self {
            writer: SectionWriter::to_sink(sink, LEAF_ENTRIES),
            bytestream: EncodedBytestream {
                profile,
                codec,
                locations: BTreeMap::new(),
                provenance: policy,
                backing: None,
            },
            chunk_records: CHUNK_RECORDS,
        })
    }

    pub(crate) fn with_chunk_records(mut self, records: usize) -> Self {
        self.chunk_records = records.max(1);
        self
    }

    pub(crate) fn add_location(
        &mut self,
        key: LocationKey,
        records: &[ByteRecord],
        facts: &[BytestreamFact],
        provenance: Provenance,
    ) -> Result<()> {
        let profile = self.bytestream.profile;
        if key.profile() != profile {
            return Err(Error::invalid_image(
                profile,
                format!(
                    "location is addressed by the profile '{}' where this bytestream's \
                     codec is declared by '{profile}'",
                    key.profile()
                ),
            ));
        }

        let byte_chunks: Vec<Vec<u8>> = records
            .chunks(self.chunk_records)
            .map(encode_bytes)
            .collect();
        let fact_chunks: Vec<Vec<u8>> =
            facts.chunks(self.chunk_records).map(encode_facts).collect();
        let location = Location {
            key: key.clone(),
            bytes: records.len() as u64,
            resolved: records
                .iter()
                .filter(|record| record.value().is_some())
                .count() as u64,
            unresolved: records
                .iter()
                .filter(|record| record.value().is_none())
                .count() as u64,
            alignments: facts
                .iter()
                .filter(|fact| matches!(fact.kind, BytestreamFactKind::Alignment { .. }))
                .count() as u64,
            unframed_bits: facts
                .iter()
                .filter_map(|fact| match fact.kind {
                    BytestreamFactKind::Unframed { bits, .. } => Some(bits),
                    BytestreamFactKind::Alignment { .. } => None,
                })
                .sum(),
            byte_chunks: byte_chunks.len() as u64,
            fact_chunks: fact_chunks.len() as u64,
            provenance,
        };

        self.writer.append(
            BytestreamSectionKey::new(key.clone(), BytestreamSectionKind::LocationMetadata, 0),
            encode_location_metadata(&location),
        )?;
        for (ordinal, payload) in byte_chunks.into_iter().enumerate() {
            self.writer.append(
                BytestreamSectionKey::new(
                    key.clone(),
                    BytestreamSectionKind::ByteChunk,
                    ordinal as u64,
                ),
                payload,
            )?;
        }
        for (ordinal, payload) in fact_chunks.into_iter().enumerate() {
            self.writer.append(
                BytestreamSectionKey::new(
                    key.clone(),
                    BytestreamSectionKind::FactChunk,
                    ordinal as u64,
                ),
                payload,
            )?;
        }
        self.bytestream.locations.insert(key, location);
        Ok(())
    }

    pub(crate) fn seal(self) -> Result<(EncodedBytestream, S, u64)> {
        let (sink, total) = self.writer.seal()?;
        Ok((self.bytestream, sink, total))
    }
}

/// The version every metadata section this layer writes carries.
const METADATA_VERSION: u64 = 1;

fn encode_location_metadata(location: &Location) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, location.bytes);
    write_varint(&mut out, location.resolved);
    write_varint(&mut out, location.unresolved);
    write_varint(&mut out, location.alignments);
    write_varint(&mut out, location.unframed_bits);
    write_varint(&mut out, location.byte_chunks);
    write_varint(&mut out, location.fact_chunks);
    encode_provenance(&mut out, &location.provenance);
    out
}

/// Delta-codes one chunk of byte records against their bit positions.
fn encode_bytes(records: &[ByteRecord]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, records.len() as u64);
    let mut previous = 0u64;
    for (index, record) in records.iter().enumerate() {
        write_varint(
            &mut out,
            if index == 0 {
                record.at_bit
            } else {
                record.at_bit.wrapping_sub(previous)
            },
        );
        previous = record.at_bit;
        match record.outcome {
            ByteOutcome::Resolved(value) => {
                write_varint(&mut out, 0);
                write_varint(&mut out, u64::from(value));
            }
            ByteOutcome::Unresolved { bits } => {
                write_varint(&mut out, 1);
                write_varint(&mut out, bits);
            }
        }
    }
    out
}

fn decode_bytes(profile: &str, bytes: &[u8]) -> Result<Vec<ByteRecord>> {
    let mut at = 0;
    let (version, used) = read_varint(profile, bytes, at)?;
    at += used;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            profile,
            format!(
                "backing states byte chunk version {version}, which this build has no reading of"
            ),
        ));
    }
    let (count, used) = read_varint(profile, bytes, at)?;
    at += used;
    let mut records = Vec::new();
    let mut position = 0u64;
    for index in 0..count {
        let (gap, used) = read_varint(profile, bytes, at)?;
        at += used;
        position = if index == 0 {
            gap
        } else {
            position.wrapping_add(gap)
        };
        let (tag, used) = read_varint(profile, bytes, at)?;
        at += used;
        let (value, used) = read_varint(profile, bytes, at)?;
        at += used;
        let outcome = match tag {
            0 => ByteOutcome::Resolved(u8::try_from(value).map_err(|_| {
                Error::invalid_image(profile, "byte chunk states a value wider than a byte")
            })?),
            1 => ByteOutcome::Unresolved { bits: value },
            other => {
                return Err(Error::invalid_image(
                    profile,
                    format!(
                        "byte chunk states an outcome {other}, which this version has no \
                         reading of"
                    ),
                ));
            }
        };
        records.push(ByteRecord::new(position, outcome));
    }
    if at != bytes.len() {
        return Err(Error::invalid_image(
            profile,
            format!(
                "byte chunk decodes {count} records out of {at} of its {} bytes, so it \
                 is not the chunk the index describes",
                bytes.len()
            ),
        ));
    }
    Ok(records)
}

fn encode_facts(facts: &[BytestreamFact]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, facts.len() as u64);
    for fact in facts {
        match &fact.kind {
            BytestreamFactKind::Alignment { at_bit, run_bits } => {
                write_varint(&mut out, 0);
                write_varint(&mut out, *at_bit);
                write_varint(&mut out, *run_bits);
            }
            BytestreamFactKind::Unframed { at_bit, bits } => {
                write_varint(&mut out, 1);
                write_varint(&mut out, *at_bit);
                write_varint(&mut out, *bits);
            }
        }
        encode_provenance(&mut out, &fact.provenance);
    }
    out
}

fn decode_facts(
    profile: &str,
    bytes: &[u8],
    known: &[&'static str],
) -> Result<Vec<BytestreamFact>> {
    let mut at = 0;
    let (version, used) = read_varint(profile, bytes, at)?;
    at += used;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            profile,
            format!(
                "backing states fact chunk version {version}, which this build has no reading of"
            ),
        ));
    }
    let (count, used) = read_varint(profile, bytes, at)?;
    at += used;
    let mut facts = Vec::new();
    for _ in 0..count {
        let (tag, used) = read_varint(profile, bytes, at)?;
        at += used;
        let (at_bit, used) = read_varint(profile, bytes, at)?;
        at += used;
        let (span, used) = read_varint(profile, bytes, at)?;
        at += used;
        let kind = match tag {
            0 => BytestreamFactKind::Alignment {
                at_bit,
                run_bits: span,
            },
            1 => BytestreamFactKind::Unframed { at_bit, bits: span },
            other => {
                return Err(Error::invalid_image(
                    profile,
                    format!(
                        "fact chunk states a kind {other}, which this version has no reading of"
                    ),
                ));
            }
        };
        let (provenance, used) = decode_provenance(profile, bytes, at, known)?;
        at += used;
        facts.push(BytestreamFact::new(kind, provenance));
    }
    if at != bytes.len() {
        return Err(Error::invalid_image(
            profile,
            format!(
                "fact chunk decodes {count} facts out of {at} of its {} bytes, so it is \
                 not the chunk the index describes",
                bytes.len()
            ),
        ));
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    const C1541: &str = "c1541";
    const CODEC: &str = "c1541-gcr";

    fn policy() -> Provenance {
        Provenance::new(C1541).note("framed on the family's declared landmark")
    }

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    fn built() -> (EncodedBytestream, LocationKey) {
        let key = LocationKey::new(C1541, 18, 0);
        let mut builder = BytestreamBuilder::new(C1541, CODEC, policy(), Vec::new())
            .expect("the codec is stated")
            .with_chunk_records(2);
        builder
            .add_location(
                key.clone(),
                &[
                    ByteRecord::new(10, ByteOutcome::Resolved(0x08)),
                    ByteRecord::new(20, ByteOutcome::Resolved(0xff)),
                    ByteRecord::new(
                        30,
                        ByteOutcome::Unresolved {
                            bits: 0b11111_00000,
                        },
                    ),
                    ByteRecord::new(40, ByteOutcome::Resolved(0x00)),
                    ByteRecord::new(50, ByteOutcome::Resolved(0x5a)),
                ],
                &[
                    BytestreamFact::new(
                        BytestreamFactKind::Alignment {
                            at_bit: 0,
                            run_bits: 40,
                        },
                        Provenance::new(C1541).note("a run of the declared landmark"),
                    ),
                    BytestreamFact::new(
                        BytestreamFactKind::Unframed {
                            at_bit: 60,
                            bits: 6,
                        },
                        Provenance::new(C1541).note("left over where the next landmark arrived"),
                    ),
                ],
                Provenance::new(C1541).note("resolved from the hardware bitstream"),
            )
            .expect("the location");
        let (mut bytestream, bytes, total) = builder.seal().expect("the backing seals");
        bytestream.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
        (bytestream, key)
    }

    #[test]
    fn a_bytestream_cannot_be_built_without_naming_the_codec_that_resolved_it() {
        let error = BytestreamBuilder::new(C1541, CODEC, Provenance::new(C1541), Vec::new())
            .expect_err("an unstated codec is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("no policy names"), "{error}");
    }

    #[test]
    fn a_pattern_the_table_does_not_assign_keeps_its_bits_rather_than_a_near_value() {
        let (bytestream, key) = built();
        let location = bytestream.location(&key).expect("the location was added");
        let records = bytestream.bytes(location).expect("the bytes read back");

        assert_eq!(
            records.iter().map(ByteRecord::at_bit).collect::<Vec<_>>(),
            [10, 20, 30, 40, 50]
        );
        assert_eq!(records[0].value(), Some(0x08));
        assert_eq!(records[1].value(), Some(0xff));
        // The unresolved group holds the bits themselves. There is no
        // nearest entry, because there is no distance to be near by.
        assert_eq!(records[2].value(), None);
        assert_eq!(
            records[2].outcome(),
            ByteOutcome::Unresolved {
                bits: 0b11111_00000
            }
        );
        assert_eq!(location.resolved(), 4);
        assert_eq!(location.unresolved(), 1);
    }

    #[test]
    fn a_landmark_states_where_framing_begins_and_nothing_about_what_follows() {
        let (bytestream, key) = built();
        let location = bytestream.location(&key).expect("added");
        let facts = bytestream.facts(location).expect("the facts read back");

        assert_eq!(facts.len(), 2);
        // The whole vocabulary: where framing was established, and which
        // bits no framed group covers. Neither says a header, a sector
        // or a file follows.
        assert_eq!(
            facts[0].kind(),
            &BytestreamFactKind::Alignment {
                at_bit: 0,
                run_bits: 40
            }
        );
        assert_eq!(
            facts[1].kind(),
            &BytestreamFactKind::Unframed {
                at_bit: 60,
                bits: 6
            }
        );
        assert_eq!(location.alignments(), 1);
        assert_eq!(location.unframed_bits(), 6);
    }

    #[test]
    fn one_location_walks_a_chunk_at_a_time_rather_than_whole() {
        let (bytestream, key) = built();
        let location = bytestream.location(&key).expect("added");

        assert_eq!(bytestream.byte_chunks(location), 3);
        assert_eq!(bytestream.byte_chunk(location, 0).expect("reads").len(), 2);

        let chunk = |ordinal| {
            BytestreamSectionKey::new(key.clone(), BytestreamSectionKind::ByteChunk, ordinal)
        };
        assert!(bytestream.holds_section(&chunk(0)));
        assert!(!bytestream.holds_section(&chunk(2)));
    }

    #[test]
    fn a_location_addressed_by_another_profile_is_refused() {
        let mut builder = BytestreamBuilder::new(C1541, CODEC, policy(), Vec::new())
            .expect("the codec is stated");
        let error = builder
            .add_location(
                LocationKey::new("apple2", 0, 0),
                &[],
                &[],
                Provenance::new(C1541).note("resolved"),
            )
            .expect_err("another family's addressing is refused");

        assert!(error.to_string().contains("apple2"), "{error}");
    }
}
