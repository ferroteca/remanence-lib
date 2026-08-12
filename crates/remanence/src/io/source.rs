// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Resolves what a medium is read from: a file named by path, or one
//! archive entry reached through the namespace its medium bears. The
//! source file is opened under the P7 claim, and nothing is loaded whole
//! (P27): a plain image and an entry stored uncompressed are
//! source-backed — reads stream from the claimed file through the
//! session cache — while a coded entry is session-backed, decoded once
//! by its catalog into private session storage and served from there
//! through the same cache.
//!
//! **The claim and the cache over it are two acts, not one.** A
//! [`ClaimedSource`] is the artifact claimed and nothing more: reads go
//! straight to the backing, which is what lets an artifact be recognized
//! before any bound is declared (F67). [`ClaimedSource::resolve`] is
//! where the load states its bound and the [`ImageSource`] — the session
//! cache and the predictive reader — comes into existence over the same
//! claim. Both answer [`Evidence`], so identification reads the same
//! bounded evidence on either side of that line.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::archive::EntrySource;
use crate::error::{Error, Result};
use crate::io::cache::{EXTENT, SessionCache};
use crate::io::device::{
    AccessIntent, AccessMode, Claim, Device, FileRangeDevice, MediumDevice, open_declared,
    read_exact_at,
};
use crate::io::handle;

/// How far the predictive reader runs ahead of a sequential access
/// pattern, in extents — part of the session's stated read-ahead (P27).
const PREFETCH_DEPTH: u64 = 8;

/// One archive wrapper that was unwrapped while resolving an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArchiveLayer {
    pub id: String,
    pub name: String,
    /// Where the archive sits, where its own handle could be named.
    pub path: Option<PathBuf>,
    pub entry_name: String,
    pub archive_size: Option<u64>,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: Option<u64>,
}

/// What the session reads image bytes from.
#[derive(Debug, Clone)]
enum Backing {
    /// The claimed source file itself, from `offset`: a plain image, or
    /// an uncompressed archive entry read in place. Source-backed
    /// (P27) — reads stream from the claim through the session cache.
    Claim { offset: u64 },
    /// Private session storage holding a decoded archive entry, from
    /// `offset`. Session-backed (P27) — served through the same cache.
    Spool { spool: Arc<File>, offset: u64 },
}

/// The predictive reader (P34): a worker that follows a sequential
/// access pattern and loads extents from the backing before they are
/// asked for. Speculation is silent and clean-only — a failed read
/// caches nothing and reports nothing, results are identical with the
/// worker absent — and the P7 claim makes the concurrent file reads
/// sound.
struct Prefetcher {
    demand: Sender<u64>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Prefetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Prefetcher")
    }
}

