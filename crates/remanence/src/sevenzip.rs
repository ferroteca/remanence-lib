// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The 7z archive catalog: signature and header grammar, the coder
//! claim, entry indexing, and bounded entry extraction. The grammar is
//! implemented here from the published 7z format description (P1) — no
//! external program is consulted, and nothing outside the claim below
//! is guessed at.
//!
//! What the catalog claims: a single-coder folder using **Copy**,
//! **LZMA**, or **LZMA2**. Everything else — a filter chain, a coder
//! this library does not implement, encryption, an external header, an
//! anti-file — is a named refusal (P3), reported with the coder or
//! construct that caused it rather than with a symptom.
//!
//! Reading is bounded (P27). The header is read as bounded metadata and
//! never confused with content; a stored member is a span of the
//! archive file, read in place; and a coded member decodes through the
//! LZ window into private session storage, one member at a time. A
//! solid folder is decoded only as far as the requested member's last
//! byte — the members before it are the price of solid compression, the
//! members after it are never touched.

use std::fs::File;
use std::sync::Arc;

use crate::archive::{
    ArchiveCatalog, ArchiveEntry, ArchiveFormatAdapter, ArchiveFormatDescriptor, EntrySource,
};
use crate::cache::session_storage_file;
use crate::device::{FileByteSource, read_exact_at, write_all_at};
use crate::error::{Error, ErrorCategory, Result};
use crate::lzma::{DecodedSink, decode_lzma, decode_lzma2};

/// The 7z signature, and the fixed prefix that follows it.
const SIGNATURE: [u8; 6] = [0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c];
/// Signature (6) + format version (2) + start-header CRC (4) + start
/// header (20): every packed stream offset is relative to its end.
const BASE_OFFSET: u64 = 32;

/// The largest header this catalog reads into memory (P27). A 7z header
/// is bounded metadata — names, sizes, digests — so a file declaring a
/// larger one is refused rather than read.
const HEADER_BOUND: u64 = 16 * 1024 * 1024;

const K_END: u64 = 0x00;
const K_HEADER: u64 = 0x01;
const K_ARCHIVE_PROPERTIES: u64 = 0x02;
const K_ADDITIONAL_STREAMS: u64 = 0x03;
const K_MAIN_STREAMS: u64 = 0x04;
const K_FILES_INFO: u64 = 0x05;
const K_PACK_INFO: u64 = 0x06;
const K_UNPACK_INFO: u64 = 0x07;
const K_SUBSTREAMS_INFO: u64 = 0x08;
const K_SIZE: u64 = 0x09;
const K_CRC: u64 = 0x0a;
const K_FOLDER: u64 = 0x0b;
const K_CODERS_UNPACK_SIZE: u64 = 0x0c;
const K_NUM_UNPACK_STREAM: u64 = 0x0d;
const K_EMPTY_STREAM: u64 = 0x0e;
const K_EMPTY_FILE: u64 = 0x0f;
const K_ANTI: u64 = 0x10;
const K_NAME: u64 = 0x11;
const K_ENCODED_HEADER: u64 = 0x17;
const K_DUMMY: u64 = 0x19;

const CODER_COPY: &[u8] = &[0x00];
const CODER_LZMA: &[u8] = &[0x03, 0x01, 0x01];
const CODER_LZMA2: &[u8] = &[0x21];

pub(crate) static SEVENZIP_DESCRIPTOR: ArchiveFormatDescriptor = ArchiveFormatDescriptor {
    id: "7z",
    name: "7z archive",
    extensions: &["7z"],
};

/// The 7z grammar's enrollment in the archive catalog.
pub(crate) struct SevenZipAdapter;

pub(crate) static SEVENZIP_ADAPTER: SevenZipAdapter = SevenZipAdapter;

impl ArchiveFormatAdapter for SevenZipAdapter {
    fn descriptor(&self) -> &'static ArchiveFormatDescriptor {
        &SEVENZIP_DESCRIPTOR
    }

    fn open(&self, file: Arc<File>, len: u64) -> Result<Box<dyn ArchiveCatalog>> {
        Ok(Box::new(SevenZipCatalog::open(file, len)?))
    }
}

fn malformed(reason: impl Into<String>) -> Error {
    Error::archive("7z", reason)
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::categorized_archive(ErrorCategory::Unsupported, "7z", reason)
}

