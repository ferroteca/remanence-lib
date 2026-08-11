// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The CBM DOS filesystem (P18, P19): the directory a Commodore disk
//! records, read out of the blocks the recording addresses.
//!
//! **It knows nothing about what is beneath it.** A filesystem adapter
//! consumes blocks and nothing else (P18): this module never sees a
//! flux transition, a bit cell, a byte of GCR or a sector header, and it
//! does not know whether the blocks it reads came off a recording, an
//! image or a synthetic disk. [`BlockSource`] is the whole of what it
//! asks for, and every refusal reaching it from there is passed on as
//! the seam that made it stated it (P4).
//!
//! **Every number here is CBM DOS's, not the drive's.** The directory
//! track, the chain link in a block's first two bytes, the 32-byte entry
//! and where its fields sit, the type nibble and the two flag bits, the
//! BAM header's disk name and identity — these are the *disk operating
//! system's* conventions, and they are declared here. What a 1541 writes
//! to put a block on a disk is the family's, and lives in the drive
//! profile a rung down.
//!
//! **Names are read beside what was recorded, never instead of it.** A
//! name is sixteen PETSCII bytes padded with `0xA0`, and PETSCII is not
//! Unicode: the reading below covers the ranges CBM DOS itself displays
//! and marks anything else unread, with the sixteen bytes as recorded
//! travelling beside it as a declared fact. Nothing is transliterated
//! silently, and no byte is dropped on the way through.
//!
//! **A size is established by walking, never by trusting.** The
//! directory records a size in blocks; the bytes a file holds are what
//! its chain says — every block but the last carrying 254, the last
//! carrying what its own link field states it filled. Both are
//! reported, and every entry names which of the two its size came from,
//! so a disagreement between them is visible rather than resolved here.
//!
//! **A block that never came back qualifies its own entry.** Where a
//! chain reaches one, the walk stops and says so: the entry falls back
//! to the count the directory records, states that it did, and carries
//! the refusal that stopped it — while reading that file refuses rather
//! than handing back the part that was reachable (P28). The listing
//! still answers, because what the directory *records* is readable
//! whether or not the blocks it points at are.

use crate::error::{Error, ErrorCategory, Result};
use crate::filesystem::{Catalog, Entry, EntryFact, EntryKind};
use crate::report::{LabelReading, VolumeLabel};

/// The seam's stable spelling, which a refusal and the space's kind both
/// carry.
pub(crate) const CBM_DOS: &str = "cbmdos";

// --------------------------------------------------- the declared facts

/// The track CBM DOS keeps its own structures on.
const DIRECTORY_TRACK: u8 = 18;
/// The block of that track holding the block-availability map, and the
/// one the directory conventionally begins at.
const BAM_SECTOR: u8 = 0;
/// How many bytes one block carries, the chain link included.
const BLOCK_BYTES: usize = 256;
/// The first two bytes of every block: the track and sector of the next
/// one, or a track of zero and the index of the last byte this one
/// filled.
const LINK_BYTES: usize = 2;
/// How many bytes of a block are the file's own.
const DATA_BYTES: usize = BLOCK_BYTES - LINK_BYTES;
/// The version byte CBM DOS 2.6 stamps into the BAM.
const DOS_VERSION: u8 = 0x41;

/// Where the BAM header's own fields sit.
const BAM_DISK_NAME_AT: usize = 0x90;
const DISK_NAME_BYTES: usize = 16;
const BAM_DISK_ID_AT: usize = 0xa2;
const DISK_ID_BYTES: usize = 2;
const BAM_DOS_TYPE_AT: usize = 0xa5;
const DOS_TYPE_BYTES: usize = 2;

/// How wide one directory entry is, and where its fields sit within one.
const ENTRY_BYTES: usize = 32;
const ENTRIES_PER_BLOCK: usize = BLOCK_BYTES / ENTRY_BYTES;
const ENTRY_TYPE_AT: usize = 2;
const ENTRY_FIRST_TRACK_AT: usize = 3;
const ENTRY_FIRST_SECTOR_AT: usize = 4;
const ENTRY_NAME_AT: usize = 5;
const ENTRY_SIDE_TRACK_AT: usize = 21;
const ENTRY_SIDE_SECTOR_AT: usize = 22;
const ENTRY_RECORD_LENGTH_AT: usize = 23;
const ENTRY_SIZE_BLOCKS_AT: usize = 30;

/// The byte a name is padded to sixteen with.
const NAME_PAD: u8 = 0xa0;

