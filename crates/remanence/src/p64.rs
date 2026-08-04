// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The P64 image-format adapter (F34, P12): one container, claimed in
//! both directions.
//!
//! P64's authoritative image layer is a flux medium (P13, P22), so this
//! adapter reads one out of a container and writes one into a new
//! container, and nothing between. It owns the container's identity,
//! recognition and evidence, version and structure validation, its
//! refusals, and its capability claim. It owns no selection,
//! reconciliation, or timing policy: those belong to the mastering
//! profile, and what arrives here outside the claim is refused rather
//! than repaired.
//!
//! **The grammar below is the claim.** Signature, version, flags,
//! integrity fields, chunk vocabulary, half-track addressing, and the
//! width and meaning of a stored pulse's position and strength are
//! enumerated here from the published format description — Benjamin
//! Rosseaux's "P64 file format specification", as distributed with the
//! VICE manual — rather than inferred from a file that happened to open
//! (P1, P3). The version is checked before anything else is touched, and
//! a version, flag bit, or chunk signature past the claim fails
//! immediately naming what was found and what is supported (P8).
//!
//! It decodes into a medium and encodes from one. It never decodes GCR,
//! synchronization, sectors, a filesystem, or files, and it exposes no
//! pulse iterator: the evidence stays behind the surface.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::c1541_mastering::MasteredMedium;
use crate::checksum::Crc32;
use crate::device::{self, AccessIntent, AccessMode};
use crate::drive_profile::C1541;
use crate::error::{Error, ErrorCategory, Result};
use crate::evidence::{DeclaredLoss, LossAccount, Provenance};
use crate::flux_capture::{SessionBacking, TimeBase};
use crate::flux_medium::{
    Cycle, Derivation, FluxMedium, LocationKey, MediumBuilder, MediumFactKind, OriginRule,
    OriginStatement, Pulse, RotationalFrame, Strength,
};

// --------------------------------------------------------- the grammar

/// The container's stable identifier.
const P64: &str = "p64";
const P64_NAME: &str = "P64 NRZI flux pulse disk image";

/// The eight bytes every P64 opens with. The container names the drive
/// family outright, which is why this adapter can state the profile a
/// decoded medium is addressed by without inferring one.
const SIGNATURE: [u8; 8] = *b"P64-1541";

/// Signature, version, flags, size, checksum.
const HEADER_BYTES: u64 = 24;
/// Signature, size, checksum.
const CHUNK_HEADER_BYTES: u64 = 12;

/// The only file-format version this release claims (P8).
const SUPPORTED_VERSION: u32 = 0x0000_0000;

const FLAG_WRITE_PROTECT: u32 = 1 << 0;
const FLAG_DOUBLE_SIDED: u32 = 1 << 1;
/// Every other bit is reserved, and the description says a writer sets
/// them to zero. One set is a feature past the claim (P8).
const CLAIMED_FLAGS: u32 = FLAG_WRITE_PROTECT | FLAG_DOUBLE_SIDED;

/// A half-track's flux, under the index byte its fourth character
/// carries.
const HALF_TRACK_CHUNK: [u8; 3] = *b"HTP";
/// The empty chunk that says the file ends where it meant to.
const END_CHUNK: [u8; 4] = *b"DONE";
/// Bit 7 of a half-track index byte is the side.
const SIDE_BIT: u8 = 0x80;

/// The 1541's reference clock, and one 300 RPM rotation of it. The
/// container fixes both, which is what makes a stored position an angle
/// rather than a duration.
const REFERENCE_CLOCK_HZ: u64 = 16_000_000;
const PULSE_SAMPLES_PER_ROTATION: u64 = 3_200_000;

/// A pulse that always triggers.
const STRENGTH_ALWAYS: u32 = 0xffff_ffff;
/// A pulse that almost never triggers — the description's own "weak".
const STRENGTH_WEAK: u32 = 0x0000_0001;
/// A pulse that never triggers at all.
const STRENGTH_NEVER: u32 = 0x0000_0000;

/// The family's declared strength states, weakest first, as indices into
/// the P30 profile's own vocabulary. The adapter's whole strength claim
/// is that these three states and the three values the format
/// description names are the same three facts.
const STATE_ABSENT: u32 = 0;
const STATE_WEAK: u32 = 1;
const STATE_STRONG: u32 = 2;

/// The most one chunk's data may be before the file is refused rather
/// than allocated for. A half-track of every cycle of a rotation does
/// not approach it; nothing honest does.
const CHUNK_BOUND: u64 = 1 << 25;

/// How much is read at once when a region is streamed for its checksum.
const STREAM_BLOCK: usize = 1 << 16;

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image(P64, reason)
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::categorized_image(ErrorCategory::Unsupported, P64, reason)
}

// ------------------------------------------------------- the range coder

// A FPAQ0-style carryless range coder over 12-bit adaptive models, one
// model per byte of a 32-bit value with one bit per byte processed. Ten
// models in all: four for a position, four for a strength, and one each
// for the two flags. The whole of it is the format description's, and it
// is written out here because a container this library reads is one it
// implements (P1).

const MODEL_POSITION: usize = 0;
const MODEL_STRENGTH: usize = 4;
const MODEL_POSITION_FLAG: usize = 8;
const MODEL_STRENGTH_FLAG: usize = 9;
const MODELS: usize = 10;
/// A byte model is indexed by the previous byte and the bits so far.
const BYTE_MODEL_ENTRIES: usize = 1 << 16;
/// A flag model is indexed by the previous flag.
const FLAG_MODEL_ENTRIES: usize = 2;
const PROBABILITIES: usize = 8 * BYTE_MODEL_ENTRIES + 2 * FLAG_MODEL_ENTRIES;
const INITIAL_PROBABILITY: u16 = 2048;
const PROBABILITY_SHIFT: u32 = 4;
const PROBABILITY_CEILING: u32 = 0xfff;

/// The adaptive state, reset before every half-track: each chunk carries
/// its own size and checksum and decodes on its own, so each starts from
/// the same declared initial state.
struct Models {
    probabilities: Vec<u16>,
    states: [u32; MODELS],
}

impl Models {
    fn new() -> Self {
        Self {
            probabilities: vec![INITIAL_PROBABILITY; PROBABILITIES],
            states: [0; MODELS],
        }
    }

    fn reset(&mut self) {
        self.probabilities.fill(INITIAL_PROBABILITY);
        self.states = [0; MODELS];
    }

    const fn offset(model: usize) -> usize {
        if model < 8 {
            model * BYTE_MODEL_ENTRIES
        } else {
            8 * BYTE_MODEL_ENTRIES + (model - 8) * FLAG_MODEL_ENTRIES
        }
    }
}

struct Encoder<'a> {
    models: &'a mut Models,
    low: u32,
    high: u32,
    out: Vec<u8>,
}

impl<'a> Encoder<'a> {
    fn new(models: &'a mut Models) -> Self {
        models.reset();
        Self {
            models,
            low: 0,
            high: u32::MAX,
            out: Vec::new(),
        }
    }

    fn bit(&mut self, at: usize, value: u32) -> u32 {
        let probability = u32::from(self.models.probabilities[at]);
        let middle = self.low + (((self.high - self.low) >> 12) * probability);
        if value != 0 {
            self.models.probabilities[at] =
                (probability + ((PROBABILITY_CEILING - probability) >> PROBABILITY_SHIFT)) as u16;
            self.high = middle;
        } else {
            self.models.probabilities[at] =
                (probability - (probability >> PROBABILITY_SHIFT)) as u16;
            self.low = middle + 1;
        }
        while (self.low ^ self.high) & 0xff00_0000 == 0 {
            self.out.push((self.high >> 24) as u8);
            self.low <<= 8;
            self.high = (self.high << 8) | 0xff;
        }
        value
    }

    fn flag(&mut self, model: usize, value: u32) {
        let at = Models::offset(model) + self.models.states[model] as usize;
        self.models.states[model] = self.bit(at, value & 1);
    }

