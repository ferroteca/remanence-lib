// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Native qcow2 v2/v3 driver, written from the published format
//! documentation (QEMU `docs/interop/qcow2`). Presents the virtual disk as
//! a [`Device`]. The support claim, per P8, is validated before anything
//! else is touched — versions 2 and 3, no encryption, no external data
//! file, no unknown incompatible feature bits — for every file of a
//! backing chain alike. A chain composes for reading and writing (U6):
//! members are raw or qcow2, a relative backing path resolves from the
//! image that names it, and every backing file is claimed immutable
//! (P7). Writes allocate copy-on-write into the top image only. A
//! missing member, a cycle, a chain past [`MAX_CHAIN_LENGTH`] files, and
//! a backing format beyond the claim are refused by name. Writing claims
//! refcount width 16 and no internal snapshots.

use std::path::{Path, PathBuf};

use crate::device::{AccessIntent, Device, FileDevice};
use crate::error::{Error, Result};
use crate::inflate::inflate;

pub(crate) const QCOW2_MAGIC: [u8; 4] = *b"QFI\xfb";

/// The highest qcow2 version this release explicitly supports (P8).
pub(crate) const SUPPORTED_VERSION_CEILING: u32 = 3;

/// The longest backing chain this release claims, counted in files with
/// the top image included. A deeper chain is refused by name (P3), never
/// walked partway.
pub(crate) const MAX_CHAIN_LENGTH: usize = 16;

/// The "backing file format name" header extension (0xe2792aca): the
/// spec's way of pinning the backing file's container so it is never
/// probed wrong.
const BACKING_FORMAT_EXTENSION: u32 = 0xe279_2aca;

const OFLAG_COPIED: u64 = 1 << 63;
const OFLAG_COMPRESSED: u64 = 1 << 62;
const OFLAG_ZERO: u64 = 1; // v3 standard cluster "reads as zero" bit.
const L2_OFFSET_MASK: u64 = 0x00ff_ffff_ffff_fe00;

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("4 bytes"))
}

fn be64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().expect("8 bytes"))
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::invalid_image("qcow2", reason)
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::categorized_image(crate::ErrorCategory::Unsupported, "qcow2", reason)
}

#[derive(Debug, Clone)]
pub(crate) struct Qcow2Header {
    pub version: u32,
    pub cluster_bits: u32,
    pub virtual_size: u64,
    pub l1_size: u32,
    pub l1_table_offset: u64,
    pub refcount_table_offset: u64,
    pub refcount_table_clusters: u32,
    pub nb_snapshots: u32,
    pub refcount_order: u32,
    /// The backing file name the header records, exactly as written.
    pub backing_file: Option<String>,
    /// The backing file's format where the image pins one ("raw",
    /// "qcow2", ...) via the header extension; absent, the magic decides.
    pub backing_format: Option<String>,
}