/// How a coder id reads in a refusal: the hex the format writes it as.
fn coder_name(id: &[u8]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

const CRC_TABLE: [u32; 256] = {
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

/// The CRC-32 every 7z digest is, computed as data streams past.
struct Crc32(u32);

impl Crc32 {
    fn new() -> Self {
        Self(u32::MAX)
    }

    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 = CRC_TABLE[((self.0 ^ u32::from(byte)) & 0xff) as usize] ^ (self.0 >> 8);
        }
    }

    fn finish(&self) -> u32 {
        !self.0
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finish()
}

/// A reader over header bytes already in memory.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn byte(&mut self) -> Result<u8> {
        let byte = *self
            .data
            .get(self.pos)
            .ok_or_else(|| malformed("header ends mid-record"))?;
        self.pos += 1;
        Ok(byte)
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(count)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| malformed("header ends mid-record"))?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u32le(&mut self) -> Result<u32> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// The format's variable-length number.
    fn number(&mut self) -> Result<u64> {
        let first = self.byte()?;
        let mut mask = 0x80u8;
        let mut value = 0u64;
        for index in 0..8 {
            if first & mask == 0 {
                let high = u64::from(first & (mask.wrapping_sub(1)));
                return Ok(value | (high << (index * 8)));
            }
            value |= u64::from(self.byte()?) << (index * 8);
            mask >>= 1;
        }
        Ok(value)
    }

    /// A record count or a byte length. Every record the format counts
    /// needs at least one byte of header, so a value past what the
    /// header still holds is malformed — refused before anything is
    /// sized to it, never allocated for on a malformed file's word.
    fn size(&mut self) -> Result<usize> {
        let value = usize::try_from(self.number()?)
            .map_err(|_| malformed("a count is larger than this host"))?;
        if value > self.data.len() - self.pos {
            return Err(malformed("a count reaches past the end of the header"));
        }
        Ok(value)
    }

    /// `count` bits, most significant first.
    fn bit_vector(&mut self, count: usize) -> Result<Vec<bool>> {
        if count.div_ceil(8) > self.data.len() - self.pos {
            return Err(malformed("a bit vector reaches past the end of the header"));
        }
        let mut bits = Vec::with_capacity(count);
        let mut mask = 0u8;
        let mut current = 0u8;
        for _ in 0..count {
            if mask == 0 {
                current = self.byte()?;
                mask = 0x80;
            }
            bits.push(current & mask != 0);
            mask >>= 1;
        }
        Ok(bits)
    }

    /// A bit vector preceded by an "all defined" shortcut byte.
    fn defined_vector(&mut self, count: usize) -> Result<Vec<bool>> {
        if self.byte()? != 0 {
            return Ok(vec![true; count]);
        }
        self.bit_vector(count)
    }

    fn digests(&mut self, count: usize) -> Result<Vec<Option<u32>>> {
        let defined = self.defined_vector(count)?;
        let mut digests = Vec::new();
        for is_defined in defined {
            digests.push(if is_defined { Some(self.u32le()?) } else { None });
        }
        Ok(digests)
    }
}

/// The one coder a claimed folder carries.
#[derive(Debug, Clone)]
struct Coder {
    id: Vec<u8>,
    properties: Vec<u8>,
}

/// One folder: a single coder over one packed stream, producing one
/// output stream the folder's substreams divide.
#[derive(Debug, Clone)]
struct Folder {
    coder: Coder,
    unpacked_size: u64,
    pack_offset: u64,
    pack_size: u64,
    digest: Option<u32>,
}

/// Where one entry's bytes sit inside a folder's decoded output.
#[derive(Debug, Clone, Copy)]
struct StreamLocation {
    folder: usize,
    offset: u64,
    size: u64,
    digest: Option<u32>,
}

/// The parsed streams section: folders, their packed spans, and the
/// substreams the entries map onto.
struct StreamsInfo {
    folders: Vec<Folder>,
    locations: Vec<StreamLocation>,
}

/// The 7z archive catalog over a claimed file.
pub(crate) struct SevenZipCatalog {
    file: Arc<File>,
    len: u64,
    entries: Vec<ArchiveEntry>,
    /// Parallel to `entries`: where each entry's bytes live, or `None`
    /// for a directory or an empty file.
    locations: Vec<Option<StreamLocation>>,
    folders: Vec<Folder>,
}

