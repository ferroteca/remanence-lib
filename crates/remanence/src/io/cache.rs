// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The session cache: the bounded working set the device stack streams
//! through (P27) and the commit-point buffer (P2). Reads load
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
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::error::{Error, Result};
use crate::io::device::{Device, read_exact_at, write_all_at};

/// Bytes per cache extent — the unit of residency, alteration tracking,
/// eviction, and spill. It is also the read-ahead unit: a miss loads its
/// whole extent, so a run of small reads costs one base read.
pub(crate) const EXTENT: u64 = 64 * 1024;

/// The stated default session cache bound (P27), in bytes: unless a
/// caller declares another bound at open, the resident working set
/// never exceeds this, regardless of image size.
pub const DEFAULT_CACHE_BYTES: u64 = 128 * 1024 * 1024;

static SPILL_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct Extent {
    data: Vec<u8>,
    dirty: bool,
    /// Recency stamp for eviction; larger is more recent.
    used: u64,
    /// Alteration stamp: bumped on every write, so an offload in
    /// flight can tell whether its data is still current.
    version: u64,
}

/// One extent handed to the offload worker: it writes `data` to `slot`
/// of `file` and reports back. The worker never touches cache state.
struct OffloadJob {
    file: Arc<File>,
    slot: u64,
    offset: u64,
    version: u64,
    data: Vec<u8>,
}

struct OffloadDone {
    offset: u64,
    version: u64,
    ok: bool,
}

/// The background offload worker (P34): spills altered extents
/// ahead of memory pressure so eviction rarely stalls on I/O. An extent
/// leaves memory only once its spill write has completed — the no-gap
/// rule — and a failed speculative write reports nothing: the extent
/// simply stays resident and a later synchronous spill owns any error.
struct Offloader {
    jobs: Sender<OffloadJob>,
    /// Receiver is not `Sync`; the lock keeps the cache embeddable in
    /// `Sync` owners.
    done: Mutex<Receiver<OffloadDone>>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Offloader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Offloader")
    }
}

impl Offloader {
    fn spawn() -> Self {
        let (jobs, job_receiver) = channel::<OffloadJob>();
        let (done_sender, done) = channel::<OffloadDone>();
        let handle = std::thread::spawn(move || {
            while let Ok(job) = job_receiver.recv() {
                let ok = write_all_at(&job.file, job.slot * EXTENT, &job.data).is_ok();
                if done_sender
                    .send(OffloadDone {
                        offset: job.offset,
                        version: job.version,
                        ok,
                    })
                    .is_err()
                {
                    return;
                }
            }
        });
        Self {
            jobs,
            done: Mutex::new(done),
            handle: Some(handle),
        }
    }
}