/// How many blocks a chain may run before the walk calls it a loop.
///
/// It is the whole of the largest disk this seam claims — thirty-five
/// tracks of at most twenty-one blocks — so a chain that exceeds it has
/// visited a block twice, whatever order it did so in. Bounding the walk
/// is what keeps a corrupt link from being read as an endless file.
const CHAIN_BOUND: usize = 35 * 21;

/// The published description every declared number above comes from.
const PROVENANCE: &str = "declared from the published CBM DOS conventions: track 18 holds \
                          the block-availability map at sector 0 and the directory chained \
                          from the link it states, each block's first two bytes naming the \
                          next block or, with a track of zero, the last byte this one \
                          filled; a directory block holds eight 32-byte entries whose type \
                          byte carries the file type in its low nibble, the locked bit at \
                          0x40 and the closed bit at 0x80";

// -------------------------------------------------------- the vocabulary

/// Where the blocks CBM DOS addresses come from.
///
/// The whole of what this filesystem asks of what is beneath it. A
/// provider answers one block by the address the recording states for
/// it, or refuses; nothing here interprets that refusal, and a block
/// that does not read stops the walk that needed it rather than being
/// filled in (P28).
pub(crate) trait BlockSource {
    /// The 256 bytes of one block, chain link included.
    fn block(&self, track: u8, sector: u8) -> Result<Vec<u8>>;
}

/// What CBM DOS calls one entry's type, from the low nibble of its type
/// byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileType {
    Deleted,
    Sequential,
    Program,
    User,
    Relative,
    /// A nibble CBM DOS assigns no type to. It is kept as the number it
    /// is rather than read as the nearest type.
    Unassigned(u8),
}

impl FileType {
    fn of(type_byte: u8) -> Self {
        match type_byte & 0x0f {
            0 => Self::Deleted,
            1 => Self::Sequential,
            2 => Self::Program,
            3 => Self::User,
            4 => Self::Relative,
            other => Self::Unassigned(other),
        }
    }

    /// The stable cross-language spelling, which is CBM DOS's own.
    pub(crate) fn name(self) -> String {
        match self {
            Self::Deleted => "del".to_owned(),
            Self::Sequential => "seq".to_owned(),
            Self::Program => "prg".to_owned(),
            Self::User => "usr".to_owned(),
            Self::Relative => "rel".to_owned(),
            Self::Unassigned(nibble) => format!("unassigned-{nibble}"),
        }
    }
}

/// One entry as the directory records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectoryEntry {
    /// The directory block and slot it was read from. Where it sits in
    /// the listing is the directory's own order, which is evidence (U4).
    pub(crate) at: (u8, u8, usize),
    pub(crate) type_byte: u8,
    pub(crate) file_type: FileType,
    /// The bit CBM DOS displays as `<`.
    pub(crate) locked: bool,
    /// The bit CBM DOS clears while a file is open; an entry that never
    /// closed is the one the directory displays with a `*`.
    pub(crate) closed: bool,
    pub(crate) first: (u8, u8),
    /// The sixteen bytes as recorded, padding included.
    pub(crate) name_raw: [u8; DISK_NAME_BYTES],
    /// What those bytes read as, less the padding.
    pub(crate) name: String,
    /// The side-sector block and record length a relative file states.
    pub(crate) side_sector: (u8, u8),
    pub(crate) record_length: u8,
    /// The size the directory records, in blocks.
    pub(crate) size_blocks: u16,
}

/// Reads sixteen PETSCII bytes as the name CBM DOS displays.
///
/// The name ends at the first pad byte, which is what CBM DOS writes to
/// fill the field. Two ranges are read — the unshifted set's printable
/// span, and the shifted set's letters — and any other byte is marked
/// unread rather than guessed at, the recorded bytes travelling beside
/// the reading as a declared fact.
fn read_name(bytes: &[u8]) -> String {
    let mut name = String::new();
    for byte in bytes.iter().copied().take_while(|byte| *byte != NAME_PAD) {
        name.push(match byte {
            0x20..=0x5f => byte as char,
            0xc1..=0xda => (byte - 0xc1 + b'a') as char,
            _ => char::REPLACEMENT_CHARACTER,
        });
    }
    name
}

/// The bytes of a field, as recorded, in the spelling a fact carries.
fn recorded(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image(CBM_DOS, reason)
}

// ------------------------------------------------------- the structures

