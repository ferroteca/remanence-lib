// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Native VDI driver — the VirtualBox Disk Image container — written from
//! the published format description. It presents the virtual disk as a
//! [`Device`], exactly as the qcow2 driver does, so a VDI opens,
//! identifies, inspects, reads and writes through the delivered device stack
//! unchanged.
//!
//! The support claim is validated before anything else is touched (P8):
//! major version 1, minor 0 or 1, which are the two shapes of the same
//! header — the fields this driver reads sit at the same offsets in both,
//! and 1.1's additions are trailing bytes it never touches. Past the
//! version the claim is enumerated (P3): the **dynamically allocated**,
//! **fixed** and **differencing** image types are read and written by
//! name, and the one other type the format defines — undo — is refused by
//! name rather than attempted, as are per-block extra data and any image
//! flag this release does not model.
//!
//! A differencing chain composes for reading and writing (U6): a block the
//! top image never allocated reads through to its parent, and writes
//! allocate copy-on-write into the top image only, which is never the
//! parent. A missing parent, a cycle, a chain past [`MAX_CHAIN_LENGTH`]
//! files, and a parent whose own version or image type falls outside the
//! claim are refused by name at the open. Every parent is claimed
//! immutable for the chain's life (P7).
//!
//! **The format records the parent's identity and no path at all**, which
//! is what makes resolution different from qcow2's: the parent is searched
//! for by identity rather than dereferenced from a name, and the identity
//! the child declares is what checks the file that resolution found — see
//! [`resolve_parent`]. A file standing where the parent should be whose
//! identity does not match is a named refusal, never a substitute read in
//! its place.
//!
//! The block map is the format's own mapping and stays in the file: an
//! entry is read where it is needed and never held resident (P27), so the
//! driver carries no mutable state of its own and a commit that does not
//! land has nothing in memory to put back. An entry marking a block
//! unallocated ([`BLOCK_FREE`]) reads as the parent's bytes, or as zeroes
//! where there is no parent; an entry marking one discarded
//! ([`BLOCK_ZERO`]) reads as zeroes and masks the parent, because the
//! format keeps the two distinct. Neither is ever confused with a block
//! that is allocated and happens to hold zeroes. Allocating a block
//! belongs to the write path alone, which the device stack reaches inside
//! commit — never during a read.

use std::path::{Path, PathBuf};

use crate::device::{AccessIntent, Device, MediumDevice};
use crate::error::{Error, Result};

/// The container signature, at [`SIGNATURE_AT`] rather than at the start:
/// the format's first 64 bytes are a human-readable creator line.
pub(crate) const VDI_SIGNATURE: [u8; 4] = 0xbeda_107fu32.to_le_bytes();

pub(crate) const SIGNATURE_AT: usize = 0x40;
pub(crate) const VERSION_AT: usize = 0x44;

/// The major version this release claims (P8).
pub(crate) const SUPPORTED_MAJOR: u32 = 1;
/// The highest minor of that major this release claims (P8).
pub(crate) const SUPPORTED_MINOR_CEILING: u32 = 1;

/// A block the image never allocated: it reads as zeroes.
const BLOCK_FREE: u32 = 0xffff_ffff;
/// A block the image allocated and then discarded: it reads as zeroes
/// too, and the format keeps it distinct from [`BLOCK_FREE`].
const BLOCK_ZERO: u32 = 0xffff_fffe;

/// The one image flag this release models. It asks that a newly expanded
/// block be filled with zeroes, which this driver does unconditionally,
/// so the bit is satisfied by construction rather than acted on.
const FLAG_ZERO_EXPAND: u32 = 0x0000_0100;

/// The longest differencing chain this release claims, counted in files
/// with the top image included. A deeper chain is refused by name (P3),
/// never walked partway.
pub(crate) const MAX_CHAIN_LENGTH: usize = 16;

/// The most VDI files this release examines in one directory while
/// searching for a parent by identity. A directory holding more is
/// refused by name rather than searched partway (P3).
const MAX_PARENT_CANDIDATES: usize = 1024;

const HEADER_SIZE_AT: usize = 0x48;
const IMAGE_TYPE_AT: usize = 0x4c;
const FLAGS_AT: usize = 0x50;
const BLOCK_MAP_OFFSET_AT: usize = 0x154;
const DATA_OFFSET_AT: usize = 0x158;
const DISK_SIZE_AT: usize = 0x170;
const BLOCK_SIZE_AT: usize = 0x178;
const BLOCK_EXTRA_AT: usize = 0x17c;
const BLOCK_COUNT_AT: usize = 0x180;
const BLOCKS_ALLOCATED_AT: usize = 0x184;
/// The identity this image was created with — what a differencing child
/// names when it names this file as its parent.
const UUID_CREATE_AT: usize = 0x188;
/// The identity of the image this one differences against, all zeroes
/// where there is none. The modification stamps that sit beside these two
/// are outside this release's claim and are neither read nor written.
const UUID_LINKAGE_AT: usize = 0x1a8;

/// Everything the driver reads out of the header, which ends at the
/// linkage identity. The leading bytes are the creator line, the
/// signature and the version; the trailing ones are legacy geometry and
/// the two stamps above.
const HEADER_READ_BYTES: usize = UUID_LINKAGE_AT + 16;

/// The smallest declared header size the fields above fit inside.
/// Version 1.0 declares 0x180 and 1.1 declares 0x190, both past it.
const MINIMUM_DECLARED_HEADER: u64 = (HEADER_READ_BYTES - HEADER_SIZE_AT) as u64;

/// The largest block size this release claims. The format's own default
/// is 1 MiB; a declared size past this is refused rather than trusted.
const MAXIMUM_BLOCK_SIZE: u64 = 64 * 1024 * 1024;

/// The bounded buffer a fresh block's zero fill is written through, so
/// allocation costs the same whatever the block size (P27).
const ZERO_FILL_CHUNK: usize = 64 * 1024;

fn le32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4 bytes"))
}

fn le64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("8 bytes"))
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::invalid_image("vdi", reason)
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::categorized_image(crate::ErrorCategory::Unsupported, "vdi", reason)
}

