// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! A compact, self-contained RFC 1951 (DEFLATE) decompressor. The structure
//! follows Mark Adler's "puff" reference implementation (see AGENTS.md,
//! "Prior art and provenance notes"), so the library has no external
//! compression dependency. Decompression streams (pledged P27): compressed
//! bytes are pulled through a bounded chunk and decompressed bytes flow to a
//! sink that keeps only the 32 KiB LZ77 window resident, so an entry of any
//! size decodes in bounded memory.

use std::fs::File;

use crate::device::{read_exact_at, write_all_at};
use crate::error::{Error, Result};

const MAX_BITS: usize = 15;
const MAX_LCODES: usize = 286;
const MAX_DCODES: usize = 30;
const MAX_CODES: usize = MAX_LCODES + MAX_DCODES;
const FIX_LCODES: usize = 288;

/// The LZ77 back-reference horizon DEFLATE mandates: a sink must keep
/// this much recent output resident, and no more.
const WINDOW_KEEP: usize = 32 * 1024;
/// The sink's resident buffer flushes down to [`WINDOW_KEEP`] whenever
/// it reaches this size.
const WINDOW_FULL: usize = 64 * 1024;

/// Where compressed bytes come from, one byte at a time.
trait ByteSource {
    /// The next compressed byte, or `None` at end of input (or on a
    /// source failure the wrapper reports separately).
    fn next_byte(&mut self) -> Option<u8>;
}

/// Compressed bytes pulled from a byte range of a file through a
/// bounded chunk.
struct FileSource<'a> {
    file: &'a File,
    next: u64,
    end: u64,
    buf: Vec<u8>,
    buf_pos: usize,
    failed: bool,
}

impl<'a> FileSource<'a> {
    fn new(file: &'a File, offset: u64, length: u64) -> Self {
        Self {
            file,
            next: offset,
            end: offset + length,
            buf: Vec::new(),
            buf_pos: 0,
            failed: false,
        }
    }
}

impl ByteSource for FileSource<'_> {
    fn next_byte(&mut self) -> Option<u8> {
        if self.buf_pos == self.buf.len() {
            if self.failed || self.next == self.end {
                return None;
            }
            let take = (self.end - self.next).min(4096) as usize;
            self.buf.resize(take, 0);
            self.buf_pos = 0;
            if read_exact_at(self.file, self.next, &mut self.buf).is_err() {
                self.failed = true;
                self.buf.clear();
                return None;
            }
            self.next += take as u64;
        }
        let byte = self.buf[self.buf_pos];
        self.buf_pos += 1;
        Some(byte)
    }
}

struct SliceSource<'a> {
    data: &'a [u8],
    pos: usize,
}

impl ByteSource for SliceSource<'_> {
    fn next_byte(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }
}

/// Where decompressed bytes go. `push` and `copy_back` return false
/// when the output would exceed the expected size, a back-reference is
/// invalid, or the sink failed.
trait InflateSink {
    fn total(&self) -> u64;
    fn push(&mut self, byte: u8) -> bool;
    fn copy_back(&mut self, dist: usize, len: usize) -> bool;
}

/// Decompressed bytes spooled to session storage, with only the LZ77
/// window resident.
struct SpoolSink<'a> {
    file: &'a File,
    window: Vec<u8>,
    flushed: u64,
    cap: u64,
    failed: bool,
}

impl<'a> SpoolSink<'a> {
    fn new(file: &'a File, cap: u64) -> Self {
        Self { file, window: Vec::new(), flushed: 0, cap, failed: false }
    }

    fn flush_half(&mut self) -> bool {
        if write_all_at(self.file, self.flushed, &self.window[..WINDOW_KEEP]).is_err() {
            self.failed = true;
            return false;
        }
        self.flushed += WINDOW_KEEP as u64;
        self.window.copy_within(WINDOW_KEEP.., 0);
        self.window.truncate(self.window.len() - WINDOW_KEEP);
        true
    }

    /// Writes the window's remainder through; the spool then holds the
    /// complete output. Returns the total length spooled.
    fn finish(mut self) -> std::result::Result<u64, ()> {
        if self.failed {
            return Err(());
        }
        if !self.window.is_empty() {
            if write_all_at(self.file, self.flushed, &self.window).is_err() {
                return Err(());
            }
            self.flushed += self.window.len() as u64;
        }
        Ok(self.flushed)
    }
}

