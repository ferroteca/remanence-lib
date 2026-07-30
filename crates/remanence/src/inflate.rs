// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! A compact, self-contained RFC 1951 (DEFLATE) decompressor. The structure
//! follows Mark Adler's "puff" reference implementation (see AGENTS.md,
//! "Prior art and provenance notes"), so the library has no external
//! compression dependency.

const MAX_BITS: usize = 15;
const MAX_LCODES: usize = 286;
const MAX_DCODES: usize = 30;
const MAX_CODES: usize = MAX_LCODES + MAX_DCODES;
const FIX_LCODES: usize = 288;

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buffer: u32,
    bit_count: u32,
    error: bool,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte_pos: 0, bit_buffer: 0, bit_count: 0, error: false }
    }

    fn bits(&mut self, need: u32) -> u32 {
        let mut value = u64::from(self.bit_buffer);
        while self.bit_count < need {
            let Some(&byte) = self.data.get(self.byte_pos) else {
                self.error = true;
                return 0;
            };
            value |= u64::from(byte) << self.bit_count;
            self.byte_pos += 1;
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
    out: &mut Vec<u8>,
    expected_size: usize,
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
            if out.len() >= expected_size {
                return false;
            }
            out.push(symbol as u8);
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
        if reader.error || dist as usize > out.len() {
            return false;
        }
        if out.len() + len as usize > expected_size {
            return false;
        }
        for _ in 0..len {
            out.push(out[out.len() - dist as usize]);
        }
    }
}

fn stored(reader: &mut BitReader<'_>, out: &mut Vec<u8>, expected_size: usize) -> bool {
    reader.bit_buffer = 0;
    reader.bit_count = 0;
    if reader.byte_pos + 4 > reader.data.len() {
        return false;
    }
    let length = usize::from(reader.data[reader.byte_pos])
        | (usize::from(reader.data[reader.byte_pos + 1]) << 8);
    let nlength = usize::from(reader.data[reader.byte_pos + 2])
        | (usize::from(reader.data[reader.byte_pos + 3]) << 8);
    reader.byte_pos += 4;
    if (length & 0xffff) != (!nlength & 0xffff) {
        return false;
    }
    if reader.byte_pos + length > reader.data.len() {
        return false;
    }
    if out.len() + length > expected_size {
        return false;
    }
    out.extend_from_slice(&reader.data[reader.byte_pos..reader.byte_pos + length]);
    reader.byte_pos += length;
    true
}

fn fixed(reader: &mut BitReader<'_>, out: &mut Vec<u8>, expected_size: usize) -> bool {
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

    codes(reader, out, expected_size, &lencode, &distcode)
}

fn dynamic(reader: &mut BitReader<'_>, out: &mut Vec<u8>, expected_size: usize) -> bool {
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

    codes(reader, out, expected_size, &lit_code, &dist_code)
}

/// Decompresses a raw DEFLATE (RFC 1951) stream. Returns `None` when the
/// stream is malformed or the output would exceed `expected_size`.
pub(crate) fn inflate(data: &[u8], expected_size: usize) -> Option<Vec<u8>> {
    let mut reader = BitReader::new(data);
    let mut out = Vec::with_capacity(expected_size);

    loop {
        let last = reader.bits(1);
        let block_type = reader.bits(2);
        if reader.error {
            return None;
        }
        let ok = match block_type {
            0 => stored(&mut reader, &mut out, expected_size),
            1 => fixed(&mut reader, &mut out, expected_size),
            2 => dynamic(&mut reader, &mut out, expected_size),
            _ => false,
        };
        if !ok {
            return None;
        }
        if last != 0 {
            break;
        }
    }

    Some(out)
}
