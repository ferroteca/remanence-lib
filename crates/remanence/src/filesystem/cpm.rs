// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The CP/M 2.2 directory, read against a **declared** layout.
//!
//! **A CP/M volume records no structure that says how to read it.**
//! Every other namespace this library reads states its own shape
//! somewhere a reader can look it up — a FAT boot record, an HDOS label,
//! a BAM header, each at a known place with a known form. CP/M has no
//! such thing: the disk parameter block that says where the directory
//! begins, how large an allocation block is and how many entries the
//! directory holds is a structure in the *BIOS*, not in the filesystem.
//!
//! **That is not the same as saying it is not on the disk.** A bootable
//! CP/M disk carries its own BIOS in the reserved tracks, so the block
//! is very often right there — the Heath distribution disks in the
//! fixture set each carry theirs, and the enrolled layouts below were
//! checked against them field by field. What is missing is not the
//! information but any *reliable way to find it*: it sits inside 8080
//! code at no fixed offset, in a form nothing on the disk identifies, on
//! disks that happen to be bootable and not on the data disks that are
//! not. Searching for a plausible fifteen bytes would be pattern-matching
//! machine code and calling the result evidence — and the same search
//! over these very artifacts turns up several candidates, of which only
//! one describes the volume it is written on.
//!
//! So the layout stays declared, and the reader applies what it is
//! given. What the on-disk blocks changed is the *basis* of the
//! declarations, not the design: they are read values now rather than
//! solved ones.
//!
//! That is a fact about the format rather than a gap to close, and it
//! decides this module's shape. The reader takes a [`Dpb`] and applies
//! it; it does not sniff one, and it does not carry a "usual" one to
//! fall back on. Two different DPBs read the same bytes as two different
//! directories, both of them self-consistent, and nothing in the bytes
//! prefers either — so a reader that guessed would not be occasionally
//! wrong, it would be *undetectably* wrong.
//!
//! What the module *can* claim, and does, is that the directory grammar
//! above the DPB is CP/M 2.2's own and does not vary: 32-byte entries, a
//! user number, a name whose high bits carry the attributes, extents
//! numbered across two fields, a record count, and allocation pointers
//! that are one byte or two according to how many blocks the DPB
//! declares. That part is version-independent, and it is what this file
//! implements.
//!
//! **Sector translation is the other thing the BIOS held, and it is
//! declared here for the same reason.** CP/M addresses logical records
//! and the BIOS maps them onto physical sectors through a skew table;
//! where that table is not the identity, an artifact in physical order
//! is not in the order records are numbered. It is the parameter most
//! worth getting right and least likely to announce itself: a wrong
//! skew still yields a directory that lists, because the first sector of
//! the directory is where both readings agree, and only the file
//! contents come back interleaved. The Heath H-17 layout below skews
//! four ways, and that is exactly how it presents.

use crate::error::{Error, ErrorCategory, Result};
use crate::filesystem::{Catalog, Entry, EntryFact, EntryKind};

/// The identity every refusal from this reader carries.
const PROFILE: &str = "cpm";

/// One CP/M record, which is the unit a directory entry counts in.
const RECORD_BYTES: u32 = 128;

/// Directory entries are 32 bytes, always.
const ENTRY_BYTES: usize = 32;

/// The byte an unused directory entry carries in its user field.
const UNUSED: u8 = 0xe5;

/// The number of allocation pointers one directory entry holds, as
/// bytes. Whether they are read as eight 16-bit pointers or sixteen
/// 8-bit ones is the DPB's business.
const POINTER_BYTES: usize = 16;

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image(PROFILE, reason)
}

/// The disk parameter block: what the BIOS knew and the disk does not
/// say.
///
/// The fields are CP/M's own, under CP/M's own names, because that is
/// what the published documentation of any given machine states and a
/// renamed field would have to be translated back before it could be
/// checked against a manual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dpb {
    /// `SPT` — 128-byte records per track.
    pub(crate) records_per_track: u32,
    /// `BSH`/`BLM` expressed directly: the allocation block, in bytes.
    /// CP/M admits 1024, 2048, 4096, 8192 and 16384 and nothing else.
    pub(crate) block_bytes: u32,
    /// `DSM` + 1 — how many allocation blocks the volume holds. This is
    /// what decides pointer width: a volume of more than 255 blocks
    /// addresses them with 16-bit pointers.
    pub(crate) blocks: u32,
    /// `DRM` + 1 — how many entries the directory holds.
    pub(crate) directory_entries: u32,
    /// `OFF` — reserved tracks before the directory, in tracks.
    pub(crate) reserved_tracks: u32,
    /// The bytes in one track, which with `reserved_tracks` is what
    /// turns the reserved-track count into an offset. CP/M's own DPB
    /// states this only implicitly, through the BIOS sector table.
    pub(crate) track_bytes: u32,
    /// The logical-to-physical sector map within one track: entry *n*
    /// is the physical sector CP/M's *n*th logical sector sits in.
    ///
    /// This is the BIOS's sector-translation table, and it is declared
    /// here for the same reason the rest of the block is — the disk does
    /// not carry it. An identity map is the honest spelling of "this
    /// family does not skew", and its length is how many sectors a track
    /// holds.
    pub(crate) skew: &'static [u32],
}