    fn dword(&mut self, model: usize, value: u32) {
        for byte_index in 0..4u32 {
            let byte_value = (value >> (byte_index * 8)) & 0xff;
            let submodel = model + byte_index as usize;
            let base = Models::offset(submodel);
            let mut context = 1u32;
            for bit in (0..8u32).rev() {
                let at = base + (((self.models.states[submodel] << 8) | context) & 0xffff) as usize;
                context = (context << 1) | self.bit(at, (byte_value >> bit) & 1);
            }
            self.models.states[submodel] = byte_value;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        for _ in 0..4 {
            self.out.push((self.high >> 24) as u8);
            self.high <<= 8;
        }
        self.out
    }
}

struct Decoder<'a> {
    models: &'a mut Models,
    low: u32,
    high: u32,
    code: u32,
    input: &'a [u8],
    at: usize,
}

impl<'a> Decoder<'a> {
    fn new(models: &'a mut Models, input: &'a [u8]) -> Result<Self> {
        models.reset();
        let mut decoder = Self {
            models,
            low: 0,
            high: u32::MAX,
            code: 0,
            input,
            at: 0,
        };
        for _ in 0..4 {
            decoder.code = (decoder.code << 8) | decoder.next()?;
        }
        Ok(decoder)
    }

    /// A stream that asks for a byte it does not have is truncated, and
    /// feeding it a zero would decode plausible pulses out of a file
    /// that ended.
    fn next(&mut self) -> Result<u32> {
        let byte = self.input.get(self.at).copied().ok_or_else(|| {
            refuse(
                "the range-encoded data ends before the pulses the chunk counts, so \
                 the chunk is shorter than what it says it holds",
            )
        })?;
        self.at += 1;
        Ok(u32::from(byte))
    }

    fn bit(&mut self, at: usize) -> Result<u32> {
        let probability = u32::from(self.models.probabilities[at]);
        let middle = self.low + (((self.high - self.low) >> 12) * probability);
        let value = if self.code <= middle {
            self.models.probabilities[at] =
                (probability + ((PROBABILITY_CEILING - probability) >> PROBABILITY_SHIFT)) as u16;
            self.high = middle;
            1
        } else {
            self.models.probabilities[at] =
                (probability - (probability >> PROBABILITY_SHIFT)) as u16;
            self.low = middle + 1;
            0
        };
        while (self.low ^ self.high) & 0xff00_0000 == 0 {
            self.low <<= 8;
            self.high = (self.high << 8) | 0xff;
            self.code = (self.code << 8) | self.next()?;
        }
        Ok(value)
    }

    fn flag(&mut self, model: usize) -> Result<u32> {
        let at = Models::offset(model) + self.models.states[model] as usize;
        let value = self.bit(at)?;
        self.models.states[model] = value;
        Ok(value)
    }

    fn dword(&mut self, model: usize) -> Result<u32> {
        let mut value = 0u32;
        for byte_index in 0..4u32 {
            let submodel = model + byte_index as usize;
            let base = Models::offset(submodel);
            let mut context = 1u32;
            for _ in 0..8 {
                let at = base + (((self.models.states[submodel] << 8) | context) & 0xffff) as usize;
                context = (context << 1) | self.bit(at)?;
            }
            let byte_value = context & 0xff;
            self.models.states[submodel] = byte_value;
            value |= byte_value << (byte_index * 8);
        }
        Ok(value)
    }
}

/// One stored pulse: an angle on the circle, and how reliably it reads,
/// both in the container's own widths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StoredPulse {
    position: u32,
    strength: u32,
}

/// Range-encodes one half-track's pulses.
///
/// Position and strength are each delta-coded against the last one
/// written, and a delta equal to the last delta is a flag rather than a
/// value. A coded position delta of zero closes the stream, which is why
/// the trailer below is a flag and a zero rather than a length.
fn encode_pulses(models: &mut Models, pulses: &[StoredPulse]) -> Vec<u8> {
    let mut encoder = Encoder::new(models);
    let (mut last_position, mut previous_delta, mut last_strength) = (0u32, 0u32, 0u32);
    for pulse in pulses {
        let delta = pulse.position.wrapping_sub(last_position);
        if delta == previous_delta {
            encoder.flag(MODEL_POSITION_FLAG, 0);
        } else {
            previous_delta = delta;
            encoder.flag(MODEL_POSITION_FLAG, 1);
            encoder.dword(MODEL_POSITION, delta);
        }
        last_position = pulse.position;

        if pulse.strength == last_strength {
            encoder.flag(MODEL_STRENGTH_FLAG, 0);
        } else {
            encoder.flag(MODEL_STRENGTH_FLAG, 1);
            encoder.dword(MODEL_STRENGTH, pulse.strength.wrapping_sub(last_strength));
        }
        last_strength = pulse.strength;
    }
    encoder.flag(MODEL_POSITION_FLAG, 1);
    encoder.dword(MODEL_POSITION, 0);
    encoder.finish()
}

/// Range-decodes one half-track's pulses, and holds the container to
/// what it said: as many pulses as it counted, every one an angle on the
/// circle, every one past the last.
fn decode_pulses(models: &mut Models, data: &[u8], count: u64) -> Result<Vec<StoredPulse>> {
    let mut decoder = Decoder::new(models, data)?;
    let mut pulses: Vec<StoredPulse> = Vec::with_capacity(count.min(1 << 16) as usize);
    let (mut last_position, mut previous_delta, mut last_strength) = (0u32, 0u32, 0u32);
    loop {
        if decoder.flag(MODEL_POSITION_FLAG)? == 1 {
            let delta = decoder.dword(MODEL_POSITION)?;
            if delta == 0 {
                break;
            }
            previous_delta = delta;
        }
        if pulses.len() as u64 == count {
            return Err(refuse(format!(
                "the chunk counts {count} pulses and its data holds more, so the \
                 count and the stream are not describing the same half-track"
            )));
        }
        let position = last_position.checked_add(previous_delta).ok_or_else(|| {
            refuse("a pulse position accumulates past what one rotation can address")
        })?;
        if u64::from(position) >= PULSE_SAMPLES_PER_ROTATION {
            return Err(refuse(format!(
                "a pulse sits at cycle {position}, which is not below the \
                 {PULSE_SAMPLES_PER_ROTATION} cycles of one rotation, so it is not an \
                 angle on this circle"
            )));
        }
        if let Some(previous) = pulses.last()
            && position <= previous.position
        {
            return Err(refuse(format!(
                "a pulse at cycle {position} does not advance past the preceding cycle \
                 {}, and a circular pulse stream that doubles back is not one",
                previous.position
            )));
        }
        last_position = position;

        if decoder.flag(MODEL_STRENGTH_FLAG)? == 1 {
            last_strength = last_strength.wrapping_add(decoder.dword(MODEL_STRENGTH)?);
        }
        pulses.push(StoredPulse {
            position,
            strength: last_strength,
        });
    }
    if pulses.len() as u64 != count {
        return Err(refuse(format!(
            "the chunk counts {count} pulses and its data ends after {}",
            pulses.len()
        )));
    }
    Ok(pulses)
}

// ------------------------------------------------- addressing and strength