impl Prefetcher {
    fn spawn(cache: Arc<Mutex<SessionCache>>, file: Arc<File>, base: u64, len: u64) -> Self {
        let (demand, demands) = channel::<u64>();
        let handle = std::thread::spawn(move || {
            prefetch_loop(&demands, &cache, &file, base, len);
        });
        Self {
            demand,
            handle: Some(handle),
        }
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        let (orphan, _) = channel();
        drop(std::mem::replace(&mut self.demand, orphan));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn prefetch_loop(
    demands: &Receiver<u64>,
    cache: &Mutex<SessionCache>,
    file: &File,
    base: u64,
    len: u64,
) {
    let mut previous: Option<u64> = None;
    while let Ok(received) = demands.recv() {
        // Coalesce to the latest demand; the pattern, not the queue,
        // drives prediction.
        let mut extent = received;
        while let Ok(next) = demands.try_recv() {
            extent = next;
        }
        let sequential = previous == Some(extent.saturating_sub(EXTENT)) && extent != 0;
        previous = Some(extent);
        if !sequential {
            continue;
        }
        for ahead in 1..=PREFETCH_DEPTH {
            let target = extent + ahead * EXTENT;
            if target >= len {
                break;
            }
            {
                let guard = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if guard.is_resident(target) {
                    continue;
                }
            }
            let take = (len - target).min(EXTENT) as usize;
            let mut data = vec![0u8; EXTENT as usize];
            if read_exact_at(file, base + target, &mut data[..take]).is_err() {
                // Silent: the caller's own access owns any diagnostic.
                break;
            }
            cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert_prefetched(target, data);
        }
    }
}

/// The bounded evidence plane a probe reads (P27): a length, and reads
/// within it.
///
/// Two things answer it, and the difference between them is the whole
/// of F67. A [`ClaimedSource`] answers straight from the backing —
/// discovery holds the claim and builds no cache, so its reads go to
/// the file and nowhere else — and an [`ImageSource`] answers through
/// the session cache and the predictive reader a load declared a bound
/// for. Identification reads the same bounded evidence either way, as
/// it always has.
pub(crate) trait Evidence {
    fn len(&self) -> u64;

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()>;

    /// The image's leading bytes (up to `limit`), for bounded probes.
    fn prefix(&self, limit: usize) -> Result<Vec<u8>> {
        let take = self.len().min(limit as u64) as usize;
        let mut bytes = vec![0u8; take];
        self.read_at(0, &mut bytes)?;
        Ok(bytes)
    }
}

/// One artifact **claimed and nothing more**: the P7 claim, the backing
/// span it covers, and the provenance it was reached through — with no
/// session cache over it and nothing spilled.
///
/// This is what discovery holds. A cache bound is the *load's*
/// declaration (P27), so it enters at [`ClaimedSource::resolve`] and
/// nowhere earlier; a verb that creates no medium has nothing to bound,
/// and the probe's reads go straight to the backing. The claim itself
/// is taken once and moves into the load, so nothing is re-opened and
/// no window exists between the question and the load.
///
/// Both names are optional because a caller-opened handle need not have
/// one: a name is recovered from the handle for location alone, under an
/// identity check, and a nameless handle is served everywhere that does
/// not need a neighbourhood (`handle.rs`).
#[derive(Debug)]
pub(crate) struct ClaimedSource {
    pub source_path: Option<PathBuf>,
    pub image_path: Option<PathBuf>,
    claim: Arc<File>,
    mode: AccessMode,
    backing: Backing,
    len: u64,
    pub archive_layers: Vec<ArchiveLayer>,
    /// Whose open the claim beneath this source is.
    pub claim_class: Claim,
}

impl ClaimedSource {
    pub(crate) fn mode(&self) -> AccessMode {
        self.mode
    }

    /// The file reads land on and where the span starts in it: the
    /// claimed artifact itself, or the session spool a coded entry was
    /// decoded into before this source existed.
    fn backing_file(&self) -> (&Arc<File>, u64) {
        match &self.backing {
            Backing::Claim { offset } => (&self.claim, *offset),
            Backing::Spool { spool, offset } => (spool, *offset),
        }
    }

    /// A [`MediumDevice`] over this claim and this backing — the plane a
    /// format adapter is opened on.
    ///
    /// This is the bridge F43 turns on. The two planes a medium has — the
    /// raw bytes identification and the HDOS reader work over, and the
    /// presented disk the format adapters expose — are different layers
    /// (P13), but they are one artifact under one claim, and before this
    /// they were reached by opening the file twice. The claim is shared
    /// rather than reacquired, so the second plane costs no second claim
    /// and cannot conflict with the first. What it does *not* carry is a
    /// cache, which is why recognition happens before any bound is
    /// declared.
    pub(crate) fn medium_device(&self, path: String) -> MediumDevice {
        let (backing, base) = self.backing_file();
        MediumDevice::range(
            Arc::clone(&self.claim),
            Arc::clone(backing),
            base,
            self.len,
            self.mode,
            path,
        )
    }

    /// The load's streamed source over this claim, under the bound the
    /// load declared (P27). The claim moves; nothing is opened again.
    pub(crate) fn resolve(self, cache_bytes: u64) -> ImageSource {
        ImageSource::new(self.claim, self.backing, self.len, cache_bytes)
    }
}

impl Evidence for ClaimedSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.len {
            return Err(Error::io(format!(
                "read past end of image (offset {offset}, length {})",
                buf.len()
            )));
        }
        let (file, base) = self.backing_file();
        FileRangeDevice::new(file, base, self.len).read_at(offset, buf)
    }
}

/// The session's image source: the P7 claim on the source file, held for
/// the session's lifetime, and the backing reads are served from.
#[derive(Debug)]
pub(crate) struct ImageSource {
    /// The claimed handle — writes denied to every other process from
    /// open until the session drops. For a plain image it is also the
    /// read backing.
    claim: Arc<File>,
    backing: Backing,
    len: u64,
    cache: Arc<Mutex<SessionCache>>,
    prefetcher: Option<Prefetcher>,
}