impl Qcow2Header {
    /// Parses and validates a header, running the P8 version gate before
    /// anything else is interpreted.
    pub fn parse(device: &mut dyn Device) -> Result<Self> {
        let mut raw = [0u8; 112];
        if device.len() < 104 {
            return Err(invalid("file too small for a qcow2 header"));
        }
        let take = (device.len().min(112)) as usize;
        device.read_at(0, &mut raw[..take])?;

        if raw[..4] != QCOW2_MAGIC {
            return Err(invalid("missing qcow2 magic"));
        }

        // P8: the version gate comes first, before any other field is
        // trusted to mean what this release thinks it means.
        let version = be32(&raw, 4);
        if version < 2 {
            return Err(unsupported(format!("unsupported qcow2 version {version}")));
        }
        if version > SUPPORTED_VERSION_CEILING {
            return Err(unsupported(format!(
                "qcow2 version {version} is newer than this release supports \
                 (ceiling: version {SUPPORTED_VERSION_CEILING}); refusing to touch it"
            )));
        }

        let backing_file_offset = be64(&raw, 8);
        let backing_file_size = be32(&raw, 16);
        let cluster_bits = be32(&raw, 20);
        if !(9..=21).contains(&cluster_bits) {
            return Err(invalid(format!("implausible cluster_bits {cluster_bits}")));
        }
        let virtual_size = be64(&raw, 24);
        let crypt_method = be32(&raw, 32);
        if crypt_method != 0 {
            return Err(unsupported("encrypted images are not supported"));
        }
        let l1_size = be32(&raw, 36);
        let l1_table_offset = be64(&raw, 40);
        let refcount_table_offset = be64(&raw, 48);
        let refcount_table_clusters = be32(&raw, 56);
        let nb_snapshots = be32(&raw, 60);

        let refcount_order = if version == 2 {
            4 // Fixed by the v2 format: 16-bit refcounts.
        } else {
            if device.len() < 112 {
                return Err(invalid("file too small for a qcow2 v3 header"));
            }
            let incompatible = be64(&raw, 72);
            // Bit 0: dirty (lazy refcounts); bit 1: corrupt; bit 2:
            // external data file; anything set is either a state we must
            // not touch or a feature beyond this release's claim.
            if incompatible != 0 {
                return Err(unsupported(format!(
                    "incompatible feature bits 0x{incompatible:x} are beyond this \
                     release's support; refusing to touch the image"
                )));
            }
            be32(&raw, 96)
        };

        // The backing chain fields (U6), read only once every gate above
        // has passed for this member.
        let (backing_file, backing_format) = if backing_file_offset == 0 {
            (None, None)
        } else {
            if backing_file_size == 0 || backing_file_size > 1023 {
                return Err(invalid(format!(
                    "implausible backing file name length {backing_file_size}"
                )));
            }
            let end = backing_file_offset
                .checked_add(backing_file_size as u64)
                .filter(|&end| end <= device.len());
            if end.is_none() {
                return Err(invalid("backing file name lies past the end of the file"));
            }
            let mut name = vec![0u8; backing_file_size as usize];
            device.read_at(backing_file_offset, &mut name)?;
            let name = String::from_utf8(name)
                .map_err(|_| invalid("backing file name is not valid UTF-8"))?;

            // Extensions sit between the header and the backing file
            // name, both inside the first cluster.
            let extensions_start = if version == 2 {
                72
            } else {
                let header_length = be32(&raw, 100) as u64;
                if header_length < 104 {
                    return Err(invalid(format!(
                        "implausible v3 header length {header_length}"
                    )));
                }
                header_length
            };
            let bound = backing_file_offset.min(1u64 << cluster_bits);
            let format = backing_format_extension(device, extensions_start, bound)?;
            (Some(name), format)
        };

        Ok(Self {
            version,
            cluster_bits,
            virtual_size,
            l1_size,
            l1_table_offset,
            refcount_table_offset,
            refcount_table_clusters,
            nb_snapshots,
            refcount_order,
            backing_file,
            backing_format,
        })
    }

    pub fn cluster_size(&self) -> u64 {
        1u64 << self.cluster_bits
    }
}

/// Scans the header extension area for the backing-format extension.
/// Unknown extension types are ignored, as the spec requires; an entry
/// running past its area is a contradiction and fails (P6).
fn backing_format_extension(
    device: &mut dyn Device,
    extensions_start: u64,
    bound: u64,
) -> Result<Option<String>> {
    let mut at = extensions_start;
    while at + 8 <= bound {
        let mut entry = [0u8; 8];
        device.read_at(at, &mut entry)?;
        let kind = be32(&entry, 0);
        if kind == 0 {
            break; // The end-of-extensions marker.
        }
        let length = be32(&entry, 4) as u64;
        if at + 8 + length > bound {
            return Err(invalid("header extension runs past its area"));
        }
        if kind == BACKING_FORMAT_EXTENSION {
            let mut name = vec![0u8; length as usize];
            device.read_at(at + 8, &mut name)?;
            let name = String::from_utf8(name).map_err(|_| {
                invalid("backing file format name is not valid UTF-8")
            })?;
            return Ok(Some(name));
        }
        at += 8 + length.div_ceil(8) * 8; // Entries pad to 8 bytes.
    }
    Ok(None)
}

/// One backing member of a chain: its bytes show through wherever the
/// image above leaves a cluster unallocated (U6).
#[derive(Debug)]
pub(crate) enum Backing<D: Device> {
    /// A raw image: the file's bytes are the virtual disk.
    Raw(D),
    /// A qcow2 image, possibly carrying its own backing in turn.
    Qcow2(Box<Qcow2<D>>),
}

impl<D: Device> Backing<D> {
    fn len(&self) -> u64 {
        match self {
            Self::Raw(device) => device.len(),
            Self::Qcow2(qcow2) => qcow2.header.virtual_size,
        }
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Raw(device) => device.read_at(offset, buf),
            Self::Qcow2(qcow2) => qcow2.read_at(offset, buf),
        }
    }
}

/// The virtual disk a qcow2 file describes, as a [`Device`].
#[derive(Debug)]
pub(crate) struct Qcow2<D: Device> {
    device: D,
    header: Qcow2Header,
    l1: Vec<u64>,
    writable_checked: bool,
    backing: Option<Backing<D>>,
}