/// The container's half-track index byte for one location the family
/// addresses.
///
/// Track 18 is half track 36, so the index is twice the family's
/// position — exactly, which is what makes a half-step a real address
/// here rather than a rounding.
fn index_byte(key: &LocationKey) -> Result<u8> {
    let side = key.surface().ok_or_else(|| {
        refuse(
            "a location the medium addresses to no surface cannot be written: the \
             container's index byte carries a side, and there is no spelling for \
             the absence of one",
        )
    })?;
    if side > 1 {
        return Err(refuse(format!(
            "the medium addresses side {side} and the container's index byte holds \
             one bit of side, so it spells sides 0 and 1 and no others"
        )));
    }
    let (numerator, denominator) = key.position();
    let doubled = numerator.checked_mul(2).ok_or_else(|| {
        refuse("a half-track position runs past what exact arithmetic can hold here")
    })?;
    if denominator == 0 || doubled % denominator != 0 {
        return Err(refuse(format!(
            "the medium addresses {numerator}/{denominator} of a step, and the \
             container addresses half-steps, so there is no index byte that is that \
             position rather than near it"
        )));
    }
    let index = doubled / denominator;
    if index > u64::from(!SIDE_BIT) {
        return Err(refuse(format!(
            "half-track {index} is past the {} the container's index byte addresses",
            !SIDE_BIT
        )));
    }
    Ok(if side == 1 {
        index as u8 | SIDE_BIT
    } else {
        index as u8
    })
}

/// The family location one index byte addresses.
fn location_of(index: u8, profile: &'static str) -> Result<LocationKey> {
    let side = u64::from(index & SIDE_BIT != 0);
    LocationKey::fraction(profile, u64::from(index & !SIDE_BIT), 2, Some(side))
}

/// The container value for one of the family's declared strength states.
///
/// The three states the P30 profile declares and the three values the
/// format description names are the same three facts, so this is a
/// translation and not a scaling. A state outside the family's
/// vocabulary is refused rather than placed somewhere plausible on the
/// container's finer scale.
fn stored_strength(strength: Strength) -> Result<u32> {
    match strength.state() {
        STATE_STRONG => Ok(STRENGTH_ALWAYS),
        STATE_WEAK => Ok(STRENGTH_WEAK),
        STATE_ABSENT => Ok(STRENGTH_NEVER),
        other => Err(refuse(format!(
            "a pulse carries the strength state {other}, and the family declares {:?}; \
             the container's scale is finer than that vocabulary and this adapter will \
             not choose a point on it that the medium did not state",
            C1541.materialization.strength_states
        ))),
    }
}

/// Which of the family's states one stored value is. Everything strictly
/// between never and always is a pulse that sometimes triggers, which
/// the family states as weak and no more finely.
fn strength_state(stored: u32) -> u32 {
    match stored {
        STRENGTH_ALWAYS => STATE_STRONG,
        STRENGTH_NEVER => STATE_ABSENT,
        _ => STATE_WEAK,
    }
}

// -------------------------------------------------------- what is reported

/// One half-track a P64 holds, in the container's addressing and the
/// family's both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P64HalfTrack {
    /// The container's own index byte, side bit included.
    pub index: u8,
    pub side: u64,
    /// The family half-track it addresses, as an exact ratio.
    pub half_track_numerator: u64,
    pub half_track_denominator: u64,
    pub pulses: u64,
    /// Pulses that always trigger, that sometimes do, and that never do.
    pub strong_pulses: u64,
    pub weak_pulses: u64,
    pub absent_pulses: u64,
}

/// A P64 container as this adapter reads or writes one.
///
/// The same report serves both directions, because the question is the
/// same in both: what does the container hold, and what does the
/// crossing not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P64Report {
    pub format_id: String,
    pub format_name: String,
    /// The container's declared format version (P8).
    pub version: u32,
    pub write_protected: bool,
    pub double_sided: bool,
    /// The drive profile the container's own signature names, and the
    /// frame that profile declares.
    pub profile_id: String,
    pub reference_clock_hz: u64,
    pub cycles_per_rotation: u64,
    pub half_tracks: Vec<P64HalfTrack>,
    /// What the crossing does not carry, in the source's own terms — on
    /// a read what the container never held, on a write what it cannot
    /// express. A count is not an account, so each entry says what it
    /// was.
    pub declared_loss: Vec<DeclaredLoss>,
    /// How the container was recognized and what this adapter claims of
    /// it (P4).
    pub evidence: Vec<String>,
}

// -------------------------------------------------------------- decoding

/// A chunk the first pass found, and where its data sits.
#[derive(Debug, Clone, Copy)]
struct HalfTrackChunk {
    index: u8,
    at: u64,
    bytes: u64,
}

/// The chunk stream, read once end to end: the header's checksum covers
/// all of it, so it is streamed for that even where a chunk's own bytes
/// are read again later.
struct ChunkStream<'a> {
    file: &'a File,
    base: u64,
    length: u64,
    at: u64,
    crc: Crc32,
}

impl ChunkStream<'_> {
    fn take(&mut self, count: u64) -> Result<Vec<u8>> {
        if self.at + count > self.length {
            return Err(refuse(
                "a chunk runs past the end of the chunk stream the header sizes, so \
                 the file is not whole",
            ));
        }
        let mut buffer = vec![0u8; count as usize];
        device::read_exact_at(self.file, self.base + self.at, &mut buffer)
            .map_err(|error| Error::io(format!("failed to read a P64 chunk: {error}")))?;
        self.crc.update(&buffer);
        self.at += count;
        Ok(buffer)
    }

    /// Streams `count` bytes past, checksumming them into the stream's
    /// running total and returning their own checksum, so a chunk of any
    /// size is verified within a bounded buffer (P27).
    fn consume(&mut self, count: u64) -> Result<u32> {
        if self.at + count > self.length {
            return Err(refuse(
                "a chunk's data runs past the end of the chunk stream the header \
                 sizes, so the file is not whole",
            ));
        }
        let mut chunk = Crc32::new();
        let mut buffer = vec![0u8; STREAM_BLOCK.min(count.max(1) as usize)];
        let mut remaining = count;
        while remaining > 0 {
            let take = remaining.min(buffer.len() as u64) as usize;
            let block = &mut buffer[..take];
            device::read_exact_at(self.file, self.base + self.at, block)
                .map_err(|error| Error::io(format!("failed to read a P64 chunk: {error}")))?;
            self.crc.update(block);
            chunk.update(block);
            self.at += take as u64;
            remaining -= take as u64;
        }
        Ok(chunk.finish())
    }
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The validated container header, before a single chunk is looked at.
struct Header {
    version: u32,
    flags: u32,
    stream_bytes: u64,
    checksum: u32,
}

/// Recognizes and validates the header, in the order P8 requires: the
/// signature says whose file this is, the version says whether this
/// release has any reading of it, and only then is anything else
/// touched.
/// Whether `prefix` opens with the eight bytes every P64 opens with.
///
/// The storage-device tier asks this before accepting a medium: block and
/// flux are disjoint families (P13), so a flux container must not be
/// admitted to a block device and silently read as raw.
pub(crate) fn has_signature(prefix: &[u8]) -> bool {
    prefix.len() >= SIGNATURE.len() && prefix[..SIGNATURE.len()] == SIGNATURE
}

fn read_header(file: &File, length: u64, path: &Path) -> Result<Header> {
    if length < HEADER_BYTES {
        return Err(refuse(format!(
            "'{}' holds {length} bytes, fewer than the {HEADER_BYTES} a P64 header \
             occupies",
            path.display()
        )));
    }
    let mut header = [0u8; HEADER_BYTES as usize];
    device::read_exact_at(file, 0, &mut header)
        .map_err(|error| Error::io(format!("failed to read the P64 header: {error}")))?;
    if header[..8] != SIGNATURE {
        return Err(refuse(format!(
            "'{}' does not open with the eight bytes '{}' every P64 opens with",
            path.display(),
            String::from_utf8_lossy(&SIGNATURE)
        )));
    }

    let version = read_u32(&header, 8);
    if version != SUPPORTED_VERSION {
        return Err(unsupported(format!(
            "the file declares P64 format version {version:#010x}, and this release \
             claims version {SUPPORTED_VERSION:#010x} only; refusing to touch it"
        )));
    }

    let flags = read_u32(&header, 12);
    if flags & !CLAIMED_FLAGS != 0 {
        return Err(unsupported(format!(
            "the file sets flag bits {:#010x}, which are reserved in the version this \
             release claims; what they mean is not something this build can say",
            flags & !CLAIMED_FLAGS
        )));
    }

    let stream_bytes = u64::from(read_u32(&header, 16));
    if HEADER_BYTES + stream_bytes != length {
        return Err(refuse(format!(
            "the header sizes its chunk stream at {stream_bytes} bytes, which with the \
             {HEADER_BYTES}-byte header accounts for {} of the file's {length}",
            HEADER_BYTES + stream_bytes
        )));
    }
    Ok(Header {
        version,
        flags,
        stream_bytes,
        checksum: read_u32(&header, 20),
    })
}