/// A 16-byte VDI identity. The format stores the same bytes a Microsoft
/// GUID does, so it renders with its first three groups read
/// little-endian — which is what the format's own tooling prints, and
/// what a differencing image is named after.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct VdiUuid([u8; 16]);

impl VdiUuid {
    fn read(raw: &[u8], offset: usize) -> Self {
        Self(raw[offset..offset + 16].try_into().expect("16 bytes"))
    }

    /// The all-zero identity, which the format spells "none" with.
    fn is_nil(self) -> bool {
        self.0.iter().all(|&byte| byte == 0)
    }
}

impl std::fmt::Display for VdiUuid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let byte = &self.0;
        write!(
            formatter,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            byte[3], byte[2], byte[1], byte[0], byte[5], byte[4], byte[7], byte[6], byte[8],
            byte[9], byte[10], byte[11], byte[12], byte[13], byte[14], byte[15]
        )
    }
}

impl std::fmt::Debug for VdiUuid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

/// The image types this release claims, by name (P3). The format defines
/// one more — undo — and it is refused by name at the header rather than
/// read as one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VdiImageType {
    /// Blocks exist only where they were written; the block map says
    /// which.
    Dynamic,
    /// Every block is present from creation, the block map addressing
    /// each in turn.
    Fixed,
    /// Blocks exist only where this image was written since it was
    /// branched; every other block belongs to the parent it names.
    Differencing,
}

impl VdiImageType {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Dynamic => "dynamically allocated",
            Self::Fixed => "fixed",
            Self::Differencing => "differencing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VdiHeader {
    pub major: u32,
    pub minor: u32,
    pub image_type: VdiImageType,
    /// Where the block map starts, in bytes from the start of the file.
    pub block_map_offset: u64,
    /// Where the first data block starts.
    pub data_offset: u64,
    /// The virtual disk's size — what the guest sees.
    pub disk_size: u64,
    pub block_size: u64,
    pub block_count: u32,
    /// This image's own identity — what a differencing child names.
    pub create_id: VdiUuid,
    /// The identity of the parent a differencing image differences
    /// against; nil on every other type, which names none.
    pub parent_id: VdiUuid,
}

impl VdiHeader {
    /// Parses and validates a header, running the P8 version gate before
    /// any other field is trusted to mean what this release thinks it
    /// means, and the enumerated image-type claim immediately after.
    pub(crate) fn parse(device: &mut dyn Device) -> Result<Self> {
        if device.len() < HEADER_READ_BYTES as u64 {
            return Err(invalid("file too small for a VDI header"));
        }
        let mut raw = [0u8; HEADER_READ_BYTES];
        device.read_at(0, &mut raw)?;

        if raw[SIGNATURE_AT..SIGNATURE_AT + 4] != VDI_SIGNATURE {
            return Err(invalid("missing VDI signature"));
        }

        // P8: the version gate comes first.
        let (major, minor) = version(&raw);
        if major < SUPPORTED_MAJOR {
            return Err(unsupported(format!(
                "unsupported VDI version {major}.{minor} (versions \
                 {SUPPORTED_MAJOR}.0 and \
                 {SUPPORTED_MAJOR}.{SUPPORTED_MINOR_CEILING} are supported)"
            )));
        }
        if major > SUPPORTED_MAJOR || minor > SUPPORTED_MINOR_CEILING {
            return Err(unsupported(format!(
                "VDI version {major}.{minor} is newer than this release \
                 supports (ceiling: version \
                 {SUPPORTED_MAJOR}.{SUPPORTED_MINOR_CEILING}); refusing to \
                 touch it"
            )));
        }

        let declared_header = le32(&raw, HEADER_SIZE_AT) as u64;
        if declared_header < MINIMUM_DECLARED_HEADER {
            return Err(invalid(format!(
                "declared header size {declared_header} is shorter than the \
                 {MINIMUM_DECLARED_HEADER} bytes version {major}.{minor} \
                 defines"
            )));
        }

        // P3: the image type is an enumerated claim, and what falls
        // outside it names itself rather than being attempted.
        let declared_type = le32(&raw, IMAGE_TYPE_AT);
        let image_type = match declared_type {
            1 => VdiImageType::Dynamic,
            2 => VdiImageType::Fixed,
            3 => {
                return Err(unsupported(
                    "VDI image type 3 (undo) is outside this release's claim; \
                     the dynamically allocated, fixed and differencing types \
                     are supported",
                ));
            }
            4 => VdiImageType::Differencing,
            other => {
                return Err(invalid(format!(
                    "VDI image type {other} is not a type the format defines"
                )));
            }
        };

        let flags = le32(&raw, FLAGS_AT);
        if flags & !FLAG_ZERO_EXPAND != 0 {
            return Err(unsupported(format!(
                "VDI image flags 0x{flags:08x} carry a bit beyond this \
                 release's claim; refusing to touch the image"
            )));
        }

        let block_extra = le32(&raw, BLOCK_EXTRA_AT);
        if block_extra != 0 {
            return Err(unsupported(format!(
                "VDI images carrying {block_extra} bytes of extra data per \
                 block are beyond this release's claim"
            )));
        }

        let block_map_offset = le32(&raw, BLOCK_MAP_OFFSET_AT) as u64;
        let data_offset = le32(&raw, DATA_OFFSET_AT) as u64;
        let disk_size = le64(&raw, DISK_SIZE_AT);
        let block_size = le32(&raw, BLOCK_SIZE_AT) as u64;
        let block_count = le32(&raw, BLOCK_COUNT_AT);
        let blocks_allocated = le32(&raw, BLOCKS_ALLOCATED_AT);

        if block_size == 0 || !block_size.is_power_of_two() || block_size < 512 {
            return Err(invalid(format!(
                "implausible block size {block_size} (the format's blocks are \
                 a power of two, 512 bytes or larger)"
            )));
        }
        if block_size > MAXIMUM_BLOCK_SIZE {
            return Err(unsupported(format!(
                "block size {block_size} is past the {MAXIMUM_BLOCK_SIZE} \
                 bytes this release claims"
            )));
        }
        if blocks_allocated > block_count {
            return Err(invalid(format!(
                "the header accounts for {blocks_allocated} allocated blocks \
                 in an image of {block_count}"
            )));
        }
        let covered = (block_count as u64)
            .checked_mul(block_size)
            .ok_or_else(|| invalid("the declared block count overflows the image"))?;
        if covered < disk_size {
            return Err(invalid(format!(
                "{block_count} blocks of {block_size} bytes cover {covered} \
                 bytes, short of the declared {disk_size}-byte disk"
            )));
        }

        let header_end = HEADER_SIZE_AT as u64 + declared_header;
        if block_map_offset < header_end {
            return Err(invalid(format!(
                "the block map at {block_map_offset} overlaps the header, \
                 which ends at {header_end}"
            )));
        }
        let block_map_end = block_map_offset
            .checked_add(block_count as u64 * 4)
            .ok_or_else(|| invalid("the block map overflows the image"))?;
        if block_map_end > data_offset {
            return Err(invalid(format!(
                "the block map ends at {block_map_end}, past the data area \
                 at {data_offset}"
            )));
        }
        if block_map_end > device.len() {
            return Err(invalid("the block map lies past the end of the file"));
        }

        // A block the image says it holds must actually be in the file:
        // every block for a fixed image, the allocated ones for a dynamic
        // or differencing one (P6 — the contradiction is sought before
        // anything is read).
        let present = match image_type {
            VdiImageType::Fixed => block_count as u64,
            VdiImageType::Dynamic | VdiImageType::Differencing => blocks_allocated as u64,
        };
        let data_end = present
            .checked_mul(block_size)
            .and_then(|bytes| data_offset.checked_add(bytes))
            .ok_or_else(|| invalid("the data area overflows the image"))?;
        if data_end > device.len() {
            return Err(invalid(format!(
                "the image declares {present} {} block(s) ending at \
                 {data_end}, past the {}-byte file",
                image_type.name(),
                device.len()
            )));
        }

        // The identities, last, so a file that is not the shape this
        // release claims never has one read out of it. A differencing
        // image that names no parent contradicts its own type (P6).
        let create_id = VdiUuid::read(&raw, UUID_CREATE_AT);
        let parent_id = VdiUuid::read(&raw, UUID_LINKAGE_AT);
        if image_type == VdiImageType::Differencing && parent_id.is_nil() {
            return Err(invalid(
                "a differencing image names no parent: its linkage identity is \
                 all zeroes",
            ));
        }

        Ok(Self {
            major,
            minor,
            image_type,
            block_map_offset,
            data_offset,
            disk_size,
            block_size,
            block_count,
            create_id,
            parent_id,
        })
    }
}

