// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Self-contained LZMA and LZMA2 decoders — the compression methods the
//! 7z catalog claims beyond stored data — written from the published
//! LZMA format description (see AGENTS.md, "Prior art and provenance
//! notes"), so the library keeps its own decompressors and takes no
//! runtime dependency (P1).
//!
//! Decoding streams (P27). Coded bytes are pulled through a bounded
//! chunk, and decoded bytes flow to a [`DecodedSink`] through an LZ
//! window that holds only the back-reference horizon the stream's own
//! dictionary declares — clamped to what the stream can actually reach
//! and refused outright past [`WINDOW_BOUND`], so no archive dictates
//! an unbounded allocation. A sink sees the output in order, in flushed
//! runs, and may stop the decode as soon as it has the range it wanted:
//! one member of a solid archive is reached without materializing the
//! rest.

use crate::device::{ByteSource, SliceByteSource};
use crate::error::{Error, ErrorCategory, Result};

/// The largest LZ window this decoder will hold resident (P27). A
/// stream declaring a dictionary larger than this — and long enough to
/// reach past it — is a named refusal, never an allocation attempt.
pub(crate) const WINDOW_BOUND: u64 = 64 * 1024 * 1024;

/// How much decoded output accumulates past the window's horizon before
/// it is handed to the sink and dropped.
const FLUSH_CHUNK: usize = 1024 * 1024;

/// How often the decoder asks its sink whether the output ahead is
/// still wanted. The question is monotone, so asking per checkpoint
/// rather than per symbol changes nothing but the cost.
const STOP_CHECK_STEP: u64 = 4096;

const NUM_POS_BITS_MAX: usize = 4;
const NUM_STATES: usize = 12;
const NUM_LEN_TO_POS_STATES: usize = 4;
const NUM_ALIGN_BITS: usize = 4;
const END_POS_MODEL_INDEX: usize = 14;
const NUM_FULL_DISTANCES: usize = 1 << (END_POS_MODEL_INDEX >> 1);
const MATCH_MIN_LEN: u32 = 2;
const PROB_INIT: u16 = 1024;
const TOP_VALUE: u32 = 1 << 24;

fn malformed(reason: impl Into<String>) -> Error {
    Error::archive("lzma", reason)
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::categorized_archive(ErrorCategory::Unsupported, "lzma", reason)
}

/// Where decoded bytes go, in order and in flushed runs.
pub(crate) trait DecodedSink {
    /// Absorbs `data`, whose first byte sits at absolute output
    /// position `at`. A run may reach past what the sink wants; the
    /// sink keeps its own range.
    fn accept(&mut self, at: u64, data: &[u8]) -> Result<()>;

    /// Whether output at or past `at` is still wanted. Once it is not,
    /// the decode stops there and the rest of the stream is never
    /// decoded — one member of a solid archive costs the members
    /// before it and nothing after.
    fn wants(&self, at: u64) -> bool;
}

/// The LZ window: the back-reference horizon the stream can still
/// reach, plus the run accumulating toward the next flush.
struct Window<'a> {
    sink: &'a mut dyn DecodedSink,
    buf: Vec<u8>,
    /// The horizon that must stay resident — the declared dictionary,
    /// clamped to what the stream can reach.
    keep: usize,
    /// Absolute output position of `buf[0]`.
    base: u64,
    /// Absolute output position the current dictionary starts at; no
    /// back-reference may reach before it.
    dict_start: u64,
    /// Set once the sink has asked to stop.
    stopped: bool,
    /// The next output position at which the sink is asked again.
    next_check: u64,
}

impl<'a> Window<'a> {
    fn new(sink: &'a mut dyn DecodedSink, keep: usize) -> Self {
        Self {
            sink,
            buf: Vec::with_capacity(keep + FLUSH_CHUNK),
            keep,
            base: 0,
            dict_start: 0,
            stopped: false,
            next_check: 0,
        }
    }

    /// Asks the sink whether the output still ahead is wanted.
    fn check_stop(&mut self) {
        let total = self.total();
        if total < self.next_check {
            return;
        }
        self.next_check = total + STOP_CHECK_STEP;
        if !self.sink.wants(total) {
            self.stopped = true;
        }
    }

    fn total(&self) -> u64 {
        self.base + self.buf.len() as u64
    }

    /// How far back a match may reach: the bytes produced since the
    /// last dictionary reset.
    fn reach(&self) -> u64 {
        self.total() - self.dict_start
    }