/// Walks the chunk stream once: every chunk's signature is one this
/// adapter claims, every chunk's checksum is its own, the stream ends
/// with the chunk that says it ends, and the whole of it checksums to
/// what the header said.
fn walk_chunks(file: &File, header: &Header) -> Result<Vec<HalfTrackChunk>> {
    let mut stream = ChunkStream {
        file,
        base: HEADER_BYTES,
        length: header.stream_bytes,
        at: 0,
        crc: Crc32::new(),
    };
    let mut found: Vec<HalfTrackChunk> = Vec::new();
    let mut ended = false;
    while !ended {
        let head = stream.take(CHUNK_HEADER_BYTES)?;
        let signature: [u8; 4] = [head[0], head[1], head[2], head[3]];
        let size = u64::from(read_u32(&head, 4));
        let stored = read_u32(&head, 8);

        if signature == END_CHUNK {
            if size != 0 {
                return Err(refuse(format!(
                    "the chunk that says the file ends carries {size} bytes, and it is \
                     the empty chunk that carries none"
                )));
            }
            if stored != Crc32::new().finish() {
                return Err(refuse(
                    "the chunk that says the file ends does not carry the checksum of \
                     no data",
                ));
            }
            ended = true;
            continue;
        }
        if signature[..3] != HALF_TRACK_CHUNK {
            return Err(unsupported(format!(
                "the file holds a '{}' chunk, and this release claims the half-track \
                 chunk 'HTPx' and the end chunk 'DONE' and no others",
                String::from_utf8_lossy(&signature)
            )));
        }
        if size > CHUNK_BOUND {
            return Err(refuse(format!(
                "half-track {} carries {size} bytes, past the {CHUNK_BOUND} this \
                 release reads into memory for one half-track",
                signature[3]
            )));
        }
        if found.iter().any(|chunk| chunk.index == signature[3]) {
            return Err(refuse(format!(
                "the file holds two chunks for half-track {}, and which of them the \
                 medium is at that address is not something the container says",
                signature[3]
            )));
        }
        let at = HEADER_BYTES + stream.at;
        let computed = stream.consume(size)?;
        if computed != stored {
            return Err(refuse(format!(
                "half-track {} checksums to {computed:#010x} where the file stores \
                 {stored:#010x}, so the chunk did not arrive as it was written",
                signature[3]
            )));
        }
        found.push(HalfTrackChunk {
            index: signature[3],
            at,
            bytes: size,
        });
    }
    if stream.at != stream.length {
        return Err(refuse(format!(
            "the file holds {} bytes past the chunk that says it ends",
            stream.length - stream.at
        )));
    }
    let computed = stream.crc.finish();
    if computed != header.checksum {
        return Err(refuse(format!(
            "the chunk stream checksums to {computed:#010x} where the header stores \
             {:#010x}, so the file did not arrive as it was written",
            header.checksum
        )));
    }
    if found.is_empty() {
        return Err(refuse(
            "the file holds no half-track at all, and a container with no half-track \
             holds no medium",
        ));
    }
    Ok(found)
}

/// The frame the container declares, checked against the family's own
/// declaration of the same two facts.
///
/// Both are stated independently — one by the format description, one by
/// the P30 profile — so they are compared rather than one being taken
/// for the other.
fn container_frame(origin: Provenance) -> Result<RotationalFrame> {
    if C1541.rotation.reference_clock != REFERENCE_CLOCK_HZ
        || C1541.rotation.cycles_per_rotation != PULSE_SAMPLES_PER_ROTATION
    {
        return Err(refuse(format!(
            "the container declares a {REFERENCE_CLOCK_HZ} Hz reference across \
             {PULSE_SAMPLES_PER_ROTATION} cycles of one rotation and the {} profile \
             declares {} across {}; the two describe the same drive and disagree",
            C1541.id, C1541.rotation.reference_clock, C1541.rotation.cycles_per_rotation
        )));
    }
    RotationalFrame::new(
        P64,
        TimeBase::new(P64, REFERENCE_CLOCK_HZ, 1)?,
        PULSE_SAMPLES_PER_ROTATION,
        OriginStatement::new(OriginRule::Declared, origin),
    )
}

/// One P64 container, opened and read.
///
/// Opening claims the file (P7) — writes denied to every other process —
/// decodes every half-track once into private session storage, and holds
/// the claim until the image is dropped.
pub struct P64Image {
    path: PathBuf,
    /// Held for the image's whole life: the claim is not released and
    /// retaken between reads.
    _claimed: File,
    mode: AccessMode,
    medium: FluxMedium,
    report: P64Report,
}

impl std::fmt::Debug for P64Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("P64Image")
            .field("path", &self.path)
            .field("half_tracks", &self.report.half_tracks.len())
            .field("backing_bytes", &self.medium.backing_bytes())
            .finish()
    }
}

