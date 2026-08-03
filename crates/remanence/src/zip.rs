// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The ZIP archive catalog: central-directory parsing and entry
//! sources, claiming STORE (0) and DEFLATE (8). The archive is read
//! where it lies, by positioned reads through the claimed handle (P27):
//! the end-of-central-directory scan and the directory parse read
//! bounded metadata, a stored entry resolves to a span of the archive
//! file, and a deflated entry decodes once through the 32 KiB LZ77
//! window into private session storage. An unclaimed compression
//! method is a named refusal before any data is touched (P3).

use std::fs::File;
use std::sync::Arc;

use crate::archive::{
    ArchiveCatalog, ArchiveEntry, ArchiveFormatAdapter, ArchiveFormatDescriptor, EntrySource,
};
use crate::cache::session_storage_file;
use crate::device::read_exact_at;
use crate::error::{Error, ErrorCategory, Result};
use crate::inflate::inflate_file_to_spool;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_DIR_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_FILE_SIGNATURE: u32 = 0x0403_4b50;

pub(crate) static ZIP_DESCRIPTOR: ArchiveFormatDescriptor = ArchiveFormatDescriptor {
    id: "zip",
    name: "ZIP archive",
    extensions: &["zip"],
};

/// The ZIP grammar's enrollment in the archive catalog.
pub(crate) struct ZipAdapter;

pub(crate) static ZIP_ADAPTER: ZipAdapter = ZipAdapter;

impl ArchiveFormatAdapter for ZipAdapter {
    fn descriptor(&self) -> &'static ArchiveFormatDescriptor {
        &ZIP_DESCRIPTOR
    }

    fn open(&self, file: Arc<File>, len: u64) -> Result<Box<dyn ArchiveCatalog>> {
        Ok(Box::new(ZipCatalog::open(file, len)?))
    }
}

/// The per-entry directory fields the catalog needs to place data.
#[derive(Debug, Clone, Copy)]
struct ZipRecord {
    compressed_size: u64,
    uncompressed_size: u64,
    compression_method: u16,
    local_header_offset: u64,
}

/// The ZIP archive catalog over a claimed file.
pub(crate) struct ZipCatalog {
    file: Arc<File>,
    len: u64,
    entries: Vec<ArchiveEntry>,
    /// Parallel to `entries`.
    records: Vec<ZipRecord>,
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

impl ZipCatalog {
    /// Parses the archive's central directory through `file`, which must
    /// be `len` bytes long. Reads the bounded end-of-central-directory
    /// tail and the directory records only.
    pub fn open(file: Arc<File>, len: u64) -> Result<Self> {
        if len < 22 {
            return Err(Error::archive("zip", "file is too small"));
        }

        // The EOCD sits in the last 22 bytes plus at most a 0xffff-byte
        // comment: a bounded tail, scanned backwards.
        let tail_length = len.min(22 + 0xffff) as usize;
        let tail_offset = len - tail_length as u64;
        let tail = read_span(&file, tail_offset, tail_length)?;
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
        let directory = read_span(&file, cd_offset, (eocd_offset - cd_offset) as usize)?;
        let mut entries = Vec::with_capacity(usize::from(total_entries));
        let mut records = Vec::with_capacity(usize::from(total_entries));
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
            let name =
                String::from_utf8_lossy(&directory[pos + 46..pos + 46 + name_len]).into_owned();
            let is_dir = name.ends_with('/');

            entries.push(ArchiveEntry {
                name,
                is_dir,
                compressed_size: Some(compressed_size),
                uncompressed_size,
            });
            records.push(ZipRecord {
                compressed_size,
                uncompressed_size,
                compression_method,
                local_header_offset,
            });
            pos += 46 + name_len + extra_len + comment_len;
        }

        Ok(Self {
            file,
            len,
            entries,
            records,
        })
    }

    /// Resolves an entry's data span in the archive file, from its local
    /// header, and checks the compression method against the claim.
    fn data_span(&self, index: usize) -> Result<(u64, ZipRecord)> {
        let record = self.records[index];
        let header = record.local_header_offset;
        if header + 30 > self.len {
            return Err(Error::archive("zip", "malformed local file header"));
        }
        let head = read_span(&self.file, header, 30)?;
        if read_u32(&head, 0) != LOCAL_FILE_SIGNATURE {
            return Err(Error::archive("zip", "malformed local file header"));
        }

        let name_len = u64::from(read_u16(&head, 26));
        let extra_len = u64::from(read_u16(&head, 28));
        let offset = header + 30 + name_len + extra_len;
        if offset + record.compressed_size > self.len {
            return Err(Error::archive("zip", "entry data out of range"));
        }
        Ok((offset, record))
    }
}

impl ArchiveCatalog for ZipCatalog {
    fn descriptor(&self) -> &'static ArchiveFormatDescriptor {
        &ZIP_DESCRIPTOR
    }

    fn archive_size(&self) -> u64 {
        self.len
    }

    fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }

    fn entry_source(&self, index: usize) -> Result<EntrySource> {
        if index >= self.entries.len() {
            return Err(Error::archive("zip", format!("entry {index} is out of range")));
        }
        let name = &self.entries[index].name;
        let (offset, record) = self.data_span(index)?;

        match record.compression_method {
            0 => {
                if record.compressed_size != record.uncompressed_size {
                    return Err(Error::archive(
                        "zip",
                        format!("stored entry '{name}' declares disagreeing sizes"),
                    ));
                }
                Ok(EntrySource::InPlace {
                    offset,
                    length: record.uncompressed_size,
                })
            }
            8 => {
                let spool = Arc::new(session_storage_file()?);
                let total = inflate_file_to_spool(
                    &self.file,
                    offset,
                    record.compressed_size,
                    record.uncompressed_size,
                    &spool,
                )?
                .ok_or_else(|| Error::archive("zip", format!("failed to inflate '{name}'")))?;
                if total != record.uncompressed_size {
                    return Err(Error::archive(
                        "zip",
                        format!(
                            "'{name}' decoded to {total} bytes, expected {}",
                            record.uncompressed_size
                        ),
                    ));
                }
                Ok(EntrySource::Spooled {
                    spool,
                    length: record.uncompressed_size,
                })
            }
            method => Err(Error::categorized_archive(
                ErrorCategory::Unsupported,
                "zip",
                format!("unsupported compression method {method}"),
            )),
        }
    }
}