/// Reads the BAM header and the evidence that this is a CBM DOS disk at
/// all.
///
/// Recognition is what the BAM states about itself: the version byte CBM
/// DOS stamps, and a directory link naming a block the disk could hold.
/// A disk failing either is not claimed — the honest absence P19 asks
/// for — and nothing here repairs one into a claim.
pub(crate) struct Bam {
    directory_start: (u8, u8),
    disk_name_raw: [u8; DISK_NAME_BYTES],
    disk_name: String,
    disk_id_raw: [u8; DISK_ID_BYTES],
    dos_type_raw: [u8; DOS_TYPE_BYTES],
    version: u8,
}

impl Bam {
    pub(crate) fn read(block: &[u8]) -> Result<Self> {
        if block.len() < BLOCK_BYTES {
            return Err(refuse(format!(
                "the block-availability map is {} bytes and CBM DOS records {BLOCK_BYTES}",
                block.len()
            )));
        }
        let version = block[2];
        if version != DOS_VERSION {
            return Err(refuse(format!(
                "track {DIRECTORY_TRACK} sector {BAM_SECTOR} states the DOS version byte \
                 {version:#04x}, and this adapter claims {DOS_VERSION:#04x}"
            )));
        }
        let directory_start = (block[0], block[1]);
        if directory_start.0 == 0 {
            return Err(refuse(
                "the block-availability map states no first directory block, its link \
                 naming track zero",
            ));
        }
        let mut disk_name_raw = [0u8; DISK_NAME_BYTES];
        disk_name_raw.copy_from_slice(&block[BAM_DISK_NAME_AT..BAM_DISK_NAME_AT + DISK_NAME_BYTES]);
        let mut disk_id_raw = [0u8; DISK_ID_BYTES];
        disk_id_raw.copy_from_slice(&block[BAM_DISK_ID_AT..BAM_DISK_ID_AT + DISK_ID_BYTES]);
        let mut dos_type_raw = [0u8; DOS_TYPE_BYTES];
        dos_type_raw.copy_from_slice(&block[BAM_DOS_TYPE_AT..BAM_DOS_TYPE_AT + DOS_TYPE_BYTES]);
        Ok(Self {
            directory_start,
            disk_name: read_name(&disk_name_raw),
            disk_name_raw,
            disk_id_raw,
            dos_type_raw,
            version,
        })
    }

    /// The BAM header as the space's label: the disk name CBM DOS
    /// displays, with every source it was read from beside it (P4).
    ///
    /// The identity and the DOS type are readings of their own rather
    /// than being folded into the name — they are what the drive matches
    /// a disk by, and a label that concatenated them would be inventing
    /// a string no disk holds.
    pub(crate) fn label(&self) -> VolumeLabel {
        VolumeLabel {
            name: Some(self.disk_name.clone()),
            answered_by: Some("bam-disk-name".to_owned()),
            readings: vec![
                LabelReading {
                    source: "bam-disk-name".to_owned(),
                    stored: Some(self.disk_name.clone()),
                },
                LabelReading {
                    source: "bam-disk-name-petscii".to_owned(),
                    stored: Some(recorded(&self.disk_name_raw)),
                },
                LabelReading {
                    source: "bam-disk-id".to_owned(),
                    stored: Some(read_name(&self.disk_id_raw)),
                },
                LabelReading {
                    source: "bam-disk-id-petscii".to_owned(),
                    stored: Some(recorded(&self.disk_id_raw)),
                },
                LabelReading {
                    source: "bam-dos-type".to_owned(),
                    stored: Some(read_name(&self.dos_type_raw)),
                },
            ],
        }
    }

    /// What recognized this disk, in human-readable terms (P4).
    pub(crate) fn evidence(&self) -> Vec<String> {
        vec![
            format!(
                "track {DIRECTORY_TRACK} sector {BAM_SECTOR} states the DOS version byte \
                 {:#04x} and a directory beginning at track {} sector {}",
                self.version, self.directory_start.0, self.directory_start.1
            ),
            format!(
                "its header names the disk {:?} with the identity {:?}",
                self.disk_name,
                read_name(&self.disk_id_raw)
            ),
            format!("every rule this reading applied is CBM DOS's: {PROVENANCE}"),
        ]
    }
}