/// The major and minor a header's packed version field declares. Split
/// out because the probe reads it from a bounded prefix, before any
/// device exists to parse a whole header from (P27).
pub(crate) fn version(prefix: &[u8]) -> (u32, u32) {
    let packed = le32(prefix, VERSION_AT);
    (packed >> 16, packed & 0xffff)
}

/// The virtual disk a VDI file describes, as a [`Device`].
#[derive(Debug)]
pub(crate) struct Vdi<D: Device> {
    device: D,
    header: VdiHeader,
    /// The image this one's unallocated blocks fall through to (U6).
    /// Only a differencing image carries one, and it is only ever read:
    /// no write path in this module reaches past [`Self::device`].
    parent: Option<Box<Vdi<D>>>,
}

impl<D: Device> Vdi<D> {
    /// Opens a standalone image. A differencing image is refused here:
    /// composing the chain takes the containing file's path, which only
    /// [`open_chain`] has.
    pub(crate) fn open(mut device: D) -> Result<Self> {
        let header = VdiHeader::parse(&mut device)?;
        if header.image_type == VdiImageType::Differencing {
            return Err(unsupported(format!(
                "image names parent {}; a standalone open does not compose \
                 the differencing chain",
                header.parent_id
            )));
        }
        Ok(Self::assemble(device, header, None))
    }

    /// Builds the driver over an already-parsed header and, for a chain
    /// member, the parent its unallocated blocks fall through to.
    pub(crate) fn assemble(
        device: D,
        header: VdiHeader,
        parent: Option<Box<Vdi<D>>>,
    ) -> Self {
        Self {
            device,
            header,
            parent,
        }
    }

    pub(crate) fn header(&self) -> &VdiHeader {
        &self.header
    }

    /// The host device the image lives in — for a chain, the top image
    /// alone, which is the only file writes ever reach.
    pub(crate) fn host_mut(&mut self) -> &mut D {
        &mut self.device
    }

    /// The block map entry for the block holding `guest_offset`. Read
    /// from the file where it is needed rather than from a resident copy
    /// (P27): the map is the format's own state, and keeping it there is
    /// what leaves this driver with none of its own.
    fn block_map_entry(&mut self, block: u32) -> Result<u32> {
        let mut raw = [0u8; 4];
        self.device
            .read_at(self.header.block_map_offset + block as u64 * 4, &mut raw)?;
        Ok(u32::from_le_bytes(raw))
    }

    /// Where an allocated block's data sits, checking the index the map
    /// gave against what the image says it holds.
    fn data_at(&self, entry: u32) -> Result<u64> {
        if entry >= self.header.block_count {
            return Err(invalid(format!(
                "a block map entry addresses data block {entry}, beyond the \
                 {} block(s) the image declares",
                self.header.block_count
            )));
        }
        Ok(self.header.data_offset + entry as u64 * self.header.block_size)
    }

    /// How many blocks the image has allocated so far, read from the
    /// header for the same reason the map is: it is the format's own
    /// accounting, and a commit that does not land puts it back with
    /// every other host write.
    fn blocks_allocated(&mut self) -> Result<u32> {
        let mut raw = [0u8; 4];
        self.device
            .read_at(BLOCKS_ALLOCATED_AT as u64, &mut raw)?;
        Ok(u32::from_le_bytes(raw))
    }