impl ImageSource {
    fn new(claim: Arc<File>, backing: Backing, len: u64, cache_bytes: u64) -> Self {
        let cache = Arc::new(Mutex::new(SessionCache::with_bytes(cache_bytes)));
        let (file, base) = match &backing {
            Backing::Claim { offset } => (Arc::clone(&claim), *offset),
            Backing::Spool { spool, offset } => (Arc::clone(spool), *offset),
        };
        let prefetcher = (len > 0).then(|| Prefetcher::spawn(Arc::clone(&cache), file, base, len));
        Self {
            claim,
            backing,
            len,
            cache,
            prefetcher,
        }
    }

    /// A source over a claim already taken, reading the whole file.
    ///
    /// The archive medium's evidence plane is this: the artifact's bytes
    /// are readable under the very claim its catalog reads through, which
    /// is plumbing rather than a vantage anyone composes on — an
    /// archive's own vantage is its namespace.
    pub(crate) fn over_claim(claim: Arc<File>, len: u64, cache_bytes: u64) -> Self {
        Self::new(claim, Backing::Claim { offset: 0 }, len, cache_bytes)
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    /// Reads `buf` at `offset`, streaming from the backing — the
    /// claimed file, or the session spool — through the session cache.
    /// Each read also feeds the predictive reader, which follows a
    /// sequential pattern ahead of demand.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.len {
            return Err(Error::io(format!(
                "read past end of image (offset {offset}, length {})",
                buf.len()
            )));
        }
        let mut device = match &self.backing {
            Backing::Claim { offset: base } => FileRangeDevice::new(&self.claim, *base, self.len),
            Backing::Spool { spool, offset } => FileRangeDevice::new(spool, *offset, self.len),
        };
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .read_at(&mut device, offset, buf)?;
        if let (Some(prefetcher), false) = (&self.prefetcher, buf.is_empty()) {
            let last_extent = (offset + buf.len() as u64 - 1) / EXTENT * EXTENT;
            let _ = prefetcher.demand.send(last_extent);
        }
        Ok(())
    }

    /// The claimed handle itself, shared — for a medium that holds the
    /// claim past the source that carried it.
    pub(crate) fn claim_handle(&self) -> Arc<File> {
        Arc::clone(&self.claim)
    }
}

impl Evidence for ImageSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        ImageSource::read_at(self, offset, buf)
    }
}

/// A read-only [`Device`] over an evidence plane, for drivers that walk
/// the image (the session's qcow2 layer walk).
pub(crate) struct SourceDevice<'a>(pub &'a dyn Evidence);

impl Device for SourceDevice<'_> {
    fn len(&self) -> u64 {
        self.0.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.0.read_at(offset, buf)
    }

    fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(Error::read_only(
            "an identification session never writes".to_owned(),
        ))
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// A file taken out of another medium's namespace as a load's source —
/// one of `load_media`'s source shapes.
///
/// It is **free-standing**: it rides the claim of the medium it came
/// from (a stored entry holds the claimed archive, a coded one its
/// private session spool), so the namespace walk that named it ends
/// before the load begins and the borrow ends with it. Nothing is
/// opened and nothing runs twice — the load consumes what the walk
/// already resolved, exactly as a discovery is consumed.
pub struct FileSource {
    pub(crate) claim: Arc<File>,
    pub(crate) mode: AccessMode,
    pub(crate) claim_class: Claim,
    pub(crate) layer: ArchiveLayer,
    pub(crate) entry: EntrySource,
}

impl std::fmt::Debug for FileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSource")
            .field("entry", &self.layer.entry_name)
            .field("bytes", &self.size())
            .finish()
    }
}

impl FileSource {
    /// The name the namespace holds this file under.
    pub fn name(&self) -> &str {
        &self.layer.entry_name
    }

    /// The file's size in bytes, as the namespace claims it.
    pub fn size(&self) -> u64 {
        match &self.entry {
            EntrySource::InPlace { length, .. } | EntrySource::Spooled { length, .. } => *length,
        }
    }

    /// The claimed handle beneath this source, shared — the archive's
    /// own claim for a stored entry, the session spool for a coded one.
    pub(crate) fn claim_handle(&self) -> Arc<File> {
        match &self.entry {
            EntrySource::InPlace { .. } => Arc::clone(&self.claim),
            EntrySource::Spooled { spool, .. } => Arc::clone(spool),
        }
    }