impl InflateSink for SpoolSink<'_> {
    fn total(&self) -> u64 {
        self.flushed + self.window.len() as u64
    }

    fn push(&mut self, byte: u8) -> bool {
        if self.failed || self.total() >= self.cap {
            return false;
        }
        if self.window.len() == WINDOW_FULL && !self.flush_half() {
            return false;
        }
        self.window.push(byte);
        true
    }

    fn copy_back(&mut self, dist: usize, len: usize) -> bool {
        if dist == 0 || dist > self.window.len() {
            return false;
        }
        for _ in 0..len {
            let byte = self.window[self.window.len() - dist];
            if !self.push(byte) {
                return false;
            }
        }
        true
    }
}

struct VecSink {
    out: Vec<u8>,
    cap: usize,
}

impl InflateSink for VecSink {
    fn total(&self) -> u64 {
        self.out.len() as u64
    }

    fn push(&mut self, byte: u8) -> bool {
        if self.out.len() >= self.cap {
            return false;
        }
        self.out.push(byte);
        true
    }

    fn copy_back(&mut self, dist: usize, len: usize) -> bool {
        if dist == 0 || dist > self.out.len() {
            return false;
        }
        for _ in 0..len {
            let byte = self.out[self.out.len() - dist];
            if !self.push(byte) {
                return false;
            }
        }
        true
    }
}

struct BitReader<'a> {
    source: &'a mut dyn ByteSource,
    bit_buffer: u32,
    bit_count: u32,
    error: bool,
}

impl<'a> BitReader<'a> {
    fn new(source: &'a mut dyn ByteSource) -> Self {
        Self { source, bit_buffer: 0, bit_count: 0, error: false }
    }

    fn bits(&mut self, need: u32) -> u32 {
        let mut value = u64::from(self.bit_buffer);
        while self.bit_count < need {
            let Some(byte) = self.source.next_byte() else {
                self.error = true;
                return 0;
            };
            value |= u64::from(byte) << self.bit_count;
            self.bit_count += 8;
        }
        self.bit_buffer = (value >> need) as u32;
        self.bit_count -= need;
        (value & ((1u64 << need) - 1)) as u32
    }
}

struct Huffman {
    count: [i16; MAX_BITS + 1],
    symbol: [i16; MAX_CODES],
}

impl Huffman {
    fn new() -> Self {
        Self { count: [0; MAX_BITS + 1], symbol: [0; MAX_CODES] }
    }
}

fn decode(reader: &mut BitReader<'_>, huffman: &Huffman) -> i32 {
    let mut code: i32 = 0;
    let mut first: i32 = 0;
    let mut index: i32 = 0;
    for len in 1..=MAX_BITS {
        code |= reader.bits(1) as i32;
        if reader.error {
            return -1;
        }
        let count = i32::from(huffman.count[len]);
        if code - count < first {
            return i32::from(huffman.symbol[(index + (code - first)) as usize]);
        }
        index += count;
        first += count;
        first <<= 1;
        code <<= 1;
    }
    -1
}

fn construct(huffman: &mut Huffman, lengths: &[i16], n: usize) -> bool {
    huffman.count.fill(0);
    for &length in &lengths[..n] {
        huffman.count[length as usize] += 1;
    }
    if huffman.count[0] as usize == n {
        return true;
    }

    let mut left: i32 = 1;
    for len in 1..=MAX_BITS {
        left <<= 1;
        left -= i32::from(huffman.count[len]);
        if left < 0 {
            return false;
        }
    }

    let mut offs = [0i16; MAX_BITS + 1];
    for len in 1..MAX_BITS {
        offs[len + 1] = offs[len] + huffman.count[len];
    }
    for (symbol, &length) in lengths[..n].iter().enumerate() {
        if length != 0 {
            huffman.symbol[offs[length as usize] as usize] = symbol as i16;
            offs[length as usize] += 1;
        }
    }
    true
}