impl SevenZipCatalog {
    /// Parses the archive's header through `file`, which must be `len`
    /// bytes long. Reads the start header, the bounded next header —
    /// decoding it first when the archive stores it coded — and nothing
    /// else.
    pub fn open(file: Arc<File>, len: u64) -> Result<Self> {
        if len < BASE_OFFSET {
            return Err(malformed("file is too small"));
        }
        let mut start = [0u8; BASE_OFFSET as usize];
        read_span(&file, 0, &mut start)?;
        if start[..6] != SIGNATURE {
            return Err(malformed("signature not found"));
        }
        if crc32(&start[12..32]) != u32::from_le_bytes([start[8], start[9], start[10], start[11]]) {
            return Err(malformed("start header fails its own CRC"));
        }

        let header_offset = u64::from_le_bytes(start[12..20].try_into().expect("eight bytes"));
        let header_size = u64::from_le_bytes(start[20..28].try_into().expect("eight bytes"));
        let header_crc = u32::from_le_bytes(start[28..32].try_into().expect("four bytes"));
        if header_size == 0 {
            // A legal empty archive: no header, so no entries.
            return Ok(Self {
                file,
                len,
                entries: Vec::new(),
                locations: Vec::new(),
                folders: Vec::new(),
            });
        }
        if header_size > HEADER_BOUND {
            return Err(unsupported(format!(
                "header is {header_size} bytes; 7z headers are bounded at {HEADER_BOUND} bytes"
            )));
        }
        let header_start = BASE_OFFSET
            .checked_add(header_offset)
            .filter(|start| start.checked_add(header_size).is_some_and(|end| end <= len))
            .ok_or_else(|| malformed("header lies outside the file"))?;

        let mut header = vec![0u8; header_size as usize];
        read_span(&file, header_start, &mut header)?;
        if crc32(&header) != header_crc {
            return Err(malformed("header fails its own CRC"));
        }

        let mut outer = Cursor::new(&header);
        let decoded;
        let mut cursor = match outer.number()? {
            K_HEADER => outer,
            K_ENCODED_HEADER => {
                decoded = decode_encoded_header(&file, len, &mut outer)?;
                let mut inner = Cursor::new(&decoded);
                if inner.number()? != K_HEADER {
                    return Err(malformed("the coded header decodes to no header"));
                }
                inner
            }
            other => {
                return Err(malformed(format!(
                    "header names unknown section {other:#04x}"
                )));
            }
        };

        let (entries, locations, folders) = parse_header(&mut cursor, len)?;
        Ok(Self {
            file,
            len,
            entries,
            locations,
            folders,
        })
    }

    /// Decodes the folder holding `location`, spooling only that span.
    fn spool(&self, location: &StreamLocation) -> Result<EntrySource> {
        let folder = &self.folders[location.folder];
        let spool = Arc::new(session_storage_file()?);
        let mut sink = SpoolRange {
            spool: &spool,
            start: location.offset,
            end: location.offset + location.size,
            written: 0,
            crc: Crc32::new(),
        };
        let mut source =
            FileByteSource::new(&self.file, folder.pack_offset, folder.pack_size);
        let properties = &folder.coder.properties;
        match folder.coder.id.as_slice() {
            CODER_LZMA2 => {
                let &[properties] = properties.as_slice() else {
                    return Err(malformed("an LZMA2 coder carries no dictionary property"));
                };
                decode_lzma2(&mut source, properties, folder.unpacked_size, &mut sink)?;
            }
            CODER_LZMA => {
                if properties.len() < 5 {
                    return Err(malformed("an LZMA coder carries a short property block"));
                }
                let dictionary = u64::from(u32::from_le_bytes(
                    properties[1..5].try_into().expect("four bytes"),
                ));
                decode_lzma(
                    &mut source,
                    properties[0],
                    dictionary,
                    folder.unpacked_size,
                    &mut sink,
                )?;
            }
            id => return Err(unsupported(format!("compression method {}", coder_name(id)))),
        }
        if source.failed() {
            return Err(Error::io("reading the coded stream failed".to_owned()));
        }

        if sink.written != location.size {
            return Err(malformed(format!(
                "member decoded to {} bytes, expected {}",
                sink.written, location.size
            )));
        }
        let computed = sink.crc.finish();
        if let Some(expected) = location.digest
            && computed != expected
        {
            return Err(malformed(format!(
                "member fails its CRC ({computed:08x}, expected {expected:08x})"
            )));
        }
        Ok(EntrySource::Spooled {
            spool,
            length: location.size,
        })
    }
}