    /// The file's bytes, whole — for a collection member whose stream
    /// is decoded once and not kept. A streamed load resolves through
    /// [`FileSource::resolve`] instead.
    pub(crate) fn read_whole(&self) -> Result<Vec<u8>> {
        let (file, offset, length) = match &self.entry {
            EntrySource::InPlace { offset, length } => (&self.claim, *offset, *length),
            EntrySource::Spooled {
                spool,
                offset,
                length,
            } => (spool, *offset, *length),
        };
        let mut bytes = vec![0u8; length as usize];
        read_exact_at(file, offset, &mut bytes).map_err(|error| {
            Error::io(format!(
                "failed to read '{}': {error}",
                self.layer.entry_name
            ))
        })?;
        Ok(bytes)
    }

    /// The source as the claim a load recognizes over, for a
    /// single-artifact load. The bound is the load's own declaration and
    /// enters at [`ClaimedSource::resolve`].
    pub(crate) fn claim(self) -> ClaimedSource {
        claim_entry(
            self.claim,
            self.mode,
            self.claim_class,
            self.layer,
            self.entry,
        )
    }
}

/// Resolves one archive entry — reached through the namespace its medium
/// bears — to a streamed image source under the archive's own claim.
///
/// **The backing is the entry's own, and the child holds it.** A stored
/// entry is source-backed: its bytes are a span of the claimed archive,
/// read in place, and the claim stays alive because this source holds a
/// handle on it. A coded entry is session-backed: its bytes were decoded
/// once into private session storage, and it is free-standing from that
/// moment (P27). Either way, ejecting the archive under a disk already
/// loaded from it takes nothing away.
pub(crate) fn claim_entry(
    claim: Arc<File>,
    mode: AccessMode,
    claim_class: Claim,
    layer: ArchiveLayer,
    entry: EntrySource,
) -> ClaimedSource {
    let source_path = layer.path.clone();
    let image_path = PathBuf::from(layer.entry_name.clone());
    let (backing, len) = match entry {
        EntrySource::InPlace { offset, length } => (Backing::Claim { offset }, length),
        EntrySource::Spooled {
            spool,
            offset,
            length,
        } => (Backing::Spool { spool, offset }, length),
    };
    ClaimedSource {
        source_path,
        image_path: Some(image_path),
        claim,
        mode,
        backing,
        len,
        archive_layers: vec![layer],
        // The child rides the archive's claim, so it is the archive's
        // class the entry inherits rather than one of its own.
        claim_class,
    }
}

/// Resolves the caller's own opened file to a streamed image source
/// under **their** claim (P7 as amended).
///
/// Nothing is opened here and no lock is taken: the handle arrived
/// claimed, the library asks it the one question it is entitled to ask —
/// may it write? — and recovers a name from it for location alone.
pub(crate) fn claim_handle(file: File) -> Result<ClaimedSource> {
    let mode = handle::afforded_access(&file);
    let name = handle::recovered_name(&file);
    let len = file
        .metadata()
        .map_err(|error| {
            Error::io(format!(
                "cannot read the size of the handed-over source: {error}"
            ))
        })?
        .len();
    Ok(ClaimedSource {
        source_path: name.clone(),
        image_path: name,
        claim: Arc::new(file),
        mode,
        backing: Backing::Claim { offset: 0 },
        len,
        archive_layers: Vec::new(),
        claim_class: Claim::CallerOpened,
    })
}

/// Claims `path` under the caller's declared intent (P7).
///
/// **A path names a file.** An artifact inside an archive is reached
/// through the archive's own namespace and loaded from the file view
/// there ([`resolve_entry`]), which is the journey every other medium
/// takes rather than a second syntax for one kind of source.
///
/// The intent is declared, never laddered. Before F43 the identification
/// path quietly degraded a failed write open to read-only while the disk
/// path refused by name; one surface cannot hold both rules, and in-force
/// P7 forbids obtaining a claim by silent fallback, so the refusal is
/// what survives.
pub(crate) fn claim_image(path: &Path, intent: AccessIntent) -> Result<ClaimedSource> {
    let file = open_declared(path, intent)?;
    let len = file
        .metadata()
        .map_err(|error| Error::io(format!("failed to stat '{}': {error}", path.display())))?
        .len();
    Ok(ClaimedSource {
        source_path: Some(path.to_path_buf()),
        image_path: Some(path.to_path_buf()),
        claim: Arc::new(file),
        mode: intent.mode(),
        backing: Backing::Claim { offset: 0 },
        len,
        archive_layers: Vec::new(),
        claim_class: Claim::LibraryOpened,
    })
}