impl Dpb {
    /// Where the directory begins within the volume.
    fn directory_offset(&self) -> u64 {
        u64::from(self.reserved_tracks) * u64::from(self.track_bytes)
    }

    /// How many bytes the directory occupies.
    fn directory_bytes(&self) -> u64 {
        u64::from(self.directory_entries) * ENTRY_BYTES as u64
    }

    /// Whether allocation pointers are 16 bits wide, which CP/M decides
    /// by the block count and never records.
    fn wide_pointers(&self) -> bool {
        self.blocks > 256
    }

    /// How many records one allocation block holds.
    fn records_per_block(&self) -> u32 {
        self.block_bytes / RECORD_BYTES
    }

    /// The check a declared layout passes before anything is read
    /// through it. A DPB that cannot describe a CP/M volume is refused
    /// naming the field, rather than producing a directory that reads
    /// plausibly and is not there.
    fn check(&self) -> Result<()> {
        if !matches!(self.block_bytes, 1024 | 2048 | 4096 | 8192 | 16384) {
            return Err(refuse(format!(
                "the declared layout states an allocation block of {} bytes; CP/M 2.2 \
                 admits 1024, 2048, 4096, 8192 and 16384 and nothing between them",
                self.block_bytes
            )));
        }
        if self.directory_entries == 0 {
            return Err(refuse(
                "the declared layout states a directory of no entries, which no CP/M \
                 volume has",
            ));
        }
        if self.blocks == 0 {
            return Err(refuse(
                "the declared layout states a volume of no allocation blocks",
            ));
        }
        if self.track_bytes == 0 || self.records_per_track == 0 {
            return Err(refuse(
                "the declared layout states a track of no length, so the reserved \
                 tracks reach no offset",
            ));
        }
        if self.skew.is_empty() {
            return Err(refuse(
                "the declared layout states no sector map; a track of no sectors \
                 cannot be walked, and an unskewed family declares the identity map \
                 rather than nothing",
            ));
        }
        if self.track_bytes % self.skew.len() as u32 != 0 {
            return Err(refuse(format!(
                "the declared layout puts {} sectors in a track of {} bytes, which do \
                 not divide",
                self.skew.len(),
                self.track_bytes
            )));
        }
        let mut seen: Vec<u32> = self.skew.to_vec();
        seen.sort_unstable();
        seen.dedup();
        if seen.len() != self.skew.len()
            || seen.last().copied().unwrap_or(0) as usize != self.skew.len() - 1
        {
            return Err(refuse(
                "the declared sector map is not a permutation of the track's sectors: \
                 it either repeats one or reaches past the track, and either way some \
                 sector would be read twice and another never",
            ));
        }
        Ok(())
    }

    /// The bytes in one sector, which the map's length fixes.
    fn sector_bytes(&self) -> u32 {
        self.track_bytes / self.skew.len() as u32
    }

    /// Whether the map is the identity, in which case the artifact is
    /// already in the order the records are numbered.
    fn skews(&self) -> bool {
        self.skew
            .iter()
            .enumerate()
            .any(|(logical, physical)| logical as u32 != *physical)
    }

    /// Puts an artifact's sectors into the order CP/M numbers its
    /// records in.
    ///
    /// The reader above works in logical order throughout, so the
    /// translation happens once, here, rather than being threaded
    /// through every offset arithmetic — where one missed application
    /// would read a file that is almost right, which is the worst
    /// failure available.
    fn to_logical(&self, image: &[u8]) -> Vec<u8> {
        if !self.skews() {
            return image.to_vec();
        }
        let sector_bytes = self.sector_bytes() as usize;
        let sectors = self.skew.len();
        let mut out = vec![0u8; image.len()];
        for (track, chunk) in image.chunks(self.track_bytes as usize).enumerate() {
            for (logical, physical) in self.skew.iter().enumerate() {
                let from = *physical as usize * sector_bytes;
                let to = track * self.track_bytes as usize + logical * sector_bytes;
                if from + sector_bytes > chunk.len() || to + sector_bytes > out.len() {
                    continue;
                }
                out[to..to + sector_bytes].copy_from_slice(&chunk[from..from + sector_bytes]);
            }
            debug_assert!(sectors > 0);
        }
        out
    }
}

// ------------------------------------------------- the declared layouts

/// One enrolled layout: what to call it, and the block to read by.
///
/// **The identity names the medium, because that is what the evidence
/// says determines the layout.** The first enrolled block was named for
/// a release as well, on the reasoning that both could vary and neither
/// is on the disk. Reading a second release's distribution disks settled
/// it: Heath CP/M 2.2.02 and 2.2.03 write the *same* layout on the same
/// drive, so a release in the identity would have been a distinction the
/// artifacts do not make, and two names for one block. The drive is what
/// the name claims; which releases it has been confirmed against is
/// recorded in the basis, where it can grow without renaming anything.
pub(crate) struct CpmLayout {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) dpb: Dpb,
    /// How this block came to be believed, carried into the account so
    /// a reader can weigh it (P4).
    pub(crate) basis: &'static str,
}

