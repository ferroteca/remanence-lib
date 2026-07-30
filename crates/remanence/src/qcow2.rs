// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Native qcow2 v2/v3 driver, written from the published format
//! documentation (QEMU `docs/interop/qcow2`). Presents the virtual disk as
//! a [`Device`]. The support claim, per P8, is validated before anything
//! else is touched: versions 2 and 3, standalone images only — no backing
//! file, no external data file, no encryption, no unknown incompatible
//! feature bits. Writing additionally requires refcount width 16 and an
//! image without internal snapshots.

use crate::device::Device;
use crate::error::{Error, Result};
use crate::inflate::inflate;

pub(crate) const QCOW2_MAGIC: [u8; 4] = *b"QFI\xfb";

/// The highest qcow2 version this release explicitly supports (P8).
const SUPPORTED_VERSION_CEILING: u32 = 3;

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
            return Err(invalid(format!("unsupported qcow2 version {version}")));
        }
        if version > SUPPORTED_VERSION_CEILING {
            return Err(invalid(format!(
                "qcow2 version {version} is newer than this release supports \
                 (ceiling: version {SUPPORTED_VERSION_CEILING}); refusing to touch it"
            )));
        }

        let backing_file_offset = be64(&raw, 8);
        if backing_file_offset != 0 {
            return Err(invalid("backing files are not supported"));
        }
        let cluster_bits = be32(&raw, 20);
        if !(9..=21).contains(&cluster_bits) {
            return Err(invalid(format!("implausible cluster_bits {cluster_bits}")));
        }
        let virtual_size = be64(&raw, 24);
        let crypt_method = be32(&raw, 32);
        if crypt_method != 0 {
            return Err(invalid("encrypted images are not supported"));
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
                return Err(invalid(format!(
                    "incompatible feature bits 0x{incompatible:x} are beyond this \
                     release's support; refusing to touch the image"
                )));
            }
            be32(&raw, 96)
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
        })
    }

    pub fn cluster_size(&self) -> u64 {
        1u64 << self.cluster_bits
    }
}

/// The virtual disk a qcow2 file describes, as a [`Device`].
#[derive(Debug)]
pub(crate) struct Qcow2<D: Device> {
    device: D,
    header: Qcow2Header,
    l1: Vec<u64>,
    writable_checked: bool,
}

impl<D: Device> Qcow2<D> {
    pub fn open(mut device: D) -> Result<Self> {
        let header = Qcow2Header::parse(&mut device)?;

        let l1_bytes = header.l1_size as usize * 8;
        let mut raw = vec![0u8; l1_bytes];
        device.read_at(header.l1_table_offset, &mut raw)?;
        let l1 = raw.chunks_exact(8).map(|chunk| be64(chunk, 0)).collect();

        Ok(Self { device, header, l1, writable_checked: false })
    }

    pub fn header(&self) -> &Qcow2Header {
        &self.header
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
            return Err(invalid(
                "image carries internal snapshots; writing is not supported",
            ));
        }
        if self.header.refcount_order != 4 {
            return Err(invalid(format!(
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

        let host_offset = entry & L2_OFFSET_MASK;
        if host_offset == 0 || entry & OFLAG_ZERO != 0 {
            buf.fill(0);
            return Ok(());
        }
        self.device.read_at(host_offset + within, buf)
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
            return Err(invalid(
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
                return Err(invalid(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A growable in-memory device for building images in tests.
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

    #[test]
    fn reads_a_compressed_cluster() {
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

        let mut device = empty_qcow2(CLUSTER);
        device.write_at(8, &(2 * CLUSTER).to_be_bytes()).unwrap();
        let Err(error) = Qcow2::open(device) else {
            panic!("backing file must be refused")
        };
        assert!(error.to_string().contains("backing"));
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
