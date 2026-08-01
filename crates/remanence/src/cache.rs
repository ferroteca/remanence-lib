// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The session cache: the bounded working set the disk stack streams
//! through (pledged P27) and the commit-point buffer (P2). Reads load
//! extents from the virtual disk on demand and serve later hits without
//! disk I/O; altered extents are the session's uncommitted truth and are
//! never dropped — they stay resident within the bound and spill to
//! private session storage beyond it, and nothing reaches the image
//! before commit. Clean extents are always evictable, because the P7
//! claim guarantees the source cannot change beneath the session, so a
//! dropped extent re-reads from its backing at will. A small image
//! simply becomes fully resident and disk I/O stops; a huge one
//! converges on the operation's working set — one policy at two sizes.
//!
//! The spill file is private transient state in the same sense as the
//! P9 journal: no user-owned path, no cleanup verb, no contract about
//! its location or form. Unlike the journal it is never load-bearing
//! after interruption — it holds exactly the uncommitted state a
//! rollback discards — so it is created unlinked (POSIX) or
//! delete-on-close (Windows) and cannot outlive the session.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::device::{Device, read_exact_at, write_all_at};
use crate::error::{Error, Result};

/// Bytes per cache extent — the unit of residency, alteration tracking,
/// eviction, and spill. It is also the read-ahead unit: a miss loads its
/// whole extent, so a run of small reads costs one base read.
pub(crate) const EXTENT: u64 = 64 * 1024;

/// The stated default residency bound (P27), in extents: 2048 extents of
/// 64 KiB — a 128 MiB working set. Peak cache memory is bounded by this
/// figure regardless of image size.
const DEFAULT_RESIDENT_EXTENTS: usize = 2048;

static SPILL_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct Extent {
    data: Vec<u8>,
    dirty: bool,
    /// Recency stamp for eviction; larger is more recent.
    used: u64,
}

/// The private spill file altered extents move to when evicted. Slots
/// are extent-sized; an extent keeps its slot for the session, so a
/// re-spill overwrites in place. The file is unlinked at creation
/// (POSIX) or delete-on-close (Windows): it cannot survive the process.
#[derive(Debug)]
struct Spill {
    file: File,
    /// Extent offset in the virtual disk -> slot index in the file.
    slots: BTreeMap<u64, u64>,
    next_slot: u64,
}

impl Spill {
    fn create() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "remanence-spill-{}-{}.tmp",
            std::process::id(),
            SPILL_SERIAL.fetch_add(1, Ordering::Relaxed),
        ));
        let file = open_spill(&path).map_err(|error| {
            Error::io(format!(
                "cannot create session spill storage '{}': {error}",
                path.display()
            ))
        })?;
        Ok(Self { file, slots: BTreeMap::new(), next_slot: 0 })
    }

    fn write(&mut self, extent_offset: u64, data: &[u8]) -> Result<()> {
        let slot = *self
            .slots
            .entry(extent_offset)
            .or_insert_with(|| {
                let slot = self.next_slot;
                self.next_slot += 1;
                slot
            });
        write_all_at(&self.file, slot * EXTENT, data)
            .map_err(|error| Error::io(format!("session spill write failed: {error}")))
    }

    fn read(&self, extent_offset: u64, data: &mut [u8]) -> Result<()> {
        let slot = *self.slots.get(&extent_offset).expect("a spilled extent has a slot");
        read_exact_at(&self.file, slot * EXTENT, data)
            .map_err(|error| Error::io(format!("session spill read failed: {error}")))
    }

    fn holds(&self, extent_offset: u64) -> bool {
        self.slots.contains_key(&extent_offset)
    }
}

#[cfg(windows)]
fn open_spill(path: &std::path::Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // Delete-on-close needs the DELETE access right beside read/write;
    // share nothing — the spill is exclusively the session's (P7's
    // spirit applied to our own transient state).
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const DELETE: u32 = 0x0001_0000;
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
    OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .share_mode(0)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(path)
}

#[cfg(not(windows))]
fn open_spill(path: &std::path::Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)?;
    // Unlink immediately: the handle keeps the storage alive, nothing
    // else can reach it, and the OS reclaims it however the process ends.
    let _ = std::fs::remove_file(path);
    Ok(file)
}

/// The bounded session cache over one virtual disk. All session reads
/// and uncommitted writes pass through it; the base device is touched
/// only to load a missing extent or to write altered extents through at
/// commit.
#[derive(Debug)]
pub(crate) struct SessionCache {
    resident: BTreeMap<u64, Extent>,
    bound: usize,
    clock: u64,
    spill: Option<Spill>,
}