/// Heath CP/M on the H-17 hard-sectored 5.25-inch disk.
///
/// **Solved from artifacts, then checked against what those artifacts
/// state.** Every value below was first solved against the CP/M 2.2.02
/// distribution disk and confirmed by reading its files back: the
/// directory begins exactly at
/// track 3; a 1024-byte block is the only size for which `BIOS.SYS`'s
/// five blocks fit its 36 records without gross over-allocation; the
/// directory runs to the eighth logical sector, which is 64 entries; and
/// the sector map is what makes `DUMP.ASM` read as assembler source and
/// `ASM.COM` open with a stack load and Digital Research's copyright
/// rather than as interleaved rubbish.
///
/// **The 2.2.03 distribution then read under it unchanged** — all three
/// of its disks, including a four-extent `BIOS.ASM` — which is what
/// moved the release out of this layout's name. Under the identity map
/// those same disks still list a plausible directory and serve
/// `DDT.COM` as `0xe5` fill, which is this format's characteristic
/// failure and the reason the file assertions exist.
///
/// **Every field was then confirmed against the disks' own BIOS.** These
/// are bootable system disks, so each carries in its reserved tracks the
/// disk parameter block the CP/M on it was built with: `SPT=20 BSH=3
/// BLM=7 EXM=0 DSM=91 DRM=63 AL0=C0 AL1=00 CKS=16 OFF=3`. That is 20
/// records to a track, a 1024-byte allocation block, 92 blocks, 64
/// directory entries and 3 reserved tracks — every value below, stated
/// by the recording rather than solved from it.
///
/// **`blocks` is the field that needed it.** CP/M keeps no allocation
/// map on the disk — it rebuilds one from the directory — so no amount
/// of reading the directory pins the volume's size. The files here reach
/// block 88 and the medium allows 92, which bounds the answer without
/// fixing it; `DSM=91` fixes it.
pub(crate) static CPM_HEATH_H17: CpmLayout = CpmLayout {
    id: "cpm-heath-h17",
    name: "Heath CP/M 2.2 (H-17 hard-sectored 5.25-inch)",
    dpb: Dpb {
        records_per_track: 20,
        block_bytes: 1024,
        blocks: 92,
        directory_entries: 64,
        reserved_tracks: 3,
        track_bytes: 2560,
        // Four-way interleave. Consecutive logical sectors sit four
        // physical sectors apart, which is why the directory's second
        // eight entries are found in physical sector 34 and not 31.
        skew: &[0, 4, 8, 2, 6, 1, 5, 9, 3, 7],
    },
    basis: "solved against the CP/M 2.2.02 distribution disk, confirmed by reading its \
            files back, and then confirmed field by field against the disk parameter \
            block those disks carry in their own reserved tracks; the 2.2.03 \
            distribution's three disks read under it unchanged and state the same block",
};

/// Heath CP/M on the soft-sectored 5.25-inch disk.
///
/// **Everything about the volume is the hard-sectored block's, and the
/// sector map is the identity.** Same reserved tracks, same allocation
/// block, same directory, same block count — because it is the same
/// filesystem written by the same release. What differs is where the
/// interleave was put.
///
/// A hard-sectored recording numbers its sectors 1..n in the order they
/// sit on the track, and the drive's BIOS supplies a four-way skew when
/// it reads them. A soft-sectored one writes the interleave into the
/// *sector numbering itself* — the ImageDisk images of this release lay
/// their ids down as 1, 8, 5, 2, 9, 6, 3, 10, 7, 4 — and the BIOS then
/// reads them straight through. The interleave is the same either way;
/// which layer performs it is not.
///
/// So this block declares the identity, and it is the identity for a
/// reason worth stating: by D60 the image adapter has already put the
/// artifact into the order the recording numbers its sectors, and for
/// this recording that is all the translation there was.
///
/// Derived from the soft-sectored CP/M 2.2.03 distribution and confirmed
/// by reading its files back — `ASM.COM` opening with a stack load and
/// Digital Research's copyright, as its hard-sectored twin does.
pub(crate) static CPM_HEATH_SOFT: CpmLayout = CpmLayout {
    id: "cpm-heath-soft",
    name: "Heath CP/M 2.2 (soft-sectored 5.25-inch)",
    dpb: Dpb {
        records_per_track: 20,
        block_bytes: 1024,
        blocks: 92,
        directory_entries: 64,
        reserved_tracks: 3,
        track_bytes: 2560,
        // The identity: the interleave is in the recording's own sector
        // numbering, and the image format resolved it (D60).
        skew: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    },
    basis: "derived from the soft-sectored CP/M 2.2.03 distribution and confirmed by \
            reading its files back; its volume parameters are the hard-sectored block's \
            unchanged — which that block's own on-disk BIOS states — and only the sector \
            translation differs, that recording carrying its interleave in the sector \
            numbering rather than in the drive",
};

/// The layouts a caller may name.
///
/// Two entries, and what separates them is not the release: 2.2.02 and
/// 2.2.03 share a block, while one release recorded two ways does not.
/// Adding a medium is a new entry here and nothing else — which is the
/// whole point of keeping the block declared.
pub(crate) static LAYOUTS: [&CpmLayout; 2] = [&CPM_HEATH_H17, &CPM_HEATH_SOFT];

/// The layout a declaration names, or the refusal listing what there is.
pub(crate) fn layout(id: &str) -> Option<&'static CpmLayout> {
    LAYOUTS.iter().copied().find(|layout| layout.id == id)
}