impl ArchiveCatalog for SevenZipCatalog {
    fn descriptor(&self) -> &'static ArchiveFormatDescriptor {
        &SEVENZIP_DESCRIPTOR
    }

    fn archive_size(&self) -> u64 {
        self.len
    }

    fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }

    fn entry_source(&self, index: usize) -> Result<EntrySource> {
        let location = self
            .locations
            .get(index)
            .ok_or_else(|| malformed(format!("entry {index} is out of range")))?;
        let Some(location) = location else {
            // A directory or an empty file: no stream to read.
            return Ok(EntrySource::InPlace {
                offset: 0,
                length: 0,
            });
        };
        let folder = &self.folders[location.folder];
        if folder.coder.id == CODER_COPY {
            if folder.pack_size != folder.unpacked_size {
                return Err(malformed(
                    "a stored folder declares disagreeing packed and unpacked sizes",
                ));
            }
            return Ok(EntrySource::InPlace {
                offset: folder.pack_offset + location.offset,
                length: location.size,
            });
        }
        self.spool(location)
    }
}

fn read_span(file: &File, offset: u64, buf: &mut [u8]) -> Result<()> {
    read_exact_at(file, offset, buf)
        .map_err(|error| Error::io(format!("failed to read archive: {error}")))
}

/// Decoded bytes filtered down to one member's span and spooled to
/// private session storage.
struct SpoolRange<'a> {
    spool: &'a File,
    start: u64,
    end: u64,
    written: u64,
    crc: Crc32,
}

impl DecodedSink for SpoolRange<'_> {
    fn accept(&mut self, at: u64, data: &[u8]) -> Result<()> {
        if at >= self.end {
            return Ok(());
        }
        let length = data.len() as u64;
        let from = self.start.saturating_sub(at);
        if from >= length {
            return Ok(());
        }
        let to = (self.end - at).min(length);
        let slice = &data[from as usize..to as usize];
        write_all_at(self.spool, self.written, slice)
            .map_err(|error| Error::io(format!("failed to spool an archive member: {error}")))?;
        self.written += slice.len() as u64;
        self.crc.update(slice);
        Ok(())
    }

    fn wants(&self, at: u64) -> bool {
        at < self.end
    }
}

/// Decoded bytes collected whole, for a coded header the format bounds.
struct CollectBounded {
    out: Vec<u8>,
}

impl DecodedSink for CollectBounded {
    fn accept(&mut self, _at: u64, data: &[u8]) -> Result<()> {
        self.out.extend_from_slice(data);
        Ok(())
    }

    fn wants(&self, _at: u64) -> bool {
        true
    }
}

/// Decodes a `kEncodedHeader` — a streams section describing one folder
/// whose output is the real header.
fn decode_encoded_header(file: &File, len: u64, cursor: &mut Cursor<'_>) -> Result<Vec<u8>> {
    let streams = parse_streams_info(cursor, len)?;
    let [folder] = streams.folders.as_slice() else {
        return Err(unsupported(format!(
            "a coded header spread over {} folders",
            streams.folders.len()
        )));
    };
    if folder.unpacked_size > HEADER_BOUND {
        return Err(unsupported(format!(
            "coded header decodes to {} bytes; 7z headers are bounded at {HEADER_BOUND} bytes",
            folder.unpacked_size
        )));
    }

    let mut sink = CollectBounded {
        out: Vec::with_capacity(folder.unpacked_size as usize),
    };
    let mut source = FileByteSource::new(file, folder.pack_offset, folder.pack_size);
    let properties = &folder.coder.properties;
    match folder.coder.id.as_slice() {
        CODER_COPY => {
            sink.out = vec![0u8; folder.unpacked_size as usize];
            read_span(file, folder.pack_offset, &mut sink.out)?;
        }
        CODER_LZMA2 => {
            let &[properties] = properties.as_slice() else {
                return Err(malformed("an LZMA2 coder carries no dictionary property"));
            };
            decode_lzma2(&mut source, properties, folder.unpacked_size, &mut sink)?;
        }
        CODER_LZMA => {
            if properties.len() < 5 {
                return Err(malformed("an LZMA coder carries a short property block"));
            }
            let dictionary = u64::from(u32::from_le_bytes(
                properties[1..5].try_into().expect("four bytes"),
            ));
            decode_lzma(
                &mut source,
                properties[0],
                dictionary,
                folder.unpacked_size,
                &mut sink,
            )?;
        }
        id => {
            return Err(unsupported(format!(
                "a header coded with method {}",
                coder_name(id)
            )));
        }
    }
    if source.failed() {
        return Err(Error::io("reading the coded header failed".to_owned()));
    }
    if let Some(expected) = folder.digest
        && crc32(&sink.out) != expected
    {
        return Err(malformed("the coded header fails its CRC"));
    }
    Ok(sink.out)
}

