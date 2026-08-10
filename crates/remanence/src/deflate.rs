// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! A compact, self-contained RFC 1951 (DEFLATE) compressor and the
//! RFC 1950 (zlib) framing around it, written against the RFCs so the
//! library keeps its no-dependency rule on the encode path as
//! `inflate.rs` keeps it on the decode path.
//!
//! One fixed-Huffman block, greedy LZ77 over the mandated 32 KiB
//! window with hash-chain matching. Two correct DEFLATE encoders
//! legitimately produce different bytes, so no cross-implementation
//! byte identity is claimed anywhere; what this one guarantees is
//! **determinism** — same input, same output, every time — which is
//! what lets an artifact this library wrote be re-serialized
//! byte-identically by this library.

use crate::inflate::inflate_bounded;

/// The LZ77 back-reference horizon DEFLATE mandates.
const WINDOW: usize = 32 * 1024;
/// The longest match one length code can spell.
const MAX_MATCH: usize = 258;
/// Below this, a match costs more than the literals it replaces.
const MIN_MATCH: usize = 3;
/// How many chain links a position follows before settling. A bound on
/// effort, not correctness; part of what makes the output a pure
/// function of the input.
const MAX_CHAIN: usize = 128;
/// Three-byte prefixes hash into this many heads.
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;

/// The length-code table of RFC 1951 §3.2.5: base length and extra
/// bits for symbols 257..=285.
const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// The distance-code table: base distance and extra bits for symbols
/// 0..=29.
const DIST_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13,
];

/// Bits accumulate least-significant first, as DEFLATE's stream is
/// read.
struct BitWriter {
    out: Vec<u8>,
    buffer: u64,
    count: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            buffer: 0,
            count: 0,
        }
    }

    /// Appends `count` bits of `value`, low bit first — the spelling of
    /// extra bits and block headers.
    fn bits(&mut self, value: u32, count: u32) {
        self.buffer |= u64::from(value) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.out.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
        }
    }

    /// Appends one Huffman code, which RFC 1951 packs most-significant
    /// bit first: the code is bit-reversed into the low-first stream.
    fn huffman(&mut self, code: u32, length: u32) {
        let mut reversed = 0u32;
        for bit in 0..length {
            reversed |= ((code >> bit) & 1) << (length - 1 - bit);
        }
        self.bits(reversed, length);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.buffer as u8);
        }
        self.out
    }
}

/// The fixed literal/length code of RFC 1951 §3.2.6.
fn fixed_literal(symbol: usize) -> (u32, u32) {
    match symbol {
        0..=143 => ((0x30 + symbol) as u32, 8),
        144..=255 => ((0x190 + symbol - 144) as u32, 9),
        256..=279 => ((symbol - 256) as u32, 7),
        _ => ((0xc0 + symbol - 280) as u32, 8),
    }
}

/// The length symbol covering `length`, with its extra bits.
fn length_code(length: usize) -> (usize, u32, u32) {
    let mut symbol = LENGTH_BASE.len() - 1;
    for (index, &base) in LENGTH_BASE.iter().enumerate() {
        if base > length {
            symbol = index - 1;
            break;
        }
    }
    // Length 258 has its own dedicated symbol with no extra bits, and
    // the scan above already lands on it.
    let extra_bits = LENGTH_EXTRA[symbol];
    let extra = (length - LENGTH_BASE[symbol]) as u32;
    (257 + symbol, extra_bits, extra)
}

/// The distance symbol covering `distance`, with its extra bits.
fn distance_code(distance: usize) -> (usize, u32, u32) {
    let mut symbol = DIST_BASE.len() - 1;
    for (index, &base) in DIST_BASE.iter().enumerate() {
        if base > distance {
            symbol = index - 1;
            break;
        }
    }
    let extra_bits = DIST_EXTRA[symbol];
    let extra = (distance - DIST_BASE[symbol]) as u32;
    (symbol, extra_bits, extra)
}

fn hash(data: &[u8], at: usize) -> usize {
    let a = u32::from(data[at]);
    let b = u32::from(data[at + 1]);
    let c = u32::from(data[at + 2]);
    (((a << 10) ^ (b << 5) ^ c).wrapping_mul(0x9e37) as usize) & (HASH_SIZE - 1)
}

/// Compresses `data` as one final fixed-Huffman DEFLATE block.
pub(crate) fn deflate(data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::new();
    // BFINAL = 1, BTYPE = 01 (fixed Huffman).
    writer.bits(1, 1);
    writer.bits(1, 2);

    let mut head = vec![usize::MAX; HASH_SIZE];
    let mut prev = vec![usize::MAX; data.len()];

    let mut at = 0;
    while at < data.len() {
        let mut best_length = 0;
        let mut best_distance = 0;
        if at + MIN_MATCH <= data.len() {
            let slot = hash(data, at);
            let mut candidate = head[slot];
            let mut chain = 0;
            while candidate != usize::MAX && chain < MAX_CHAIN {
                let distance = at - candidate;
                if distance > WINDOW {
                    break;
                }
                let limit = MAX_MATCH.min(data.len() - at);
                let mut length = 0;
                while length < limit && data[candidate + length] == data[at + length] {
                    length += 1;
                }
                if length > best_length {
                    best_length = length;
                    best_distance = distance;
                    if length == MAX_MATCH {
                        break;
                    }
                }
                candidate = prev[candidate];
                chain += 1;
            }
        }

        if best_length >= MIN_MATCH {
            let (symbol, extra_bits, extra) = length_code(best_length);
            let (code, length) = fixed_literal(symbol);
            writer.huffman(code, length);
            if extra_bits > 0 {
                writer.bits(extra, extra_bits);
            }
            let (symbol, extra_bits, extra) = distance_code(best_distance);
            // Fixed distance codes are five plain bits each.
            writer.huffman(symbol as u32, 5);
            if extra_bits > 0 {
                writer.bits(extra, extra_bits);
            }
            // Every covered position still enters the chains, so a
            // later match may begin inside this one.
            let end = at + best_length;
            while at < end {
                if at + MIN_MATCH <= data.len() {
                    let slot = hash(data, at);
                    prev[at] = head[slot];
                    head[slot] = at;
                }
                at += 1;
            }
        } else {
            let (code, length) = fixed_literal(usize::from(data[at]));
            writer.huffman(code, length);
            if at + MIN_MATCH <= data.len() {
                let slot = hash(data, at);
                prev[at] = head[slot];
                head[slot] = at;
            }
            at += 1;
        }
    }

    // End of block.
    let (code, length) = fixed_literal(256);
    writer.huffman(code, length);
    writer.finish()
}