/// One directory entry, read but not yet joined to its siblings.
#[derive(Debug, Clone)]
struct DirectoryEntry {
    user: u8,
    name: [u8; 8],
    kind: [u8; 3],
    /// The extent this entry carries, assembled from `EX` and `S2` as
    /// CP/M splits it.
    extent: u32,
    /// `RC` — records in this extent.
    records: u32,
    /// The allocation blocks this extent claims, zeros dropped: block 0
    /// holds the directory and is never a file's data, so a zero
    /// pointer is an empty slot rather than a claim.
    blocks: Vec<u32>,
    /// The attribute bits, which CP/M carries in the high bit of each
    /// name and type byte rather than in a field of its own.
    read_only: bool,
    system: bool,
    archived: bool,
}

impl DirectoryEntry {
    /// Reads one 32-byte slot, or `None` where the slot holds no entry.
    ///
    /// A slot whose user byte is `0xe5` is free — that is CP/M's own
    /// deletion mark — and a user number outside the sixteen CP/M
    /// defines is not an entry either.
    fn read(slot: &[u8], dpb: &Dpb) -> Option<Self> {
        debug_assert_eq!(slot.len(), ENTRY_BYTES);
        let user = slot[0];
        if user == UNUSED || user > 15 {
            return None;
        }

        let mut name = [0u8; 8];
        let mut kind = [0u8; 3];
        for (at, slot_byte) in slot[1..9].iter().enumerate() {
            name[at] = slot_byte & 0x7f;
        }
        for (at, slot_byte) in slot[9..12].iter().enumerate() {
            kind[at] = slot_byte & 0x7f;
        }

        // The extent is split across two fields with the record count
        // between them, which is the layout CP/M 2.2 fixed.
        let extent_low = u32::from(slot[12]);
        let extent_high = u32::from(slot[14]);
        let extent = extent_high << 5 | extent_low;
        let records = u32::from(slot[15]);

        let pointers = &slot[16..32];
        let blocks = if dpb.wide_pointers() {
            pointers
                .chunks_exact(2)
                .map(|pair| u32::from(u16::from_le_bytes([pair[0], pair[1]])))
                .filter(|block| *block != 0)
                .collect()
        } else {
            pointers
                .iter()
                .map(|byte| u32::from(*byte))
                .filter(|block| *block != 0)
                .collect()
        };

        Some(Self {
            user,
            name,
            kind,
            extent,
            records,
            blocks,
            read_only: slot[9] & 0x80 != 0,
            system: slot[10] & 0x80 != 0,
            archived: slot[11] & 0x80 != 0,
        })
    }

    /// The `NAME.TYP` this entry states, trailing blanks removed as
    /// CP/M pads with them.
    fn display_name(&self) -> String {
        let name = String::from_utf8_lossy(&self.name).trim_end().to_owned();
        let kind = String::from_utf8_lossy(&self.kind).trim_end().to_owned();
        if kind.is_empty() {
            name
        } else {
            format!("{name}.{kind}")
        }
    }
}

/// One file, assembled from every extent that names it.
#[derive(Debug, Clone)]
struct CpmFile {
    user: u8,
    name: String,
    /// The blocks this file claims, in extent order — which is the
    /// order its bytes are in.
    blocks: Vec<u32>,
    /// The records the last extent states, which is what makes the size
    /// exact to a record rather than to a block.
    last_extent_records: u32,
    extents: u32,
    read_only: bool,
    system: bool,
    archived: bool,
}

impl CpmFile {
    /// The size CP/M can state, which is exact to a 128-byte record and
    /// no finer.
    ///
    /// CP/M records no byte count. A file's length is its full extents
    /// plus the records its last extent claims, and the tail of the
    /// final record is whatever was in the block — conventionally an
    /// end-of-file mark for text, and nothing at all for binaries. The
    /// size here is therefore the recorded one, never a guess at where
    /// the data stopped.
    fn size_bytes(&self) -> u64 {
        u64::from(self.records()) * u64::from(RECORD_BYTES)
    }

    fn records(&self) -> u32 {
        // Every extent but the last is full by construction; the last
        // states its own count.
        self.extents.saturating_sub(1) * 128 + self.last_extent_records
    }

    fn attributes(&self) -> String {
        let mut flags = String::new();
        if self.read_only {
            flags.push_str("R/O ");
        }
        if self.system {
            flags.push_str("SYS ");
        }
        if self.archived {
            flags.push_str("ARC ");
        }
        if flags.is_empty() {
            "DIR R/W".to_owned()
        } else {
            flags.trim_end().to_owned()
        }
    }
}

/// The CP/M catalog, read out of the extent it occupies under one
/// declared layout.
#[derive(Debug)]
pub(crate) struct CpmCatalog {
    image: Vec<u8>,
    dpb: Dpb,
    /// The version whose layout was declared, for the account.
    declared_as: &'static str,
    files: Vec<CpmFile>,
    evidence: Vec<String>,
}

/// The largest extent this reader will take whole (P27), matching the
/// HDOS reader's bound: CP/M 2.2 addresses at most 8 MB in one volume,
/// so the bound is the format's own ceiling rather than an arbitrary
/// one.
const CPM_BOUND: u64 = 8 * 1024 * 1024;