/// Reads one folder: the single-coder shape this catalog claims.
fn parse_folder(cursor: &mut Cursor<'_>) -> Result<Coder> {
    let coders = cursor.number()?;
    if coders != 1 {
        return Err(unsupported(format!(
            "a folder chaining {coders} coders (filters are outside the claim)"
        )));
    }
    let flags = cursor.byte()?;
    let id_size = usize::from(flags & 0x0f);
    let is_complex = flags & 0x10 != 0;
    let has_properties = flags & 0x20 != 0;
    if flags & 0xc0 != 0 {
        return Err(unsupported("a coder declaring reserved attributes"));
    }
    let id = cursor.bytes(id_size)?.to_vec();
    if is_complex {
        let inputs = cursor.number()?;
        let outputs = cursor.number()?;
        if inputs != 1 || outputs != 1 {
            return Err(unsupported(format!(
                "a coder with {inputs} inputs and {outputs} outputs"
            )));
        }
    }
    let properties = if has_properties {
        let size = cursor.size()?;
        cursor.bytes(size)?.to_vec()
    } else {
        Vec::new()
    };
    Ok(Coder { id, properties })
}

/// Reads a `PackInfo`/`UnPackInfo`/`SubStreamsInfo` section and resolves
/// it to folders with absolute packed spans and their substreams.
fn parse_streams_info(cursor: &mut Cursor<'_>, len: u64) -> Result<StreamsInfo> {
    let mut pack_position = 0u64;
    let mut pack_sizes: Vec<u64> = Vec::new();
    let mut coders: Vec<Coder> = Vec::new();
    let mut unpacked_sizes: Vec<u64> = Vec::new();
    let mut folder_digests: Vec<Option<u32>> = Vec::new();
    let mut substream_counts: Vec<usize> = Vec::new();
    let mut substream_sizes: Vec<u64> = Vec::new();
    let mut substream_digests: Vec<Option<u32>> = Vec::new();
    let mut has_substreams = false;

    loop {
        match cursor.number()? {
            K_END => break,
            K_PACK_INFO => {
                pack_position = cursor.number()?;
                let count = cursor.size()?;
                loop {
                    match cursor.number()? {
                        K_END => break,
                        K_SIZE => {
                            pack_sizes = (0..count)
                                .map(|_| cursor.number())
                                .collect::<Result<Vec<_>>>()?;
                        }
                        K_CRC => {
                            cursor.digests(count)?;
                        }
                        other => {
                            return Err(malformed(format!(
                                "pack info names unknown property {other:#04x}"
                            )));
                        }
                    }
                }
                if pack_sizes.len() != count {
                    return Err(malformed("pack info declares no sizes"));
                }
            }
            K_UNPACK_INFO => {
                loop {
                    match cursor.number()? {
                        K_END => break,
                        K_FOLDER => {
                            let count = cursor.size()?;
                            if cursor.byte()? != 0 {
                                return Err(unsupported("folders held in an external stream"));
                            }
                            coders = (0..count)
                                .map(|_| parse_folder(cursor))
                                .collect::<Result<Vec<_>>>()?;
                        }
                        K_CODERS_UNPACK_SIZE => {
                            unpacked_sizes = (0..coders.len())
                                .map(|_| cursor.number())
                                .collect::<Result<Vec<_>>>()?;
                        }
                        K_CRC => {
                            folder_digests = cursor.digests(coders.len())?;
                        }
                        other => {
                            return Err(malformed(format!(
                                "unpack info names unknown property {other:#04x}"
                            )));
                        }
                    }
                }
            }
            K_SUBSTREAMS_INFO => {
                has_substreams = true;
                parse_substreams_info(
                    cursor,
                    &unpacked_sizes,
                    &folder_digests,
                    &mut substream_counts,
                    &mut substream_sizes,
                    &mut substream_digests,
                )?;
            }
            other => {
                return Err(malformed(format!(
                    "streams info names unknown section {other:#04x}"
                )));
            }
        }
    }

    if coders.len() != unpacked_sizes.len() {
        return Err(malformed("a folder declares no unpacked size"));
    }
    if coders.len() != pack_sizes.len() {
        return Err(unsupported(format!(
            "{} folders over {} packed streams (one stream per folder is the claim)",
            coders.len(),
            pack_sizes.len()
        )));
    }
    if folder_digests.len() != coders.len() {
        folder_digests = vec![None; coders.len()];
    }

    let mut folders = Vec::with_capacity(coders.len());
    let mut offset = BASE_OFFSET
        .checked_add(pack_position)
        .ok_or_else(|| malformed("packed streams start outside the file"))?;
    for (index, coder) in coders.into_iter().enumerate() {
        let pack_size = pack_sizes[index];
        if offset.checked_add(pack_size).is_none_or(|end| end > len) {
            return Err(malformed("a packed stream reaches past the end of the file"));
        }
        folders.push(Folder {
            coder,
            unpacked_size: unpacked_sizes[index],
            pack_offset: offset,
            pack_size,
            digest: folder_digests[index],
        });
        offset += pack_size;
    }

    if !has_substreams {
        substream_counts = vec![1; folders.len()];
        substream_sizes = folders.iter().map(|folder| folder.unpacked_size).collect();
        substream_digests = folders.iter().map(|folder| folder.digest).collect();
    }
    if substream_counts.len() != folders.len() {
        return Err(malformed("member counts do not match the folders declared"));
    }

    let mut locations = Vec::with_capacity(substream_sizes.len());
    let mut next = 0usize;
    for (index, folder) in folders.iter().enumerate() {
        let mut offset = 0u64;
        for _ in 0..substream_counts[index] {
            let size = *substream_sizes
                .get(next)
                .ok_or_else(|| malformed("a member declares no size"))?;
            if offset + size > folder.unpacked_size {
                return Err(malformed("a member reaches past its folder's output"));
            }
            locations.push(StreamLocation {
                folder: index,
                offset,
                size,
                digest: substream_digests.get(next).copied().flatten(),
            });
            offset += size;
            next += 1;
        }
    }

    Ok(StreamsInfo { folders, locations })
}

