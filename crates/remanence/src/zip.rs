// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Minimal ZIP central-directory reader supporting STORE (0) and DEFLATE (8),
//! mirroring the subset of the ZIP format the archive resolver relies on.
//! The archive is read where it lies, by positioned reads through the
//! claimed handle (P27): the end-of-central-directory scan and the
//! directory parse read bounded metadata, and an entry's data is resolved to
//! a span of the archive file — never the archive loaded whole.

use std::fs::File;

use crate::device::read_exact_at;
use crate::error::{Error, Result};

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

/// Where an entry's data sits in the archive file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryData {
    /// STORE: the entry's bytes are the span itself, readable in place.
    Stored { offset: u64, length: u64 },
    /// DEFLATE: the span holds the compressed stream.
    Deflated { offset: u64, compressed_length: u64 },
}

/// Minimal ZIP archive reader over a claimed file.
pub(crate) struct ZipArchive {
    entries: Vec<ZipEntry>,
    len: u64,
}

fn read_u16(data: &[u8], pos: usize) -> u16 {
    u16::from_le_bytes([data[pos], data[pos + 1]])
}

fn read_u32(data: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

fn read_span(file: &File, offset: u64, length: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; length];
    read_exact_at(file, offset, &mut bytes)
        .map_err(|error| Error::io(format!("failed to read archive: {error}")))?;
    Ok(bytes)
}

impl ZipArchive {
    /// Parses the archive's central directory through `file`, which must
    /// be `len` bytes long. Reads the bounded end-of-central-directory
    /// tail and the directory records only.
    pub fn open(file: &File, len: u64) -> Result<Self> {
        if len < 22 {
            return Err(Error::archive("zip", "file is too small"));
        }

        // The EOCD sits in the last 22 bytes plus at most a 0xffff-byte
        // comment: a bounded tail, scanned backwards.
        let tail_length = len.min(22 + 0xffff) as usize;
        let tail_offset = len - tail_length as u64;
        let tail = read_span(file, tail_offset, tail_length)?;
        let eocd = (0..=tail_length - 22)
            .rev()
            .find(|&pos| read_u32(&tail, pos) == EOCD_SIGNATURE)
            .ok_or_else(|| Error::archive("zip", "end of central directory not found"))?;

        let total_entries = read_u16(&tail, eocd + 10);
        let cd_offset = u64::from(read_u32(&tail, eocd + 16));
        let eocd_offset = tail_offset + eocd as u64;
        if cd_offset > eocd_offset {
            return Err(Error::archive("zip", "malformed central directory"));
        }

        // The directory records are bounded metadata: names and fixed
        // fields, never entry data.
        let directory = read_span(file, cd_offset, (eocd_offset - cd_offset) as usize)?;
        let mut entries = Vec::with_capacity(usize::from(total_entries));
        let mut pos = 0usize;
        for _ in 0..total_entries {
            if pos + 46 > directory.len() || read_u32(&directory, pos) != CENTRAL_DIR_SIGNATURE {
                return Err(Error::archive("zip", "malformed central directory"));
            }

            let compression_method = read_u16(&directory, pos + 10);
            let compressed_size = u64::from(read_u32(&directory, pos + 20));
            let uncompressed_size = u64::from(read_u32(&directory, pos + 24));
            let name_len = usize::from(read_u16(&directory, pos + 28));
            let extra_len = usize::from(read_u16(&directory, pos + 30));
            let comment_len = usize::from(read_u16(&directory, pos + 32));
            let local_header_offset = u64::from(read_u32(&directory, pos + 42));

            if pos + 46 + name_len > directory.len() {
                return Err(Error::archive("zip", "malformed entry name"));
            }
            let name = String::from_utf8_lossy(&directory[pos + 46..pos + 46 + name_len])
                .into_owned();
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

        Ok(Self { entries, len })
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Resolves the named entry's data span in the archive file, from
    /// its local header. An unsupported compression method is a named
    /// refusal here, before any data is touched.
    pub fn entry_data(&self, file: &File, entry: &ZipEntry) -> Result<EntryData> {
        let header = entry.local_header_offset;
        if header + 30 > self.len {
            return Err(Error::archive("zip", "malformed local file header"));
        }
        let head = read_span(file, header, 30)?;
        if read_u32(&head, 0) != LOCAL_FILE_SIGNATURE {
            return Err(Error::archive("zip", "malformed local file header"));
        }

        let name_len = u64::from(read_u16(&head, 26));
        let extra_len = u64::from(read_u16(&head, 28));
        let offset = header + 30 + name_len + extra_len;
        if offset + entry.compressed_size > self.len {
            return Err(Error::archive("zip", "entry data out of range"));
        }

        match entry.compression_method {
            0 => {
                if entry.compressed_size != entry.uncompressed_size {
                    return Err(Error::archive(
                        "zip",
                        format!("stored entry '{}' declares disagreeing sizes", entry.name),
                    ));
                }
                Ok(EntryData::Stored { offset, length: entry.uncompressed_size })
            }
            8 => Ok(EntryData::Deflated {
                offset,
                compressed_length: entry.compressed_size,
            }),
            method => Err(Error::categorized_archive(
                crate::ErrorCategory::Unsupported,
                "zip",
                format!("unsupported compression method {method}"),
            )),
        }
    }
}
