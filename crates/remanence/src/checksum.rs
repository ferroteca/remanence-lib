// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The small checks several of the formats this library reads happen to
//! share, and that its own private backing uses to say a record arrived
//! whole.
//!
//! **CRC-32** is the ordinary reversed 0xedb88320 one; nothing about it
//! is specific to the format that first needed it.
//!
//! **CRC-16/CCITT** is what every FM and MFM floppy recording covers its
//! address and data fields with — the forward 0x1021 polynomial seeded
//! all-ones, computed over the address mark bytes as well as the field,
//! which is why the seed is exposed rather than assumed: a caller feeds
//! it the marks first and the field after, exactly as the recording
//! itself was written.
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

/// The CRC-16/CCITT an FM or MFM recording covers its fields with.
///
/// It runs forward over the 0x1021 polynomial from an all-ones seed. The
/// state is public in the sense that matters here — a caller drives it —
/// because a floppy's checksum covers the *address marks* and then the
/// field, and the marks are recovered by the channel rather than being
/// bytes the field carries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Crc16Ccitt(u16);

impl Crc16Ccitt {
    pub(crate) const fn new() -> Self {
        Self(0xffff)
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 ^= u16::from(byte) << 8;
            for _ in 0..8 {
                self.0 = if self.0 & 0x8000 != 0 {
                    (self.0 << 1) ^ 0x1021
                } else {
                    self.0 << 1
                };
            }
        }
    }

    pub(crate) const fn finish(self) -> u16 {
        self.0
    }
}

pub(crate) fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc = Crc16Ccitt::new();
    crc.update(data);
    crc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ccitt_check_matches_its_published_vector() {
        // The conventional check value for this parameterization: the
        // ASCII digits one to nine.
        assert_eq!(crc16_ccitt(b"123456789"), 0x29b1);
    }

    #[test]
    fn an_mfm_address_field_checks_the_way_a_recording_writes_it() {
        // A real IBM System 34 id field: three A1 sync bytes, the IDAM,
        // then cylinder 0, head 0, sector 1, size code 2. A recording
        // whose stored CRC equals this is one whose field is intact, and
        // the whole point of driving the state by hand is that the marks
        // are covered too.
        let mut crc = Crc16Ccitt::new();
        crc.update(&[0xa1, 0xa1, 0xa1, 0xfe]);
        crc.update(&[0x00, 0x00, 0x01, 0x02]);
        let stated = crc.finish();

        // Feeding the same bytes in one run is the same computation, so
        // a caller that has the marks in hand may do either.
        assert_eq!(
            stated,
            crc16_ccitt(&[0xa1, 0xa1, 0xa1, 0xfe, 0x00, 0x00, 0x01, 0x02])
        );

        // And appending the stored CRC leaves the register at zero,
        // which is the property a reader actually checks with.
        let mut verify = Crc16Ccitt::new();
        verify.update(&[0xa1, 0xa1, 0xa1, 0xfe, 0x00, 0x00, 0x01, 0x02]);
        verify.update(&stated.to_be_bytes());
        assert_eq!(verify.finish(), 0);
    }
}