fn parse_substreams_info(
    cursor: &mut Cursor<'_>,
    unpacked_sizes: &[u64],
    folder_digests: &[Option<u32>],
    counts: &mut Vec<usize>,
    sizes: &mut Vec<u64>,
    digests: &mut Vec<Option<u32>>,
) -> Result<()> {
    *counts = vec![1usize; unpacked_sizes.len()];
    let mut sizes_read = false;
    let mut digests_read: Vec<Option<u32>> = Vec::new();

    loop {
        match cursor.number()? {
            K_END => break,
            K_NUM_UNPACK_STREAM => {
                for count in counts.iter_mut() {
                    *count = cursor.size()?;
                }
            }
            K_SIZE => {
                sizes.clear();
                for (folder, &count) in counts.iter().enumerate() {
                    if count == 0 {
                        continue;
                    }
                    let mut sum = 0u64;
                    for _ in 1..count {
                        let size = cursor.number()?;
                        sum += size;
                        sizes.push(size);
                    }
                    let last = unpacked_sizes[folder]
                        .checked_sub(sum)
                        .ok_or_else(|| malformed("member sizes exceed their folder's output"))?;
                    sizes.push(last);
                }
                sizes_read = true;
            }
            K_CRC => {
                // Only members whose digest the folder does not already
                // supply carry one here.
                let wanted: usize = counts
                    .iter()
                    .enumerate()
                    .map(|(folder, &count)| {
                        if count == 1 && folder_digests.get(folder).copied().flatten().is_some() {
                            0
                        } else {
                            count
                        }
                    })
                    .sum();
                digests_read = cursor.digests(wanted)?;
            }
            other => {
                return Err(malformed(format!(
                    "substreams info names unknown property {other:#04x}"
                )));
            }
        }
    }

    if !sizes_read {
        sizes.clear();
        for (folder, &count) in counts.iter().enumerate() {
            if count > 1 {
                return Err(malformed("a folder splits into members of no declared size"));
            }
            if count == 1 {
                sizes.push(unpacked_sizes[folder]);
            }
        }
    }

    let mut supplied = digests_read.into_iter();
    digests.clear();
    for (folder, &count) in counts.iter().enumerate() {
        let folder_digest = folder_digests.get(folder).copied().flatten();
        if count == 1 && folder_digest.is_some() {
            digests.push(folder_digest);
            continue;
        }
        for _ in 0..count {
            digests.push(supplied.next().flatten());
        }
    }
    Ok(())
}