/// Walks the directory from the block the BAM names.
///
/// The chain is followed, never assumed: each block states where the
/// next one is, and a slot whose type byte is zero names nothing and is
/// left out of the listing rather than reported as a file with no name.
pub(crate) fn directory(
    blocks: &dyn BlockSource,
    bam: &Bam,
) -> Result<Vec<DirectoryEntry>> {
    let mut entries = Vec::new();
    let mut at = bam.directory_start;
    let mut visited = Vec::new();
    while at.0 != 0 {
        if visited.contains(&at) {
            return Err(refuse(format!(
                "the directory chain returns to track {} sector {}, so it does not end",
                at.0, at.1
            )));
        }
        if visited.len() >= CHAIN_BOUND {
            return Err(refuse(format!(
                "the directory chain runs past {CHAIN_BOUND} blocks, which is the whole \
                 of the largest disk this seam claims"
            )));
        }
        visited.push(at);
        let block = blocks.block(at.0, at.1)?;
        if block.len() < BLOCK_BYTES {
            return Err(refuse(format!(
                "track {} sector {} holds {} bytes and CBM DOS records {BLOCK_BYTES}",
                at.0,
                at.1,
                block.len()
            )));
        }
        for slot in 0..ENTRIES_PER_BLOCK {
            let entry = &block[slot * ENTRY_BYTES..(slot + 1) * ENTRY_BYTES];
            let type_byte = entry[ENTRY_TYPE_AT];
            if type_byte == 0 {
                continue;
            }
            let mut name_raw = [0u8; DISK_NAME_BYTES];
            name_raw.copy_from_slice(&entry[ENTRY_NAME_AT..ENTRY_NAME_AT + DISK_NAME_BYTES]);
            entries.push(DirectoryEntry {
                at: (at.0, at.1, slot),
                type_byte,
                file_type: FileType::of(type_byte),
                locked: type_byte & 0x40 != 0,
                closed: type_byte & 0x80 != 0,
                first: (entry[ENTRY_FIRST_TRACK_AT], entry[ENTRY_FIRST_SECTOR_AT]),
                name: read_name(&name_raw),
                name_raw,
                side_sector: (entry[ENTRY_SIDE_TRACK_AT], entry[ENTRY_SIDE_SECTOR_AT]),
                record_length: entry[ENTRY_RECORD_LENGTH_AT],
                size_blocks: u16::from(entry[ENTRY_SIZE_BLOCKS_AT])
                    | u16::from(entry[ENTRY_SIZE_BLOCKS_AT + 1]) << 8,
            });
        }
        at = (block[0], block[1]);
    }
    Ok(entries)
}

/// What one file's chain holds, established by walking it.
pub(crate) struct Chain {
    pub(crate) bytes: Vec<u8>,
    pub(crate) blocks: u64,
}

/// Follows one file's chain and answers what it holds.
///
/// Every block but the last carries its whole data span; the last states
/// how far it filled in the byte where a link would name a sector. A
/// chain reaching a block that does not read stops there and says so —
/// the file is not shortened to what was reachable, because a truncated
/// file that reports success is worse than no file at all (P28).
pub(crate) fn read_chain(blocks: &dyn BlockSource, first: (u8, u8)) -> Result<Chain> {
    let mut bytes = Vec::new();
    let mut visited: Vec<(u8, u8)> = Vec::new();
    let mut at = first;
    if at.0 == 0 {
        return Ok(Chain {
            bytes,
            blocks: 0,
        });
    }
    while at.0 != 0 {
        if visited.contains(&at) {
            return Err(refuse(format!(
                "the chain returns to track {} sector {}, so this file does not end",
                at.0, at.1
            )));
        }
        if visited.len() >= CHAIN_BOUND {
            return Err(refuse(format!(
                "the chain runs past {CHAIN_BOUND} blocks, which is the whole of the \
                 largest disk this seam claims"
            )));
        }
        visited.push(at);
        let block = blocks.block(at.0, at.1)?;
        if block.len() < BLOCK_BYTES {
            return Err(refuse(format!(
                "track {} sector {} holds {} bytes and CBM DOS records {BLOCK_BYTES}",
                at.0,
                at.1,
                block.len()
            )));
        }
        let (next_track, next_sector) = (block[0], block[1]);
        if next_track == 0 {
            // The last block: its second byte is the index of the last
            // byte it filled, so the data runs from the link's end to
            // there inclusive.
            let last = usize::from(next_sector);
            if last < LINK_BYTES {
                return Err(refuse(format!(
                    "track {} sector {} states that it filled up to byte {last}, which is \
                     inside its own chain link",
                    at.0, at.1
                )));
            }
            bytes.extend_from_slice(&block[LINK_BYTES..=last.min(BLOCK_BYTES - 1)]);
            break;
        }
        bytes.extend_from_slice(&block[LINK_BYTES..LINK_BYTES + DATA_BYTES]);
        at = (next_track, next_sector);
    }
    Ok(Chain {
        bytes,
        blocks: visited.len() as u64,
    })
}