impl CpmCatalog {
    /// Reads the directory one enrolled layout says is there, carrying
    /// how that layout came to be believed into the account.
    pub(crate) fn open_declared(
        volume: &mut dyn crate::io::device::Device,
        layout: &'static CpmLayout,
    ) -> Result<Self> {
        let mut catalog = Self::open(volume, layout.dpb, layout.id)?;
        catalog
            .evidence
            .push(format!("{}: {}", layout.name, layout.basis));
        Ok(catalog)
    }

    /// Reads the directory a declared layout says is there.
    pub(crate) fn open(
        volume: &mut dyn crate::io::device::Device,
        dpb: Dpb,
        declared_as: &'static str,
    ) -> Result<Self> {
        dpb.check()?;

        let length = volume.len();
        if length > CPM_BOUND {
            return Err(Error::categorized_image(
                ErrorCategory::Unsupported,
                PROFILE,
                format!(
                    "this extent is {length} bytes and the CP/M reader is bounded to {CPM_BOUND}"
                ),
            ));
        }

        let directory_end = dpb
            .directory_offset()
            .checked_add(dpb.directory_bytes())
            .ok_or_else(|| refuse("the declared directory runs past what an offset holds"))?;
        if directory_end > length {
            return Err(refuse(format!(
                "the declared layout puts {} directory entries at offset {}, ending at \
                 {directory_end}, and the extent holds {length} bytes: the layout and \
                 the artifact do not describe the same volume",
                dpb.directory_entries,
                dpb.directory_offset(),
            )));
        }

        let mut image = vec![0u8; length as usize];
        volume.read_at(0, &mut image)?;
        // Everything below addresses in CP/M's logical order, so the
        // artifact is put into it once, here.
        let image = dpb.to_logical(&image);

        let directory = &image[dpb.directory_offset() as usize..directory_end as usize];
        let mut entries: Vec<DirectoryEntry> = directory
            .chunks_exact(ENTRY_BYTES)
            .filter_map(|slot| DirectoryEntry::read(slot, &dpb))
            .collect();

        // Extents are joined in the order the directory states them,
        // which is the order the file was written in; the slot order is
        // not that order, so it is the extent number that sorts.
        entries.sort_by_key(|entry| entry.extent);

        let files = assemble(&entries, &dpb)?;

        let evidence = vec![
            format!(
                "read against the declared '{declared_as}' layout, which the volume \
                 itself does not record"
            ),
            format!(
                "{} reserved track(s) of {} bytes place the directory at offset {}",
                dpb.reserved_tracks,
                dpb.track_bytes,
                dpb.directory_offset()
            ),
            format!(
                "{} directory entries over an allocation block of {} bytes, addressed \
                 by {}-bit pointers across {} blocks",
                dpb.directory_entries,
                dpb.block_bytes,
                if dpb.wide_pointers() { 16 } else { 8 },
                dpb.blocks
            ),
            if dpb.skews() {
                format!(
                    "the {}-sector track is skewed, so the artifact was put into CP/M's \
                     logical record order before anything was read from it: {:?}",
                    dpb.skew.len(),
                    dpb.skew
                )
            } else {
                "the declared sector map is the identity, so the artifact is already in \
                 CP/M's logical record order"
                    .to_owned()
            },
            format!(
                "{} file(s) assembled from {} extent(s)",
                files.len(),
                entries.len()
            ),
        ];

        Ok(Self {
            image,
            dpb,
            declared_as,
            files,
            evidence,
        })
    }

    fn entry(file: &CpmFile) -> Entry {
        Entry {
            name: file.name.clone(),
            kind: EntryKind::File,
            size_bytes: file.size_bytes(),
            declared: vec![
                EntryFact::new("user", file.user.to_string()),
                EntryFact::new("size-records", file.records().to_string()),
                EntryFact::new("extents", file.extents.to_string()),
                EntryFact::new("attributes", file.attributes()),
                EntryFact::new("read-only", file.read_only.to_string()),
                EntryFact::new("system", file.system.to_string()),
                EntryFact::new("archived", file.archived.to_string()),
            ],
        }
    }

    fn find(&self, path: &str) -> Option<&CpmFile> {
        self.files
            .iter()
            .find(|file| file.name.eq_ignore_ascii_case(path))
    }
}

/// Joins the extents that name one file into that file.
///
/// A file is identified by its user number and its name together, which
/// is CP/M's own identity: the same name under two user numbers is two
/// files, and nothing here merges them.
fn assemble(entries: &[DirectoryEntry], dpb: &Dpb) -> Result<Vec<CpmFile>> {
    let mut files: Vec<CpmFile> = Vec::new();
    for entry in entries {
        let name = entry.display_name();
        if name.is_empty() {
            return Err(refuse(
                "a directory entry in use states no name at all, which the declared \
                 layout cannot have read correctly",
            ));
        }
        for block in &entry.blocks {
            if *block >= dpb.blocks {
                return Err(refuse(format!(
                    "'{name}' claims allocation block {block} and the declared layout \
                     states {} of them: the layout and the artifact do not describe the \
                     same volume",
                    dpb.blocks
                )));
            }
        }

        match files
            .iter_mut()
            .find(|file| file.user == entry.user && file.name == name)
        {
            Some(file) => {
                file.blocks.extend(entry.blocks.iter().copied());
                file.extents += 1;
                file.last_extent_records = entry.records;
            }
            None => files.push(CpmFile {
                user: entry.user,
                name,
                blocks: entry.blocks.clone(),
                last_extent_records: entry.records,
                extents: 1,
                read_only: entry.read_only,
                system: entry.system,
                archived: entry.archived,
            }),
        }
    }
    Ok(files)
}