fn codes(
    reader: &mut BitReader<'_>,
    sink: &mut dyn InflateSink,
    lencode: &Huffman,
    distcode: &Huffman,
) -> bool {
    const LENGTH_BASE: [i32; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83,
        99, 115, 131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5,
        5, 0,
    ];
    const DIST_BASE: [i32; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769,
        1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11,
        12, 12, 13, 13,
    ];

    loop {
        let mut symbol = decode(reader, lencode);
        if symbol < 0 {
            return false;
        }
        if symbol == 256 {
            return true;
        }
        if symbol < 256 {
            if !sink.push(symbol as u8) {
                return false;
            }
            continue;
        }

        symbol -= 257;
        if symbol >= 29 {
            return false;
        }
        let len = LENGTH_BASE[symbol as usize] + reader.bits(LENGTH_EXTRA[symbol as usize]) as i32;

        let symbol = decode(reader, distcode);
        if !(0..30).contains(&symbol) {
            return false;
        }
        let dist = DIST_BASE[symbol as usize] + reader.bits(DIST_EXTRA[symbol as usize]) as i32;
        if reader.error {
            return false;
        }
        if !sink.copy_back(dist as usize, len as usize) {
            return false;
        }
    }
}

fn stored(reader: &mut BitReader<'_>, sink: &mut dyn InflateSink) -> bool {
    reader.bit_buffer = 0;
    reader.bit_count = 0;
    let mut header = [0u8; 4];
    for byte in &mut header {
        let Some(next) = reader.source.next_byte() else {
            return false;
        };
        *byte = next;
    }
    let length = usize::from(header[0]) | (usize::from(header[1]) << 8);
    let nlength = usize::from(header[2]) | (usize::from(header[3]) << 8);
    if (length & 0xffff) != (!nlength & 0xffff) {
        return false;
    }
    for _ in 0..length {
        let Some(byte) = reader.source.next_byte() else {
            return false;
        };
        if !sink.push(byte) {
            return false;
        }
    }
    true
}

fn fixed(reader: &mut BitReader<'_>, sink: &mut dyn InflateSink) -> bool {
    let mut lengths = [0i16; FIX_LCODES];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..144 => 8,
            144..256 => 9,
            256..280 => 7,
            _ => 8,
        };
    }

    let mut lencode = Huffman::new();
    construct(&mut lencode, &lengths, FIX_LCODES);

    let dist_lengths = [5i16; MAX_DCODES];
    let mut distcode = Huffman::new();
    construct(&mut distcode, &dist_lengths, MAX_DCODES);

    codes(reader, sink, &lencode, &distcode)
}

fn dynamic(reader: &mut BitReader<'_>, sink: &mut dyn InflateSink) -> bool {
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

    let hlit = reader.bits(5) as usize + 257;
    let hdist = reader.bits(5) as usize + 1;
    let hclen = reader.bits(4) as usize + 4;
    if reader.error || hlit > MAX_LCODES || hdist > MAX_DCODES {
        return false;
    }

    let mut code_lengths = [0i16; 19];
    for &order in &ORDER[..hclen] {
        code_lengths[order] = reader.bits(3) as i16;
    }
    let mut lencode = Huffman::new();
    if !construct(&mut lencode, &code_lengths, 19) {
        return false;
    }

    let mut lengths = [0i16; MAX_LCODES + MAX_DCODES];
    let mut index = 0;
    while index < hlit + hdist {
        let symbol = decode(reader, &lencode);
        if symbol < 0 {
            return false;
        }
        if symbol < 16 {
            lengths[index] = symbol as i16;
            index += 1;
        } else {
            let mut len = 0i16;
            let repeat;
            if symbol == 16 {
                if index == 0 {
                    return false;
                }
                len = lengths[index - 1];
                repeat = 3 + reader.bits(2) as usize;
            } else if symbol == 17 {
                repeat = 3 + reader.bits(3) as usize;
            } else {
                repeat = 11 + reader.bits(7) as usize;
            }
            if index + repeat > hlit + hdist {
                return false;
            }
            for _ in 0..repeat {
                lengths[index] = len;
                index += 1;
            }
        }
    }
    if lengths[256] == 0 {
        return false;
    }

    let mut lit_code = Huffman::new();
    let mut dist_code = Huffman::new();
    if !construct(&mut lit_code, &lengths, hlit) {
        return false;
    }
    construct(&mut dist_code, &lengths[hlit..], hdist);

    codes(reader, sink, &lit_code, &dist_code)
}

/// Runs the block loop: true when the stream decoded to its final
/// block, false when it is malformed, truncated, or over the sink's cap.
fn inflate_into(source: &mut dyn ByteSource, sink: &mut dyn InflateSink) -> bool {
    let mut reader = BitReader::new(source);
    loop {
        let last = reader.bits(1);
        let block_type = reader.bits(2);
        if reader.error {
            return false;
        }
        let ok = match block_type {
            0 => stored(&mut reader, sink),
            1 => fixed(&mut reader, sink),
            2 => dynamic(&mut reader, sink),
            _ => false,
        };
        if !ok {
            return false;
        }
        if last != 0 {
            break;
        }
    }
    true
}