// ---------------------------------------------------------- the catalog

/// The CBM DOS namespace, read through the blocks beneath it.
///
/// It holds no listing of its own: every verb reads the directory it was
/// asked about, so nothing here can go stale against the blocks below
/// (P19), and the reads stay bounded by what the question needs (P27).
pub(crate) struct CbmDosCatalog<'a> {
    blocks: &'a dyn BlockSource,
    bam: Bam,
}

impl std::fmt::Debug for Bam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bam")
            .field("disk_name", &self.disk_name)
            .field("directory_start", &self.directory_start)
            .finish()
    }
}

impl std::fmt::Debug for CbmDosCatalog<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CbmDosCatalog").field("bam", &self.bam).finish()
    }
}

impl<'a> CbmDosCatalog<'a> {
    /// Opens the namespace on a source of blocks, or refuses where the
    /// disk does not claim CBM DOS.
    pub(crate) fn open(blocks: &'a dyn BlockSource) -> Result<Self> {
        let block = blocks.block(DIRECTORY_TRACK, BAM_SECTOR)?;
        let bam = Bam::read(&block)?;
        Ok(Self { blocks, bam })
    }

    /// One entry in the vocabulary the P19 seam answers in.
    ///
    /// Every CBM fact the directory records travels beside the named
    /// fields in CBM DOS's own spelling — the two-outcome rule at the
    /// node's surface. It cannot fail: a chain that does not read is
    /// something this entry *states*, not something that stops it being
    /// one.
    fn entry(&self, recorded_entry: &DirectoryEntry) -> Entry {
        // The size is walked where the chain reads, and the directory's
        // own block count where it does not. A block that never came
        // back qualifies the entry it belongs to; it does not take the
        // listing down with it, because what the directory *records* is
        // readable either way and saying so is the whole point of the
        // basis travelling beside the number (P4, P28).
        let walked = read_chain(self.blocks, recorded_entry.first);
        let (size_bytes, basis) = match &walked {
            Ok(chain) => (chain.bytes.len() as u64, "chain-walked"),
            Err(_) => (
                u64::from(recorded_entry.size_blocks) * DATA_BYTES as u64,
                "size-blocks-recorded",
            ),
        };
        let mut declared = vec![
            EntryFact::new("type", recorded_entry.file_type.name()),
            EntryFact::new("type-raw", recorded_entry.type_byte.to_string()),
            EntryFact::new("locked", recorded_entry.locked.to_string()),
            EntryFact::new("closed", recorded_entry.closed.to_string()),
            EntryFact::new("size-blocks", recorded_entry.size_blocks.to_string()),
            EntryFact::new("size-basis", basis),
            EntryFact::new(
                "first-block",
                format!("{}/{}", recorded_entry.first.0, recorded_entry.first.1),
            ),
            EntryFact::new("name-petscii", recorded(&recorded_entry.name_raw)),
            EntryFact::new(
                "directory-slot",
                format!(
                    "{}/{}#{}",
                    recorded_entry.at.0, recorded_entry.at.1, recorded_entry.at.2
                ),
            ),
        ];
        match &walked {
            Ok(chain) => declared.push(EntryFact::new("blocks-walked", chain.blocks.to_string())),
            // What stopped the walk, in the terms the seam beneath
            // stated it: the entry says its bytes are not reachable
            // rather than reporting a size nobody can read out.
            Err(error) => declared.push(EntryFact::new("chain-refusal", error.to_string())),
        }
        if recorded_entry.file_type == FileType::Relative {
            declared.push(EntryFact::new(
                "record-length",
                recorded_entry.record_length.to_string(),
            ));
            declared.push(EntryFact::new(
                "side-sector",
                format!(
                    "{}/{}",
                    recorded_entry.side_sector.0, recorded_entry.side_sector.1
                ),
            ));
        }
        Entry {
            name: recorded_entry.name.clone(),
            kind: EntryKind::File,
            size_bytes,
            declared,
        }
    }

    fn find(&self, path: &str) -> Result<Option<DirectoryEntry>> {
        let entries = directory(self.blocks, &self.bam)?;
        Ok(entries
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(path)))
    }
}

