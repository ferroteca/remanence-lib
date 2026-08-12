// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The private hardware-bitstream model (P23): circular, track-relative
//! clocked bit state, as a declared drive family's read channel makes it.
//!
//! It is the layer above the flux medium and below the encoded
//! bytestream. A [`crate::flux_medium::FluxMedium`] holds pulses and no
//! opinion about the counter that clocks them; a bitstream holds one bit
//! per cell, in the family's own addressing, and asserts exactly what a
//! drive's read channel resolved.
//!
//! **It is pre-sync and pre-decoding.** There is no GCR symbol here, no
//! byte, no synchronization landmark, no header and no sector. Locating
//! the family's alignment landmark is the codec's act one layer up, and
//! a bitstream that had located one would erase the distinction between
//! what a drive's channel resolves and what its codec makes of that.
//!
//! **Every bit says how it came to be.** A pulse the medium states reads
//! the same every time yields a recorded bit; a pulse it states does not
//! yields one resolved under a declared rule, either flatly or
//! reproducibly from a seed. There is no fourth answer and no default:
//! the rule that resolved a bit travels with it, because a bit that
//! could not say which it was would be claiming to be recorded evidence.
//!
//! The cell is the declared zone's, the resync behavior is the declared
//! channel's, and both arrive from a P30 drive profile — nothing here
//! derives either from what it is reading. The arithmetic is exact
//! integer throughout: positions are scaled by the cell's own
//! denominator so that a rational cell places a boundary exactly rather
//! than near it.
//!
//! The backing is the flux-capture layer's, keyed by the family's own
//! addressing (P27). Every item is crate-private: the presentation above
//! builds one of these and the codec reads one, and nothing outside the
//! crate sees a bit.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::evidence::Provenance;
use crate::flux_capture::{
    ByteSink, ByteSource, CHUNK_RECORDS, LEAF_ENTRIES, SectionAddress, SectionCache, SectionWriter,
    decode_provenance, encode_provenance, read_varint, write_varint,
};
use crate::flux_medium::{Cycle, LocationKey, read_location_key, write_location_key};

/// How a cell's bit came to be what it is.
///
/// None of these is a default, and the vocabulary is deliberately three
/// values rather than a flag: "resolved by a declaration" and "resolved
/// reproducibly from a seed" are different claims about repeatability,
/// and a layer that spelled them the same could not say which one a
/// caller is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitEvidence {
    /// The medium states this pulse — or this absence of one — reads the
    /// same every time, so the cell's bit is what was recorded.
    Recorded,
    /// The medium states the pulse does not read the same every time,
    /// and the policy declared one answer for every such pulse.
    Declared,
    /// The same, resolved reproducibly instead: the domain is what makes
    /// the answer repeatable (P29).
    Seeded { domain: u64 },
}

impl BitEvidence {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Declared => "declared",
            Self::Seeded { .. } => "seeded",
        }
    }

    /// Whether the bit is what the medium recorded rather than what a
    /// rule resolved.
    pub(crate) fn is_recorded(self) -> bool {
        matches!(self, Self::Recorded)
    }
}

/// One bit cell: where the channel closed it, what it read, and how that
/// bit came to be.
///
/// `end` is scaled by the location's cell denominator, so a cell of
/// 3200000/61539 cycles closes exactly where it closes rather than at
/// the nearest whole cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BitCell {
    end: u64,
    one: bool,
    evidence: BitEvidence,
}

impl BitCell {
    pub(crate) fn new(end: u64, one: bool, evidence: BitEvidence) -> Self {
        Self { end, one, evidence }
    }

    /// Where the channel closed this cell, in scaled cycles from the
    /// location's origin.
    pub(crate) fn end(&self) -> u64 {
        self.end
    }

    pub(crate) fn one(&self) -> bool {
        self.one
    }

    pub(crate) fn evidence(&self) -> BitEvidence {
        self.evidence
    }
}