impl P64Image {
    /// Opens the P64 image at `path` with the stated default session
    /// cache bound ([`crate::DEFAULT_CACHE_BYTES`]).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_cache(path, crate::DEFAULT_CACHE_BYTES)
    }

    /// Opens the image under a caller-declared cache bound (P27): at
    /// most `cache_bytes` of the decoded medium stays resident. The
    /// bound narrows the working set; it never refuses service.
    pub fn open_with_cache(path: impl AsRef<Path>, cache_bytes: u64) -> Result<Self> {
        let path = path.as_ref();
        let claimed = device::open_declared(path, AccessIntent::Read)?;
        let length = claimed
            .metadata()
            .map_err(|error| Error::io(format!("cannot size '{}': {error}", path.display())))?
            .len();

        let header = read_header(&claimed, length, path)?;
        let chunks = walk_chunks(&claimed, &header)?;

        let mut evidence = vec![
            format!(
                "'{}' opens with the P64 signature and declares format version \
                 {:#010x}, the version this release claims",
                path.display(),
                header.version
            ),
            format!(
                "{} half-track chunks and the end chunk account for every byte of the \
                 {}-byte chunk stream, each checksummed against its own stored value \
                 and all of them against the header's",
                chunks.len(),
                header.stream_bytes
            ),
        ];
        if header.flags & FLAG_WRITE_PROTECT != 0 {
            evidence.push("the container declares the medium write-protected".to_owned());
        }
        evidence.push(format!(
            "positions read as angles on a {PULSE_SAMPLES_PER_ROTATION}-cycle circle \
             of the {REFERENCE_CLOCK_HZ} Hz reference the container declares, which is \
             what the {} profile declares for the same drive",
            C1541.id
        ));
        evidence.push(format!(
            "strength read in the family's own vocabulary: {:#010x} is strong, \
             {:#010x} is absent, and every value between is a pulse that sometimes \
             triggers, which the family states as weak",
            STRENGTH_ALWAYS, STRENGTH_NEVER
        ));

        // The container addresses side 0 before side 1 and the medium
        // addresses by position, so the chunks are put in the medium's
        // order before any of them is decoded.
        let mut located: Vec<(LocationKey, HalfTrackChunk)> = chunks
            .into_iter()
            .map(|chunk| Ok((location_of(chunk.index, C1541.id)?, chunk)))
            .collect::<Result<Vec<_>>>()?;
        located.sort_by(|left, right| left.0.cmp(&right.0));

        let mut builder = MediumBuilder::new(
            C1541.id,
            container_frame(Provenance::new(P64).note(
                "the container places every pulse on a circle whose start it does not \
                 state, so cycle zero is the container's own datum rather than a seam \
                 or an index any evidence located",
            ))?,
            Derivation::Stored,
            Provenance::new(P64)
                .note(format!(
                    "decoded from the P64 container '{}', which holds a medium at rest",
                    path.display()
                ))
                .note(
                    "the container records no policy for the content it holds, and \
                     this library derived none of it: these pulses are what was \
                     stored, not an observation of a recording",
                ),
            SessionBacking::create()?,
        )?;

        let mut models = Models::new();
        let mut loss = LossAccount::new();
        let mut half_tracks = Vec::with_capacity(located.len());
        for (key, chunk) in located {
            let stored = read_half_track(&claimed, &mut models, chunk)?;
            let (numerator, denominator) = key.position();
            let mut counts = [0u64; 3];
            let mut pulses = Vec::with_capacity(stored.len());
            for pulse in &stored {
                let state = strength_state(pulse.strength);
                counts[state as usize] += 1;
                if state == STATE_WEAK && pulse.strength != STRENGTH_WEAK {
                    loss.add(
                        "strength-resolution",
                        "pulses stored at a trigger value between never and always \
                         that the family's vocabulary states only as weak",
                        1,
                    );
                }
                pulses.push(Pulse::new(
                    Cycle::from(pulse.position),
                    Strength::certain(state),
                ));
            }
            half_tracks.push(P64HalfTrack {
                index: chunk.index,
                side: key.surface().unwrap_or(0),
                half_track_numerator: numerator,
                half_track_denominator: denominator,
                pulses: stored.len() as u64,
                strong_pulses: counts[STATE_STRONG as usize],
                weak_pulses: counts[STATE_WEAK as usize],
                absent_pulses: counts[STATE_ABSENT as usize],
            });
            builder.add_location(
                key,
                &pulses,
                &[],
                Provenance::new(P64).note(format!(
                    "stored in the container's half-track chunk {}",
                    chunk.index
                )),
            )?;
        }

        loss.add(
            "container-provenance",
            "the reduction that produced this medium, which the container records \
             nothing of; what is here is what was stored",
            half_tracks.len() as u64,
        );
        loss.add(
            "located-origin",
            "the rule that placed the circle's start, which the container does not \
             state: its cycle zero is a datum rather than a seam or an index",
            1,
        );

        let (mut medium, sink, total) = builder.seal()?;
        medium.attach_backing(Box::new(sink.into_source()), total, cache_bytes);
        evidence.push(format!(
            "every half-track decoded once into a {total}-byte section backing in \
             private session storage, addressed by half-track"
        ));

        let report = P64Report {
            format_id: P64.to_owned(),
            format_name: P64_NAME.to_owned(),
            version: header.version,
            write_protected: header.flags & FLAG_WRITE_PROTECT != 0,
            double_sided: header.flags & FLAG_DOUBLE_SIDED != 0,
            profile_id: C1541.id.to_owned(),
            reference_clock_hz: REFERENCE_CLOCK_HZ,
            cycles_per_rotation: PULSE_SAMPLES_PER_ROTATION,
            half_tracks,
            declared_loss: loss.into_entries(),
            evidence,
        };

        Ok(Self {
            path: path.to_path_buf(),
            _claimed: claimed,
            mode: AccessMode::ReadOnly,
            medium,
            report,
        })
    }

    /// The path the image was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The container format's stable identifier.
    pub fn format_id(&self) -> &'static str {
        P64
    }

    pub fn format_name(&self) -> &'static str {
        P64_NAME
    }

    /// Which P7 mode the open obtained on the file.
    pub fn access_mode(&self) -> AccessMode {
        self.mode
    }

    /// The container as the adapter read it.
    pub fn inspect(&self) -> &P64Report {
        &self.report
    }

    /// How many bytes of private session storage the decoded medium
    /// occupies, and how much of that is currently resident. The medium
    /// is never held whole (P27); this is what says so.
    pub fn backing_bytes(&self) -> u64 {
        self.medium.backing_bytes()
    }

    pub fn resident_bytes(&self) -> u64 {
        self.medium.resident_bytes()
    }

    /// The decoded medium itself. The pulses stay behind the public
    /// surface: the presentation above the medium reads them, and
    /// nothing outside the crate does.
    pub(crate) fn medium(&self) -> &FluxMedium {
        &self.medium
    }
}

/// Reads and decodes one half-track chunk's data.
fn read_half_track(
    file: &File,
    models: &mut Models,
    chunk: HalfTrackChunk,
) -> Result<Vec<StoredPulse>> {
    if chunk.bytes < 8 {
        return Err(refuse(format!(
            "half-track {} carries {} bytes, fewer than the eight its pulse count and \
             data size occupy",
            chunk.index, chunk.bytes
        )));
    }
    let mut data = vec![0u8; chunk.bytes as usize];
    device::read_exact_at(file, chunk.at, &mut data)
        .map_err(|error| Error::io(format!("failed to read a P64 half-track: {error}")))?;
    let count = u64::from(read_u32(&data, 0));
    let coded = u64::from(read_u32(&data, 4));
    if count > PULSE_SAMPLES_PER_ROTATION {
        return Err(refuse(format!(
            "half-track {} counts {count} pulses, more than the \
             {PULSE_SAMPLES_PER_ROTATION} cycles one rotation holds, and no two \
             pulses share a cycle",
            chunk.index
        )));
    }
    if 8 + coded != chunk.bytes {
        return Err(refuse(format!(
            "half-track {} sizes its range-encoded data at {coded} bytes, which with \
             its eight-byte prologue accounts for {} of the chunk's {}",
            chunk.index,
            8 + coded,
            chunk.bytes
        )));
    }
    decode_pulses(models, &data[8..], count)
}

// -------------------------------------------------------------- encoding