    fn reset_dictionary(&mut self) {
        self.dict_start = self.total();
    }

    fn flush(&mut self, take: usize) -> Result<()> {
        if take == 0 {
            return Ok(());
        }
        self.sink.accept(self.base, &self.buf[..take])?;
        self.base += take as u64;
        self.buf.copy_within(take.., 0);
        self.buf.truncate(self.buf.len() - take);
        Ok(())
    }

    fn push(&mut self, byte: u8) -> Result<()> {
        if self.buf.len() == self.keep + FLUSH_CHUNK {
            self.flush(FLUSH_CHUNK)?;
        }
        self.buf.push(byte);
        Ok(())
    }

    /// The byte `dist` places back, where 1 is the byte just produced.
    fn byte_back(&self, dist: u32) -> Result<u8> {
        let dist = u64::from(dist);
        if dist == 0 || dist > self.reach() || dist > self.buf.len() as u64 {
            return Err(malformed(format!(
                "back-reference of {dist} reaches before the dictionary"
            )));
        }
        Ok(self.buf[self.buf.len() - dist as usize])
    }

    fn copy_match(&mut self, dist: u32, len: u32, limit: u64) -> Result<()> {
        for _ in 0..len {
            if self.total() >= limit {
                return Err(malformed("match runs past the declared output size"));
            }
            let byte = self.byte_back(dist)?;
            self.push(byte)?;
        }
        Ok(())
    }

    /// Hands the window's remainder to the sink; the sink then has the
    /// complete output.
    fn finish(&mut self) -> Result<()> {
        let take = self.buf.len();
        self.flush(take)
    }
}

/// The range decoder LZMA's probability model reads through.
struct RangeDecoder<'a> {
    source: &'a mut dyn ByteSource,
    range: u32,
    code: u32,
    /// Set when the coded stream ran out mid-symbol; every subsequent
    /// read is meaningless and the decode fails by name.
    exhausted: bool,
    /// Set when the range coder reaches a state a valid stream cannot
    /// produce — corruption the coder itself detects.
    anomaly: bool,
}

impl<'a> RangeDecoder<'a> {
    fn new(source: &'a mut dyn ByteSource) -> Result<Self> {
        let mut decoder = Self {
            source,
            range: u32::MAX,
            code: 0,
            exhausted: false,
            anomaly: false,
        };
        if decoder.next_byte() != 0 {
            return Err(malformed("range coder does not start with a zero byte"));
        }
        for _ in 0..4 {
            decoder.code = (decoder.code << 8) | u32::from(decoder.next_byte());
        }
        if decoder.exhausted {
            return Err(malformed("range coder initialization is truncated"));
        }
        Ok(decoder)
    }

    fn next_byte(&mut self) -> u8 {
        match self.source.next_byte() {
            Some(byte) => byte,
            None => {
                self.exhausted = true;
                0
            }
        }
    }

    /// The refusal this decoder owes, if its state went bad.
    fn fault(&self) -> Option<Error> {
        if self.exhausted {
            return Some(malformed("the coded stream ended mid-symbol"));
        }
        if self.anomaly {
            return Some(malformed(
                "the range coder reached a state no valid stream produces",
            ));
        }
        None
    }

    fn normalize(&mut self) {
        if self.range < TOP_VALUE {
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(self.next_byte());
        }
    }

    fn decode_bit(&mut self, prob: &mut u16) -> u32 {
        let value = u32::from(*prob);
        let bound = (self.range >> 11) * value;
        let bit;
        if self.code < bound {
            *prob = (value + ((2048 - value) >> 5)) as u16;
            self.range = bound;
            bit = 0;
        } else {
            *prob = (value - (value >> 5)) as u16;
            self.code -= bound;
            self.range -= bound;
            bit = 1;
        }
        self.normalize();
        bit
    }

    fn decode_direct_bits(&mut self, count: u32) -> u32 {
        let mut result = 0u32;
        for _ in 0..count {
            self.range >>= 1;
            self.code = self.code.wrapping_sub(self.range);
            let t = 0u32.wrapping_sub(self.code >> 31);
            self.code = self.code.wrapping_add(self.range & t);
            if self.code == self.range {
                self.anomaly = true;
            }
            self.normalize();
            result = (result << 1).wrapping_add(t.wrapping_add(1));
        }
        result
    }