impl Drop for Offloader {
    fn drop(&mut self) {
        // Closing the job channel ends the worker; joining keeps the
        // thread from outliving the session.
        let (orphan, _) = channel();
        drop(std::mem::replace(&mut self.jobs, orphan));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// How many offload jobs may be in flight at once: each carries one
/// cloned extent, so this bounds the speculation's own memory (P34's
/// demand rule, spending P27's budget).
const OFFLOAD_IN_FLIGHT: usize = 4;

/// The private spill file altered extents move to when evicted. Slots
/// are extent-sized; an extent keeps its slot for the session, so a
/// re-spill overwrites in place. The file is unlinked at creation
/// (POSIX) or delete-on-close (Windows): it cannot survive the process.
#[derive(Debug)]
struct Spill {
    /// Shared with offload jobs in flight; a write to a spill the
    /// cache has already discarded lands in an unlinked file, harmlessly.
    file: Arc<File>,
    /// Extent offset in the virtual disk -> slot index in the file.
    slots: BTreeMap<u64, u64>,
    next_slot: u64,
}

/// Creates one file of private session storage (P27): no user-owned
/// path, no cleanup verb — unlinked at birth (POSIX) or delete-on-close
/// (Windows), so it cannot outlive the process however the session
/// ends. The cache spills altered extents here; the archive path spools
/// decoded entries here.
pub(crate) fn session_storage_file() -> Result<File> {
    let path = std::env::temp_dir().join(format!(
        "remanence-session-{}-{}.tmp",
        std::process::id(),
        SPILL_SERIAL.fetch_add(1, Ordering::Relaxed),
    ));
    open_spill(&path).map_err(|error| {
        Error::io(format!(
            "cannot create session storage '{}': {error}",
            path.display()
        ))
    })
}

impl Spill {
    fn create() -> Result<Self> {
        let file = Arc::new(session_storage_file()?);
        Ok(Self {
            file,
            slots: BTreeMap::new(),
            next_slot: 0,
        })
    }

    fn slot_for(&mut self, extent_offset: u64) -> u64 {
        *self.slots.entry(extent_offset).or_insert_with(|| {
            let slot = self.next_slot;
            self.next_slot += 1;
            slot
        })
    }

    fn write(&mut self, extent_offset: u64, data: &[u8]) -> Result<()> {
        let slot = self.slot_for(extent_offset);
        write_all_at(&self.file, slot * EXTENT, data)
            .map_err(|error| Error::io(format!("session spill write failed: {error}")))
    }

    fn read(&self, extent_offset: u64, data: &mut [u8]) -> Result<()> {
        let slot = *self
            .slots
            .get(&extent_offset)
            .expect("a spilled extent has a slot");
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
    /// The background offload worker, spawned on first use where
    /// offloading is enabled.
    offloader: Option<Offloader>,
    offload_enabled: bool,
    /// Extent offset -> version handed to an offload job in flight.
    in_flight: BTreeMap<u64, u64>,
    /// Extent offset -> version whose slot the worker has written: an
    /// eviction matching it drops without I/O.
    prespilled: BTreeMap<u64, u64>,
}

impl SessionCache {
    /// A cache bounded at `bytes` of resident extents — P27's declared
    /// session configuration. Rounded up to whole extents, one at
    /// minimum: a tiny bound narrows the working set, it never refuses
    /// service.
    pub fn with_bytes(bytes: u64) -> Self {
        let extents = bytes.div_ceil(EXTENT).max(1);
        Self::with_bound(usize::try_from(extents).unwrap_or(usize::MAX))
    }

    /// As [`SessionCache::with_bytes`], with background offload enabled:
    /// altered extents pre-spill on a worker thread ahead of memory
    /// pressure, so eviction rarely stalls on I/O.
    pub fn with_bytes_offloading(bytes: u64) -> Self {
        let mut cache = Self::with_bytes(bytes);
        cache.offload_enabled = true;
        cache
    }

    /// A cache holding at most `bound` resident extents (tests use tiny
    /// bounds to force eviction and spill on small images).
    fn with_bound(bound: usize) -> Self {
        Self {
            resident: BTreeMap::new(),
            bound: bound.max(1),
            clock: 0,
            spill: None,
            offloader: None,
            offload_enabled: false,
            in_flight: BTreeMap::new(),
            prespilled: BTreeMap::new(),
        }
    }

    /// Whether uncommitted changes exist, resident or spilled.
    pub fn modified(&self) -> bool {
        self.resident.values().any(|extent| extent.dirty)
            || self
                .spill
                .as_ref()
                .is_some_and(|spill| !spill.slots.is_empty())
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Applies finished offloads without blocking: a completion whose
    /// extent is unchanged since the job was cut lets that extent drop
    /// from memory now — its slot already holds the data, so the no-gap
    /// rule is satisfied — and a failed or outdated one changes nothing.
    fn drain_offloads(&mut self) {
        let Some(offloader) = &self.offloader else {
            return;
        };
        let done: Vec<OffloadDone> = {
            let receiver = offloader
                .done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            receiver.try_iter().collect()
        };
        for finished in done {
            self.apply_offload(finished);
        }
    }

    fn apply_offload(&mut self, finished: OffloadDone) {
        let expected = self.in_flight.remove(&finished.offset);
        if expected != Some(finished.version) || !finished.ok {
            return;
        }
        match self.resident.get(&finished.offset) {
            Some(extent) if extent.dirty && extent.version == finished.version => {
                self.prespilled.insert(finished.offset, finished.version);
                self.resident.remove(&finished.offset);
            }
            _ => {}
        }
    }

    /// Blocks until no offload is in flight — the join every act that
    /// consumes the altered set performs first.
    pub fn join_offloads(&mut self) {
        while !self.in_flight.is_empty() {
            let Some(offloader) = &self.offloader else {
                self.in_flight.clear();
                return;
            };
            let finished = {
                let receiver = offloader
                    .done
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                receiver.recv()
            };
            match finished {
                Ok(finished) => self.apply_offload(finished),
                Err(_) => {
                    self.in_flight.clear();
                    return;
                }
            }
        }
    }

    /// Hands least-recently-used altered extents to the offload worker
    /// once the cache nears its bound, keeping eviction off the hot
    /// path. Speculation only: the extent stays resident and unchanged
    /// until its completed write proves the slot current.
    fn maybe_offload(&mut self) {
        if !self.offload_enabled || self.bound < 4 {
            return;
        }
        let high_water = self.bound - self.bound / 8;
        if self.resident.len() < high_water {
            return;
        }
        while self.in_flight.len() < OFFLOAD_IN_FLIGHT {
            let candidate = self
                .resident
                .iter()
                .filter(|(offset, extent)| {
                    extent.dirty
                        && !self.in_flight.contains_key(offset)
                        && self.prespilled.get(offset) != Some(&extent.version)
                })
                .min_by_key(|(_, extent)| extent.used)
                .map(|(&offset, extent)| (offset, extent.version, extent.data.clone()));
            let Some((offset, version, data)) = candidate else {
                return;
            };
            if self.spill.is_none() {
                let Ok(spill) = Spill::create() else {
                    // Speculation is silent: without spill storage the
                    // synchronous path owns any error, later.
                    self.offload_enabled = false;
                    return;
                };
                self.spill = Some(spill);
            }
            let spill = self.spill.as_mut().expect("present");
            let slot = spill.slot_for(offset);
            let file = Arc::clone(&spill.file);
            if self.offloader.is_none() {
                self.offloader = Some(Offloader::spawn());
            }
            let offloader = self.offloader.as_ref().expect("just spawned");
            if offloader
                .jobs
                .send(OffloadJob {
                    file,
                    slot,
                    offset,
                    version,
                    data,
                })
                .is_err()
            {
                // A dead worker degrades to synchronous spilling.
                return;
            }
            self.in_flight.insert(offset, version);
        }
    }

    /// Evicts one extent to make room: the least-recently-used clean
    /// extent is simply dropped (its backing re-supplies it); when every
    /// resident extent is altered, the least-recently-used one moves to
    /// spill storage — instantly when a completed offload already wrote
    /// its slot, synchronously otherwise. Altered data is never lost to
    /// eviction, and an extent with an offload in flight is never
    /// touched (its slot is being written).
    fn evict_one(&mut self) -> Result<()> {
        let clean = self
            .resident
            .iter()
            .filter(|(_, extent)| !extent.dirty)
            .min_by_key(|(_, extent)| extent.used)
            .map(|(&offset, _)| offset);
        if let Some(offset) = clean {
            self.resident.remove(&offset);
            return Ok(());
        }
        let dirty = self
            .resident
            .iter()
            .filter(|(offset, _)| !self.in_flight.contains_key(offset))
            .min_by_key(|(_, extent)| extent.used)
            .map(|(&offset, _)| offset);
        let Some(offset) = dirty else {
            // Every candidate is being offloaded; one completion frees
            // room or returns an extent to the synchronous path.
            self.join_one();
            return Ok(());
        };
        let extent = self
            .resident
            .remove(&offset)
            .expect("the victim is resident");
        if self.prespilled.get(&offset) == Some(&extent.version) {
            // The worker already wrote exactly this data to its slot.
            self.prespilled.remove(&offset);
            return Ok(());
        }
        if self.spill.is_none() {
            self.spill = Some(Spill::create()?);
        }
        self.spill
            .as_mut()
            .expect("just created")
            .write(offset, &extent.data)?;
        Ok(())
    }

    fn join_one(&mut self) {
        let Some(offloader) = &self.offloader else {
            self.in_flight.clear();
            return;
        };
        let finished = {
            let receiver = offloader
                .done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            receiver.recv()
        };
        match finished {
            Ok(finished) => self.apply_offload(finished),
            Err(_) => self.in_flight.clear(),
        }
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
    fn ensure_resident(&mut self, base: &mut dyn Device, extent_offset: u64) -> Result<()> {
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
        self.resident.insert(
            extent_offset,
            Extent {
                data,
                dirty,
                used,
                version: used,
            },
        );
        Ok(())
    }

    /// Whether the extent at `extent_offset` is resident right now —
    /// the prefetch worker's cheap look-before-reading.
    pub fn is_resident(&self, extent_offset: u64) -> bool {
        self.resident.contains_key(&extent_offset)
    }

    /// Installs a speculatively loaded clean extent (P34 speculation).
    /// Speculation never replaces present state and never causes spill
    /// I/O: it fills free room, or takes the place of one evictable
    /// clean extent, or is discarded.
    pub fn insert_prefetched(&mut self, extent_offset: u64, data: Vec<u8>) {
        debug_assert_eq!(data.len(), EXTENT as usize);
        if self.resident.contains_key(&extent_offset)
            || self
                .spill
                .as_ref()
                .is_some_and(|spill| spill.holds(extent_offset))
        {
            return;
        }
        if self.resident.len() >= self.bound {
            let clean = self
                .resident
                .iter()
                .filter(|(_, extent)| !extent.dirty)
                .min_by_key(|(_, extent)| extent.used)
                .map(|(&offset, _)| offset);
            let Some(victim) = clean else {
                return;
            };
            self.resident.remove(&victim);
        }
        let used = self.tick();
        self.resident.insert(
            extent_offset,
            Extent {
                data,
                dirty: false,
                used,
                version: used,
            },
        );
    }

    /// Reads `buf` through the cache: resident extents serve without
    /// disk I/O, missing ones load from spill storage or the base. The
    /// read is bounded by the base device exactly as an uncached read
    /// would be.
    pub fn read_at(&mut self, base: &mut dyn Device, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.drain_offloads();
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
    pub fn write_at(&mut self, base: &mut dyn Device, offset: u64, data: &[u8]) -> Result<()> {
        self.drain_offloads();
        let end = offset + data.len() as u64;
        let mut extent_offset = offset / EXTENT * EXTENT;
        while extent_offset < end {
            self.ensure_resident(base, extent_offset)?;
            let used = self.tick();
            let extent = self.resident.get_mut(&extent_offset).expect("just ensured");
            extent.used = used;
            extent.dirty = true;
            extent.version = used;
            let from = extent_offset.max(offset);
            let to = (extent_offset + EXTENT).min(end);
            extent.data[(from - extent_offset) as usize..(to - extent_offset) as usize]
                .copy_from_slice(&data[(from - offset) as usize..(to - offset) as usize]);
            extent_offset += EXTENT;
        }
        self.maybe_offload();
        Ok(())
    }

    /// Calls `f` for every altered extent in offset order — resident
    /// extents from memory, spilled ones through one bounded buffer.
    /// The commit pipeline streams from this; nothing materializes the
    /// altered set whole.
    pub fn for_each_dirty(&self, f: &mut dyn FnMut(u64, &[u8]) -> Result<()>) -> Result<()> {
        debug_assert!(
            self.in_flight.is_empty(),
            "join offloads before consuming the altered set"
        );
        let mut spill_buf = vec![0u8; EXTENT as usize];
        let mut resident = self
            .resident
            .iter()
            .filter(|(_, extent)| extent.dirty)
            .peekable();
        let mut spilled = self
            .spill
            .as_ref()
            .map(|spill| spill.slots.keys().copied())
            .into_iter()
            .flatten()
            .peekable();
        loop {
            let next_resident = resident.peek().map(|(offset, _)| **offset);
            let next_spilled = spilled.peek().copied();
            match (next_resident, next_spilled) {
                (None, None) => return Ok(()),
                // A re-loaded spilled extent is resident and newest;
                // its stale slot is skipped.
                (Some(r), Some(s)) if s == r => {
                    spilled.next();
                }
                (Some(r), Some(s)) if s < r => {
                    self.spill
                        .as_ref()
                        .expect("a spilled offset has a spill")
                        .read(s, &mut spill_buf)?;
                    f(s, &spill_buf)?;
                    spilled.next();
                }
                (Some(_), _) => {
                    let (&offset, extent) = resident.next().expect("peeked");
                    f(offset, &extent.data)?;
                }
                (None, Some(s)) => {
                    self.spill
                        .as_ref()
                        .expect("a spilled offset has a spill")
                        .read(s, &mut spill_buf)?;
                    f(s, &spill_buf)?;
                    spilled.next();
                }
            }
        }
    }

    /// Writes every altered extent through to `base` — resident and
    /// spilled alike, in offset order, one bounded buffer at a time. The
    /// cache keeps its state: the commit marks it committed only once
    /// the write-through is durably applied, so a failure anywhere
    /// leaves the buffered state intact.
    pub fn write_through(&self, base: &mut dyn Device) -> Result<()> {
        self.for_each_dirty(&mut |extent_offset, data| {
            let take = if extent_offset + EXTENT > base.len() {
                // The device may legitimately end mid-extent.
                (base.len().max(extent_offset) - extent_offset) as usize
            } else {
                EXTENT as usize
            };
            let take = take.min(data.len());
            if take > 0 {
                base.write_at(extent_offset, &data[..take])?;
            }
            Ok(())
        })
    }

    /// The commit landed: altered extents are now the image's own bytes,
    /// so they become clean — and stay resident, still serving reads —
    /// and the spill storage is released.
    pub fn mark_committed(&mut self) {
        self.join_offloads();
        for extent in self.resident.values_mut() {
            extent.dirty = false;
        }
        self.spill = None;
        self.prespilled.clear();
    }

    /// Rollback (P2): every altered extent is discarded, resident and
    /// spilled alike. Clean extents still mirror the untouched image and
    /// stay resident.
    pub fn discard_dirty(&mut self) {
        self.join_offloads();
        self.resident.retain(|_, extent| !extent.dirty);
        self.spill = None;
        self.prespilled.clear();
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
        let mut cache = SessionCache::with_bytes(DEFAULT_CACHE_BYTES);

        let mut first = [0u8; 512];
        cache.read_at(&mut base, 100, &mut first).expect("reads");
        assert_eq!(base.reads, 1, "the miss loads its whole extent");

        let mut second = [0u8; 512];
        cache
            .read_at(&mut base, 40_000, &mut second)
            .expect("reads");
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
        cache
            .read_at(&mut base, E as u64, &mut buf)
            .expect("reads B evicting A");
        cache.read_at(&mut base, 0, &mut buf).expect("re-reads A");
        assert_eq!(
            base.reads, 3,
            "the evicted clean extent re-reads from the base"
        );
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
        assert!(
            cache.modified(),
            "spilled alterations still count as modified"
        );

        let mut back = [0u8; 7];
        cache
            .read_at(&mut base, 10, &mut back)
            .expect("reads the altered bytes back");
        assert_eq!(
            &back, b"altered",
            "the spilled extent returns the altered data"
        );
        // The surrounding bytes of the seeded extent survived the trip.
        let mut around = [0u8; 4];
        cache
            .read_at(&mut base, 6, &mut around)
            .expect("reads around");
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
        assert_eq!(
            &base.bytes[2 * E + 9..2 * E + 15],
            b"second",
            "the resident one landed"
        );

        cache.mark_committed();
        assert!(!cache.modified(), "a committed cache reports no changes");
        // The committed extent stays resident and keeps serving.
        let reads_before = base.reads;
        let mut buf = [0u8; 6];
        cache
            .read_at(&mut base, 2 * E as u64 + 9, &mut buf)
            .expect("reads");
        assert_eq!(&buf, b"second");
        assert_eq!(
            base.reads, reads_before,
            "committed extents keep serving from cache"
        );
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
    fn background_offload_keeps_altered_state_whole() {
        let extents = 64usize;
        let mut base = CountingDevice::new(extents * E);
        let mut cache = SessionCache::with_bytes_offloading(8 * EXTENT);

        let mut expected: Vec<u8> = (0..extents * E).map(|n| (n % 251) as u8).collect();
        for n in 0..extents {
            let payload = [n as u8; 16];
            let offset = n * E + 7;
            cache
                .write_at(&mut base, offset as u64, &payload)
                .expect("writes under offload");
            expected[offset..offset + 16].copy_from_slice(&payload);
        }
        assert!(
            cache.modified(),
            "spilled and in-flight state counts as modified"
        );

        cache.join_offloads();
        cache.write_through(&mut base).expect("writes through");
        cache.mark_committed();
        assert!(!cache.modified());
        assert_eq!(
            base.bytes, expected,
            "every altered extent landed, none torn"
        );
    }

    #[test]
    fn rollback_discards_offloaded_state_too() {
        let extents = 32usize;
        let mut base = CountingDevice::new(extents * E);
        let mut cache = SessionCache::with_bytes_offloading(4 * EXTENT);

        for n in 0..extents {
            cache
                .write_at(&mut base, (n * E) as u64, &[0xEE; 32])
                .expect("writes under offload");
        }
        cache.discard_dirty();
        assert!(!cache.modified());
        assert_eq!(base.writes, 0, "nothing ever reaches the base");

        let mut back = [0u8; 32];
        cache.read_at(&mut base, 0, &mut back).expect("reads");
        let original: Vec<u8> = (0..32).map(|n| (n % 251) as u8).collect();
        assert_eq!(&back[..], &original[..], "the base bytes are back");
    }

    #[test]
    fn reads_past_the_end_are_refused() {
        let mut base = CountingDevice::new(E);
        let mut cache = SessionCache::with_bytes(DEFAULT_CACHE_BYTES);
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
        let mut cache = SessionCache::with_bytes(DEFAULT_CACHE_BYTES);

        cache
            .write_at(&mut base, (len - 8) as u64, b"tailtail")
            .expect("writes at the tail");
        let mut back = [0u8; 8];
        cache
            .read_at(&mut base, (len - 8) as u64, &mut back)
            .expect("reads");
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
