// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! CRC-32, the check that several of the formats this library reads
//! happen to share, and that its own private backing uses to say a
//! record arrived whole.
//!
//! The polynomial is the ordinary reversed 0xedb88320 one; nothing here
//! is specific to the format that first needed it.
#![allow(dead_code)]

pub(crate) const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut index = 0usize;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ 0xedb8_8320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

/// A CRC-32 computed as data streams past.
pub(crate) struct Crc32(u32);

impl Crc32 {
    pub(crate) fn new() -> Self {
        Self(u32::MAX)
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 = CRC_TABLE[((self.0 ^ u32::from(byte)) & 0xff) as usize] ^ (self.0 >> 8);
        }
    }

    pub(crate) fn finish(&self) -> u32 {
        !self.0
    }
}

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finish()
}