impl Catalog for CbmDosCatalog<'_> {
    fn label(&self) -> Option<VolumeLabel> {
        Some(self.bam.label())
    }

    fn evidence(&self) -> Vec<String> {
        self.bam.evidence()
    }

    fn entries(&self, path: &str) -> Result<Vec<Entry>> {
        if !path_is_root(path) {
            return Err(Error::categorized_image(
                ErrorCategory::NotDirectory,
                CBM_DOS,
                format!("'{path}' holds no names; the CBM DOS directory is flat"),
            ));
        }
        Ok(directory(self.blocks, &self.bam)?
            .iter()
            .map(|entry| self.entry(entry))
            .collect())
    }

    fn stat(&self, path: &str) -> Result<Option<Entry>> {
        Ok(self.find(path)?.as_ref().map(|entry| self.entry(entry)))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let entry = self.find(path)?.ok_or_else(|| {
            Error::categorized_io(
                ErrorCategory::NotFound,
                format!("'{path}' names nothing in this CBM DOS directory"),
            )
        })?;
        Ok(read_chain(self.blocks, entry.first)?.bytes)
    }
}

fn path_is_root(path: &str) -> bool {
    path.split(['/', '\\'])
        .all(|segment| segment.is_empty() || segment == ".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A disk built block by block, so the reader is tested against
    /// exactly what CBM DOS records and nothing else.
    #[derive(Default)]
    struct Disk(BTreeMap<(u8, u8), Vec<u8>>);

    impl Disk {
        fn put(&mut self, track: u8, sector: u8, block: Vec<u8>) {
            self.0.insert((track, sector), block);
        }

        /// A BAM naming the directory at 18/1, the disk `name` and the
        /// identity `id`.
        fn with_bam(&mut self, name: &str, id: &str) {
            let mut block = vec![0u8; BLOCK_BYTES];
            block[0] = DIRECTORY_TRACK;
            block[1] = 1;
            block[2] = DOS_VERSION;
            block[BAM_DISK_NAME_AT..BAM_DISK_NAME_AT + DISK_NAME_BYTES].fill(NAME_PAD);
            for (at, byte) in name.bytes().enumerate() {
                block[BAM_DISK_NAME_AT + at] = byte;
            }
            for (at, byte) in id.bytes().enumerate() {
                block[BAM_DISK_ID_AT + at] = byte;
            }
            block[BAM_DOS_TYPE_AT] = b'2';
            block[BAM_DOS_TYPE_AT + 1] = b'A';
            self.put(DIRECTORY_TRACK, BAM_SECTOR, block);
        }

        /// One directory block holding `slots`, linked to `next`.
        fn with_directory(&mut self, sector: u8, next: (u8, u8), slots: &[(u8, &str, (u8, u8), u16)]) {
            let mut block = vec![0u8; BLOCK_BYTES];
            block[0] = next.0;
            block[1] = next.1;
            for (slot, (type_byte, name, first, size_blocks)) in slots.iter().enumerate() {
                let at = slot * ENTRY_BYTES;
                block[at + ENTRY_TYPE_AT] = *type_byte;
                block[at + ENTRY_FIRST_TRACK_AT] = first.0;
                block[at + ENTRY_FIRST_SECTOR_AT] = first.1;
                block[at + ENTRY_NAME_AT..at + ENTRY_NAME_AT + DISK_NAME_BYTES].fill(NAME_PAD);
                for (index, byte) in name.bytes().enumerate() {
                    block[at + ENTRY_NAME_AT + index] = byte;
                }
                block[at + ENTRY_SIZE_BLOCKS_AT] = *size_blocks as u8;
                block[at + ENTRY_SIZE_BLOCKS_AT + 1] = (*size_blocks >> 8) as u8;
            }
            self.put(DIRECTORY_TRACK, sector, block);
        }

        /// A chain of `contents`, laid down from `first` along `path`.
        fn with_file(&mut self, path: &[(u8, u8)], contents: &[u8]) {
            let mut remaining = contents;
            for (index, at) in path.iter().enumerate() {
                let mut block = vec![0u8; BLOCK_BYTES];
                let take = remaining.len().min(DATA_BYTES);
                block[LINK_BYTES..LINK_BYTES + take].copy_from_slice(&remaining[..take]);
                remaining = &remaining[take..];
                match path.get(index + 1) {
                    Some(next) if !remaining.is_empty() => {
                        block[0] = next.0;
                        block[1] = next.1;
                    }
                    _ => {
                        block[0] = 0;
                        block[1] = (LINK_BYTES + take - 1) as u8;
                    }
                }
                self.put(at.0, at.1, block);
                if remaining.is_empty() {
                    break;
                }
            }
        }
    }

    impl BlockSource for Disk {
        fn block(&self, track: u8, sector: u8) -> Result<Vec<u8>> {
            self.0.get(&(track, sector)).cloned().ok_or_else(|| {
                Error::categorized_image(
                    ErrorCategory::Unavailable,
                    "test-disk",
                    format!("track {track} sector {sector} does not read"),
                )
            })
        }
    }

    fn disk() -> Disk {
        let mut disk = Disk::default();
        disk.with_bam("PINBALL", "PC");
        disk.with_directory(
            1,
            (0, 0xff),
            &[
                (0x82, "LOADER", (17, 0), 2),
                (0x81, "SCORES", (17, 2), 1),
                // A slot the directory holds and CBM DOS shows with a
                // star: opened and never closed.
                (0x02, "CRASHED", (17, 4), 1),
                (0xc2, "LOCKED", (17, 5), 1),
            ],
        );
        disk.with_file(&[(17, 0), (17, 1)], &[0xaa; 300]);
        disk.with_file(&[(17, 2)], b"HI");
        disk.with_file(&[(17, 4)], b"X");
        disk.with_file(&[(17, 5)], b"Y");
        disk
    }

    #[test]
    fn the_directory_is_read_in_its_own_order_with_the_facts_cbm_dos_records() {
        let disk = disk();
        let catalog = CbmDosCatalog::open(&disk).expect("the BAM claims the disk");
        let entries = catalog.entries("").expect("the directory reads");

        // Directory order is evidence: nothing is sorted on the way out.
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["LOADER", "SCORES", "CRASHED", "LOCKED"]);

        let fact = |entry: &Entry, key: &str| {
            entry
                .declared
                .iter()
                .find(|fact| fact.key == key)
                .map(|fact| fact.value.clone())
                .unwrap_or_else(|| panic!("{key} is not declared on {entry:?}"))
        };
        assert_eq!(fact(&entries[0], "type"), "prg");
        assert_eq!(fact(&entries[0], "type-raw"), "130");
        assert_eq!(fact(&entries[0], "closed"), "true");
        assert_eq!(fact(&entries[0], "locked"), "false");
        assert_eq!(fact(&entries[0], "size-blocks"), "2");
        assert_eq!(fact(&entries[0], "blocks-walked"), "2");
        assert_eq!(fact(&entries[0], "first-block"), "17/0");
        assert_eq!(fact(&entries[0], "directory-slot"), "18/1#0");
        assert_eq!(fact(&entries[1], "type"), "seq");
        // The star and the lock are the two bits, read as themselves.
        assert_eq!(fact(&entries[2], "closed"), "false");
        assert_eq!(fact(&entries[3], "locked"), "true");

        // And the name is read beside what was recorded, padding and all.
        assert_eq!(
            fact(&entries[1], "name-petscii"),
            "53 43 4f 52 45 53 a0 a0 a0 a0 a0 a0 a0 a0 a0 a0"
        );
    }

    #[test]
    fn a_size_is_what_the_chain_holds_rather_than_what_the_entry_claims() {
        let disk = disk();
        let catalog = CbmDosCatalog::open(&disk).expect("the BAM claims the disk");
        let entries = catalog.entries("").expect("the directory reads");

        // 300 bytes across two blocks: 254 in the first and 46 in the
        // second, which is what the last block's own link field states.
        assert_eq!(entries[0].size_bytes, 300);
        assert_eq!(entries[1].size_bytes, 2);
        assert_eq!(catalog.read_file("LOADER").expect("reads"), vec![0xaa; 300]);
        assert_eq!(catalog.read_file("SCORES").expect("reads"), b"HI");
    }

    #[test]
    fn the_bam_header_is_the_label_and_the_identity_is_a_reading_beside_it() {
        let disk = disk();
        let catalog = CbmDosCatalog::open(&disk).expect("the BAM claims the disk");
        let label = catalog.label().expect("a CBM DOS disk names itself");

        assert_eq!(label.name.as_deref(), Some("PINBALL"));
        assert_eq!(label.answered_by.as_deref(), Some("bam-disk-name"));
        let reading = |source: &str| {
            label
                .readings
                .iter()
                .find(|reading| reading.source == source)
                .and_then(|reading| reading.stored.clone())
                .unwrap_or_else(|| panic!("{source} is not a reading of {label:?}"))
        };
        assert_eq!(reading("bam-disk-id"), "PC");
        assert_eq!(reading("bam-dos-type"), "2A");
        // The recorded bytes travel beside every reading of them.
        assert_eq!(
            reading("bam-disk-name-petscii"),
            "50 49 4e 42 41 4c 4c a0 a0 a0 a0 a0 a0 a0 a0 a0"
        );
    }

    #[test]
    fn a_disk_that_does_not_claim_cbm_dos_is_refused_rather_than_read() {
        let mut disk = Disk::default();
        // A blank track 18: no version byte, no directory link.
        disk.put(DIRECTORY_TRACK, BAM_SECTOR, vec![0u8; BLOCK_BYTES]);
        let error = CbmDosCatalog::open(&disk).expect_err("nothing claims this disk");
        assert!(error.to_string().contains("DOS version byte"), "{error}");

        // And one whose BAM does not read at all keeps the refusal the
        // seam beneath it made, rather than a coarser one of this one's.
        let error = CbmDosCatalog::open(&Disk::default()).expect_err("no block reads");
        assert_eq!(error.category(), ErrorCategory::Unavailable);
        assert!(error.to_string().contains("does not read"), "{error}");
    }

    #[test]
    fn a_block_the_chain_reaches_and_cannot_read_stops_the_walk() {
        let mut disk = disk();
        // The second block of LOADER goes missing: the file is refused
        // rather than shortened to the part that reads.
        disk.0.remove(&(17, 1));
        let catalog = CbmDosCatalog::open(&disk).expect("the BAM still claims the disk");
        let error = catalog.read_file("LOADER").expect_err("the chain is broken");
        assert_eq!(error.category(), ErrorCategory::Unavailable);
        assert!(error.to_string().contains("17 sector 1"), "{error}");

        // The listing still answers: what the directory records is
        // readable whether or not the file's own blocks are, and the
        // entry says which of the two its size came from.
        let entries = catalog.entries("").expect("the directory still reads");
        let fact = |entry: &Entry, key: &str| {
            entry
                .declared
                .iter()
                .find(|fact| fact.key == key)
                .map(|fact| fact.value.clone())
        };
        assert_eq!(
            fact(&entries[0], "size-basis").as_deref(),
            Some("size-blocks-recorded")
        );
        assert_eq!(entries[0].size_bytes, 2 * 254, "the two blocks it records");
        assert!(
            fact(&entries[0], "chain-refusal")
                .expect("the entry says what stopped the walk")
                .contains("17 sector 1")
        );
        assert!(fact(&entries[0], "blocks-walked").is_none());
        // Its neighbours are untouched by it.
        assert_eq!(
            fact(&entries[1], "size-basis").as_deref(),
            Some("chain-walked")
        );
    }

    #[test]
    fn a_chain_that_returns_to_itself_is_refused_rather_than_walked_forever() {
        let mut disk = disk();
        let mut looping = vec![0u8; BLOCK_BYTES];
        looping[0] = 17;
        looping[1] = 0;
        disk.put(17, 1, looping);
        let catalog = CbmDosCatalog::open(&disk).expect("the BAM claims the disk");
        let error = catalog.read_file("LOADER").expect_err("the chain loops");
        assert!(error.to_string().contains("does not end"), "{error}");
    }

    #[test]
    fn a_name_byte_with_no_reading_is_marked_rather_than_guessed() {
        // The shifted set's letters read as lowercase; a byte in neither
        // range is marked unread, and the recorded bytes carry it.
        assert_eq!(read_name(&[0x41, 0x42, NAME_PAD, 0x43]), "AB");
        assert_eq!(read_name(&[0xc1, 0xc2]), "ab");
        assert_eq!(read_name(&[0x41, 0x00, 0x42]), "A\u{fffd}B");
    }

    #[test]
    fn an_unused_slot_names_nothing_and_is_not_listed() {
        let mut disk = Disk::default();
        disk.with_bam("EMPTY", "00");
        disk.with_directory(1, (0, 0xff), &[]);
        let catalog = CbmDosCatalog::open(&disk).expect("the BAM claims the disk");
        assert!(catalog.entries("").expect("the directory reads").is_empty());
        assert!(catalog.stat("ANYTHING").expect("absence is an answer").is_none());
    }
}