/// The names section's UTF-16LE, NUL-separated strings. Path separators
/// are reported as `/` whichever separator the archive stored, so an
/// entry is addressed the same way in every archive grammar.
fn parse_names(data: &[u8], count: usize) -> Result<Vec<String>> {
    if data.len() % 2 != 0 {
        return Err(malformed("the names block is not whole UTF-16 units"));
    }
    let mut names = Vec::with_capacity(count);
    let mut units: Vec<u16> = Vec::new();
    for pair in data.chunks_exact(2) {
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if unit == 0 {
            names.push(String::from_utf16_lossy(&units).replace('\\', "/"));
            units.clear();
            continue;
        }
        units.push(unit);
    }
    if !units.is_empty() {
        return Err(malformed("the last name is unterminated"));
    }
    if names.len() != count {
        return Err(malformed(format!(
            "the names block holds {} names for {count} files",
            names.len()
        )));
    }
    Ok(names)
}

type ParsedHeader = (Vec<ArchiveEntry>, Vec<Option<StreamLocation>>, Vec<Folder>);

fn parse_header(cursor: &mut Cursor<'_>, len: u64) -> Result<ParsedHeader> {
    let mut streams: Option<StreamsInfo> = None;
    let mut files: Option<(Vec<String>, Vec<bool>, Vec<bool>, Vec<bool>)> = None;

    loop {
        match cursor.number()? {
            K_END => break,
            K_ARCHIVE_PROPERTIES => skip_archive_properties(cursor)?,
            K_ADDITIONAL_STREAMS => {
                return Err(unsupported("an archive carrying additional streams"));
            }
            K_MAIN_STREAMS => streams = Some(parse_streams_info(cursor, len)?),
            K_FILES_INFO => files = Some(parse_files_info(cursor)?),
            other => {
                return Err(malformed(format!(
                    "header names unknown section {other:#04x}"
                )));
            }
        }
    }

    let streams = streams.unwrap_or(StreamsInfo {
        folders: Vec::new(),
        locations: Vec::new(),
    });
    let Some((names, empty_streams, empty_files, anti)) = files else {
        return Ok((Vec::new(), Vec::new(), streams.folders));
    };

    // A solid folder attributes no packed size to a single member; a
    // folder holding exactly one member does.
    let mut members_per_folder = vec![0usize; streams.folders.len()];
    for location in &streams.locations {
        members_per_folder[location.folder] += 1;
    }

    let mut entries = Vec::with_capacity(names.len());
    let mut locations = Vec::with_capacity(names.len());
    let mut empty_index = 0usize;
    let mut stream_index = 0usize;
    for (index, name) in names.into_iter().enumerate() {
        if empty_streams.get(index).copied().unwrap_or(false) {
            let is_anti = anti.get(empty_index).copied().unwrap_or(false);
            let is_file = empty_files.get(empty_index).copied().unwrap_or(false);
            empty_index += 1;
            if is_anti {
                return Err(unsupported(format!("'{name}' is an anti-file")));
            }
            entries.push(ArchiveEntry {
                name,
                is_dir: !is_file,
                compressed_size: None,
                uncompressed_size: 0,
            });
            locations.push(None);
            continue;
        }

        let location = *streams.locations.get(stream_index).ok_or_else(|| {
            malformed(format!("'{name}' claims a member the archive does not hold"))
        })?;
        stream_index += 1;
        let folder = &streams.folders[location.folder];
        let only_member = members_per_folder[location.folder] == 1;
        entries.push(ArchiveEntry {
            name,
            is_dir: false,
            compressed_size: only_member.then_some(folder.pack_size),
            uncompressed_size: location.size,
        });
        locations.push(Some(location));
    }

    Ok((entries, locations, streams.folders))
}

fn skip_archive_properties(cursor: &mut Cursor<'_>) -> Result<()> {
    loop {
        if cursor.number()? == K_END {
            return Ok(());
        }
        let size = cursor.size()?;
        cursor.bytes(size)?;
    }
}

type FilesInfo = (Vec<String>, Vec<bool>, Vec<bool>, Vec<bool>);