impl<D: Device> Qcow2<D> {
    /// Opens a standalone image. An image naming a backing file is
    /// refused here: composing the chain takes the containing file's
    /// path, which only [`open_chain`] has.
    pub fn open(mut device: D) -> Result<Self> {
        let header = Qcow2Header::parse(&mut device)?;
        if let Some(name) = &header.backing_file {
            return Err(unsupported(format!(
                "image names backing file '{name}'; a standalone open does \
                 not compose the backing chain"
            )));
        }
        Self::assemble(device, header, None)
    }

    /// Builds the driver over an already-parsed header and, for a chain
    /// member, the backing image its unallocated clusters fall through to.
    fn assemble(
        mut device: D,
        header: Qcow2Header,
        backing: Option<Backing<D>>,
    ) -> Result<Self> {
        let l1_bytes = header.l1_size as usize * 8;
        let mut raw = vec![0u8; l1_bytes];
        device.read_at(header.l1_table_offset, &mut raw)?;
        let l1 = raw.chunks_exact(8).map(|chunk| be64(chunk, 0)).collect();

        Ok(Self { device, header, l1, writable_checked: false, backing })
    }

    pub fn header(&self) -> &Qcow2Header {
        &self.header
    }

    /// The host device the image lives in — for a chain, the top image
    /// alone, which is the only member writes ever reach.
    pub(crate) fn host_mut(&mut self) -> &mut D {
        &mut self.device
    }

    /// The cached L1 table, snapshotted before a commit stages writes so
    /// a commit that does not land can put the cache back (P9): the
    /// write path updates the cache alongside the staged host writes,
    /// and a discarded staging must discard both.
    pub(crate) fn l1_snapshot(&self) -> Vec<u64> {
        self.l1.clone()
    }

    pub(crate) fn restore_l1(&mut self, l1: Vec<u64>) {
        self.l1 = l1;
    }

    #[cfg(test)]
    fn into_device(self) -> D {
        self.device
    }

    fn cluster_size(&self) -> u64 {
        self.header.cluster_size()
    }

    fn l2_entries(&self) -> u64 {
        self.cluster_size() / 8
    }

    /// The additional constraints writing carries, checked once, before
    /// the first write (P6: surprises are sought before mutation).
    fn check_writable(&mut self) -> Result<()> {
        if self.writable_checked {
            return Ok(());
        }
        if self.header.nb_snapshots != 0 {
            return Err(unsupported(
                "image carries internal snapshots; writing is not supported",
            ));
        }
        if self.header.refcount_order != 4 {
            return Err(unsupported(format!(
                "refcount width 2^{} is beyond this release's write support \
                 (only 16-bit refcounts are claimed)",
                self.header.refcount_order
            )));
        }
        self.writable_checked = true;
        Ok(())
    }

    fn l2_entry(&mut self, guest_offset: u64) -> Result<u64> {
        let cluster = guest_offset >> self.header.cluster_bits;
        let l1_index = (cluster / self.l2_entries()) as usize;
        let l2_index = cluster % self.l2_entries();
        let Some(&l1_entry) = self.l1.get(l1_index) else {
            return Err(invalid("guest offset beyond L1 coverage"));
        };
        let l2_offset = l1_entry & L2_OFFSET_MASK;
        if l2_offset == 0 {
            return Ok(0);
        }
        let mut raw = [0u8; 8];
        self.device.read_at(l2_offset + l2_index * 8, &mut raw)?;
        Ok(u64::from_be_bytes(raw))
    }

    fn read_cluster(&mut self, guest_offset: u64, buf: &mut [u8]) -> Result<()> {
        let cluster_size = self.cluster_size();
        let within = guest_offset % cluster_size;
        debug_assert!(within + buf.len() as u64 <= cluster_size);

        let entry = self.l2_entry(guest_offset)?;
        if entry & OFLAG_COMPRESSED != 0 {
            let x = 62 - (self.header.cluster_bits - 8);
            let host_offset = entry & ((1u64 << x) - 1);
            let sectors = ((entry >> x) & ((1u64 << (62 - x)) - 1)) + 1;
            let length = sectors * 512 - (host_offset & 511);
            // The sector-rounded span may run past an unpadded file's end.
            let available = self.device.len().saturating_sub(host_offset);
            let length = length.min(available) as usize;
            let mut compressed = vec![0u8; length];
            self.device.read_at(host_offset, &mut compressed)?;
            let cluster = inflate(&compressed, cluster_size as usize)
                .ok_or_else(|| invalid("corrupt compressed cluster"))?;
            if (cluster.len() as u64) < within + buf.len() as u64 {
                return Err(invalid("compressed cluster shorter than expected"));
            }
            buf.copy_from_slice(
                &cluster[within as usize..within as usize + buf.len()],
            );
            return Ok(());
        }

        if entry & OFLAG_ZERO != 0 {
            // An explicit "reads as zero" cluster masks any backing
            // content behind it.
            buf.fill(0);
            return Ok(());
        }
        let host_offset = entry & L2_OFFSET_MASK;
        if host_offset == 0 {
            // Unallocated: the backing image shows through (U6); zeros
            // with no backing.
            return self.read_backing(guest_offset, buf);
        }
        self.device.read_at(host_offset + within, buf)
    }