    fn read_block(&mut self, guest_offset: u64, buf: &mut [u8]) -> Result<()> {
        let block_size = self.header.block_size;
        let within = guest_offset % block_size;
        debug_assert!(within + buf.len() as u64 <= block_size);

        let block = (guest_offset / block_size) as u32;
        let entry = self.block_map_entry(block)?;
        if entry == BLOCK_FREE {
            // The image never allocated this block, so it holds none of
            // it: the parent shows through (U6), and zeroes stand where
            // there is no parent.
            return self.read_parent(guest_offset, buf);
        }
        if entry == BLOCK_ZERO {
            // Allocated and then discarded. The format keeps this
            // distinct from never-allocated, so it reads as zeroes and
            // masks the parent rather than falling through to it.
            buf.fill(0);
            return Ok(());
        }
        let at = self.data_at(entry)?;
        self.device.read_at(at + within, buf)
    }

    /// Reads from the parent, zero-filling wherever the chain has no
    /// bytes: with no parent at all, and past a shorter parent's end.
    fn read_parent(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let Some(parent) = self.parent.as_mut() else {
            buf.fill(0);
            return Ok(());
        };
        let parent_size = parent.header.disk_size;
        if offset >= parent_size {
            buf.fill(0);
            return Ok(());
        }
        let take = (parent_size - offset).min(buf.len() as u64) as usize;
        parent.read_at(offset, &mut buf[..take])?;
        buf[take..].fill(0);
        Ok(())
    }

    fn write_block(&mut self, guest_offset: u64, data: &[u8]) -> Result<()> {
        let block_size = self.header.block_size;
        let within = guest_offset % block_size;
        debug_assert!(within + data.len() as u64 <= block_size);

        let block = (guest_offset / block_size) as u32;
        let entry = self.block_map_entry(block)?;
        if entry != BLOCK_FREE && entry != BLOCK_ZERO {
            let at = self.data_at(entry)?;
            return self.device.write_at(at + within, data);
        }

        // The block has no data behind it. A fixed image declares that it
        // has one for every block, so a hole in one is a contradiction
        // rather than something to allocate into (P6).
        if self.header.image_type == VdiImageType::Fixed {
            return Err(invalid(format!(
                "a fixed image leaves block {block} unallocated; refusing to \
                 write through a block map that contradicts the image type"
            )));
        }

        let index = self.blocks_allocated()?;
        if index >= self.header.block_count {
            return Err(invalid(format!(
                "the image already accounts for all {} of its blocks; \
                 refusing to allocate another",
                self.header.block_count
            )));
        }
        let at = self.header.data_offset + index as u64 * block_size;

        // The fresh block must read as what it read before the write
        // everywhere the write does not reach. A block the image never
        // allocated read as the parent's bytes, so the copy-on-write seed
        // comes from there; a discarded block masked the parent, so its
        // seed is zeroes. Both go through a bounded buffer, so allocation
        // costs the same whatever the block size (P27).
        let block_start = guest_offset - within;
        let tail = within + data.len() as u64;
        if entry == BLOCK_FREE && self.parent.is_some() {
            self.copy_from_parent(at, block_start, within)?;
            self.device.write_at(at + within, data)?;
            self.copy_from_parent(at + tail, block_start + tail, block_size - tail)?;
        } else {
            self.write_zeroes(at, within)?;
            self.device.write_at(at + within, data)?;
            self.write_zeroes(at + tail, block_size - tail)?;
        }

        // Only then the accounting: the map entry that reaches the new
        // block, and the count that says it exists.
        self.device.write_at(
            self.header.block_map_offset + block as u64 * 4,
            &index.to_le_bytes(),
        )?;
        self.device
            .write_at(BLOCKS_ALLOCATED_AT as u64, &(index + 1).to_le_bytes())
    }

    /// Seeds `length` bytes at `offset` in the file with what the parent
    /// presents from `guest_offset`, a bounded chunk at a time (P27).
    /// This is the whole of copy-on-write: the parent is read, never
    /// written, and the bytes land in this image's own fresh block.
    fn copy_from_parent(
        &mut self,
        offset: u64,
        guest_offset: u64,
        length: u64,
    ) -> Result<()> {
        if length == 0 {
            return Ok(());
        }
        let mut chunk = vec![0u8; (length as usize).min(ZERO_FILL_CHUNK)];
        let mut done = 0u64;
        while done < length {
            let take = ((length - done) as usize).min(chunk.len());
            self.read_parent(guest_offset + done, &mut chunk[..take])?;
            self.device.write_at(offset + done, &chunk[..take])?;
            done += take as u64;
        }
        Ok(())
    }

    fn write_zeroes(&mut self, offset: u64, length: u64) -> Result<()> {
        if length == 0 {
            return Ok(());
        }
        let zeroes = vec![0u8; (length as usize).min(ZERO_FILL_CHUNK)];
        let mut done = 0u64;
        while done < length {
            let take = ((length - done) as usize).min(zeroes.len());
            self.device.write_at(offset + done, &zeroes[..take])?;
            done += take as u64;
        }
        Ok(())
    }
}