fn parse_files_info(cursor: &mut Cursor<'_>) -> Result<FilesInfo> {
    let count = cursor.size()?;
    let mut names: Option<Vec<String>> = None;
    let mut empty_streams = vec![false; count];
    let mut empty_files: Vec<bool> = Vec::new();
    let mut anti: Vec<bool> = Vec::new();

    loop {
        let property = cursor.number()?;
        if property == K_END {
            break;
        }
        let size = cursor.size()?;
        let start = cursor.pos;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= cursor.data.len())
            .ok_or_else(|| malformed("a files property reaches past the header"))?;

        match property {
            K_EMPTY_STREAM => empty_streams = cursor.bit_vector(count)?,
            K_EMPTY_FILE => {
                let empties = empty_streams.iter().filter(|bit| **bit).count();
                empty_files = cursor.bit_vector(empties)?;
            }
            K_ANTI => {
                let empties = empty_streams.iter().filter(|bit| **bit).count();
                anti = cursor.bit_vector(empties)?;
            }
            K_NAME => {
                if size < 1 {
                    return Err(malformed("the names block declares no content"));
                }
                if cursor.byte()? != 0 {
                    return Err(unsupported("names held in an external stream"));
                }
                names = Some(parse_names(&cursor.data[cursor.pos..end], count)?);
            }
            K_DUMMY => {}
            // Times, attributes, start positions: recorded by the
            // format, unread by this catalog, and skipped by their
            // declared size rather than guessed at.
            _ => {}
        }
        if cursor.pos > end {
            return Err(malformed("a files property overran its declared size"));
        }
        cursor.pos = end;
    }

    let names = names.ok_or_else(|| malformed("the archive names none of its files"))?;
    Ok((names, empty_streams, empty_files, anti))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crc_matches_the_published_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn variable_length_numbers_read_at_every_width() {
        let mut cursor = Cursor::new(&[0x7f]);
        assert_eq!(cursor.number().expect("one byte"), 0x7f);

        let mut cursor = Cursor::new(&[0x80, 0x34]);
        assert_eq!(cursor.number().expect("two bytes"), 0x34);

        // Two continuation bits, so two little-endian bytes, then the
        // first byte's low bits become the high part.
        let mut cursor = Cursor::new(&[0xc1, 0x02, 0x03]);
        assert_eq!(cursor.number().expect("three bytes"), 0x0001_0302);

        let mut cursor = Cursor::new(&[0xff, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            cursor.number().expect("eight bytes"),
            0x0807_0605_0403_0201
        );
    }

    #[test]
    fn a_count_larger_than_the_header_is_refused_before_allocation() {
        // 768 records claimed by a header with two bytes left: refused
        // on the claim, never sized to it.
        let mut cursor = Cursor::new(&[0x83, 0x00, 0xaa, 0xbb]);
        let error = cursor.size().expect_err("the count is refused");
        assert!(error.to_string().contains("reaches past the end"), "{error}");
    }

    #[test]
    fn a_bit_vector_reads_most_significant_first() {
        let mut cursor = Cursor::new(&[0b1010_0000]);
        assert_eq!(
            cursor.bit_vector(4).expect("four bits"),
            vec![true, false, true, false]
        );
    }

    #[test]
    fn names_read_as_utf16_with_uniform_separators() {
        let mut data = Vec::new();
        for unit in "dir\\file.raw".encode_utf16() {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&[0, 0]);
        assert_eq!(
            parse_names(&data, 1).expect("one name"),
            vec!["dir/file.raw".to_owned()]
        );
    }

    #[test]
    fn a_coder_chain_is_refused_by_name() {
        // Two coders in a folder: a filter chain, outside the claim.
        let mut cursor = Cursor::new(&[0x02]);
        let error = parse_folder(&mut cursor).expect_err("a chain is refused");
        assert_eq!(error.category(), ErrorCategory::Unsupported);
        assert!(error.to_string().contains("chaining 2 coders"), "{error}");
    }

    #[test]
    fn an_unclaimed_coder_names_itself() {
        // One coder, id 06f10701 (AES-256 + SHA-256), no properties.
        let mut cursor = Cursor::new(&[0x01, 0x04, 0x06, 0xf1, 0x07, 0x01]);
        let coder = parse_folder(&mut cursor).expect("the folder parses");
        assert_eq!(coder_name(&coder.id), "06f10701");
    }
}