/// The Adler-32 checksum of RFC 1950 §8.
pub(crate) fn adler32(data: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    // The largest run over which the sums fit in u32 before reduction.
    const RUN: usize = 5552;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for run in data.chunks(RUN) {
        for &byte in run {
            a += u32::from(byte);
            b += a;
        }
        a %= MODULUS;
        b %= MODULUS;
    }
    (b << 16) | a
}

/// Wraps `data` as one zlib (RFC 1950) stream: the two-byte header,
/// the DEFLATE stream, and the big-endian Adler-32 of the
/// uncompressed data.
pub(crate) fn zlib_compress(data: &[u8]) -> Vec<u8> {
    // CMF 0x78: method 8 (DEFLATE), a 32 KiB window. FLG 0x01: no
    // preset dictionary, fastest-compression flag, and the value that
    // makes the pair a multiple of 31 as the header check requires.
    let mut out = vec![0x78, 0x01];
    out.extend_from_slice(&deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Unwraps one zlib stream: header checked, stream inflated under
/// `cap`, and the trailer verified against the inflated bytes. The
/// trailer is taken from the stream's final four bytes, so trailing
/// content after a well-formed stream fails the check rather than
/// passing unexamined. `None` on any of it.
pub(crate) fn zlib_decompress(data: &[u8], cap: usize) -> Option<Vec<u8>> {
    if data.len() < 2 + 4 {
        return None;
    }
    let cmf = data[0];
    let flg = data[1];
    if cmf & 0x0f != 8 {
        return None;
    }
    if (u32::from(cmf) * 256 + u32::from(flg)) % 31 != 0 {
        return None;
    }
    // A preset dictionary is legal zlib and meaningless here: nothing
    // that writes these streams uses one.
    if flg & 0x20 != 0 {
        return None;
    }
    let inflated = inflate_bounded(&data[2..data.len() - 4], cap)?;
    let stated = u32::from_be_bytes(data[data.len() - 4..].try_into().expect("four bytes"));
    (adler32(&inflated) == stated).then_some(inflated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler_matches_the_published_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn deflate_round_trips_through_the_decoder() {
        let cases: [&[u8]; 5] = [
            b"",
            b"a",
            b"hello, hello, hello, hello",
            &[0u8; 4096],
            b"abcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabc",
        ];
        for case in cases {
            let compressed = deflate(case);
            let restored = crate::inflate::inflate(&compressed, case.len().max(1))
                .expect("our own stream inflates");
            assert_eq!(restored, case);
        }
    }

    #[test]
    fn deflate_round_trips_varied_binary_data() {
        // A deterministic pseudo-random buffer with repetition mixed
        // in, the shape of a packed point stream.
        let mut data = Vec::new();
        let mut state: u64 = 0x1234_5678_9abc_def0;
        for i in 0..100_000u32 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            data.push((state >> 56) as u8);
            if i % 7 == 0 {
                data.extend_from_slice(b"remanence");
            }
        }
        let compressed = deflate(&data);
        let restored =
            crate::inflate::inflate(&compressed, data.len()).expect("our own stream inflates");
        assert_eq!(restored, data);
    }

    #[test]
    fn repetitive_data_actually_compresses() {
        let data = vec![0x42u8; 100_000];
        let compressed = deflate(&data);
        assert!(
            compressed.len() < data.len() / 50,
            "a constant run should collapse, not merely survive: {} bytes",
            compressed.len()
        );
    }

    #[test]
    fn zlib_frame_round_trips_and_checks() {
        let data = b"the medium holds what the medium holds";
        let framed = zlib_compress(data);
        assert_eq!(framed[0], 0x78);
        assert_eq!(
            zlib_decompress(&framed, data.len()).as_deref(),
            Some(&data[..])
        );

        // A flipped payload bit fails somewhere — the stream or the
        // trailer — and never comes back as data.
        let mut corrupt = framed.clone();
        let middle = corrupt.len() / 2;
        corrupt[middle] ^= 0x40;
        assert_eq!(zlib_decompress(&corrupt, data.len()), None);

        // Trailing content lands under the trailer check.
        let mut trailing = framed.clone();
        trailing.push(0);
        assert_eq!(zlib_decompress(&trailing, data.len()), None);
    }

    #[test]
    fn compression_is_deterministic() {
        let mut data = Vec::new();
        for i in 0..50_000u32 {
            data.push((i % 251) as u8);
        }
        assert_eq!(zlib_compress(&data), zlib_compress(&data));
    }
}