    /// Reads from the backing image, zero-filling wherever the chain has
    /// no bytes: with no backing at all, and past a shorter backing's end.
    fn read_backing(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let Some(backing) = self.backing.as_mut() else {
            buf.fill(0);
            return Ok(());
        };
        let backing_len = backing.len();
        if offset >= backing_len {
            buf.fill(0);
            return Ok(());
        }
        let take = (backing_len - offset).min(buf.len() as u64) as usize;
        backing.read_at(offset, &mut buf[..take])?;
        buf[take..].fill(0);
        Ok(())
    }

    // Refcount plumbing (write path; 16-bit entries only, no snapshots,
    // so every allocated cluster's count is 0 or 1).

    fn refcount_block_entries(&self) -> u64 {
        self.cluster_size() / 2
    }

    fn refcount_table_entry(&mut self, table_index: u64) -> Result<u64> {
        let table_len =
            self.header.refcount_table_clusters as u64 * self.cluster_size() / 8;
        if table_index >= table_len {
            return Err(unsupported(
                "refcount table cannot cover the image; growing it is beyond \
                 this release's write support",
            ));
        }
        let mut raw = [0u8; 8];
        self.device
            .read_at(self.header.refcount_table_offset + table_index * 8, &mut raw)?;
        Ok(u64::from_be_bytes(raw))
    }

    fn set_refcount(&mut self, cluster_index: u64, value: u16) -> Result<()> {
        let table_index = cluster_index / self.refcount_block_entries();
        let block_index = cluster_index % self.refcount_block_entries();
        let mut block_offset = self.refcount_table_entry(table_index)?;
        if block_offset == 0 {
            // Allocate a refcount block. Appending is safe: the new block
            // accounts for itself below.
            block_offset = self.append_cluster()?;
            let zeroes = vec![0u8; self.cluster_size() as usize];
            self.device.write_at(block_offset, &zeroes)?;
            self.device.write_at(
                self.header.refcount_table_offset + table_index * 8,
                &block_offset.to_be_bytes(),
            )?;
            let own_index = block_offset >> self.header.cluster_bits;
            // The block may or may not describe its own cluster; only
            // write its own count when it does.
            if own_index / self.refcount_block_entries() == table_index {
                let own_block_index = own_index % self.refcount_block_entries();
                self.device.write_at(
                    block_offset + own_block_index * 2,
                    &1u16.to_be_bytes(),
                )?;
            } else {
                self.set_refcount(own_index, 1)?;
            }
        }
        self.device
            .write_at(block_offset + block_index * 2, &value.to_be_bytes())
    }

    /// Appends a cluster-aligned cluster to the end of the host file and
    /// returns its offset. The caller records its refcount.
    fn append_cluster(&mut self) -> Result<u64> {
        let cluster_size = self.cluster_size();
        let offset = self.device.len().div_ceil(cluster_size) * cluster_size;
        let zeroes = vec![0u8; cluster_size as usize];
        self.device.write_at(offset, &zeroes)?;
        Ok(offset)
    }

    fn allocate_cluster(&mut self) -> Result<u64> {
        let offset = self.append_cluster()?;
        self.set_refcount(offset >> self.header.cluster_bits, 1)?;
        Ok(offset)
    }

    fn ensure_l2(&mut self, guest_offset: u64) -> Result<u64> {
        let cluster = guest_offset >> self.header.cluster_bits;
        let l1_index = (cluster / self.l2_entries()) as usize;
        let Some(&l1_entry) = self.l1.get(l1_index) else {
            return Err(invalid("guest offset beyond L1 coverage"));
        };
        let l2_offset = l1_entry & L2_OFFSET_MASK;
        if l2_offset != 0 {
            return Ok(l2_offset);
        }
        let new_l2 = self.allocate_cluster()?;
        let new_entry = new_l2 | OFLAG_COPIED;
        self.device.write_at(
            self.header.l1_table_offset + l1_index as u64 * 8,
            &new_entry.to_be_bytes(),
        )?;
        self.l1[l1_index] = new_entry;
        Ok(new_l2)
    }