/// What one bitstream-level fact says: the facts about a location that
/// are not about any one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BitstreamFactKind {
    /// The write splice the medium located, carried through as the angle
    /// it is. It is not a landmark and introduces nothing.
    Seam { angle: Cycle },
    /// The medium claims this location is recorded and blank.
    Unformatted,
    /// A run of zero cells longer than the family's encoding admits
    /// between transitions. An observation, not a conclusion: it says
    /// the recording departed from the encoding there and nothing about
    /// why.
    LongZeroRun { at_bit: u64, cells: u64 },
    /// A cell the channel closed shorter than the encoding's shortest
    /// interval, two transitions having arrived closer together than one
    /// cell.
    ShortCell { at_bit: u64 },
}

/// One fact about a location, with how it came to be known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BitstreamFact {
    kind: BitstreamFactKind,
    provenance: Provenance,
}

impl BitstreamFact {
    pub(crate) fn new(kind: BitstreamFactKind, provenance: Provenance) -> Self {
        Self { kind, provenance }
    }

    pub(crate) fn kind(&self) -> &BitstreamFactKind {
        &self.kind
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What one section of a bitstream's backing holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BitstreamSectionKind {
    LocationMetadata,
    CellChunk,
    FactChunk,
}

/// The complete address of one section of a bitstream's backing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BitstreamSectionKey {
    location: LocationKey,
    kind: BitstreamSectionKind,
    ordinal: u64,
}

impl BitstreamSectionKey {
    pub(crate) fn new(location: LocationKey, kind: BitstreamSectionKind, ordinal: u64) -> Self {
        Self {
            location,
            kind,
            ordinal,
        }
    }
}