/// What the container will and will not carry of `medium`, computed and
/// written nowhere.
fn describe(medium: &FluxMedium, destination: Option<&Path>) -> Result<P64Report> {
    if medium.profile() != C1541.id {
        return Err(refuse(format!(
            "the medium is addressed by the '{}' profile and this container is the \
             1541's; a P64 is not a container for another family's medium",
            medium.profile()
        )));
    }
    let frame = medium.frame();
    let (clock, per_clock) = frame.reference_clock().ticks_per_second();
    if clock != REFERENCE_CLOCK_HZ
        || per_clock != 1
        || frame.cycles_per_rotation() != PULSE_SAMPLES_PER_ROTATION
    {
        return Err(refuse(format!(
            "the medium is expressed against {clock}/{per_clock} Hz across {} cycles \
             of one rotation, and the container expresses {REFERENCE_CLOCK_HZ} Hz \
             across {PULSE_SAMPLES_PER_ROTATION}; re-projecting it is a reduction, and \
             reductions belong to the mastering profile rather than here",
            frame.cycles_per_rotation()
        )));
    }

    let mut loss = LossAccount::new();
    let mut half_tracks = Vec::new();
    let mut double_sided = false;
    let mut write_protected = false;
    for location in medium.locations() {
        let key = location.key();
        let index = index_byte(key)?;
        let side = key.surface().unwrap_or(0);
        double_sided |= side == 1;

        let mut counts = [0u64; 3];
        let mut seeded = 0u64;
        for pulse in medium.pulses(location)? {
            let state = pulse.strength().state();
            // Refuses here rather than at the write, so the account is
            // complete before anything is created.
            stored_strength(pulse.strength())?;
            counts[state as usize] += 1;
            if pulse.strength().seed_domain().is_some() {
                seeded += 1;
            }
        }
        if seeded > 0 {
            loss.add(
                "strength-seed",
                "pulses whose strength names the domain that makes its variation \
                 reproducible, which the container has no field for",
                seeded,
            );
        }

        let facts = medium.facts(location)?;
        let mut unexpressed = 0u64;
        for fact in &facts {
            match fact.kind() {
                MediumFactKind::WriteProtect => write_protected = true,
                _ => unexpressed += 1,
            }
        }
        if unexpressed > 0 {
            loss.add(
                "medium-fact",
                "facts the medium states about a location — where its seam sits, \
                 whether its content matched a neighbour's — which the container \
                 carries no field for",
                unexpressed,
            );
        }
        if !location.provenance().notes.is_empty() {
            loss.add(
                "location-provenance",
                "how each half-track's content came to be known, which the container \
                 records nothing of",
                location.provenance().notes.len() as u64,
            );
        }

        let (numerator, denominator) = key.position();
        half_tracks.push(P64HalfTrack {
            index,
            side,
            half_track_numerator: numerator,
            half_track_denominator: denominator,
            pulses: location.pulses(),
            strong_pulses: counts[STATE_STRONG as usize],
            weak_pulses: counts[STATE_WEAK as usize],
            absent_pulses: counts[STATE_ABSENT as usize],
        });
    }
    if half_tracks.is_empty() {
        return Err(refuse(
            "the medium claims no location, and a container with no half-track holds \
             no medium",
        ));
    }

    if !medium.provenance().notes.is_empty() {
        loss.add(
            "reduction-policy",
            "the declared policy the medium carries, every input of it; the container \
             records no policy and a P64 says nothing about how it came to be",
            medium.provenance().notes.len() as u64,
        );
    }
    loss.add(
        "located-origin",
        "the rule that placed the circle's start and the evidence behind it; the \
         container's cycle zero is a datum and states nothing about what put it there",
        1,
    );
    loss.add(
        "derivation",
        "the medium's statement that it was derived rather than recorded; a container \
         that holds it makes no such statement, and a reader of the file learns \
         nothing about it",
        1,
    );

    let mut evidence = vec![
        format!(
            "writing P64 format version {SUPPORTED_VERSION:#010x}, the version this \
             release claims"
        ),
        format!(
            "{} half-tracks addressed by the container's index byte, twice the \
             family's position, with bit 7 the side",
            half_tracks.len()
        ),
        format!(
            "positions written as angles on the {PULSE_SAMPLES_PER_ROTATION}-cycle \
             circle of the container's {REFERENCE_CLOCK_HZ} Hz reference, which is the \
             frame the medium is already expressed in; nothing is re-projected"
        ),
        format!(
            "strength written in the container's own three named values: strong as \
             {STRENGTH_ALWAYS:#010x}, weak as {STRENGTH_WEAK:#010x}, absent as \
             {STRENGTH_NEVER:#010x}; a state outside the family's vocabulary is \
             refused rather than placed on the container's finer scale"
        ),
        "every chunk carries its own checksum and the header carries the chunk \
         stream's, so the file states its own integrity"
            .to_owned(),
    ];
    if let Some(destination) = destination {
        evidence.push(format!(
            "written to '{}', which is created under this library's own claim and \
             never an overwrite of something already there",
            destination.display()
        ));
    }

    Ok(P64Report {
        format_id: P64.to_owned(),
        format_name: P64_NAME.to_owned(),
        version: SUPPORTED_VERSION,
        write_protected,
        double_sided,
        profile_id: C1541.id.to_owned(),
        reference_clock_hz: REFERENCE_CLOCK_HZ,
        cycles_per_rotation: PULSE_SAMPLES_PER_ROTATION,
        half_tracks,
        declared_loss: loss.into_entries(),
        evidence,
    })
}

/// Writes `medium` into a new P64 at `path`.
///
/// Every check has passed before a byte is written (P6), the artifact is
/// built beside its destination and moved into place whole, so an
/// interruption leaves the destination absent rather than half a file
/// (P9), and a destination that already exists is a named refusal rather
/// than an overwrite (P7).
fn write_new_artifact(medium: &FluxMedium, path: &Path) -> Result<P64Report> {
    let report = describe(medium, Some(path))?;
    if path.try_exists().unwrap_or(false) {
        return Err(Error::io(format!(
            "cannot write '{}': something is already there, and a destination this \
             library did not create is never overwritten",
            path.display()
        )));
    }

    let staging = staging_path(path);
    let file = device::create_claimed(&staging)?;
    // The bytes are on the medium before the name exists, so what the
    // destination names is either the whole artifact or nothing.
    let built = stream_artifact(medium, &report, &file).and_then(|()| {
        file.sync_all().map_err(|error| {
            Error::io(format!(
                "cannot commit '{}' to storage: {error}",
                staging.display()
            ))
        })
    });
    drop(file);
    if let Err(error) = built {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    std::fs::rename(&staging, path).map_err(|error| {
        let _ = std::fs::remove_file(&staging);
        Error::io(format!(
            "cannot put the written artifact in place at '{}': {error}",
            path.display()
        ))
    })?;
    Ok(report)
}

/// Where the artifact is built: beside its destination, so moving it
/// into place is a rename within one filesystem rather than a copy.
fn staging_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .map_or_else(|| "artifact".to_owned(), |name| name.to_string_lossy().into_owned());
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    destination.with_file_name(format!(
        ".{name}.remanence-{}-{nonce}.part",
        std::process::id()
    ))
}