    fn write_cluster(&mut self, guest_offset: u64, data: &[u8]) -> Result<()> {
        self.check_writable()?;
        let cluster_size = self.cluster_size();
        let within = guest_offset % cluster_size;
        debug_assert!(within + data.len() as u64 <= cluster_size);

        let entry = self.l2_entry(guest_offset)?;
        let is_standard = entry & OFLAG_COMPRESSED == 0
            && entry & L2_OFFSET_MASK != 0
            && entry & OFLAG_ZERO == 0;

        if is_standard {
            if entry & OFLAG_COPIED == 0 {
                return Err(unsupported(
                    "cluster lacks the copied flag (shared with a snapshot?); \
                     refusing to write",
                ));
            }
            let host_offset = entry & L2_OFFSET_MASK;
            return self.device.write_at(host_offset + within, data);
        }

        // Unallocated, zero, or compressed: allocate a fresh standard
        // cluster, seed it with the old contents, then apply the write.
        let mut cluster = vec![0u8; cluster_size as usize];
        self.read_cluster(guest_offset - within, &mut cluster)?;
        cluster[within as usize..within as usize + data.len()].copy_from_slice(data);

        let l2_offset = self.ensure_l2(guest_offset)?;
        let new_cluster = self.allocate_cluster()?;
        self.device.write_at(new_cluster, &cluster)?;

        if entry & OFLAG_COMPRESSED != 0 {
            // The compressed cluster is no longer referenced; without
            // snapshots its count was 1, so it simply becomes free space.
            let x = 62 - (self.header.cluster_bits - 8);
            let old_host = entry & ((1u64 << x) - 1);
            self.set_refcount(old_host >> self.header.cluster_bits, 0)?;
        }

        let cluster_index = guest_offset >> self.header.cluster_bits;
        let l2_index = cluster_index % self.l2_entries();
        let new_entry = new_cluster | OFLAG_COPIED;
        self.device
            .write_at(l2_offset + l2_index * 8, &new_entry.to_be_bytes())
    }
}

/// Opens the image at `path` with its whole backing chain composed
/// (U6). `device` is the top file, already claimed per the caller's
/// declared intent; every backing file is claimed immutable for the
/// chain's life (P7) — writes denied to every other process, the
/// library's own access read-only. A relative backing path resolves
/// from the image that names it. A missing member, a cycle, a chain
/// past [`MAX_CHAIN_LENGTH`] files, and a backing format beyond raw and
/// qcow2 are refused by name (P3).
pub(crate) fn open_chain(device: FileDevice, path: &Path) -> Result<Qcow2<FileDevice>> {
    let canonical_top = std::fs::canonicalize(path).map_err(|error| {
        Error::io(format!("cannot resolve '{}': {error}", path.display()))
    })?;
    open_member(device, path, &mut vec![canonical_top])
}