impl<D: Device> Device for Vdi<D> {
    fn len(&self) -> u64 {
        self.header.disk_size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.header.disk_size {
            return Err(invalid("read past the end of the virtual disk"));
        }
        let block_size = self.header.block_size;
        let mut done = 0usize;
        while done < buf.len() {
            let at = offset + done as u64;
            let within = at % block_size;
            let take = ((block_size - within) as usize).min(buf.len() - done);
            let (_, rest) = buf.split_at_mut(done);
            self.read_block(at, &mut rest[..take])?;
            done += take;
        }
        Ok(())
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if offset + data.len() as u64 > self.header.disk_size {
            return Err(invalid("write past the end of the virtual disk"));
        }
        let block_size = self.header.block_size;
        let mut done = 0usize;
        while done < data.len() {
            let at = offset + done as u64;
            let within = at % block_size;
            let take = ((block_size - within) as usize).min(data.len() - done);
            self.write_block(at, &data[done..done + take])?;
            done += take;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.device.flush()
    }
}

/// Opens the image at `path` with its whole differencing chain composed
/// (U6). `device` is the top file, already claimed per the caller's
/// declared intent; every parent is claimed immutable for the chain's
/// life (P7) — writes denied to every other process, the library's own
/// access read-only. A missing parent, a cycle, a chain past
/// [`MAX_CHAIN_LENGTH`] files, and a parent whose own version or image
/// type falls outside the claim are refused by name (P3).
pub(crate) fn open_chain(device: MediumDevice, path: &Path) -> Result<Vdi<MediumDevice>> {
    open_member(device, path, &mut Vec::new())
}

/// Opens one member and, where it differences, the rest of the chain
/// beneath it. `chain` carries the identities of the members already
/// open, top-down: the format names a parent by identity, so a cycle is
/// an identity already in the chain rather than a path already visited,
/// and that reading catches an image naming itself as squarely as it
/// catches two naming each other.
fn open_member(
    mut device: MediumDevice,
    path: &Path,
    chain: &mut Vec<VdiUuid>,
) -> Result<Vdi<MediumDevice>> {
    let header = VdiHeader::parse(&mut device)?;
    if header.image_type != VdiImageType::Differencing {
        return Ok(Vdi::assemble(device, header, None));
    }

    chain.push(header.create_id);
    if chain.contains(&header.parent_id) {
        return Err(invalid(format!(
            "the differencing chain cycles: '{}' names parent {}, which is \
             already a member",
            path.display(),
            header.parent_id
        )));
    }
    if chain.len() >= MAX_CHAIN_LENGTH {
        return Err(unsupported(format!(
            "the differencing chain runs past the {MAX_CHAIN_LENGTH} files \
             this release claims; refusing to open it partway"
        )));
    }

    let resolved = resolve_parent(path, header.parent_id)?;

    // A parent once used as a writable top image may carry an interrupted
    // commit of its own; it is reconciled before the chain composes over
    // it (P9), exactly as at a top-level open.
    crate::journal::reconcile_at(&resolved)?;

    // The immutability claim (P7); contention is an immediate, named
    // failure inside this open.
    let parent_device = MediumDevice::open(&resolved, AccessIntent::Read)?;
    let parent = open_member(parent_device, &resolved, chain)?;

    // The identity is checked again against the member actually opened,
    // where the chain is joined: resolution selects a file, and this is
    // what says the file selected is the one the child named.
    if parent.header.create_id != header.parent_id {
        return Err(invalid(format!(
            "'{}' declares identity {}, but '{}' names parent {}; refusing to \
             read it as a substitute",
            resolved.display(),
            parent.header.create_id,
            path.display(),
            header.parent_id
        )));
    }

    Ok(Vdi::assemble(device, header, Some(Box::new(parent))))
}

/// Finds the file holding `parent`, the identity `child` declares.
///
/// The format records the parent's identity and **no path at all**, so
/// resolution searches rather than dereferences a name. Two directories
/// are searched, in order: the one holding `child`, then the one above
/// it — which is where the format's own tooling leaves a base image when
/// the differencing images over it sit in a subdirectory of their own.
///
/// In each, the file *named* for the identity is nominated first, because
/// that is how the format's tooling names a differencing image, in both
/// spellings it is written with. A nominated file is taken to be the
/// parent: an identity that does not match is a refusal rather than a
/// fallback to searching, so a substitute standing where the parent
/// should be is never silently read in its place. Failing a nomination,
/// every VDI beside it is examined and the one whose own identity matches
/// is the parent — two in one directory is a contradiction, and none
/// anywhere is the missing-parent refusal, which names what it looked for
/// and every candidate it could not examine (P4).
fn resolve_parent(child: &Path, parent: VdiUuid) -> Result<PathBuf> {
    let here = match child.parent() {
        Some(directory) if !directory.as_os_str().is_empty() => directory.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let mut directories = vec![here.clone()];
    if let Some(above) = here.parent().filter(|above| !above.as_os_str().is_empty()) {
        directories.push(above.to_path_buf());
    }
    let child_itself = std::fs::canonicalize(child).ok();
    let mut unexamined: Vec<String> = Vec::new();

    for directory in &directories {
        for nominated in [format!("{{{parent}}}.vdi"), format!("{parent}.vdi")] {
            let candidate = directory.join(nominated);
            if !candidate.is_file() {
                continue;
            }
            // A file the search cannot read an identity out of is still
            // the nominated parent; opening it as a member is what names
            // why it cannot be read.
            if let Some(identity) = identity_of(&candidate)?.filter(|found| *found != parent) {
                return Err(invalid(format!(
                    "'{}' is named for parent {parent} but declares identity \
                     {identity}; refusing to read it as a substitute",
                    candidate.display()
                )));
            }
            return Ok(candidate);
        }

        let Ok(listing) = std::fs::read_dir(directory) else {
            continue;
        };
        let mut matched: Vec<PathBuf> = Vec::new();
        let mut examined = 0usize;
        for entry in listing.flatten() {
            let candidate = entry.path();
            if !is_vdi_file(&candidate) {
                continue;
            }
            if std::fs::canonicalize(&candidate).ok() == child_itself {
                continue;
            }
            examined += 1;
            if examined > MAX_PARENT_CANDIDATES {
                return Err(unsupported(format!(
                    "'{}' holds more than the {MAX_PARENT_CANDIDATES} VDI files \
                     this release searches for a parent; refusing to search it \
                     partway",
                    directory.display()
                )));
            }
            match identity_of(&candidate) {
                Ok(Some(identity)) if identity == parent => matched.push(candidate),
                Ok(_) => {}
                Err(error) => {
                    unexamined.push(format!("'{}' ({error})", candidate.display()));
                }
            }
        }
        match matched.len() {
            0 => {}
            1 => return Ok(matched.swap_remove(0)),
            found => {
                return Err(invalid(format!(
                    "'{}' holds {found} images declaring identity {parent}; \
                     refusing to choose between them",
                    directory.display()
                )));
            }
        }
    }

    let searched = directories
        .iter()
        .map(|directory| format!("'{}'", directory.display()))
        .collect::<Vec<_>>()
        .join(" and ");
    let mut reason = format!(
        "the parent of '{}' is missing: no image declaring identity {parent} \
         was found in {searched}",
        child.display()
    );
    if !unexamined.is_empty() {
        reason.push_str(&format!(
            " ({} candidate(s) could not be examined: {})",
            unexamined.len(),
            unexamined.join(", ")
        ));
    }
    Err(Error::not_found(reason))
}

fn is_vdi_file(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase() == "vdi")
        .unwrap_or(false)
        && path.is_file()
}

/// The identity `path` declares as its own, read from a bounded prefix.
/// `None` says the file declares none this release can read — it is not a
/// VDI of the claimed major version, so it is not a candidate; an error
/// is the host failing to deliver the bytes, or another process holding
/// the file against the P7 claim, which is not the same thing as a
/// mismatch and is never reported as one.
fn identity_of(path: &Path) -> Result<Option<VdiUuid>> {
    const NEEDED: usize = UUID_CREATE_AT + 16;
    let mut device = MediumDevice::open(path, AccessIntent::Read)?;
    if device.len() < NEEDED as u64 {
        return Ok(None);
    }
    let mut raw = [0u8; NEEDED];
    device.read_at(0, &mut raw)?;
    if raw[SIGNATURE_AT..SIGNATURE_AT + 4] != VDI_SIGNATURE {
        return Ok(None);
    }
    // Another major puts every field at an offset this release does not
    // know, the identity included (P8).
    if version(&raw).0 != SUPPORTED_MAJOR {
        return Ok(None);
    }
    Ok(Some(VdiUuid::read(&raw, UUID_CREATE_AT)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A growable in-memory device for building images in tests.
    #[derive(Debug)]
    struct VecDevice(Vec<u8>);

    impl Device for VecDevice {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
            let start = offset as usize;
            buf.copy_from_slice(&self.0[start..start + buf.len()]);
            Ok(())
        }

        fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
            let end = offset as usize + data.len();
            if end > self.0.len() {
                self.0.resize(end, 0);
            }
            self.0[offset as usize..end].copy_from_slice(data);
            Ok(())
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    const BLOCK: u64 = 64 * 1024;
    const MAP_AT: u64 = 0x200;

    /// A version 1.1 header over `disk_size`, with the block map at
    /// [`MAP_AT`] and the data area on the next 512-byte boundary after
    /// it. Every entry starts free; `image_type` is the declared type.
    fn vdi_shell(disk_size: u64, image_type: u32) -> VecDevice {
        let block_count = disk_size.div_ceil(BLOCK) as u32;
        let map_end = MAP_AT + block_count as u64 * 4;
        let data_offset = map_end.div_ceil(512) * 512;

        let mut image = vec![0u8; data_offset as usize];
        image[..38].copy_from_slice(b"<<< remanence test VDI container >>>\n\0");
        image[SIGNATURE_AT..SIGNATURE_AT + 4].copy_from_slice(&VDI_SIGNATURE);
        image[VERSION_AT..VERSION_AT + 4].copy_from_slice(&0x0001_0001u32.to_le_bytes());
        image[HEADER_SIZE_AT..HEADER_SIZE_AT + 4].copy_from_slice(&0x190u32.to_le_bytes());
        image[IMAGE_TYPE_AT..IMAGE_TYPE_AT + 4].copy_from_slice(&image_type.to_le_bytes());
        image[BLOCK_MAP_OFFSET_AT..BLOCK_MAP_OFFSET_AT + 4]
            .copy_from_slice(&(MAP_AT as u32).to_le_bytes());
        image[DATA_OFFSET_AT..DATA_OFFSET_AT + 4]
            .copy_from_slice(&(data_offset as u32).to_le_bytes());
        image[DISK_SIZE_AT..DISK_SIZE_AT + 8].copy_from_slice(&disk_size.to_le_bytes());
        image[BLOCK_SIZE_AT..BLOCK_SIZE_AT + 4].copy_from_slice(&(BLOCK as u32).to_le_bytes());
        image[BLOCK_COUNT_AT..BLOCK_COUNT_AT + 4].copy_from_slice(&block_count.to_le_bytes());
        for block in 0..block_count as usize {
            let at = MAP_AT as usize + block * 4;
            image[at..at + 4].copy_from_slice(&BLOCK_FREE.to_le_bytes());
        }
        VecDevice(image)
    }

    /// An empty dynamically allocated image: nothing allocated, every
    /// block reading as zeroes.
    fn empty_dynamic(disk_size: u64) -> VecDevice {
        vdi_shell(disk_size, 1)
    }

    /// A recognizable identity, distinct per `tag`.
    fn identity(tag: u8) -> VdiUuid {
        let mut bytes = [tag; 16];
        bytes[0] = 0x11;
        bytes[15] = tag;
        VdiUuid(bytes)
    }

    /// Stamps an image's own identity and, where it differences, the one
    /// it names as its parent.
    fn with_identity(mut device: VecDevice, create: VdiUuid, parent: VdiUuid) -> VecDevice {
        device.write_at(UUID_CREATE_AT as u64, &create.0).unwrap();
        device.write_at(UUID_LINKAGE_AT as u64, &parent.0).unwrap();
        device
    }

    /// An empty differencing image over `parent`: every block free, so
    /// every block reads through.
    fn empty_differencing(disk_size: u64, parent: VdiUuid) -> VecDevice {
        with_identity(vdi_shell(disk_size, 4), identity(0xd0), parent)
    }

    /// Attaches `parent` beneath a differencing image, the way
    /// [`open_member`] does once resolution has found the file.
    fn over(device: VecDevice, parent: Vdi<VecDevice>) -> Vdi<VecDevice> {
        let mut device = device;
        let header = VdiHeader::parse(&mut device).expect("parses");
        Vdi::assemble(device, header, Some(Box::new(parent)))
    }

    /// A fixed image whose every block is present, carrying `content` from
    /// the start of the virtual disk.
    fn fixed_with(disk_size: u64, content: &[u8]) -> VecDevice {
        let mut device = vdi_shell(disk_size, 2);
        let block_count = disk_size.div_ceil(BLOCK) as u32;
        let data_offset = le32(&device.0, DATA_OFFSET_AT) as u64;
        for block in 0..block_count {
            let at = MAP_AT + block as u64 * 4;
            device.write_at(at, &block.to_le_bytes()).unwrap();
        }
        device
            .write_at(
                data_offset + block_count as u64 * BLOCK - 1,
                &[0],
            )
            .unwrap();
        device.write_at(data_offset, content).unwrap();
        device
            .write_at(
                BLOCKS_ALLOCATED_AT as u64,
                &block_count.to_le_bytes(),
            )
            .unwrap();
        device
    }

    #[test]
    fn round_trips_writes_through_the_block_map() {
        let disk_size = 16 * BLOCK;
        let mut vdi = Vdi::open(empty_dynamic(disk_size)).expect("opens");
        assert_eq!(vdi.len(), disk_size);

        // Unallocated reads as zero, and nothing is allocated to answer.
        let mut buf = vec![0xffu8; 100];
        vdi.read_at(5 * BLOCK + 17, &mut buf).expect("reads");
        assert!(buf.iter().all(|&byte| byte == 0));
        assert_eq!(vdi.blocks_allocated().expect("count"), 0);

        // A write spanning two blocks survives the round trip.
        let payload: Vec<u8> = (0..2 * BLOCK as u32 + 99).map(|n| (n % 251) as u8).collect();
        vdi.write_at(7 * BLOCK - 50, &payload).expect("writes");
        let mut back = vec![0u8; payload.len()];
        vdi.read_at(7 * BLOCK - 50, &mut back).expect("reads back");
        assert_eq!(back, payload);
        assert_eq!(
            vdi.blocks_allocated().expect("count"),
            4,
            "the write touched four blocks and allocated each once"
        );

        // Neighbouring bytes inside an allocated block stay zero.
        let mut edge = [0xffu8; 8];
        vdi.read_at(7 * BLOCK - 58, &mut edge).expect("reads edge");
        assert!(edge.iter().all(|&byte| byte == 0));

        // A second write into an allocated block reuses it.
        vdi.write_at(7 * BLOCK - 50, &[1, 2, 3]).expect("rewrites");
        assert_eq!(vdi.blocks_allocated().expect("count"), 4);
    }

    #[test]
    fn an_unallocated_block_is_never_an_allocated_zero_one() {
        let mut vdi = Vdi::open(empty_dynamic(4 * BLOCK)).expect("opens");

        // Block 2 is marked discarded rather than free: it reads as
        // zeroes all the same, because the format says so.
        vdi.device
            .write_at(MAP_AT + 2 * 4, &BLOCK_ZERO.to_le_bytes())
            .unwrap();
        let mut buf = vec![0xffu8; 64];
        vdi.read_at(2 * BLOCK, &mut buf).expect("reads");
        assert!(buf.iter().all(|&byte| byte == 0));

        // Writing zeroes into block 1 allocates it: the map now holds a
        // data index, not a sentinel, and the block is a real one whose
        // contents happen to be zero.
        vdi.write_at(BLOCK, &[0u8; 16]).expect("writes zeroes");
        assert_eq!(vdi.block_map_entry(1).expect("entry"), 0);
        assert_eq!(vdi.blocks_allocated().expect("count"), 1);
        assert_eq!(vdi.block_map_entry(0).expect("entry"), BLOCK_FREE);
        assert_eq!(vdi.block_map_entry(2).expect("entry"), BLOCK_ZERO);
    }

    #[test]
    fn a_fixed_image_reads_and_writes_its_blocks_in_place() {
        let disk_size = 4 * BLOCK;
        let content: Vec<u8> = (0..3000u32).map(|n| (n % 253 + 1) as u8).collect();
        let mut vdi = Vdi::open(fixed_with(disk_size, &content)).expect("opens");
        assert_eq!(vdi.header().image_type, VdiImageType::Fixed);

        let mut back = vec![0u8; content.len()];
        vdi.read_at(0, &mut back).expect("reads");
        assert_eq!(back, content);

        // Every block is already there, so a write allocates nothing.
        vdi.write_at(3 * BLOCK + 9, b"in place").expect("writes");
        let mut probe = [0u8; 8];
        vdi.read_at(3 * BLOCK + 9, &mut probe).expect("reads back");
        assert_eq!(&probe, b"in place");
        assert_eq!(vdi.blocks_allocated().expect("count"), 4);
    }

    #[test]
    fn p8_gates_run_before_anything_else() {
        let mut device = empty_dynamic(BLOCK);
        device
            .write_at(VERSION_AT as u64, &0x0002_0000u32.to_le_bytes())
            .unwrap();
        // Break a field the parse would reach afterwards, to prove the
        // version is what answered.
        device
            .write_at(IMAGE_TYPE_AT as u64, &9u32.to_le_bytes())
            .unwrap();
        let error = Vdi::open(device).expect_err("a future version is refused");
        assert!(error.to_string().contains("2.0"), "{error}");
        assert!(error.to_string().contains("ceiling"), "{error}");

        let mut device = empty_dynamic(BLOCK);
        device
            .write_at(VERSION_AT as u64, &0x0000_0001u32.to_le_bytes())
            .unwrap();
        let error = Vdi::open(device).expect_err("an older version is refused");
        assert!(error.to_string().contains("0.1"), "{error}");

        // A minor past the claim is refused as squarely as a major.
        let mut device = empty_dynamic(BLOCK);
        device
            .write_at(VERSION_AT as u64, &0x0001_0002u32.to_le_bytes())
            .unwrap();
        let error = Vdi::open(device).expect_err("a future minor is refused");
        assert!(error.to_string().contains("1.2"), "{error}");
    }

    #[test]
    fn unclaimed_image_types_are_refused_by_name() {
        let error =
            Vdi::open(vdi_shell(BLOCK, 3)).expect_err("an unclaimed type is refused");
        assert_eq!(error.category(), crate::ErrorCategory::Unsupported);
        assert!(error.to_string().contains("undo"), "{error}");

        let error =
            Vdi::open(vdi_shell(BLOCK, 9)).expect_err("an undefined type is refused");
        assert_eq!(error.category(), crate::ErrorCategory::InvalidImage);
        assert!(error.to_string().contains('9'), "{error}");
    }

    #[test]
    fn a_standalone_open_refuses_to_read_a_differencing_image_alone() {
        let parent = identity(0x2a);
        let error = Vdi::open(empty_differencing(BLOCK, parent))
            .expect_err("a chain member is not a standalone image");
        assert_eq!(error.category(), crate::ErrorCategory::Unsupported);
        let message = error.to_string();
        assert!(
            message.contains(&parent.to_string()),
            "the refusal names the parent it will not go and find: {message}"
        );

        // And a differencing image naming no parent contradicts its own
        // declared type (P6).
        let error = Vdi::open(vdi_shell(BLOCK, 4)).expect_err("a nil linkage is refused");
        assert_eq!(error.category(), crate::ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("all zeroes"), "{error}");
    }

    #[test]
    fn reads_compose_through_the_chain() {
        let disk_size = 4 * BLOCK;
        let base_identity = identity(0xb0);
        let content: Vec<u8> = (0..4000u32).map(|n| (n % 253 + 1) as u8).collect();
        let base = Vdi::open(with_identity(
            fixed_with(disk_size, &content),
            base_identity,
            VdiUuid([0; 16]),
        ))
        .expect("the base opens");

        let top = empty_differencing(disk_size, base_identity);
        let mut top = over(top, base);

        // Every block is free in the top image, so the whole disk is the
        // parent's.
        let mut back = vec![0u8; content.len()];
        top.read_at(0, &mut back).expect("reads through");
        assert_eq!(back, content);

        // A discarded block masks the parent instead of falling through.
        top.device
            .write_at(MAP_AT + 4, &BLOCK_ZERO.to_le_bytes())
            .unwrap();
        let mut masked = vec![0xffu8; 64];
        top.read_at(BLOCK, &mut masked).expect("reads the mask");
        assert!(
            masked.iter().all(|&byte| byte == 0),
            "a discarded block reads as zeroes over whatever the parent holds"
        );
    }

    #[test]
    fn a_write_copies_the_parents_block_before_changing_it() {
        let disk_size = 4 * BLOCK;
        let base_identity = identity(0xb1);
        let filled: Vec<u8> = (0..disk_size as u32).map(|n| (n % 251 + 1) as u8).collect();
        let base = Vdi::open(with_identity(
            fixed_with(disk_size, &filled),
            base_identity,
            VdiUuid([0; 16]),
        ))
        .expect("the base opens");
        let base_before = base.device.0.clone();

        let mut top = over(empty_differencing(disk_size, base_identity), base);
        top.write_at(2 * BLOCK + 100, b"changed here").expect("writes");

        // The allocated block carries the parent's bytes everywhere the
        // write did not reach, and the write where it did.
        let mut whole = vec![0u8; BLOCK as usize];
        top.read_at(2 * BLOCK, &mut whole).expect("reads back");
        let parent_block = &filled[2 * BLOCK as usize..3 * BLOCK as usize];
        assert_eq!(&whole[..100], &parent_block[..100]);
        assert_eq!(&whole[100..112], b"changed here");
        assert_eq!(&whole[112..], &parent_block[112..]);
        assert_eq!(top.blocks_allocated().expect("count"), 1);

        // Nothing about the write reached the parent (P7, U6).
        assert_eq!(
            top.parent.as_ref().expect("a parent").device.0,
            base_before,
            "the parent is read and never written"
        );
    }

    #[test]
    fn a_chain_member_reads_zero_past_a_shorter_parents_end() {
        let base_identity = identity(0xb2);
        let base = Vdi::open(with_identity(
            fixed_with(BLOCK, &[7u8; 64]),
            base_identity,
            VdiUuid([0; 16]),
        ))
        .expect("the base opens");
        let mut top = over(empty_differencing(2 * BLOCK, base_identity), base);

        let mut buf = vec![0xffu8; 32];
        top.read_at(BLOCK + 16, &mut buf).expect("reads past the parent");
        assert!(buf.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn the_structural_claims_are_refusals_rather_than_repairs() {
        // Extra data per block.
        let mut device = empty_dynamic(BLOCK);
        device
            .write_at(BLOCK_EXTRA_AT as u64, &16u32.to_le_bytes())
            .unwrap();
        let error = Vdi::open(device).expect_err("extra block data is refused");
        assert!(error.to_string().contains("extra data"), "{error}");

        // An image flag this release does not model.
        let mut device = empty_dynamic(BLOCK);
        device
            .write_at(FLAGS_AT as u64, &0x4000u32.to_le_bytes())
            .unwrap();
        let error = Vdi::open(device).expect_err("an unmodelled flag is refused");
        assert!(error.to_string().contains("flags"), "{error}");

        // A block size the format cannot mean.
        let mut device = empty_dynamic(BLOCK);
        device
            .write_at(BLOCK_SIZE_AT as u64, &3000u32.to_le_bytes())
            .unwrap();
        let error = Vdi::open(device).expect_err("a non-power-of-two block is refused");
        assert!(error.to_string().contains("block size"), "{error}");

        // A block count that cannot cover the declared disk.
        let mut device = empty_dynamic(8 * BLOCK);
        device
            .write_at(BLOCK_COUNT_AT as u64, &2u32.to_le_bytes())
            .unwrap();
        let error = Vdi::open(device).expect_err("a short block map is refused");
        assert!(error.to_string().contains("short of the declared"), "{error}");

        // A fixed image whose blocks are not all in the file.
        let mut device = fixed_with(4 * BLOCK, b"truncated");
        device.0.truncate(device.0.len() - 4096);
        let error = Vdi::open(device).expect_err("a truncated fixed image is refused");
        assert!(error.to_string().contains("past the"), "{error}");
    }

    #[test]
    fn the_virtual_disk_bounds_are_refusals_rather_than_clamps() {
        let disk_size = 2 * BLOCK;
        let mut vdi = Vdi::open(empty_dynamic(disk_size)).expect("opens");
        let mut buf = [0u8; 64];
        assert!(vdi.read_at(disk_size - 32, &mut buf).is_err());
        assert!(vdi.write_at(disk_size - 32, &buf).is_err());
    }

    #[test]
    fn a_zero_length_read_touches_nothing() {
        let mut vdi = Vdi::open(empty_dynamic(BLOCK)).expect("opens");
        vdi.read_at(BLOCK, &mut []).expect("an empty read is at rest");
    }
}