/// Streams the container out one half-track at a time, so what is held
/// at once is one chunk rather than one image (P27).
fn stream_artifact(medium: &FluxMedium, report: &P64Report, file: &File) -> Result<()> {
    // The header states the size and checksum of everything after it, so
    // its place is reserved and filled once the chunks are all written.
    let mut at = HEADER_BYTES;
    write_at(file, 0, &[0u8; HEADER_BYTES as usize])?;

    let mut crc = Crc32::new();
    let mut models = Models::new();
    let mut chunk = Vec::new();
    for location in medium.locations() {
        // Addressed from the location itself rather than from the
        // report beside it, so the bytes cannot drift from what was
        // claimed by the two being walked in step.
        let index = index_byte(location.key())?;
        let mut stored = Vec::with_capacity(location.pulses() as usize);
        for pulse in medium.pulses(location)? {
            stored.push(StoredPulse {
                position: u32::try_from(pulse.position()).map_err(|_| {
                    refuse("a pulse sits past what the container's position field holds")
                })?,
                strength: stored_strength(pulse.strength())?,
            });
        }
        let coded = encode_pulses(&mut models, &stored);

        let mut data = Vec::with_capacity(8 + coded.len());
        data.extend_from_slice(&(stored.len() as u32).to_le_bytes());
        data.extend_from_slice(&(coded.len() as u32).to_le_bytes());
        data.extend_from_slice(&coded);

        chunk.clear();
        chunk.extend_from_slice(&HALF_TRACK_CHUNK);
        chunk.push(index);
        chunk.extend_from_slice(&(data.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&crate::checksum::crc32(&data).to_le_bytes());
        chunk.extend_from_slice(&data);

        crc.update(&chunk);
        write_at(file, at, &chunk)?;
        at += chunk.len() as u64;
    }

    let mut end = Vec::with_capacity(CHUNK_HEADER_BYTES as usize);
    end.extend_from_slice(&END_CHUNK);
    end.extend_from_slice(&0u32.to_le_bytes());
    end.extend_from_slice(&Crc32::new().finish().to_le_bytes());
    crc.update(&end);
    write_at(file, at, &end)?;
    at += end.len() as u64;

    let stream_bytes = u32::try_from(at - HEADER_BYTES)
        .map_err(|_| refuse("the artifact runs past what the container's size field holds"))?;
    let mut header = Vec::with_capacity(HEADER_BYTES as usize);
    header.extend_from_slice(&SIGNATURE);
    header.extend_from_slice(&SUPPORTED_VERSION.to_le_bytes());
    let mut flags = 0u32;
    if report.write_protected {
        flags |= FLAG_WRITE_PROTECT;
    }
    if report.double_sided {
        flags |= FLAG_DOUBLE_SIDED;
    }
    header.extend_from_slice(&flags.to_le_bytes());
    header.extend_from_slice(&stream_bytes.to_le_bytes());
    header.extend_from_slice(&crc.finish().to_le_bytes());
    write_at(file, 0, &header)
}

fn write_at(file: &File, offset: u64, data: &[u8]) -> Result<()> {
    device::write_all_at(file, offset, data)
        .map_err(|error| Error::io(format!("failed to write the P64 artifact: {error}")))
}

impl MasteredMedium {
    /// What a P64 will and will not carry of this medium, computed and
    /// written nowhere.
    ///
    /// The account is complete before anything is created, so a caller
    /// reads what the crossing costs and then decides (P29). A medium
    /// the container's claim cannot encode is refused here rather than
    /// approximated into it.
    pub fn describe_p64(&self) -> Result<P64Report> {
        describe(self.medium(), None)
    }

    /// Writes this medium into a new P64 image at `path`, and reports
    /// what the container carried and what it did not.
    ///
    /// The medium is untouched. An existing destination is a named
    /// refusal rather than an overwrite, and an interruption leaves the
    /// destination absent rather than half an artifact (P6, P7, P9).
    pub fn write_p64(&self, path: impl AsRef<Path>) -> Result<P64Report> {
        write_new_artifact(self.medium(), path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flux_medium::MediumFact;

    fn medium_of(pulses: &[(Cycle, u32)]) -> FluxMedium {
        let frame = container_frame(Provenance::new(P64).note("declared for the test")).expect(
            "the container and the profile declare the same frame",
        );
        let mut builder = MediumBuilder::new(
            C1541.id,
            frame,
            Derivation::SelectedAndProjected,
            Provenance::new(C1541.id).note("mastered for the test"),
            Vec::new(),
        )
        .expect("the policy is stated");
        let built: Vec<Pulse> = pulses
            .iter()
            .map(|(position, state)| Pulse::new(*position, Strength::certain(*state)))
            .collect();
        builder
            .add_location(
                LocationKey::new(C1541.id, 18, 0),
                &built,
                &[],
                Provenance::new(C1541.id).note("reduced for the test"),
            )
            .expect("the location");
        builder
            .add_location(
                LocationKey::fraction(C1541.id, 37, 2, Some(0)).expect("a half-track"),
                &built,
                &[MediumFact::new(
                    MediumFactKind::Seam { angle: 1000 },
                    Provenance::new(C1541.id).note("located for the test"),
                )],
                Provenance::new(C1541.id).note("reduced for the test"),
            )
            .expect("the half-track");
        let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
        medium.attach_backing(Box::new(VecSource(bytes)), total, 1 << 20);
        medium
    }

    struct VecSource(Vec<u8>);

    impl crate::flux_capture::ByteSource for VecSource {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos());
        std::env::temp_dir().join(format!(
            "remanence-p64-{tag}-{}-{nonce}.p64",
            std::process::id()
        ))
    }

    #[test]
    fn the_range_coder_returns_what_it_was_given() {
        // The codec is the container's, written out here, so the first
        // thing to establish is that this writing of it is its own
        // inverse over the shapes the format actually produces: a
        // repeated delta, a changed one, a strength that holds and one
        // that moves.
        let pulses: Vec<StoredPulse> = [
            (0u32, STRENGTH_ALWAYS),
            (64, STRENGTH_ALWAYS),
            (128, STRENGTH_ALWAYS),
            (500, STRENGTH_WEAK),
            (501, STRENGTH_NEVER),
            (3_199_999, STRENGTH_ALWAYS),
        ]
        .into_iter()
        .map(|(position, strength)| StoredPulse { position, strength })
        .collect();
        let mut models = Models::new();

        let coded = encode_pulses(&mut models, &pulses);
        let decoded = decode_pulses(&mut models, &coded, pulses.len() as u64)
            .expect("the stream decodes");

        assert_eq!(decoded, pulses);
        // And it is worth something: six pulses in well under six times
        // eight bytes.
        assert!(coded.len() < 48, "{} bytes", coded.len());
    }

    #[test]
    fn an_empty_half_track_is_a_half_track_that_is_blank() {
        // A location the medium claims and states no pulse at is not the
        // same fact as a location it claims nothing at, and the
        // container carries the difference as a chunk with no pulses.
        let mut models = Models::new();
        let coded = encode_pulses(&mut models, &[]);
        assert_eq!(
            decode_pulses(&mut models, &coded, 0).expect("the stream decodes"),
            []
        );
    }

    #[test]
    fn a_truncated_pulse_stream_is_refused_rather_than_decoded_short() {
        let pulses: Vec<StoredPulse> = (0..64)
            .map(|index| StoredPulse {
                position: index * 97 + 1,
                strength: STRENGTH_ALWAYS,
            })
            .collect();
        let mut models = Models::new();
        let coded = encode_pulses(&mut models, &pulses);

        let error = decode_pulses(&mut models, &coded[..coded.len() - 4], pulses.len() as u64)
            .expect_err("a stream that ends early is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn the_index_byte_is_twice_the_family_position_and_carries_the_side() {
        // Track 18 is half track 36, and a half-step is a real address
        // rather than a rounding toward a whole one.
        let whole = LocationKey::new(C1541.id, 18, 0);
        let half = LocationKey::fraction(C1541.id, 37, 2, Some(0)).expect("a half-track");
        let far = LocationKey::new(C1541.id, 18, 1);

        assert_eq!(index_byte(&whole).expect("addressable"), 36);
        assert_eq!(index_byte(&half).expect("addressable"), 37);
        assert_eq!(index_byte(&far).expect("addressable"), 36 | SIDE_BIT);

        assert_eq!(location_of(36, C1541.id).expect("addressable"), whole);
        assert_eq!(location_of(37, C1541.id).expect("addressable"), half);
        assert_eq!(
            location_of(36 | SIDE_BIT, C1541.id).expect("addressable"),
            far
        );
    }

    #[test]
    fn a_position_the_container_cannot_address_is_refused_rather_than_rounded() {
        let third = LocationKey::fraction(C1541.id, 55, 3, Some(0)).expect("a third of a step");
        let error = index_byte(&third).expect_err("a third of a step has no index byte");
        assert!(error.to_string().contains("rather than near it"), "{error}");

        let past = LocationKey::new(C1541.id, 90, 0);
        let error = index_byte(&past).expect_err("half-track 180 is past the byte");
        assert!(error.to_string().contains("past the 127"), "{error}");
    }

    #[test]
    fn a_medium_encodes_and_reopens_as_the_same_half_tracks_and_pulses() {
        // The conformance claim, and it is a same-layer comparison
        // because both ends are a flux medium: what goes in comes back
        // addressed the same way, at the same angles, with the same
        // strengths.
        let source = medium_of(&[
            (0, STATE_STRONG),
            (64, STATE_STRONG),
            (65, STATE_WEAK),
            (2_000_000, STATE_ABSENT),
            (3_199_999, STATE_STRONG),
        ]);
        let path = temp_path("round-trip");

        let written = write_new_artifact(&source, &path).expect("the artifact is written");
        let reopened = P64Image::open(&path).expect("the artifact reopens");

        let expected: Vec<(u8, u64, u64, u64)> = written
            .half_tracks
            .iter()
            .map(|track| {
                (
                    track.index,
                    track.half_track_numerator,
                    track.half_track_denominator,
                    track.pulses,
                )
            })
            .collect();
        let found: Vec<(u8, u64, u64, u64)> = reopened
            .inspect()
            .half_tracks
            .iter()
            .map(|track| {
                (
                    track.index,
                    track.half_track_numerator,
                    track.half_track_denominator,
                    track.pulses,
                )
            })
            .collect();
        assert_eq!(found, expected);
        assert_eq!(found.len(), 2);

        for location in reopened.medium().locations() {
            let pulses = reopened
                .medium()
                .pulses(location)
                .expect("the pulses read back");
            assert_eq!(
                pulses
                    .iter()
                    .map(|pulse| (pulse.position(), pulse.strength().state()))
                    .collect::<Vec<_>>(),
                [
                    (0, STATE_STRONG),
                    (64, STATE_STRONG),
                    (65, STATE_WEAK),
                    (2_000_000, STATE_ABSENT),
                    (3_199_999, STATE_STRONG),
                ]
            );
        }
        // A medium read out of a container was not derived by this
        // library, and says so rather than borrowing a reduction's word
        // for it.
        assert_eq!(reopened.medium().derivation(), Derivation::Stored);
        assert_eq!(reopened.medium().derivation().as_str(), "stored");

        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_same_medium_writes_the_same_bytes() {
        // Encoding is deterministic, so reproducibility survives the
        // crossing: the same medium is the same file, byte for byte.
        let source = medium_of(&[(0, STATE_STRONG), (997, STATE_WEAK), (99_991, STATE_STRONG)]);
        let first = temp_path("deterministic-a");
        let second = temp_path("deterministic-b");

        write_new_artifact(&source, &first).expect("the first artifact");
        write_new_artifact(&source, &second).expect("the second artifact");

        assert_eq!(
            std::fs::read(&first).expect("the first reads"),
            std::fs::read(&second).expect("the second reads")
        );
        std::fs::remove_file(&first).ok();
        std::fs::remove_file(&second).ok();
    }

    #[test]
    fn an_existing_destination_is_refused_and_left_exactly_as_it_was() {
        let source = medium_of(&[(0, STATE_STRONG)]);
        let path = temp_path("occupied");
        std::fs::write(&path, b"not a P64, and not this library's to replace")
            .expect("the destination is occupied");

        let error = write_new_artifact(&source, &path).expect_err("an occupied path is refused");

        assert!(error.to_string().contains("already there"), "{error}");
        assert_eq!(
            std::fs::read(&path).expect("it is still there"),
            b"not a P64, and not this library's to replace"
        );
        // And nothing was left beside it either.
        assert!(
            !path.with_extension("part").exists(),
            "a staged artifact outlived the refusal"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_loss_is_declared_before_the_artifact_exists_and_in_the_mediums_terms() {
        let source = medium_of(&[(0, STATE_STRONG), (64, STATE_WEAK)]);

        let report = describe(&source, None).expect("the claim is computed");

        for loss in &report.declared_loss {
            assert!(!loss.code.is_empty());
            assert!(loss.detail.len() > 20, "{loss:?}");
            assert!(loss.count > 0, "{loss:?}");
        }
        let by_code = |code: &str| {
            report
                .declared_loss
                .iter()
                .find(|loss| loss.code == code)
                .unwrap_or_else(|| panic!("{code} is not accounted for: {:?}", report.declared_loss))
        };
        // The reduction's policy, each half-track's own provenance, the
        // seam the medium located, the rule that placed the circle's
        // start, and the medium's statement that it was derived at all.
        by_code("reduction-policy");
        by_code("location-provenance");
        assert_eq!(by_code("medium-fact").count, 1);
        by_code("located-origin");
        by_code("derivation");
        // The claim is stated beside the account rather than left to be
        // inferred from it.
        assert!(report.evidence.len() >= 5, "{:?}", report.evidence);
        assert!(
            report.evidence.iter().any(|line| line.contains("index byte")),
            "{:?}",
            report.evidence
        );
    }

    #[test]
    fn a_medium_the_claim_cannot_encode_is_refused_rather_than_approximated() {
        // A strength state outside the family's declared vocabulary has
        // no value on the container's scale that the medium asked for,
        // and choosing one would put an unevidenced claim in the file.
        let source = medium_of(&[(0, 7)]);

        let error = describe(&source, None).expect_err("an unspellable strength is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("will not choose"), "{error}");
    }

    #[test]
    fn a_file_that_is_not_a_p64_is_refused_by_what_it_is_not() {
        let path = temp_path("foreign");
        std::fs::write(&path, b"G64 or something else entirely, but not this")
            .expect("the file is written");

        let error = P64Image::open(&path).expect_err("a foreign file is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("P64-1541"), "{error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_version_past_the_claim_is_refused_before_anything_else_is_touched() {
        // P8: the version gate runs first, so a file whose structure is
        // nonsense past the header still fails on the version rather
        // than on the nonsense.
        let path = temp_path("future");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&SIGNATURE);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(&path, &bytes).expect("the file is written");

        let error = P64Image::open(&path).expect_err("a version past the claim is refused");

        assert_eq!(error.category(), ErrorCategory::Unsupported);
        assert!(error.to_string().contains("version 0x00000001"), "{error}");
        assert!(error.to_string().contains("0x00000000 only"), "{error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_reserved_flag_bit_is_a_feature_past_the_claim() {
        let path = temp_path("reserved");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&SIGNATURE);
        bytes.extend_from_slice(&SUPPORTED_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0b1000u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(&path, &bytes).expect("the file is written");

        let error = P64Image::open(&path).expect_err("a reserved bit is refused");

        assert_eq!(error.category(), ErrorCategory::Unsupported);
        assert!(error.to_string().contains("reserved"), "{error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_chunk_that_did_not_arrive_as_it_was_written_is_refused() {
        let source = medium_of(&[(0, STATE_STRONG), (64, STATE_STRONG), (128, STATE_WEAK)]);
        let path = temp_path("corrupt");
        write_new_artifact(&source, &path).expect("the artifact is written");

        let mut bytes = std::fs::read(&path).expect("it reads");
        // One bit inside the first half-track's range-encoded data.
        let at = HEADER_BYTES as usize + CHUNK_HEADER_BYTES as usize + 10;
        bytes[at] ^= 0x01;
        std::fs::write(&path, &bytes).expect("it rewrites");

        let error = P64Image::open(&path).expect_err("a changed chunk is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("did not arrive"), "{error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_chunk_vocabulary_past_the_claim_is_refused_by_name() {
        // The claim is 'HTPx' and 'DONE'. A chunk this release has no
        // reading of is a feature past the claim, not something to walk
        // past on the way to one it knows.
        let source = medium_of(&[(0, STATE_STRONG)]);
        let path = temp_path("unknown-chunk");
        write_new_artifact(&source, &path).expect("the artifact is written");

        let mut bytes = std::fs::read(&path).expect("it reads");
        bytes[HEADER_BYTES as usize..HEADER_BYTES as usize + 3].copy_from_slice(b"XYZ");
        // The stream checksum still has to match, or that is what fails.
        let stream = &bytes[HEADER_BYTES as usize..];
        let checksum = crate::checksum::crc32(stream).to_le_bytes();
        bytes[20..24].copy_from_slice(&checksum);
        std::fs::write(&path, &bytes).expect("it rewrites");

        let error = P64Image::open(&path).expect_err("an unclaimed chunk is refused");

        assert_eq!(error.category(), ErrorCategory::Unsupported);
        assert!(error.to_string().contains("and no others"), "{error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_decoded_medium_is_addressed_out_of_session_storage_rather_than_held() {
        let source = medium_of(&[(0, STATE_STRONG), (64, STATE_STRONG), (128, STATE_STRONG)]);
        let path = temp_path("bounded");
        write_new_artifact(&source, &path).expect("the artifact is written");

        let image = P64Image::open_with_cache(&path, 64).expect("the artifact reopens");
        for location in image.medium().locations() {
            image.medium().pulses(location).expect("the pulses read");
        }

        assert!(image.backing_bytes() > 0);
        assert!(image.resident_bytes() <= 64, "{}", image.resident_bytes());
        assert_eq!(image.access_mode(), AccessMode::ReadOnly);
        assert_eq!(image.format_id(), "p64");

        drop(image);
        std::fs::remove_file(&path).ok();
    }
}