/// Decompresses a raw DEFLATE (RFC 1951) stream held in memory. For
/// bounded inputs only — a qcow2 compressed cluster, a test vector —
/// where the format itself bounds the sizes (P27); an archive entry of
/// arbitrary size takes [`inflate_file_to_spool`] instead. Returns
/// `None` when the stream is malformed or the output would exceed
/// `expected_size`.
pub(crate) fn inflate(data: &[u8], expected_size: usize) -> Option<Vec<u8>> {
    let mut source = SliceSource { data, pos: 0 };
    let mut sink = VecSink { out: Vec::with_capacity(expected_size), cap: expected_size };
    inflate_into(&mut source, &mut sink).then_some(sink.out)
}

/// Decompresses a raw DEFLATE stream read from `length` bytes of `file`
/// at `offset` into `spool`, holding only the LZ77 window and a bounded
/// input chunk in memory (pledged P27). Returns `Ok(Some(total))` with
/// the spooled length, `Ok(None)` when the stream is malformed or would
/// exceed `expected_size`, and `Err` when reading or spooling itself
/// failed.
pub(crate) fn inflate_file_to_spool(
    file: &File,
    offset: u64,
    length: u64,
    expected_size: u64,
    spool: &File,
) -> Result<Option<u64>> {
    let mut source = FileSource::new(file, offset, length);
    let mut sink = SpoolSink::new(spool, expected_size);
    let ok = inflate_into(&mut source, &mut sink);
    if source.failed {
        return Err(Error::io("reading the compressed stream failed".to_owned()));
    }
    if sink.failed {
        return Err(Error::io("spooling the decompressed stream failed".to_owned()));
    }
    if !ok {
        return Ok(None);
    }
    match sink.finish() {
        Ok(total) => Ok(Some(total)),
        Err(()) => Err(Error::io("spooling the decompressed stream failed".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DEFLATE stream of stored blocks wrapping `payload` — legal
    /// RFC 1951, hand-craftable without a compressor.
    fn stored_deflate(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunks = payload.chunks(0xffff).peekable();
        loop {
            let Some(chunk) = chunks.next() else {
                // An empty payload still needs one final stored block.
                out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
                break;
            };
            let last = chunks.peek().is_none();
            out.push(u8::from(last));
            let len = chunk.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
            if last {
                break;
            }
        }
        out
    }

    #[test]
    fn stored_blocks_round_trip_through_a_slice() {
        let payload: Vec<u8> = (0..100_000u32).map(|n| (n % 249) as u8).collect();
        let stream = stored_deflate(&payload);
        let out = inflate(&stream, payload.len()).expect("inflates");
        assert_eq!(out, payload);
    }

    #[test]
    fn oversized_output_is_refused() {
        let payload = vec![0xa5u8; 1000];
        let stream = stored_deflate(&payload);
        assert!(inflate(&stream, 999).is_none());
    }

    #[test]
    fn a_stream_spools_to_session_storage_through_the_window() {
        // Larger than the resident window, so the spool path flushes.
        let payload: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        let stream = stored_deflate(&payload);

        let input = crate::cache::session_storage_file().expect("input storage");
        write_all_at(&input, 0, &stream).expect("stream writes");
        let spool = crate::cache::session_storage_file().expect("spool storage");

        let total = inflate_file_to_spool(
            &input,
            0,
            stream.len() as u64,
            payload.len() as u64,
            &spool,
        )
        .expect("io succeeds")
        .expect("stream decodes");
        assert_eq!(total, payload.len() as u64);

        let mut back = vec![0u8; payload.len()];
        read_exact_at(&spool, 0, &mut back).expect("spool reads");
        assert_eq!(back, payload);
    }

    #[test]
    fn a_truncated_stream_is_malformed_not_an_io_error() {
        let payload = vec![0x11u8; 4096];
        let stream = stored_deflate(&payload);
        let input = crate::cache::session_storage_file().expect("input storage");
        write_all_at(&input, 0, &stream).expect("stream writes");
        let spool = crate::cache::session_storage_file().expect("spool storage");

        let outcome = inflate_file_to_spool(
            &input,
            0,
            (stream.len() / 2) as u64,
            payload.len() as u64,
            &spool,
        )
        .expect("io succeeds");
        assert_eq!(outcome, None, "a truncated stream is a named refusal");
    }
}