impl Catalog for CpmCatalog {
    fn entries(&self, path: &str) -> Result<Vec<Entry>> {
        if !crate::filesystem::space::path_is_root(path) {
            return Err(Error::categorized_image(
                ErrorCategory::NotDirectory,
                PROFILE,
                format!("'{path}' holds no names; the CP/M directory is flat"),
            ));
        }
        Ok(self.files.iter().map(Self::entry).collect())
    }

    fn stat(&self, path: &str) -> Result<Option<Entry>> {
        Ok(self.find(path).map(Self::entry))
    }

    fn label(&self) -> Option<crate::filesystem::VolumeLabel> {
        // CP/M 2.2 has no volume label. The disc label a user wrote on
        // the sleeve is not a field, and a synthesized one would be this
        // library inventing a fact.
        None
    }

    fn evidence(&self) -> Vec<String> {
        self.evidence.clone()
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let file = self.find(path).ok_or_else(|| {
            Error::categorized_image(
                ErrorCategory::NotFound,
                PROFILE,
                format!("'{path}' is not in the directory this layout reads"),
            )
        })?;

        let mut wanted = file.records();
        let mut bytes = Vec::with_capacity(file.size_bytes() as usize);
        for block in &file.blocks {
            if wanted == 0 {
                break;
            }
            let take = wanted.min(self.dpb.records_per_block());
            // Allocation blocks are numbered from the start of the data
            // area, not the start of the artifact: block 0 is the first
            // block after the reserved tracks, which is where the
            // directory itself sits.
            let start =
                self.dpb.directory_offset() + u64::from(*block) * u64::from(self.dpb.block_bytes);
            let end = start + u64::from(take) * u64::from(RECORD_BYTES);
            if end > self.image.len() as u64 {
                return Err(Error::categorized_image(
                    ErrorCategory::Unavailable,
                    PROFILE,
                    format!(
                        "'{path}' reaches allocation block {block}, which the declared \
                         '{}' layout places at {start}..{end} and the artifact ends at \
                         {}: the file runs past what this volume holds",
                        self.declared_as,
                        self.image.len()
                    ),
                ));
            }
            bytes.extend_from_slice(&self.image[start as usize..end as usize]);
            wanted -= take;
        }

        if wanted > 0 {
            return Err(Error::categorized_image(
                ErrorCategory::Unavailable,
                PROFILE,
                format!(
                    "'{path}' states {} records and its extents claim blocks for only \
                     {}: the directory is short of its own file, and nothing is filled in",
                    file.records(),
                    file.records() - wanted
                ),
            ));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layout to build test volumes against. It is a *test* layout and
    /// deliberately not any shipped machine's: what these tests check is
    /// the grammar above the DPB, which is the part that does not vary.
    fn dpb() -> Dpb {
        Dpb {
            records_per_track: 20,
            block_bytes: 1024,
            blocks: 200,
            directory_entries: 64,
            reserved_tracks: 2,
            track_bytes: 2560,
            // The identity: these tests exercise the grammar above the
            // block, and the skew has its own test below.
            skew: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        }
    }

    struct Volume(Vec<u8>);

    impl crate::io::device::Device for Volume {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            buf.copy_from_slice(&self.0[at..at + buf.len()]);
            Ok(())
        }

        fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
            unreachable!("the reader never writes")
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    /// A volume whose directory is empty, sized to the layout.
    fn blank(dpb: &Dpb) -> Vec<u8> {
        let mut image = vec![0u8; (dpb.blocks * dpb.block_bytes) as usize];
        let start = dpb.directory_offset() as usize;
        let end = start + dpb.directory_bytes() as usize;
        image[start..end].fill(UNUSED);
        image
    }

    /// Writes one directory entry into slot `at`.
    #[allow(clippy::too_many_arguments)]
    fn write_entry(
        image: &mut [u8],
        dpb: &Dpb,
        at: usize,
        user: u8,
        name: &[u8; 11],
        extent: u32,
        records: u32,
        blocks: &[u8],
    ) {
        let start = dpb.directory_offset() as usize + at * ENTRY_BYTES;
        let slot = &mut image[start..start + ENTRY_BYTES];
        slot.fill(0);
        slot[0] = user;
        slot[1..12].copy_from_slice(name);
        slot[12] = (extent & 0x1f) as u8;
        slot[14] = (extent >> 5) as u8;
        slot[15] = records as u8;
        slot[16..16 + blocks.len()].copy_from_slice(blocks);
    }

    /// Lays a file's bytes into the blocks an entry claims.
    ///
    /// Block numbering starts at the data area, exactly as the reader
    /// addresses it: block 0 is the first block after the reserved
    /// tracks, which is where the directory sits.
    fn write_blocks(image: &mut [u8], dpb: &Dpb, blocks: &[u8], bytes: &[u8]) {
        let mut written = 0usize;
        for block in blocks {
            if written >= bytes.len() {
                break;
            }
            let start =
                dpb.directory_offset() as usize + *block as usize * dpb.block_bytes as usize;
            let take = (bytes.len() - written).min(dpb.block_bytes as usize);
            image[start..start + take].copy_from_slice(&bytes[written..written + take]);
            written += take;
        }
    }

    #[test]
    fn a_directory_reads_back_the_files_it_states() {
        let dpb = dpb();
        let mut image = blank(&dpb);
        write_entry(&mut image, &dpb, 0, 0, b"README  TXT", 0, 3, &[10, 11]);
        write_entry(&mut image, &dpb, 1, 0, b"STAT    COM", 0, 8, &[12]);
        let payload: Vec<u8> = (0..3 * 128).map(|at| (at % 251) as u8).collect();
        write_blocks(&mut image, &dpb, &[10, 11], &payload);

        let catalog = CpmCatalog::open(&mut Volume(image), dpb, "test").expect("the layout reads");
        let entries = catalog.entries("").expect("the root lists");
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["README.TXT", "STAT.COM"]);

        // The size is exact to a record, which is the finest CP/M states.
        assert_eq!(entries[0].size_bytes, 3 * 128);
        assert_eq!(entries[1].size_bytes, 8 * 128);
        assert_eq!(catalog.read_file("README.TXT").expect("it reads"), payload);
    }

    #[test]
    fn extents_of_one_file_join_in_the_order_the_directory_states() {
        // A file longer than one extent: the second extent continues it,
        // and its record count is what makes the total exact.
        let dpb = dpb();
        let mut image = blank(&dpb);
        let full: Vec<u8> = (0..128 * 128).map(|at| (at % 253) as u8).collect();
        let tail: Vec<u8> = (0..2 * 128).map(|at| (at % 241) as u8).collect();
        // Extent 0 is full at 128 records, which over a 1 KB block is
        // all sixteen of the entry's pointers.
        let first: Vec<u8> = (20..36).collect();
        write_entry(&mut image, &dpb, 3, 0, b"BIG     DAT", 0, 128, &first);
        write_entry(&mut image, &dpb, 1, 0, b"BIG     DAT", 1, 2, &[40]);
        write_blocks(&mut image, &dpb, &first, &full);
        write_blocks(&mut image, &dpb, &[40], &tail);

        let catalog = CpmCatalog::open(&mut Volume(image), dpb, "test").expect("the layout reads");
        let entries = catalog.entries("").expect("the root lists");
        assert_eq!(entries.len(), 1, "two extents are one file");
        assert_eq!(entries[0].size_bytes, (128 + 2) * 128);

        let read = catalog.read_file("BIG.DAT").expect("it reads");
        assert_eq!(read.len(), (128 + 2) * 128);
        assert_eq!(&read[..full.len()], &full[..]);
        assert_eq!(&read[full.len()..], &tail[..]);
    }

    #[test]
    fn the_same_name_under_two_user_numbers_is_two_files() {
        let dpb = dpb();
        let mut image = blank(&dpb);
        write_entry(&mut image, &dpb, 0, 0, b"SAME    TXT", 0, 1, &[10]);
        write_entry(&mut image, &dpb, 1, 3, b"SAME    TXT", 0, 1, &[11]);

        let catalog = CpmCatalog::open(&mut Volume(image), dpb, "test").expect("the layout reads");
        let entries = catalog.entries("").expect("the root lists");
        assert_eq!(entries.len(), 2);
        let users: Vec<&str> = entries
            .iter()
            .map(|entry| {
                entry
                    .declared
                    .iter()
                    .find(|fact| fact.key == "user")
                    .expect("the user is declared")
                    .value
                    .as_str()
            })
            .collect();
        assert_eq!(users, ["0", "3"]);
    }

    #[test]
    fn the_attribute_bits_are_read_off_the_name_and_carried() {
        let dpb = dpb();
        let mut image = blank(&dpb);
        let mut name = *b"SYSTEM  COM";
        name[8] |= 0x80; // R/O, carried in the first type byte
        name[9] |= 0x80; // SYS
        write_entry(&mut image, &dpb, 0, 0, &name, 0, 1, &[10]);

        let catalog = CpmCatalog::open(&mut Volume(image), dpb, "test").expect("the layout reads");
        let entries = catalog.entries("").expect("the root lists");
        // The high bits are attributes and are not part of the name.
        assert_eq!(entries[0].name, "SYSTEM.COM");
        let fact = |key: &str| {
            entries[0]
                .declared
                .iter()
                .find(|fact| fact.key == key)
                .expect("declared")
                .value
                .clone()
        };
        assert_eq!(fact("read-only"), "true");
        assert_eq!(fact("system"), "true");
        assert_eq!(fact("attributes"), "R/O SYS");
    }

    #[test]
    fn a_deleted_entry_is_not_a_file() {
        let dpb = dpb();
        let mut image = blank(&dpb);
        write_entry(&mut image, &dpb, 0, 0, b"GONE    TXT", 0, 1, &[10]);
        // CP/M deletes by stamping the user byte, leaving the rest
        // intact — which is why the name is still legible and is still
        // not a file.
        image[dpb.directory_offset() as usize] = UNUSED;

        let catalog = CpmCatalog::open(&mut Volume(image), dpb, "test").expect("the layout reads");
        assert!(catalog.entries("").expect("the root lists").is_empty());
    }

    #[test]
    fn a_layout_the_artifact_cannot_bear_is_refused_rather_than_read() {
        // The directory the layout declares runs past the extent. That
        // is the layout and the artifact disagreeing, and there is no
        // reading of the bytes that resolves it.
        let dpb = dpb();
        let mut image = blank(&dpb);
        image.truncate(dpb.directory_offset() as usize + 16);
        let error = CpmCatalog::open(&mut Volume(image), dpb, "test")
            .expect_err("the layout does not fit the artifact");
        assert!(
            error
                .to_string()
                .contains("do not describe the same volume"),
            "{error}"
        );
    }

    #[test]
    fn a_block_outside_the_declared_volume_is_refused_by_name() {
        let dpb = dpb();
        let mut image = blank(&dpb);
        // 250 is past the 200 blocks the layout declares.
        write_entry(&mut image, &dpb, 0, 0, b"WRONG   DAT", 0, 1, &[250]);
        let error = CpmCatalog::open(&mut Volume(image), dpb, "test")
            .expect_err("a block outside the volume is refused");
        assert!(
            error.to_string().contains("allocation block 250"),
            "{error}"
        );
    }

    #[test]
    fn an_impossible_block_size_is_refused_before_anything_is_read() {
        let mut dpb = dpb();
        dpb.block_bytes = 1536;
        let error = CpmCatalog::open(&mut Volume(vec![0u8; 4096]), dpb, "test")
            .expect_err("CP/M has no 1536-byte block");
        assert!(error.to_string().contains("1024, 2048"), "{error}");
    }

    #[test]
    fn a_skewed_layout_is_put_into_logical_order_before_it_is_read() {
        // The failure this guards against is the quiet one: under a
        // wrong map the directory still lists, because its first sector
        // is where every candidate agrees, and only the file contents
        // come back interleaved. So the assertion is on the contents.
        let skewed = Dpb {
            skew: &[0, 4, 8, 2, 6, 1, 5, 9, 3, 7],
            ..dpb()
        };
        let plain = dpb();

        // Build the volume in logical order, then interleave it into the
        // physical order such a layout would have been written in.
        let mut logical = blank(&plain);
        write_entry(&mut logical, &plain, 0, 0, b"SKEWED  DAT", 0, 8, &[10]);
        let payload: Vec<u8> = (0..8 * 128).map(|at| (at % 251) as u8).collect();
        write_blocks(&mut logical, &plain, &[10], &payload);

        let sector = skewed.sector_bytes() as usize;
        let track = skewed.track_bytes as usize;
        let mut physical = vec![0u8; logical.len()];
        for (index, chunk) in logical.chunks(track).enumerate() {
            for (from, to) in skewed.skew.iter().enumerate() {
                let source = from * sector;
                let target = index * track + *to as usize * sector;
                if source + sector <= chunk.len() && target + sector <= physical.len() {
                    physical[target..target + sector]
                        .copy_from_slice(&chunk[source..source + sector]);
                }
            }
        }

        let catalog =
            CpmCatalog::open(&mut Volume(physical.clone()), skewed, "test").expect("it reads");
        assert_eq!(
            catalog.read_file("SKEWED.DAT").expect("the file reads"),
            payload,
            "the declared map put the artifact back into record order"
        );

        // And the same artifact read as though it were not skewed gives
        // back something else entirely — which is the whole reason the
        // map is a declared parameter rather than an assumption.
        let unskewed = CpmCatalog::open(&mut Volume(physical), plain, "test");
        let wrong = unskewed
            .map(|catalog| catalog.read_file("SKEWED.DAT").ok())
            .ok()
            .flatten();
        assert_ne!(
            wrong.as_deref(),
            Some(payload.as_slice()),
            "reading a skewed artifact in physical order cannot give the right bytes"
        );
    }

    #[test]
    fn a_sector_map_that_is_not_a_permutation_is_refused() {
        let broken = Dpb {
            skew: &[0, 4, 8, 2, 6, 1, 5, 9, 3, 3],
            ..dpb()
        };
        let error = CpmCatalog::open(&mut Volume(blank(&dpb())), broken, "test")
            .expect_err("a repeated sector reads one twice and another never");
        assert!(error.to_string().contains("not a permutation"), "{error}");
    }

    #[test]
    fn a_wide_pointer_volume_reads_its_pointers_two_bytes_at_a_time() {
        // Past 256 blocks CP/M switches pointer width, and nothing on
        // the disk says so — the block count in the layout is what
        // decides it.
        let dpb = Dpb {
            blocks: 400,
            ..dpb()
        };
        assert!(dpb.wide_pointers());
        let mut image = blank(&dpb);
        // Block 300 as a little-endian pair.
        write_entry(&mut image, &dpb, 0, 0, b"WIDE    DAT", 0, 1, &[0x2c, 0x01]);
        let payload: Vec<u8> = (0..128).map(|at| at as u8).collect();
        let start = dpb.directory_offset() as usize + 300 * dpb.block_bytes as usize;
        image[start..start + 128].copy_from_slice(&payload);

        let catalog = CpmCatalog::open(&mut Volume(image), dpb, "test").expect("the layout reads");
        assert_eq!(catalog.read_file("WIDE.DAT").expect("it reads"), payload);
    }
}