    fn decode_bit_tree(&mut self, probs: &mut [u16], bits: u32) -> u32 {
        let mut m = 1u32;
        for _ in 0..bits {
            m = (m << 1) + self.decode_bit(&mut probs[m as usize]);
        }
        m - (1 << bits)
    }

    fn decode_bit_tree_reverse(&mut self, probs: &mut [u16], offset: usize, bits: u32) -> u32 {
        let mut m = 1usize;
        let mut symbol = 0u32;
        for index in 0..bits {
            let bit = self.decode_bit(&mut probs[offset + m]);
            m = (m << 1) + bit as usize;
            symbol |= bit << index;
        }
        symbol
    }

    /// Whether the decoder finished on a clean range-coder state, the
    /// stream's own end-of-payload check.
    fn is_finished(&self) -> bool {
        self.code == 0
    }
}

struct LenDecoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 1 << NUM_POS_BITS_MAX],
    mid: [[u16; 8]; 1 << NUM_POS_BITS_MAX],
    high: [u16; 256],
}

impl LenDecoder {
    fn new() -> Self {
        Self {
            choice: PROB_INIT,
            choice2: PROB_INIT,
            low: [[PROB_INIT; 8]; 1 << NUM_POS_BITS_MAX],
            mid: [[PROB_INIT; 8]; 1 << NUM_POS_BITS_MAX],
            high: [PROB_INIT; 256],
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn decode(&mut self, range: &mut RangeDecoder<'_>, pos_state: usize) -> u32 {
        if range.decode_bit(&mut self.choice) == 0 {
            return range.decode_bit_tree(&mut self.low[pos_state], 3);
        }
        if range.decode_bit(&mut self.choice2) == 0 {
            return 8 + range.decode_bit_tree(&mut self.mid[pos_state], 3);
        }
        16 + range.decode_bit_tree(&mut self.high, 8)
    }
}

/// The LZMA probability model and match history — the state an LZMA2
/// chunk may keep, reset, or re-parameterize.
struct LzmaState {
    lc: u32,
    lp: u32,
    pb: u32,
    literals: Vec<u16>,
    pos_slot: [[u16; 64]; NUM_LEN_TO_POS_STATES],
    pos_decoders: [u16; 1 + NUM_FULL_DISTANCES - END_POS_MODEL_INDEX],
    align: [u16; 1 << NUM_ALIGN_BITS],
    is_match: [u16; NUM_STATES << NUM_POS_BITS_MAX],
    is_rep: [u16; NUM_STATES],
    is_rep_g0: [u16; NUM_STATES],
    is_rep_g1: [u16; NUM_STATES],
    is_rep_g2: [u16; NUM_STATES],
    is_rep0_long: [u16; NUM_STATES << NUM_POS_BITS_MAX],
    len: LenDecoder,
    rep_len: LenDecoder,
    state: u32,
    reps: [u32; 4],
}

impl LzmaState {
    fn new(properties: u8) -> Result<Self> {
        let mut value = u32::from(properties);
        if value >= 9 * 5 * 5 {
            return Err(malformed(format!(
                "properties byte {properties} is out of range"
            )));
        }
        let lc = value % 9;
        value /= 9;
        let lp = value % 5;
        let pb = value / 5;
        let mut state = Self {
            lc,
            lp,
            pb,
            literals: Vec::new(),
            pos_slot: [[PROB_INIT; 64]; NUM_LEN_TO_POS_STATES],
            pos_decoders: [PROB_INIT; 1 + NUM_FULL_DISTANCES - END_POS_MODEL_INDEX],
            align: [PROB_INIT; 1 << NUM_ALIGN_BITS],
            is_match: [PROB_INIT; NUM_STATES << NUM_POS_BITS_MAX],
            is_rep: [PROB_INIT; NUM_STATES],
            is_rep_g0: [PROB_INIT; NUM_STATES],
            is_rep_g1: [PROB_INIT; NUM_STATES],
            is_rep_g2: [PROB_INIT; NUM_STATES],
            is_rep0_long: [PROB_INIT; NUM_STATES << NUM_POS_BITS_MAX],
            len: LenDecoder::new(),
            rep_len: LenDecoder::new(),
            state: 0,
            reps: [0; 4],
        };
        state.reset();
        Ok(state)
    }

    /// Re-parameterizes from a fresh properties byte, which also resets.
    fn reset_properties(&mut self, properties: u8) -> Result<()> {
        let fresh = Self::new(properties)?;
        *self = fresh;
        Ok(())
    }

    fn reset(&mut self) {
        self.literals = vec![PROB_INIT; 0x300 << (self.lc + self.lp)];
        self.pos_slot = [[PROB_INIT; 64]; NUM_LEN_TO_POS_STATES];
        self.pos_decoders = [PROB_INIT; 1 + NUM_FULL_DISTANCES - END_POS_MODEL_INDEX];
        self.align = [PROB_INIT; 1 << NUM_ALIGN_BITS];
        self.is_match = [PROB_INIT; NUM_STATES << NUM_POS_BITS_MAX];
        self.is_rep = [PROB_INIT; NUM_STATES];
        self.is_rep_g0 = [PROB_INIT; NUM_STATES];
        self.is_rep_g1 = [PROB_INIT; NUM_STATES];
        self.is_rep_g2 = [PROB_INIT; NUM_STATES];
        self.is_rep0_long = [PROB_INIT; NUM_STATES << NUM_POS_BITS_MAX];
        self.len.reset();
        self.rep_len.reset();
        self.state = 0;
        self.reps = [0; 4];
    }

    fn decode_literal(
        &mut self,
        range: &mut RangeDecoder<'_>,
        window: &mut Window<'_>,
    ) -> Result<()> {
        let previous = if window.reach() == 0 {
            0u32
        } else {
            u32::from(window.byte_back(1)?)
        };
        let position = window.total() - window.dict_start;
        let literal_state =
            (((position as u32) & ((1 << self.lp) - 1)) << self.lc) + (previous >> (8 - self.lc));
        let base = 0x300 * literal_state as usize;
        let probs = &mut self.literals[base..base + 0x300];

        let mut symbol = 1u32;
        if self.state >= 7 {
            let mut match_byte = u32::from(window.byte_back(self.reps[0] + 1)?);
            loop {
                let match_bit = (match_byte >> 7) & 1;
                match_byte <<= 1;
                let index = ((1 + match_bit) << 8) + symbol;
                let bit = range.decode_bit(&mut probs[index as usize]);
                symbol = (symbol << 1) | bit;
                if match_bit != bit {
                    break;
                }
                if symbol >= 0x100 {
                    break;
                }
            }
        }
        while symbol < 0x100 {
            symbol = (symbol << 1) | range.decode_bit(&mut probs[symbol as usize]);
        }

        self.state = if self.state < 4 {
            0
        } else if self.state < 10 {
            self.state - 3
        } else {
            self.state - 6
        };
        window.push(symbol as u8)
    }

    fn decode_distance(&mut self, range: &mut RangeDecoder<'_>, len: u32) -> u32 {
        let len_state = ((len - MATCH_MIN_LEN) as usize).min(NUM_LEN_TO_POS_STATES - 1);
        let pos_slot = range.decode_bit_tree(&mut self.pos_slot[len_state], 6);
        if pos_slot < 4 {
            return pos_slot;
        }
        let direct_bits = (pos_slot >> 1) - 1;
        let mut dist = (2 | (pos_slot & 1)) << direct_bits;
        if (pos_slot as usize) < END_POS_MODEL_INDEX {
            let offset = (dist - pos_slot) as usize;
            dist += range.decode_bit_tree_reverse(&mut self.pos_decoders, offset, direct_bits);
        } else {
            dist += range.decode_direct_bits(direct_bits - NUM_ALIGN_BITS as u32) << NUM_ALIGN_BITS;
            dist += range.decode_bit_tree_reverse(&mut self.align, 0, NUM_ALIGN_BITS as u32);
        }
        dist
    }
}

/// How a decode run ended.
enum Ending {
    /// The declared output size was produced.
    Complete,
    /// The stream's end marker was read.
    EndMarker,
    /// The sink asked to stop, so the rest was never decoded.
    Stopped,
}

/// Decodes until `limit` bytes of total output exist, the end marker
/// appears, or the sink stops.
fn decode_run(
    state: &mut LzmaState,
    range: &mut RangeDecoder<'_>,
    window: &mut Window<'_>,
    limit: u64,
) -> Result<Ending> {
    let pos_mask = (1u64 << state.pb) - 1;
    loop {
        window.check_stop();
        if window.stopped {
            return Ok(Ending::Stopped);
        }
        if window.total() >= limit {
            return Ok(Ending::Complete);
        }
        if let Some(fault) = range.fault() {
            return Err(fault);
        }

        let pos_state = ((window.total() - window.dict_start) & pos_mask) as usize;
        let match_index = ((state.state as usize) << NUM_POS_BITS_MAX) + pos_state;
        if range.decode_bit(&mut state.is_match[match_index]) == 0 {
            state.decode_literal(range, window)?;
            continue;
        }

        let len;
        if range.decode_bit(&mut state.is_rep[state.state as usize]) != 0 {
            if window.reach() == 0 {
                return Err(malformed("a repeated match precedes any output"));
            }
            if range.decode_bit(&mut state.is_rep_g0[state.state as usize]) == 0 {
                if range.decode_bit(&mut state.is_rep0_long[match_index]) == 0 {
                    state.state = if state.state < 7 { 9 } else { 11 };
                    let byte = window.byte_back(state.reps[0] + 1)?;
                    window.push(byte)?;
                    continue;
                }
            } else {
                let dist;
                if range.decode_bit(&mut state.is_rep_g1[state.state as usize]) == 0 {
                    dist = state.reps[1];
                } else {
                    if range.decode_bit(&mut state.is_rep_g2[state.state as usize]) == 0 {
                        dist = state.reps[2];
                    } else {
                        dist = state.reps[3];
                        state.reps[3] = state.reps[2];
                    }
                    state.reps[2] = state.reps[1];
                }
                state.reps[1] = state.reps[0];
                state.reps[0] = dist;
            }
            len = state.rep_len.decode(range, pos_state) + MATCH_MIN_LEN;
            state.state = if state.state < 7 { 8 } else { 11 };
        } else {
            state.reps[3] = state.reps[2];
            state.reps[2] = state.reps[1];
            state.reps[1] = state.reps[0];
            len = state.len.decode(range, pos_state) + MATCH_MIN_LEN;
            state.state = if state.state < 7 { 7 } else { 10 };
            let dist = state.decode_distance(range, len);
            if dist == u32::MAX {
                return match range.fault() {
                    Some(fault) => Err(fault),
                    None => Ok(Ending::EndMarker),
                };
            }
            state.reps[0] = dist;
        }

        window.copy_match(state.reps[0] + 1, len, limit)?;
    }
}

/// The LZ window this stream needs: its declared dictionary, clamped to
/// what `expected` bytes of output can actually reach back through, and
/// refused past [`WINDOW_BOUND`] rather than allocated.
fn window_keep(dictionary: u64, expected: u64) -> Result<usize> {
    let keep = dictionary.min(expected);
    if keep > WINDOW_BOUND {
        return Err(unsupported(format!(
            "a {dictionary}-byte dictionary is past the {WINDOW_BOUND}-byte window bound"
        )));
    }
    Ok(keep as usize)
}

/// Decodes a raw LZMA stream (5 properties bytes already parsed as
/// `properties` and `dictionary`) of exactly `expected` output bytes.
pub(crate) fn decode_lzma(
    source: &mut dyn ByteSource,
    properties: u8,
    dictionary: u64,
    expected: u64,
    sink: &mut dyn DecodedSink,
) -> Result<()> {
    let mut state = LzmaState::new(properties)?;
    let mut window = Window::new(sink, window_keep(dictionary, expected)?);
    let mut range = RangeDecoder::new(source)?;

    match decode_run(&mut state, &mut range, &mut window, expected)? {
        Ending::Stopped => return window.finish(),
        Ending::EndMarker => {
            if window.total() != expected {
                return Err(malformed(format!(
                    "stream ended after {} bytes, expected {expected}",
                    window.total()
                )));
            }
            if !range.is_finished() {
                return Err(malformed("stream ended on an unclean range coder state"));
            }
        }
        Ending::Complete => {}
    }
    window.finish()
}

/// The dictionary size an LZMA2 properties byte declares.
fn lzma2_dictionary(properties: u8) -> Result<u64> {
    let value = u32::from(properties);
    if value > 40 {
        return Err(malformed(format!(
            "LZMA2 dictionary code {properties} is out of range"
        )));
    }
    if value == 40 {
        return Ok(u64::from(u32::MAX));
    }
    Ok(u64::from(2 | (value & 1)) << (value / 2 + 11))
}

/// Decodes an LZMA2 stream of exactly `expected` output bytes. Chunks
/// are read whole — the format bounds a chunk's coded run at 64 KiB —
/// while the decoded output streams through the window to the sink.
pub(crate) fn decode_lzma2(
    source: &mut dyn ByteSource,
    properties: u8,
    expected: u64,
    sink: &mut dyn DecodedSink,
) -> Result<()> {
    let dictionary = lzma2_dictionary(properties)?;
    let mut window = Window::new(sink, window_keep(dictionary, expected)?);
    let mut state: Option<LzmaState> = None;
    let mut needs_dictionary_reset = true;
    let mut needs_state_reset = true;

    loop {
        window.check_stop();
        if window.stopped {
            return window.finish();
        }
        let Some(control) = source.next_byte() else {
            return Err(malformed("LZMA2 stream ends without its terminator"));
        };
        if control == 0 {
            break;
        }

        if control < 3 {
            // An uncompressed chunk: control 1 also resets the
            // dictionary, control 2 continues it.
            if control == 1 {
                window.reset_dictionary();
                needs_dictionary_reset = false;
            } else if needs_dictionary_reset {
                return Err(malformed("LZMA2 continues a dictionary that never started"));
            }
            needs_state_reset = true;
            let size = u64::from(read_be16(source)?) + 1;
            if window.total() + size > expected {
                return Err(malformed("LZMA2 chunk runs past the declared output size"));
            }
            for _ in 0..size {
                let Some(byte) = source.next_byte() else {
                    return Err(malformed("LZMA2 uncompressed chunk is truncated"));
                };
                window.push(byte)?;
            }
            continue;
        }
        if control < 0x80 {
            return Err(malformed(format!(
                "LZMA2 control byte {control:#04x} names no chunk kind"
            )));
        }

        let unpacked = ((u64::from(control) & 0x1f) << 16) + u64::from(read_be16(source)?) + 1;
        let packed = usize::from(read_be16(source)?) + 1;
        let reset = (control >> 5) & 0x3;
        if reset >= 2 {
            let Some(properties) = source.next_byte() else {
                return Err(malformed("LZMA2 chunk is truncated before its properties"));
            };
            match &mut state {
                Some(state) => state.reset_properties(properties)?,
                None => state = Some(LzmaState::new(properties)?),
            }
            needs_state_reset = false;
        } else if reset == 1 {
            match &mut state {
                Some(state) => state.reset(),
                None => return Err(malformed("LZMA2 resets a state that was never declared")),
            }
            needs_state_reset = false;
        } else if needs_state_reset {
            return Err(malformed("LZMA2 continues a state that was never declared"));
        }
        if reset == 3 {
            window.reset_dictionary();
            needs_dictionary_reset = false;
        } else if needs_dictionary_reset {
            return Err(malformed("LZMA2 continues a dictionary that never started"));
        }

        let Some(state) = state.as_mut() else {
            return Err(malformed("LZMA2 chunk carries no properties"));
        };
        if window.total() + unpacked > expected {
            return Err(malformed("LZMA2 chunk runs past the declared output size"));
        }

        let mut coded = vec![0u8; packed];
        for byte in &mut coded {
            let Some(next) = source.next_byte() else {
                return Err(malformed("LZMA2 chunk is truncated"));
            };
            *byte = next;
        }

        let limit = window.total() + unpacked;
        let mut chunk = SliceByteSource::new(&coded);
        let mut range = RangeDecoder::new(&mut chunk)?;
        match decode_run(state, &mut range, &mut window, limit)? {
            Ending::EndMarker => return Err(malformed("an LZMA2 chunk carries an end marker")),
            Ending::Stopped => return window.finish(),
            Ending::Complete => {}
        }
    }

    if window.total() != expected {
        return Err(malformed(format!(
            "LZMA2 stream decoded to {} bytes, expected {expected}",
            window.total()
        )));
    }
    window.finish()
}

fn read_be16(source: &mut dyn ByteSource) -> Result<u16> {
    let (Some(high), Some(low)) = (source.next_byte(), source.next_byte()) else {
        return Err(malformed("LZMA2 chunk header is truncated"));
    };
    Ok((u16::from(high) << 8) | u16::from(low))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects everything a decode produces, for tests small enough to
    /// hold whole.
    struct Collect(Vec<u8>);

    impl DecodedSink for Collect {
        fn accept(&mut self, at: u64, data: &[u8]) -> Result<()> {
            assert_eq!(at, self.0.len() as u64, "runs arrive in order");
            self.0.extend_from_slice(data);
            Ok(())
        }

        fn wants(&self, _at: u64) -> bool {
            true
        }
    }

    /// Wants only the output before `until`, so the decode stops there.
    struct WantsPrefix {
        out: Vec<u8>,
        until: u64,
    }

    impl DecodedSink for WantsPrefix {
        fn accept(&mut self, at: u64, data: &[u8]) -> Result<()> {
            let take = (self.until.saturating_sub(at) as usize).min(data.len());
            self.out.extend_from_slice(&data[..take]);
            Ok(())
        }

        fn wants(&self, at: u64) -> bool {
            at < self.until
        }
    }

    /// A single-chunk LZMA2 stream of uncompressed data — legal LZMA2,
    /// hand-craftable without a compressor.
    fn uncompressed_lzma2(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut first = true;
        for chunk in payload.chunks(0x10000) {
            out.push(if first { 0x01 } else { 0x02 });
            first = false;
            let size = (chunk.len() - 1) as u16;
            out.extend_from_slice(&size.to_be_bytes());
            out.extend_from_slice(chunk);
        }
        out.push(0x00);
        out
    }

    /// The LZMA range encoder, for tests only: the exact mirror of
    /// [`RangeDecoder`], so a stream built here is one the decoder must
    /// read back byte for byte.
    struct RangeEncoder {
        low: u64,
        range: u32,
        cache: u8,
        cache_size: u64,
        out: Vec<u8>,
    }

    impl RangeEncoder {
        fn new() -> Self {
            Self {
                low: 0,
                range: u32::MAX,
                cache: 0,
                cache_size: 1,
                out: Vec::new(),
            }
        }

        fn shift_low(&mut self) {
            if (self.low as u32) < 0xff00_0000 || (self.low >> 32) != 0 {
                let carry = (self.low >> 32) as u8;
                let mut byte = self.cache;
                loop {
                    self.out.push(byte.wrapping_add(carry));
                    byte = 0xff;
                    self.cache_size -= 1;
                    if self.cache_size == 0 {
                        break;
                    }
                }
                self.cache = (self.low >> 24) as u8;
            }
            self.cache_size += 1;
            self.low = u64::from((self.low as u32) << 8);
        }

        fn encode_bit(&mut self, prob: &mut u16, bit: u32) {
            let value = u32::from(*prob);
            let bound = (self.range >> 11) * value;
            if bit == 0 {
                self.range = bound;
                *prob = (value + ((2048 - value) >> 5)) as u16;
            } else {
                self.low += u64::from(bound);
                self.range -= bound;
                *prob = (value - (value >> 5)) as u16;
            }
            if self.range < TOP_VALUE {
                self.range <<= 8;
                self.shift_low();
            }
        }

        fn finish(mut self) -> Vec<u8> {
            for _ in 0..5 {
                self.shift_low();
            }
            self.out
        }
    }

    /// Encodes `payload` as an all-literal LZMA stream: every symbol is
    /// a literal, so the state never leaves 0 and no match finder is
    /// needed to produce a legal stream.
    fn encode_lzma_literals(payload: &[u8], properties: u8) -> Vec<u8> {
        let mut value = u32::from(properties);
        let lc = value % 9;
        value /= 9;
        let lp = value % 5;
        let pb = value / 5;

        let mut is_match = vec![PROB_INIT; NUM_STATES << NUM_POS_BITS_MAX];
        let mut literals = vec![PROB_INIT; 0x300usize << (lc + lp)];
        let mut encoder = RangeEncoder::new();

        for (index, &byte) in payload.iter().enumerate() {
            let position = index as u32;
            let pos_state = (position & ((1 << pb) - 1)) as usize;
            // State stays 0 across literals, so the match flag indexes
            // the first row.
            encoder.encode_bit(&mut is_match[pos_state], 0);

            let previous = if index == 0 {
                0u32
            } else {
                u32::from(payload[index - 1])
            };
            let literal_state = ((position & ((1 << lp) - 1)) << lc) + (previous >> (8 - lc));
            let base = 0x300 * literal_state as usize;
            let probs = &mut literals[base..base + 0x300];

            let mut m = 1usize;
            for shift in (0..8).rev() {
                let bit = (u32::from(byte) >> shift) & 1;
                encoder.encode_bit(&mut probs[m], bit);
                m = (m << 1) | bit as usize;
            }
        }
        encoder.finish()
    }

    #[test]
    fn a_raw_lzma_stream_round_trips_through_its_declared_size() {
        // Properties 93: lc=3, lp=0, pb=2 — LZMA's own defaults, and
        // what a 7z folder coded `030101` carries.
        const PROPERTIES: u8 = 93;
        let payload: Vec<u8> = (0..70_000u32).map(|n| (n % 253) as u8).collect();
        let stream = encode_lzma_literals(&payload, PROPERTIES);

        let mut source = SliceByteSource::new(&stream);
        let mut sink = Collect(Vec::new());
        decode_lzma(
            &mut source,
            PROPERTIES,
            1 << 16,
            payload.len() as u64,
            &mut sink,
        )
        .expect("decodes");
        assert_eq!(sink.0, payload);
    }

    #[test]
    fn a_raw_lzma_stream_stops_where_its_reader_stops() {
        const PROPERTIES: u8 = 93;
        let payload: Vec<u8> = (0..70_000u32).map(|n| (n % 253) as u8).collect();
        let stream = encode_lzma_literals(&payload, PROPERTIES);

        let mut source = SliceByteSource::new(&stream);
        let mut sink = WantsPrefix {
            out: Vec::new(),
            until: 4096,
        };
        decode_lzma(
            &mut source,
            PROPERTIES,
            1 << 16,
            payload.len() as u64,
            &mut sink,
        )
        .expect("decodes");
        assert_eq!(sink.out, payload[..4096]);
    }

    #[test]
    fn uncompressed_lzma2_chunks_round_trip() {
        let payload: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        let stream = uncompressed_lzma2(&payload);
        let mut source = SliceByteSource::new(&stream);
        let mut sink = Collect(Vec::new());
        // Dictionary code 24 → 16 MiB, the size 7-Zip picks at -mx=9.
        decode_lzma2(&mut source, 24, payload.len() as u64, &mut sink).expect("decodes");
        assert_eq!(sink.0, payload);
    }

    #[test]
    fn a_sink_that_wants_a_prefix_leaves_the_rest_undecoded() {
        let payload: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        let stream = uncompressed_lzma2(&payload);
        let mut source = SliceByteSource::new(&stream);
        let mut sink = WantsPrefix {
            out: Vec::new(),
            until: 1000,
        };
        decode_lzma2(&mut source, 24, payload.len() as u64, &mut sink).expect("decodes");
        assert_eq!(sink.out, payload[..1000]);
    }

    #[test]
    fn a_truncated_lzma2_stream_is_refused_by_name() {
        let payload = vec![0x5au8; 4096];
        let mut stream = uncompressed_lzma2(&payload);
        stream.truncate(stream.len() / 2);
        let mut source = SliceByteSource::new(&stream);
        let mut sink = Collect(Vec::new());
        let error = decode_lzma2(&mut source, 24, payload.len() as u64, &mut sink)
            .expect_err("truncation is refused");
        assert!(error.to_string().contains("truncated"), "{error}");
    }

    #[test]
    fn a_declared_size_the_stream_undershoots_is_refused() {
        let payload = vec![0x11u8; 1024];
        let stream = uncompressed_lzma2(&payload);
        let mut source = SliceByteSource::new(&stream);
        let mut sink = Collect(Vec::new());
        let error = decode_lzma2(&mut source, 24, payload.len() as u64 + 1, &mut sink)
            .expect_err("the size disagreement is refused");
        assert!(error.to_string().contains("expected"), "{error}");
    }

    #[test]
    fn a_dictionary_past_the_window_bound_is_refused_before_allocation() {
        // Code 40 declares the 4 GiB maximum; a stream long enough to
        // reach through it is refused rather than allocated.
        let error = window_keep(lzma2_dictionary(40).expect("code 40 parses"), u64::MAX)
            .expect_err("the bound is enforced");
        assert_eq!(error.category(), ErrorCategory::Unsupported);
        assert!(error.to_string().contains("window bound"), "{error}");
    }

    #[test]
    fn a_small_stream_clamps_the_window_to_what_it_can_reach() {
        let keep = window_keep(lzma2_dictionary(24).expect("code 24 parses"), 4096)
            .expect("a small stream fits");
        assert_eq!(keep, 4096);
    }
}