impl SessionCache {
    pub fn new() -> Self {
        Self::with_bound(DEFAULT_RESIDENT_EXTENTS)
    }

    /// A cache holding at most `bound` resident extents (tests use tiny
    /// bounds to force eviction and spill on small images).
    pub fn with_bound(bound: usize) -> Self {
        Self {
            resident: BTreeMap::new(),
            bound: bound.max(1),
            clock: 0,
            spill: None,
        }
    }

    /// Whether uncommitted changes exist, resident or spilled.
    pub fn modified(&self) -> bool {
        self.resident.values().any(|extent| extent.dirty)
            || self.spill.as_ref().is_some_and(|spill| !spill.slots.is_empty())
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Evicts one extent to make room: the least-recently-used clean
    /// extent is simply dropped (its backing re-supplies it); when every
    /// resident extent is altered, the least-recently-used one moves to
    /// spill storage. Altered data is never lost to eviction.
    fn evict_one(&mut self) -> Result<()> {
        let clean = self
            .resident
            .iter()
            .filter(|(_, extent)| !extent.dirty)
            .min_by_key(|(_, extent)| extent.used)
            .map(|(&offset, _)| offset);
        let victim = match clean {
            Some(offset) => offset,
            None => self
                .resident
                .iter()
                .min_by_key(|(_, extent)| extent.used)
                .map(|(&offset, _)| offset)
                .expect("eviction runs only with residents"),
        };
        let extent = self.resident.remove(&victim).expect("the victim is resident");
        if extent.dirty {
            if self.spill.is_none() {
                self.spill = Some(Spill::create()?);
            }
            self.spill
                .as_mut()
                .expect("just created")
                .write(victim, &extent.data)?;
        }
        Ok(())
    }

    fn make_room(&mut self) -> Result<()> {
        while self.resident.len() >= self.bound {
            self.evict_one()?;
        }
        Ok(())
    }

    /// Ensures the extent at `extent_offset` is resident: from spill
    /// storage when it was altered and evicted (it stays altered), else
    /// seeded from the base — clamped at the base's length, zero past it,
    /// so a partial write keeps its surrounding bytes.
    fn ensure_resident(
        &mut self,
        base: &mut dyn Device,
        extent_offset: u64,
    ) -> Result<()> {
        if self.resident.contains_key(&extent_offset) {
            return Ok(());
        }
        self.make_room()?;
        let mut data = vec![0u8; EXTENT as usize];
        let dirty = match &self.spill {
            Some(spill) if spill.holds(extent_offset) => {
                spill.read(extent_offset, &mut data)?;
                true
            }
            _ => {
                let base_len = base.len();
                if extent_offset < base_len {
                    let take = ((base_len - extent_offset) as usize).min(EXTENT as usize);
                    base.read_at(extent_offset, &mut data[..take])?;
                }
                false
            }
        };
        let used = self.tick();
        self.resident.insert(extent_offset, Extent { data, dirty, used });
        Ok(())
    }

    /// Reads `buf` through the cache: resident extents serve without
    /// disk I/O, missing ones load from spill storage or the base. The
    /// read is bounded by the base device exactly as an uncached read
    /// would be.
    pub fn read_at(
        &mut self,
        base: &mut dyn Device,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        if offset + buf.len() as u64 > base.len() {
            return Err(Error::io(format!(
                "read past end of device (offset {offset}, length {})",
                buf.len()
            )));
        }
        let end = offset + buf.len() as u64;
        let mut extent_offset = offset / EXTENT * EXTENT;
        while extent_offset < end {
            self.ensure_resident(base, extent_offset)?;
            let used = self.tick();
            let extent = self.resident.get_mut(&extent_offset).expect("just ensured");
            extent.used = used;
            let from = extent_offset.max(offset);
            let to = (extent_offset + EXTENT).min(end);
            buf[(from - offset) as usize..(to - offset) as usize].copy_from_slice(
                &extent.data[(from - extent_offset) as usize..(to - extent_offset) as usize],
            );
            extent_offset += EXTENT;
        }
        Ok(())
    }

    /// Buffers a write in the cache; nothing reaches `base` until the
    /// commit writes altered extents through (P2).
    pub fn write_at(
        &mut self,
        base: &mut dyn Device,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        let end = offset + data.len() as u64;
        let mut extent_offset = offset / EXTENT * EXTENT;
        while extent_offset < end {
            self.ensure_resident(base, extent_offset)?;
            let used = self.tick();
            let extent = self.resident.get_mut(&extent_offset).expect("just ensured");
            extent.used = used;
            extent.dirty = true;
            let from = extent_offset.max(offset);
            let to = (extent_offset + EXTENT).min(end);
            extent.data[(from - extent_offset) as usize..(to - extent_offset) as usize]
                .copy_from_slice(&data[(from - offset) as usize..(to - offset) as usize]);
            extent_offset += EXTENT;
        }
        Ok(())
    }

    /// Writes every altered extent through to `base` — resident and
    /// spilled alike, in offset order, one bounded buffer at a time. The
    /// cache keeps its state: the commit marks it committed only once
    /// the write-through is durably applied, so a failure anywhere
    /// leaves the buffered state intact.
    pub fn write_through(&self, base: &mut dyn Device) -> Result<()> {
        let mut offsets: Vec<u64> = self
            .resident
            .iter()
            .filter(|(_, extent)| extent.dirty)
            .map(|(&offset, _)| offset)
            .collect();
        if let Some(spill) = &self.spill {
            offsets.extend(spill.slots.keys().copied());
        }
        offsets.sort_unstable();
        offsets.dedup();

        let mut spill_buf = vec![0u8; EXTENT as usize];
        for extent_offset in offsets {
            let data: &[u8] = match self.resident.get(&extent_offset) {
                Some(extent) => &extent.data,
                None => {
                    self.spill
                        .as_ref()
                        .expect("a non-resident altered extent is spilled")
                        .read(extent_offset, &mut spill_buf)?;
                    &spill_buf
                }
            };
            let take = if extent_offset + EXTENT > base.len() {
                // The device may legitimately end mid-extent.
                (base.len().max(extent_offset) - extent_offset) as usize
            } else {
                EXTENT as usize
            };
            if take > 0 {
                base.write_at(extent_offset, &data[..take])?;
            }
        }
        Ok(())
    }

    /// The commit landed: altered extents are now the image's own bytes,
    /// so they become clean — and stay resident, still serving reads —
    /// and the spill storage is released.
    pub fn mark_committed(&mut self) {
        for extent in self.resident.values_mut() {
            extent.dirty = false;
        }
        self.spill = None;
    }

    /// Rollback (P2): every altered extent is discarded, resident and
    /// spilled alike. Clean extents still mirror the untouched image and
    /// stay resident.
    pub fn discard_dirty(&mut self) {
        self.resident.retain(|_, extent| !extent.dirty);
        self.spill = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base device that counts its I/O, so a test can prove what the
    /// cache did and did not touch.
    struct CountingDevice {
        bytes: Vec<u8>,
        reads: usize,
        writes: usize,
    }

    impl CountingDevice {
        fn new(len: usize) -> Self {
            Self {
                bytes: (0..len).map(|n| (n % 251) as u8).collect(),
                reads: 0,
                writes: 0,
            }
        }
    }

    impl Device for CountingDevice {
        fn len(&self) -> u64 {
            self.bytes.len() as u64
        }

        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
            self.reads += 1;
            let start = offset as usize;
            buf.copy_from_slice(&self.bytes[start..start + buf.len()]);
            Ok(())
        }

        fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
            self.writes += 1;
            let start = offset as usize;
            self.bytes[start..start + data.len()].copy_from_slice(data);
            Ok(())
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    const E: usize = EXTENT as usize;

    #[test]
    fn a_hit_serves_from_cache_without_base_io() {
        let mut base = CountingDevice::new(4 * E);
        let mut cache = SessionCache::new();

        let mut first = [0u8; 512];
        cache.read_at(&mut base, 100, &mut first).expect("reads");
        assert_eq!(base.reads, 1, "the miss loads its whole extent");

        let mut second = [0u8; 512];
        cache.read_at(&mut base, 40_000, &mut second).expect("reads");
        assert_eq!(
            base.reads, 1,
            "a later read inside the loaded extent costs no base I/O"
        );
        assert_eq!(&second[..], &base.bytes[40_000..40_512]);
    }

    #[test]
    fn clean_extents_evict_and_reread_from_the_base() {
        let mut base = CountingDevice::new(4 * E);
        let mut cache = SessionCache::with_bound(1);

        let mut buf = [0u8; 16];
        cache.read_at(&mut base, 0, &mut buf).expect("reads A");
        cache.read_at(&mut base, E as u64, &mut buf).expect("reads B evicting A");
        cache.read_at(&mut base, 0, &mut buf).expect("re-reads A");
        assert_eq!(base.reads, 3, "the evicted clean extent re-reads from the base");
        assert_eq!(&buf[..], &base.bytes[..16]);
    }

    #[test]
    fn altered_extents_spill_and_survive_eviction() {
        let mut base = CountingDevice::new(4 * E);
        let mut cache = SessionCache::with_bound(1);

        cache.write_at(&mut base, 10, b"altered").expect("writes");
        // Forcing the altered extent out: it must go to spill storage,
        // not to the base and not to the void.
        let mut buf = [0u8; 16];
        cache
            .read_at(&mut base, 2 * E as u64, &mut buf)
            .expect("reads another extent, evicting the altered one");
        assert_eq!(base.writes, 0, "nothing reaches the base before commit");
        assert!(cache.modified(), "spilled alterations still count as modified");

        let mut back = [0u8; 7];
        cache.read_at(&mut base, 10, &mut back).expect("reads the altered bytes back");
        assert_eq!(&back, b"altered", "the spilled extent returns the altered data");
        // The surrounding bytes of the seeded extent survived the trip.
        let mut around = [0u8; 4];
        cache.read_at(&mut base, 6, &mut around).expect("reads around");
        assert_eq!(&around[..], &base.bytes[6..10]);
    }

    #[test]
    fn write_through_projects_resident_and_spilled_extents() {
        let mut base = CountingDevice::new(4 * E);
        let mut cache = SessionCache::with_bound(1);

        cache.write_at(&mut base, 5, b"first").expect("writes A");
        cache
            .write_at(&mut base, 2 * E as u64 + 9, b"second")
            .expect("writes C, spilling A");

        cache.write_through(&mut base).expect("writes through");
        assert_eq!(&base.bytes[5..10], b"first", "the spilled extent landed");
        assert_eq!(&base.bytes[2 * E + 9..2 * E + 15], b"second", "the resident one landed");

        cache.mark_committed();
        assert!(!cache.modified(), "a committed cache reports no changes");
        // The committed extent stays resident and keeps serving.
        let reads_before = base.reads;
        let mut buf = [0u8; 6];
        cache.read_at(&mut base, 2 * E as u64 + 9, &mut buf).expect("reads");
        assert_eq!(&buf, b"second");
        assert_eq!(base.reads, reads_before, "committed extents keep serving from cache");
    }

    #[test]
    fn rollback_discards_altered_extents_everywhere() {
        let mut base = CountingDevice::new(4 * E);
        let mut cache = SessionCache::with_bound(1);

        cache.write_at(&mut base, 20, b"doomed").expect("writes");
        let mut buf = [0u8; 8];
        cache
            .read_at(&mut base, 3 * E as u64, &mut buf)
            .expect("reads, spilling the altered extent");
        cache.discard_dirty();
        assert!(!cache.modified());

        let mut back = [0u8; 6];
        cache.read_at(&mut base, 20, &mut back).expect("reads");
        assert_eq!(&back[..], &base.bytes[20..26], "the base bytes are back");
        assert_eq!(base.writes, 0, "rollback never touches the base");
    }

    #[test]
    fn reads_past_the_end_are_refused() {
        let mut base = CountingDevice::new(E);
        let mut cache = SessionCache::new();
        let mut buf = [0u8; 32];
        let error = cache
            .read_at(&mut base, EXTENT - 16, &mut buf)
            .expect_err("a read past the device end is refused");
        assert!(error.to_string().contains("past end"));
    }

    #[test]
    fn a_partial_tail_extent_round_trips() {
        // A device ending mid-extent: seeds clamp, write-through clamps.
        let len = E + E / 2;
        let mut base = CountingDevice::new(len);
        let mut cache = SessionCache::new();

        cache
            .write_at(&mut base, (len - 8) as u64, b"tailtail")
            .expect("writes at the tail");
        let mut back = [0u8; 8];
        cache.read_at(&mut base, (len - 8) as u64, &mut back).expect("reads");
        assert_eq!(&back, b"tailtail");

        cache.write_through(&mut base).expect("writes through");
        assert_eq!(&base.bytes[len - 8..], b"tailtail");
        assert_eq!(
            base.bytes.len(),
            len,
            "the clamped write-through never grows the device"
        );
    }
}
