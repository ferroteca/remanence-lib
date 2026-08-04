// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Native VDI driver — the standalone VirtualBox Disk Image container —
//! written from the published format description. It presents the virtual
//! disk as a [`Device`], exactly as the qcow2 driver does, so a VDI opens,
//! identifies, inspects, reads and writes through the delivered disk stack
//! unchanged.
//!
//! The support claim is validated before anything else is touched (P8):
//! major version 1, minor 0 or 1, which are the two shapes of the same
//! header — the fields this driver reads sit at the same offsets in both,
//! and 1.1's additions are trailing bytes it never touches. Past the
//! version the claim is enumerated (P3): the **dynamically allocated** and
//! **fixed** image types are read and written by name, and every other
//! type the format defines — undo, and differencing among them — is
//! refused by name rather than attempted, as are per-block extra data and
//! any image flag this release does not model.
//!
//! The block map is the format's own mapping and stays in the file: an
//! entry is read where it is needed and never held resident (P27), so the
//! driver carries no mutable state of its own and a commit that does not
//! land has nothing in memory to put back. An entry marking a block
//! unallocated ([`BLOCK_FREE`]) or discarded ([`BLOCK_ZERO`]) reads as
//! zeroes because the format says so, and is never confused with a block
//! that is allocated and happens to hold zeroes. Allocating a block
//! belongs to the write path alone, which the disk stack reaches inside
//! commit — never during a read.

use crate::device::Device;
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

/// Everything the driver reads out of the header, which ends at the first
/// UUID. The leading bytes are the creator line, the signature and the
/// version; the trailing ones are identity and legacy geometry.
const HEADER_READ_BYTES: usize = 0x188;

/// The smallest declared header size the fields above fit inside.
/// Version 1.0 declares 0x170 and 1.1 declares 0x190, both past it.
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

/// The image types this release claims, by name (P3). The format defines
/// two more — undo and differencing — and each is refused by name at the
/// header rather than read as one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VdiImageType {
    /// Blocks exist only where they were written; the block map says
    /// which.
    Dynamic,
    /// Every block is present from creation, the block map addressing
    /// each in turn.
    Fixed,
}

impl VdiImageType {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Dynamic => "dynamically allocated",
            Self::Fixed => "fixed",
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
                     the dynamically allocated and fixed types are supported",
                ));
            }
            4 => {
                return Err(unsupported(
                    "VDI image type 4 (differencing) is outside this release's \
                     claim; the dynamically allocated and fixed types are \
                     supported",
                ));
            }
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
        // one (P6 — the contradiction is sought before anything is read).
        let present = match image_type {
            VdiImageType::Fixed => block_count as u64,
            VdiImageType::Dynamic => blocks_allocated as u64,
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

        Ok(Self {
            major,
            minor,
            image_type,
            block_map_offset,
            data_offset,
            disk_size,
            block_size,
            block_count,
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
}

impl<D: Device> Vdi<D> {
    pub(crate) fn open(mut device: D) -> Result<Self> {
        let header = VdiHeader::parse(&mut device)?;
        Ok(Self { device, header })
    }

    pub(crate) fn header(&self) -> &VdiHeader {
        &self.header
    }

    /// The host device the image lives in — a VDI is one file, so this is
    /// the only file writes ever reach.
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
        if entry == BLOCK_FREE || entry == BLOCK_ZERO {
            // The format says so: an unallocated block and a discarded
            // one read as zeroes, and neither is an allocated block that
            // happens to hold them.
            buf.fill(0);
            return Ok(());
        }
        let at = self.data_at(entry)?;
        self.device.read_at(at + within, buf)
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

        // The fresh block reads as zeroes everywhere the write does not
        // reach, which is what the entry it replaces read as. Zeroes go
        // through a bounded buffer, so allocation costs the same whatever
        // the block size (P27).
        self.write_zeroes(at, within)?;
        self.device.write_at(at + within, data)?;
        let tail = within + data.len() as u64;
        self.write_zeroes(at + tail, block_size - tail)?;

        // Only then the accounting: the map entry that reaches the new
        // block, and the count that says it exists.
        self.device.write_at(
            self.header.block_map_offset + block as u64 * 4,
            &index.to_le_bytes(),
        )?;
        self.device
            .write_at(BLOCKS_ALLOCATED_AT as u64, &(index + 1).to_le_bytes())
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
        for (declared, named) in [(3u32, "undo"), (4, "differencing")] {
            let error = Vdi::open(vdi_shell(BLOCK, declared))
                .expect_err("an unclaimed type is refused");
            assert_eq!(error.category(), crate::ErrorCategory::Unsupported);
            assert!(error.to_string().contains(named), "{error}");
        }

        let error =
            Vdi::open(vdi_shell(BLOCK, 9)).expect_err("an undefined type is refused");
        assert_eq!(error.category(), crate::ErrorCategory::InvalidImage);
        assert!(error.to_string().contains('9'), "{error}");
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
