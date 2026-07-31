// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Minimal ZIP central-directory reader supporting STORE (0) and DEFLATE (8),
//! mirroring the subset of the ZIP format the archive resolver relies on.

use crate::error::{Error, Result};
use crate::inflate::inflate;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_DIR_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_FILE_SIGNATURE: u32 = 0x0403_4b50;

/// Minimal ZIP entry metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZipEntry {
    pub name: String,
    pub is_dir: bool,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub compression_method: u16,
    pub local_header_offset: u64,
}

/// Minimal ZIP archive reader.
pub(crate) struct ZipArchive {
    data: Vec<u8>,
    entries: Vec<ZipEntry>,
}

fn read_u16(data: &[u8], pos: usize) -> u16 {
    u16::from_le_bytes([data[pos], data[pos + 1]])
}

fn read_u32(data: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

impl ZipArchive {
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        if data.len() < 22 {
            return Err(Error::archive("zip", "file is too small"));
        }

        // Locate the End Of Central Directory record by scanning backwards.
        let min_pos = data.len().saturating_sub(22 + 0xffff);
        let eocd = (min_pos..=data.len() - 22)
            .rev()
            .find(|&pos| read_u32(&data, pos) == EOCD_SIGNATURE)
            .ok_or_else(|| Error::archive("zip", "end of central directory not found"))?;

        let total_entries = read_u16(&data, eocd + 10);
        let cd_offset = read_u32(&data, eocd + 16) as usize;

        let mut entries = Vec::with_capacity(usize::from(total_entries));
        let mut pos = cd_offset;
        for _ in 0..total_entries {
            if pos + 46 > data.len() || read_u32(&data, pos) != CENTRAL_DIR_SIGNATURE {
                return Err(Error::archive("zip", "malformed central directory"));
            }

            let compression_method = read_u16(&data, pos + 10);
            let compressed_size = u64::from(read_u32(&data, pos + 20));
            let uncompressed_size = u64::from(read_u32(&data, pos + 24));
            let name_len = usize::from(read_u16(&data, pos + 28));
            let extra_len = usize::from(read_u16(&data, pos + 30));
            let comment_len = usize::from(read_u16(&data, pos + 32));
            let local_header_offset = u64::from(read_u32(&data, pos + 42));

            if pos + 46 + name_len > data.len() {
                return Err(Error::archive("zip", "malformed entry name"));
            }
            let name =
                String::from_utf8_lossy(&data[pos + 46..pos + 46 + name_len]).into_owned();
            let is_dir = name.ends_with('/');

            entries.push(ZipEntry {
                name,
                is_dir,
                compressed_size,
                uncompressed_size,
                compression_method,
                local_header_offset,
            });
            pos += 46 + name_len + extra_len + comment_len;
        }

        Ok(Self { data, entries })
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Reads and decompresses the named entry.
    pub fn read_entry(&self, entry: &ZipEntry) -> Result<Vec<u8>> {
        let header = entry.local_header_offset as usize;
        if header + 30 > self.data.len()
            || read_u32(&self.data, header) != LOCAL_FILE_SIGNATURE
        {
            return Err(Error::archive("zip", "malformed local file header"));
        }

        let name_len = usize::from(read_u16(&self.data, header + 26));
        let extra_len = usize::from(read_u16(&self.data, header + 28));
        let data_start = header + 30 + name_len + extra_len;
        let compressed_size = entry.compressed_size as usize;
        if data_start + compressed_size > self.data.len() {
            return Err(Error::archive("zip", "entry data out of range"));
        }

        let compressed = &self.data[data_start..data_start + compressed_size];

        match entry.compression_method {
            0 => Ok(compressed.to_vec()),
            8 => inflate(compressed, entry.uncompressed_size as usize).ok_or_else(|| {
                Error::archive("zip", format!("failed to inflate '{}'", entry.name))
            }),
            method => Err(Error::categorized_archive(
                crate::ErrorCategory::Unsupported,
                "zip",
                format!("unsupported compression method {method}"),
            )),
        }
    }
}