fn open_member(
    mut device: FileDevice,
    path: &Path,
    visited: &mut Vec<PathBuf>,
) -> Result<Qcow2<FileDevice>> {
    let header = Qcow2Header::parse(&mut device)?;
    let Some(name) = header.backing_file.clone() else {
        return Qcow2::assemble(device, header, None);
    };

    // A relative backing path resolves from the containing image (U6).
    let named = Path::new(&name);
    let resolved = if named.is_absolute() {
        named.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new("")).join(named)
    };

    let canonical = match std::fs::canonicalize(&resolved) {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::not_found(format!(
                "backing file '{name}' of '{}' does not exist (resolved to '{}')",
                path.display(),
                resolved.display()
            )));
        }
        Err(error) => {
            return Err(Error::io(format!(
                "cannot resolve backing file '{}': {error}",
                resolved.display()
            )));
        }
    };
    if visited.contains(&canonical) {
        return Err(invalid(format!(
            "the backing chain cycles: '{}' is already a member",
            resolved.display()
        )));
    }
    if visited.len() >= MAX_CHAIN_LENGTH {
        return Err(unsupported(format!(
            "the backing chain runs past the {MAX_CHAIN_LENGTH} files this \
             release claims; refusing to open it partway"
        )));
    }
    visited.push(canonical);

    // A backing file once used as a writable top image may carry an
    // interrupted commit of its own; it is reconciled before the chain
    // composes over it (P9), exactly as at a top-level open.
    crate::journal::reconcile_at(&resolved)?;

    // The immutability claim (P7); contention is an immediate, named
    // failure inside this open.
    let mut backing_device = FileDevice::open(&resolved, AccessIntent::Read)?;

    // The parent pins its backing file's format where the image records
    // one; otherwise the magic decides, exactly as at the top.
    let is_qcow2 = match header.backing_format.as_deref() {
        Some("qcow2") => true,
        Some("raw") => false,
        Some(other) => {
            return Err(unsupported(format!(
                "backing file format '{other}' is beyond this release's \
                 support (raw and qcow2 are claimed)"
            )));
        }
        None => {
            let mut magic = [0u8; 4];
            if backing_device.len() >= 4 {
                backing_device.read_at(0, &mut magic)?;
            }
            magic == QCOW2_MAGIC
        }
    };

    let backing = if is_qcow2 {
        Backing::Qcow2(Box::new(open_member(backing_device, &resolved, visited)?))
    } else {
        Backing::Raw(backing_device)
    };
    Qcow2::assemble(device, header, Some(backing))
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

    const CLUSTER_BITS: u32 = 12;
    const CLUSTER: u64 = 1 << CLUSTER_BITS;

    /// A minimal empty v3 image: header, refcount table, one refcount
    /// block, and an L1 table — clusters 0..=3, all refcounted.
    fn empty_qcow2(virtual_size: u64) -> VecDevice {
        let l2_entries = CLUSTER / 8;
        let l1_size = virtual_size.div_ceil(CLUSTER * l2_entries) as u32;
        assert!(l1_size as u64 <= CLUSTER / 8, "test image L1 fits one cluster");

        let mut image = vec![0u8; 4 * CLUSTER as usize];
        image[..4].copy_from_slice(&QCOW2_MAGIC);
        image[4..8].copy_from_slice(&3u32.to_be_bytes()); // version
        image[20..24].copy_from_slice(&CLUSTER_BITS.to_be_bytes());
        image[24..32].copy_from_slice(&virtual_size.to_be_bytes());
        image[36..40].copy_from_slice(&l1_size.to_be_bytes());
        image[40..48].copy_from_slice(&(3 * CLUSTER).to_be_bytes()); // L1 offset
        image[48..56].copy_from_slice(&CLUSTER.to_be_bytes()); // refcount table
        image[56..60].copy_from_slice(&1u32.to_be_bytes()); // its clusters
        image[96..100].copy_from_slice(&4u32.to_be_bytes()); // refcount_order
        image[100..104].copy_from_slice(&112u32.to_be_bytes()); // header_length

        // Refcount table entry 0 -> block at cluster 2; counts for 0..=3.
        image[CLUSTER as usize..CLUSTER as usize + 8]
            .copy_from_slice(&(2 * CLUSTER).to_be_bytes());
        for cluster in 0..4usize {
            let at = 2 * CLUSTER as usize + cluster * 2;
            image[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
        }

        VecDevice(image)
    }

    #[test]
    fn round_trips_writes_through_the_mapping() {
        let virtual_size = 64 * CLUSTER;
        let mut qcow2 = Qcow2::open(empty_qcow2(virtual_size)).expect("opens");
        assert_eq!(qcow2.len(), virtual_size);

        // Unallocated reads as zero.
        let mut buf = vec![0xffu8; 100];
        qcow2.read_at(5 * CLUSTER + 17, &mut buf).expect("reads");
        assert!(buf.iter().all(|&byte| byte == 0));

        // A write spanning two clusters survives the round trip.
        let payload: Vec<u8> = (0..2 * CLUSTER as u32 + 99)
            .map(|n| (n % 251) as u8)
            .collect();
        qcow2.write_at(7 * CLUSTER - 50, &payload).expect("writes");
        let mut back = vec![0u8; payload.len()];
        qcow2.read_at(7 * CLUSTER - 50, &mut back).expect("reads back");
        assert_eq!(back, payload);

        // Neighboring bytes stay zero.
        let mut edge = [0xffu8; 8];
        qcow2.read_at(7 * CLUSTER - 58, &mut edge).expect("reads edge");
        assert!(edge.iter().all(|&byte| byte == 0));
    }

    /// A device carrying one compressed cluster at guest cluster 9, and
    /// the content it decompresses to.
    fn compressed_qcow2() -> (VecDevice, Vec<u8>) {
        let mut device = empty_qcow2(64 * CLUSTER);

        // An L2 table at cluster 4 for L1 index 0.
        device
            .write_at(3 * CLUSTER, &((4 * CLUSTER) | OFLAG_COPIED).to_be_bytes())
            .unwrap();
        device.write_at(5 * CLUSTER - 1, &[0]).unwrap(); // extend through cluster 4

        // Cluster content, compressed as a single DEFLATE stored block.
        let content: Vec<u8> = (0..CLUSTER as u32).map(|n| (n % 977 % 256) as u8).collect();
        let mut stream = vec![0x01, 0x00, 0x10, 0xff, 0xef]; // final, stored, len 4096
        stream.extend_from_slice(&content);
        device.write_at(5 * CLUSTER, &stream).unwrap();

        // L2 entry for guest cluster 9: compressed, at cluster 5.
        let x = 62 - (CLUSTER_BITS - 8);
        let sectors = (stream.len() as u64).div_ceil(512);
        let entry = OFLAG_COMPRESSED | ((sectors - 1) << x) | (5 * CLUSTER);
        device.write_at(4 * CLUSTER + 9 * 8, &entry.to_be_bytes()).unwrap();

        (device, content)
    }

    #[test]
    fn reads_a_compressed_cluster() {
        let (device, content) = compressed_qcow2();
        let mut qcow2 = Qcow2::open(device).expect("opens");
        let mut back = vec![0u8; CLUSTER as usize];
        qcow2.read_at(9 * CLUSTER, &mut back).expect("reads compressed");
        assert_eq!(back, content);
    }

    #[test]
    fn p8_gates_run_before_anything_else() {
        let mut device = empty_qcow2(CLUSTER);
        device.write_at(4, &9u32.to_be_bytes()).unwrap(); // version 9
        let Err(error) = Qcow2::open(device) else {
            panic!("future version must be refused")
        };
        assert!(error.to_string().contains("ceiling"));

        let mut device = empty_qcow2(CLUSTER);
        device.write_at(72, &(1u64 << 5).to_be_bytes()).unwrap();
        let Err(error) = Qcow2::open(device) else {
            panic!("unknown feature bit must be refused")
        };
        assert!(error.to_string().contains("incompatible feature"));
    }

    /// Attaches `backing` beneath an image the builders above produced.
    fn with_backing(mut device: VecDevice, backing: Backing<VecDevice>) -> Qcow2<VecDevice> {
        let header = Qcow2Header::parse(&mut device).expect("parses");
        Qcow2::assemble(device, header, Some(backing)).expect("assembles")
    }

    #[test]
    fn reads_compose_through_the_chain() {
        let virtual_size = 64 * CLUSTER;
        let deep: Vec<u8> = (0..CLUSTER as u32).map(|n| (n % 251) as u8).collect();
        let shadowed = vec![0x5au8; CLUSTER as usize];
        let top_own = vec![0xa5u8; CLUSTER as usize];

        // The base holds clusters 3 and 5; the top shadows cluster 5
        // with its own allocation; a middle image allocates nothing.
        let mut base = Qcow2::open(empty_qcow2(virtual_size)).expect("base opens");
        base.write_at(3 * CLUSTER, &deep).expect("base writes");
        base.write_at(5 * CLUSTER, &shadowed).expect("base writes");

        let mid = with_backing(
            empty_qcow2(virtual_size),
            Backing::Qcow2(Box::new(base)),
        );

        let mut top_builder = Qcow2::open(empty_qcow2(virtual_size)).expect("top opens");
        top_builder.write_at(5 * CLUSTER, &top_own).expect("top writes");
        let mut top = with_backing(top_builder.into_device(), Backing::Qcow2(Box::new(mid)));

        // Unallocated falls through two members; allocated shadows; a
        // cluster nobody holds is zero.
        let mut buf = vec![0u8; CLUSTER as usize];
        top.read_at(3 * CLUSTER, &mut buf).expect("reads");
        assert_eq!(buf, deep);
        top.read_at(5 * CLUSTER, &mut buf).expect("reads");
        assert_eq!(buf, top_own);
        top.read_at(7 * CLUSTER, &mut buf).expect("reads");
        assert!(buf.iter().all(|&byte| byte == 0));

        // One read spanning a fall-through cluster and a zero one
        // composes byte-exactly across the boundary.
        let mut span = vec![0xffu8; CLUSTER as usize];
        top.read_at(3 * CLUSTER + CLUSTER / 2, &mut span).expect("reads");
        assert_eq!(span[..CLUSTER as usize / 2], deep[CLUSTER as usize / 2..]);
        assert!(span[CLUSTER as usize / 2..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn a_zero_cluster_masks_the_backing() {
        let virtual_size = 64 * CLUSTER;
        let mut base = Qcow2::open(empty_qcow2(virtual_size)).expect("base opens");
        base.write_at(3 * CLUSTER, &vec![0xEEu8; CLUSTER as usize])
            .expect("base writes");

        // The top marks guest cluster 3 "reads as zero" by hand: an L2
        // table at cluster 4, its entry 3 carrying only the zero flag.
        let mut device = empty_qcow2(virtual_size);
        device
            .write_at(3 * CLUSTER, &((4 * CLUSTER) | OFLAG_COPIED).to_be_bytes())
            .unwrap();
        device.write_at(5 * CLUSTER - 1, &[0]).unwrap();
        device
            .write_at(4 * CLUSTER + 3 * 8, &OFLAG_ZERO.to_be_bytes())
            .unwrap();

        let mut top = with_backing(device, Backing::Qcow2(Box::new(base)));
        let mut buf = vec![0xffu8; CLUSTER as usize];
        top.read_at(3 * CLUSTER, &mut buf).expect("reads");
        assert!(
            buf.iter().all(|&byte| byte == 0),
            "the zero cluster masks the backing content"
        );
    }

    #[test]
    fn a_short_backing_reads_zero_past_its_end() {
        // A raw backing member half a cluster short of two clusters.
        let raw_len = 2 * CLUSTER as usize - CLUSTER as usize / 2;
        let raw: Vec<u8> = (0..raw_len as u32).map(|n| (n % 249 + 1) as u8).collect();
        let mut top = with_backing(
            empty_qcow2(64 * CLUSTER),
            Backing::Raw(VecDevice(raw.clone())),
        );

        // A read spanning the backing's end: its bytes, then zeros.
        let mut buf = vec![0xffu8; CLUSTER as usize];
        top.read_at(CLUSTER, &mut buf).expect("reads");
        assert_eq!(buf[..CLUSTER as usize / 2], raw[CLUSTER as usize..]);
        assert!(buf[CLUSTER as usize / 2..].iter().all(|&byte| byte == 0));

        // Far past the backing: all zero.
        let mut far = vec![0xffu8; 64];
        top.read_at(10 * CLUSTER, &mut far).expect("reads");
        assert!(far.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn compressed_clusters_decompress_wherever_they_sit() {
        let (device, content) = compressed_qcow2();
        let base = Qcow2::open(device).expect("base opens");
        let mut top = with_backing(
            empty_qcow2(64 * CLUSTER),
            Backing::Qcow2(Box::new(base)),
        );

        let mut back = vec![0u8; CLUSTER as usize];
        top.read_at(9 * CLUSTER, &mut back).expect("reads through");
        assert_eq!(back, content, "the backing's compressed cluster decompresses");
    }

    #[test]
    fn writing_through_a_chain_allocates_into_the_top_only() {
        let backing_bytes = vec![0x5au8; CLUSTER as usize];
        let mut top = with_backing(
            empty_qcow2(64 * CLUSTER),
            Backing::Raw(VecDevice(backing_bytes.clone())),
        );
        top.write_at(0, &[1, 2, 3]).expect("chain write allocates");

        let mut composed = vec![0u8; CLUSTER as usize];
        top.read_at(0, &mut composed).expect("written cluster reads");
        assert_eq!(&composed[..3], &[1, 2, 3]);
        assert_eq!(&composed[3..], &backing_bytes[3..]);

        let Some(Backing::Raw(backing)) = &top.backing else {
            panic!("raw backing remains attached");
        };
        assert_eq!(backing.0, backing_bytes, "the backing is never modified");
    }

    #[test]
    fn the_header_carries_the_backing_name_and_pinned_format() {
        let mut device = empty_qcow2(CLUSTER);
        let name = b"base.img";
        device.write_at(0x200, name).unwrap();
        device.write_at(8, &0x200u64.to_be_bytes()).unwrap();
        device
            .write_at(16, &(name.len() as u32).to_be_bytes())
            .unwrap();
        // The backing-format extension right after the v3 header: type,
        // length 3, "raw" padded to 8.
        device
            .write_at(112, &BACKING_FORMAT_EXTENSION.to_be_bytes())
            .unwrap();
        device.write_at(116, &3u32.to_be_bytes()).unwrap();
        device.write_at(120, b"raw").unwrap();

        let header = Qcow2Header::parse(&mut device).expect("parses");
        assert_eq!(header.backing_file.as_deref(), Some("base.img"));
        assert_eq!(header.backing_format.as_deref(), Some("raw"));

        // The standalone open names what it will not compose.
        let error = Qcow2::open(device).expect_err("standalone open refuses");
        assert!(error.to_string().contains("base.img"));
        assert!(error.to_string().contains("standalone"));
    }
}

impl<D: Device> Device for Qcow2<D> {
    fn len(&self) -> u64 {
        self.header.virtual_size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.header.virtual_size {
            return Err(invalid("read past the end of the virtual disk"));
        }
        let cluster_size = self.cluster_size();
        let mut done = 0usize;
        while done < buf.len() {
            let at = offset + done as u64;
            let within = at % cluster_size;
            let take = ((cluster_size - within) as usize).min(buf.len() - done);
            let (_, rest) = buf.split_at_mut(done);
            self.read_cluster(at, &mut rest[..take])?;
            done += take;
        }
        Ok(())
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if offset + data.len() as u64 > self.header.virtual_size {
            return Err(invalid("write past the end of the virtual disk"));
        }
        let cluster_size = self.cluster_size();
        let mut done = 0usize;
        while done < data.len() {
            let at = offset + done as u64;
            let within = at % cluster_size;
            let take = ((cluster_size - within) as usize).min(data.len() - done);
            self.write_cluster(at, &data[done..done + take])?;
            done += take;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.device.flush()
    }
}