impl SectionAddress for BitstreamSectionKey {
    fn namespace(&self) -> &'static str {
        self.location.profile()
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_location_key(out, &self.location);
        write_varint(
            out,
            match self.kind {
                BitstreamSectionKind::LocationMetadata => 0,
                BitstreamSectionKind::CellChunk => 1,
                BitstreamSectionKind::FactChunk => 2,
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
            0 => BitstreamSectionKind::LocationMetadata,
            1 => BitstreamSectionKind::CellChunk,
            2 => BitstreamSectionKind::FactChunk,
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

/// What a bitstream keeps resident for one location: its identity, the
/// clock it was resolved against, and its shape — never its bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Location {
    key: LocationKey,
    /// Which of the family's declared density zones supplied the cell.
    zone: u32,
    /// The cell, as an exact rational of reference-clock cycles.
    cell_numerator: u64,
    cell_denominator: u64,
    cells: u64,
    one_bits: u64,
    resolved_bits: u64,
    short_cells: u64,
    /// What is left of the circle after the last whole cell: the circle
    /// does not divide into cells, and the remainder is stated rather
    /// than rounded into a bit.
    wrap_slack: u64,
    cell_chunks: u64,
    fact_chunks: u64,
    provenance: Provenance,
}

impl Location {
    pub(crate) fn key(&self) -> &LocationKey {
        &self.key
    }

    pub(crate) fn zone(&self) -> u32 {
        self.zone
    }

    /// The cell this location's bits were clocked at, exactly.
    pub(crate) fn cell(&self) -> (u64, u64) {
        (self.cell_numerator, self.cell_denominator)
    }

    pub(crate) fn cells(&self) -> u64 {
        self.cells
    }

    pub(crate) fn one_bits(&self) -> u64 {
        self.one_bits
    }

    /// How many of the bits were resolved by a declared rule rather than
    /// read off the medium as recorded.
    pub(crate) fn resolved_bits(&self) -> u64 {
        self.resolved_bits
    }

    pub(crate) fn short_cells(&self) -> u64 {
        self.short_cells
    }

    pub(crate) fn wrap_slack(&self) -> u64 {
        self.wrap_slack
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Where a bitstream's sections are read back from, and the bounded
/// working set they are served through (P27).
struct BitstreamBacking {
    bytes: Box<dyn ByteSource + Send + Sync>,
    total_bytes: u64,
    cache: Mutex<SectionCache<BitstreamSectionKey>>,
    known: Vec<&'static str>,
}

impl std::fmt::Debug for BitstreamBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitstreamBacking")
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

/// One circular, track-relative bit sequence per location the family
/// addresses: what a drive's read channel resolved.
#[derive(Debug)]
pub(crate) struct HardwareBitstream {
    profile: &'static str,
    /// The frame the cells are angles in, carried from the medium
    /// unchanged: the presentation clocks the circle, it does not
    /// redefine it.
    reference_clock_hz: u64,
    cycles_per_rotation: u64,
    locations: BTreeMap<LocationKey, Location>,
    provenance: Provenance,
    backing: Option<BitstreamBacking>,
}

impl HardwareBitstream {
    pub(crate) fn profile(&self) -> &'static str {
        self.profile
    }

    pub(crate) fn reference_clock_hz(&self) -> u64 {
        self.reference_clock_hz
    }

    pub(crate) fn cycles_per_rotation(&self) -> u64 {
        self.cycles_per_rotation
    }

    /// The read channel and the medium's own policy, in that order: the
    /// bitstream cannot exist without both, and neither is dropped on
    /// the way up.
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
        self.backing = Some(BitstreamBacking {
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

    fn backing(&self) -> Result<&BitstreamBacking> {
        self.backing.as_ref().ok_or_else(|| {
            Error::invalid_image(
                self.profile,
                "bitstream has no backing to read its cells from, so nothing was ever \
                 written for it to serve",
            )
        })
    }

    fn section(&self, key: &BitstreamSectionKey) -> Result<Vec<u8>> {
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

    fn holds_section(&self, key: &BitstreamSectionKey) -> bool {
        self.backing.as_ref().is_some_and(|backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .holds(key)
        })
    }

    /// One chunk of one location's cells. The codec above walks a
    /// location a chunk at a time, so no location is ever decoded whole
    /// on the way to a byte.
    pub(crate) fn cell_chunk(&self, location: &Location, ordinal: u64) -> Result<Vec<BitCell>> {
        let key = BitstreamSectionKey::new(
            location.key.clone(),
            BitstreamSectionKind::CellChunk,
            ordinal,
        );
        decode_cells(self.profile, &self.section(&key)?)
    }

    pub(crate) fn cell_chunks(&self, location: &Location) -> u64 {
        location.cell_chunks
    }

    /// One location's cells, whole. The layer's own tests ask; the codec
    /// walks chunks.
    pub(crate) fn cells(&self, location: &Location) -> Result<Vec<BitCell>> {
        let mut cells = Vec::with_capacity(location.cells as usize);
        for ordinal in 0..location.cell_chunks {
            cells.extend(self.cell_chunk(location, ordinal)?);
        }
        Ok(cells)
    }

    /// One location's bitstream-level facts, in the order the channel
    /// stated them.
    pub(crate) fn facts(&self, location: &Location) -> Result<Vec<BitstreamFact>> {
        let known = self.backing()?.known.clone();
        let mut facts = Vec::new();
        for ordinal in 0..location.fact_chunks {
            let key = BitstreamSectionKey::new(
                location.key.clone(),
                BitstreamSectionKind::FactChunk,
                ordinal,
            );
            facts.extend(decode_facts(self.profile, &self.section(&key)?, &known)?);
        }
        Ok(facts)
    }
}

/// Builds a bitstream and its backing together: the cells stream into
/// the sink as each location is clocked, and only identity and shape
/// stay resident.
///
/// It cannot be started without naming the read channel and the medium
/// policy beneath it, which is what makes "a bitstream carries its
/// profile and its source's selection as provenance" a property of the
/// code rather than a convention.
#[derive(Debug)]
pub(crate) struct BitstreamBuilder<S: ByteSink> {
    writer: SectionWriter<BitstreamSectionKey, S>,
    bitstream: HardwareBitstream,
    chunk_records: usize,
}

impl<S: ByteSink> BitstreamBuilder<S> {
    pub(crate) fn new(
        profile: &'static str,
        reference_clock_hz: u64,
        cycles_per_rotation: u64,
        policy: Provenance,
        sink: S,
    ) -> Result<Self> {
        if policy.notes.is_empty() {
            return Err(Error::invalid_image(
                profile,
                "bitstream states no read channel for the presentation that produced \
                 it, and a channel no policy names is a refusal rather than a default",
            ));
        }
        if cycles_per_rotation == 0 {
            return Err(Error::invalid_image(
                profile,
                "bitstream declares a rotation of zero cycles, which states no circle \
                 to clock",
            ));
        }
        Ok(Self {
            writer: SectionWriter::to_sink(sink, LEAF_ENTRIES),
            bitstream: HardwareBitstream {
                profile,
                reference_clock_hz,
                cycles_per_rotation,
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

    /// How many cells a chunk of this builder's backing holds. The codec
    /// above needs it to turn a bit index into a chunk.
    pub(crate) fn chunk_records(&self) -> usize {
        self.chunk_records
    }

    /// Adds one location and every cell the channel resolved at it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_location(
        &mut self,
        key: LocationKey,
        zone: u32,
        cell: (u64, u64),
        wrap_slack: u64,
        cells: &[BitCell],
        facts: &[BitstreamFact],
        provenance: Provenance,
    ) -> Result<()> {
        let profile = self.bitstream.profile;
        if key.profile() != profile {
            return Err(Error::invalid_image(
                profile,
                format!(
                    "location is addressed by the profile '{}' where this bitstream's \
                     channel is declared by '{profile}'",
                    key.profile()
                ),
            ));
        }
        if cell.1 == 0 || cell.0 == 0 {
            return Err(Error::invalid_image(
                profile,
                format!(
                    "location is clocked at a cell of {}/{} cycles, which states no cell",
                    cell.0, cell.1
                ),
            ));
        }
        let mut previous = 0u64;
        for (ordinal, cell) in cells.iter().enumerate() {
            if cell.end <= previous && ordinal > 0 {
                return Err(Error::invalid_image(
                    profile,
                    format!(
                        "cell {ordinal} closes at {} which does not advance past the \
                         preceding {previous}",
                        cell.end
                    ),
                ));
            }
            previous = cell.end;
        }

        let cell_chunks: Vec<Vec<u8>> =
            cells.chunks(self.chunk_records).map(encode_cells).collect();
        let fact_chunks: Vec<Vec<u8>> =
            facts.chunks(self.chunk_records).map(encode_facts).collect();
        let location = Location {
            key: key.clone(),
            zone,
            cell_numerator: cell.0,
            cell_denominator: cell.1,
            cells: cells.len() as u64,
            one_bits: cells.iter().filter(|cell| cell.one).count() as u64,
            resolved_bits: cells
                .iter()
                .filter(|cell| !cell.evidence.is_recorded())
                .count() as u64,
            short_cells: facts
                .iter()
                .filter(|fact| matches!(fact.kind, BitstreamFactKind::ShortCell { .. }))
                .count() as u64,
            wrap_slack,
            cell_chunks: cell_chunks.len() as u64,
            fact_chunks: fact_chunks.len() as u64,
            provenance,
        };

        self.writer.append(
            BitstreamSectionKey::new(key.clone(), BitstreamSectionKind::LocationMetadata, 0),
            encode_location_metadata(&location),
        )?;
        for (ordinal, payload) in cell_chunks.into_iter().enumerate() {
            self.writer.append(
                BitstreamSectionKey::new(
                    key.clone(),
                    BitstreamSectionKind::CellChunk,
                    ordinal as u64,
                ),
                payload,
            )?;
        }
        for (ordinal, payload) in fact_chunks.into_iter().enumerate() {
            self.writer.append(
                BitstreamSectionKey::new(
                    key.clone(),
                    BitstreamSectionKind::FactChunk,
                    ordinal as u64,
                ),
                payload,
            )?;
        }
        self.bitstream.locations.insert(key, location);
        Ok(())
    }

    pub(crate) fn seal(self) -> Result<(HardwareBitstream, S, u64)> {
        let (sink, total) = self.writer.seal()?;
        Ok((self.bitstream, sink, total))
    }
}

/// The version every metadata section this layer writes carries.
const METADATA_VERSION: u64 = 1;

fn encode_location_metadata(location: &Location) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, u64::from(location.zone));
    write_varint(&mut out, location.cell_numerator);
    write_varint(&mut out, location.cell_denominator);
    write_varint(&mut out, location.cells);
    write_varint(&mut out, location.one_bits);
    write_varint(&mut out, location.resolved_bits);
    write_varint(&mut out, location.short_cells);
    write_varint(&mut out, location.wrap_slack);
    write_varint(&mut out, location.cell_chunks);
    write_varint(&mut out, location.fact_chunks);
    encode_provenance(&mut out, &location.provenance);
    out
}

/// Delta-codes one chunk of cells.
///
/// The ends ascend, so every gap is unsigned and the first is the span
/// from the location's origin. The evidence travels with its bit rather
/// than in a parallel channel: they are one fact.
fn encode_cells(cells: &[BitCell]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, cells.len() as u64);
    let mut previous = 0;
    for (index, cell) in cells.iter().enumerate() {
        write_varint(
            &mut out,
            if index == 0 {
                cell.end
            } else {
                cell.end - previous
            },
        );
        previous = cell.end;
        let (tag, domain) = match cell.evidence {
            BitEvidence::Recorded => (0, None),
            BitEvidence::Declared => (1, None),
            BitEvidence::Seeded { domain } => (2, Some(domain)),
        };
        write_varint(&mut out, u64::from(cell.one) | tag << 1);
        if let Some(domain) = domain {
            write_varint(&mut out, domain);
        }
    }
    out
}

fn decode_cells(profile: &str, bytes: &[u8]) -> Result<Vec<BitCell>> {
    let mut at = 0;
    let (version, used) = read_varint(profile, bytes, at)?;
    at += used;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            profile,
            format!(
                "backing states cell chunk version {version}, which this build has no reading of"
            ),
        ));
    }
    let (count, used) = read_varint(profile, bytes, at)?;
    at += used;
    let mut cells = Vec::new();
    let mut end = 0u64;
    for index in 0..count {
        let (gap, used) = read_varint(profile, bytes, at)?;
        at += used;
        end = if index == 0 {
            gap
        } else {
            end.checked_add(gap).ok_or_else(|| {
                Error::invalid_image(profile, "cell chunk accumulates past one rotation")
            })?
        };
        let (packed, used) = read_varint(profile, bytes, at)?;
        at += used;
        let evidence = match packed >> 1 {
            0 => BitEvidence::Recorded,
            1 => BitEvidence::Declared,
            2 => {
                let (domain, used) = read_varint(profile, bytes, at)?;
                at += used;
                BitEvidence::Seeded { domain }
            }
            other => {
                return Err(Error::invalid_image(
                    profile,
                    format!(
                        "cell chunk states the evidence {other}, which this version has \
                         no reading of"
                    ),
                ));
            }
        };
        cells.push(BitCell::new(end, packed & 1 == 1, evidence));
    }
    if at != bytes.len() {
        return Err(Error::invalid_image(
            profile,
            format!(
                "cell chunk decodes {count} cells out of {at} of its {} bytes, so it is \
                 not the chunk the index describes",
                bytes.len()
            ),
        ));
    }
    Ok(cells)
}

fn encode_facts(facts: &[BitstreamFact]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, facts.len() as u64);
    for fact in facts {
        match &fact.kind {
            BitstreamFactKind::Seam { angle } => {
                write_varint(&mut out, 0);
                write_varint(&mut out, *angle);
            }
            BitstreamFactKind::Unformatted => write_varint(&mut out, 1),
            BitstreamFactKind::LongZeroRun { at_bit, cells } => {
                write_varint(&mut out, 2);
                write_varint(&mut out, *at_bit);
                write_varint(&mut out, *cells);
            }
            BitstreamFactKind::ShortCell { at_bit } => {
                write_varint(&mut out, 3);
                write_varint(&mut out, *at_bit);
            }
        }
        encode_provenance(&mut out, &fact.provenance);
    }
    out
}

fn decode_facts(profile: &str, bytes: &[u8], known: &[&'static str]) -> Result<Vec<BitstreamFact>> {
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
        let kind = match tag {
            0 => {
                let (angle, used) = read_varint(profile, bytes, at)?;
                at += used;
                BitstreamFactKind::Seam { angle }
            }
            1 => BitstreamFactKind::Unformatted,
            2 => {
                let (at_bit, used) = read_varint(profile, bytes, at)?;
                at += used;
                let (cells, used) = read_varint(profile, bytes, at)?;
                at += used;
                BitstreamFactKind::LongZeroRun { at_bit, cells }
            }
            3 => {
                let (at_bit, used) = read_varint(profile, bytes, at)?;
                at += used;
                BitstreamFactKind::ShortCell { at_bit }
            }
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
        facts.push(BitstreamFact::new(kind, provenance));
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

    fn policy() -> Provenance {
        Provenance::new(C1541).note(
            "clocked at the zone's declared cell, resyncing on \
                                     every detected transition",
        )
    }

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    fn built() -> (HardwareBitstream, LocationKey) {
        let key = LocationKey::new(C1541, 18, 0);
        let mut builder = BitstreamBuilder::new(C1541, 16_000_000, 3_200_000, policy(), Vec::new())
            .expect("the channel is stated")
            // Two cells a chunk, so the cells below span several sections
            // and a walk has to visit them.
            .with_chunk_records(2);
        builder
            .add_location(
                key.clone(),
                1,
                (56, 1),
                17,
                &[
                    BitCell::new(56, false, BitEvidence::Recorded),
                    BitCell::new(110, true, BitEvidence::Recorded),
                    BitCell::new(166, true, BitEvidence::Declared),
                    BitCell::new(222, true, BitEvidence::Seeded { domain: 7 }),
                    BitCell::new(278, false, BitEvidence::Recorded),
                ],
                &[
                    BitstreamFact::new(
                        BitstreamFactKind::Seam { angle: 1000 },
                        Provenance::new(C1541).note("carried from the medium"),
                    ),
                    BitstreamFact::new(
                        BitstreamFactKind::LongZeroRun {
                            at_bit: 4,
                            cells: 9,
                        },
                        Provenance::new(C1541).note("longer than the encoding admits"),
                    ),
                ],
                Provenance::new(C1541).note("clocked from the mastered medium"),
            )
            .expect("the location");
        let (mut bitstream, bytes, total) = builder.seal().expect("the backing seals");
        bitstream.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
        (bitstream, key)
    }

    #[test]
    fn a_bitstream_cannot_be_built_without_naming_the_channel_that_clocked_it() {
        let error = BitstreamBuilder::new(
            C1541,
            16_000_000,
            3_200_000,
            Provenance::new(C1541),
            Vec::new(),
        )
        .expect_err("an unstated channel is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("no policy names"), "{error}");
    }

    #[test]
    fn every_bit_says_whether_it_was_recorded_or_resolved_and_by_which_rule() {
        // The layer's strongest claim, made a property of the encoding:
        // a bit that could not say which it was would be claiming to be
        // recorded evidence.
        let (bitstream, key) = built();
        let location = bitstream.location(&key).expect("the location was added");
        let cells = bitstream.cells(location).expect("the cells read back");

        assert_eq!(
            cells.iter().map(BitCell::end).collect::<Vec<_>>(),
            [56, 110, 166, 222, 278]
        );
        assert_eq!(
            cells.iter().map(BitCell::one).collect::<Vec<_>>(),
            [false, true, true, true, false]
        );
        assert_eq!(cells[0].evidence(), BitEvidence::Recorded);
        assert_eq!(cells[2].evidence(), BitEvidence::Declared);
        assert_eq!(cells[3].evidence(), BitEvidence::Seeded { domain: 7 });
        assert_eq!(cells[3].evidence().as_str(), "seeded");
        // Two of the five were resolved rather than read.
        assert_eq!(location.resolved_bits(), 2);
        assert_eq!(location.one_bits(), 3);
    }

    #[test]
    fn the_circle_states_what_did_not_divide_into_cells_rather_than_rounding_it() {
        let (bitstream, key) = built();
        let location = bitstream.location(&key).expect("added");

        assert_eq!(location.cell(), (56, 1));
        assert_eq!(location.zone(), 1);
        // 3,200,000 cycles is not a whole number of 56-cycle cells, and
        // the remainder is a stated fact rather than a bit nobody clocked.
        assert_eq!(location.wrap_slack(), 17);
    }

    #[test]
    fn one_location_walks_a_chunk_at_a_time_rather_than_whole() {
        let (bitstream, key) = built();
        let location = bitstream.location(&key).expect("added");

        // Five cells, two to a chunk: three chunks, and reading the
        // first leaves the last untouched.
        assert_eq!(bitstream.cell_chunks(location), 3);
        let first = bitstream.cell_chunk(location, 0).expect("the chunk reads");
        assert_eq!(first.len(), 2);

        let chunk = |ordinal| {
            BitstreamSectionKey::new(key.clone(), BitstreamSectionKind::CellChunk, ordinal)
        };
        assert!(bitstream.holds_section(&chunk(0)));
        assert!(!bitstream.holds_section(&chunk(2)));
    }

    #[test]
    fn cells_that_do_not_advance_are_refused_rather_than_sorted() {
        let mut builder = BitstreamBuilder::new(C1541, 16_000_000, 3_200_000, policy(), Vec::new())
            .expect("the channel is stated");
        let error = builder
            .add_location(
                LocationKey::new(C1541, 1, 0),
                0,
                (52, 1),
                0,
                &[
                    BitCell::new(52, false, BitEvidence::Recorded),
                    BitCell::new(52, true, BitEvidence::Recorded),
                ],
                &[],
                Provenance::new(C1541).note("clocked"),
            )
            .expect_err("two cells closing at one angle contradict each other");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_location_addressed_by_another_profile_is_refused() {
        let mut builder = BitstreamBuilder::new(C1541, 16_000_000, 3_200_000, policy(), Vec::new())
            .expect("the channel is stated");
        let error = builder
            .add_location(
                LocationKey::new("apple2", 0, 0),
                0,
                (52, 1),
                0,
                &[],
                &[],
                Provenance::new(C1541).note("clocked"),
            )
            .expect_err("another family's addressing is refused");

        assert!(error.to_string().contains("apple2"), "{error}");
    }

    #[test]
    fn the_facts_carried_up_stay_the_facts_they_were() {
        let (bitstream, key) = built();
        let location = bitstream.location(&key).expect("added");
        let facts = bitstream.facts(location).expect("the facts read back");

        assert_eq!(facts.len(), 2);
        // The seam is the medium's, carried through as the angle it is:
        // it is not a landmark and it introduces nothing.
        assert_eq!(facts[0].kind(), &BitstreamFactKind::Seam { angle: 1000 });
        assert_eq!(
            facts[1].kind(),
            &BitstreamFactKind::LongZeroRun {
                at_bit: 4,
                cells: 9
            }
        );
        assert_eq!(
            facts[1].provenance().notes,
            ["longer than the encoding admits"]
        );
    }
}
