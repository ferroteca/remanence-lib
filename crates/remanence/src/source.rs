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

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::archive::EntrySource;
use crate::cache::{EXTENT, SessionCache};
use crate::device::{
    AccessIntent, AccessMode, Claim, Device, FileRangeDevice, MediumDevice, open_declared,
    read_exact_at,
};
use crate::error::{Error, Result};
use crate::handle;

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
#[derive(Debug)]
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

/// The session's image source: the P7 claim on the source file, held for
/// the session's lifetime, and the backing reads are served from.
#[derive(Debug)]
pub(crate) struct ImageSource {
    /// The claimed handle — writes denied to every other process from
    /// open until the session drops. For a plain image it is also the
    /// read backing.
    claim: Arc<File>,
    mode: AccessMode,
    backing: Backing,
    len: u64,
    cache: Arc<Mutex<SessionCache>>,
    prefetcher: Option<Prefetcher>,
}

impl ImageSource {
    fn new(claim: Arc<File>, mode: AccessMode, backing: Backing, len: u64, cache_bytes: u64) -> Self {
        let cache = Arc::new(Mutex::new(SessionCache::with_bytes(cache_bytes)));
        let (file, base) = match &backing {
            Backing::Claim { offset } => (Arc::clone(&claim), *offset),
            Backing::Spool { spool, offset } => (Arc::clone(spool), *offset),
        };
        let prefetcher = (len > 0).then(|| Prefetcher::spawn(Arc::clone(&cache), file, base, len));
        Self {
            claim,
            mode,
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
    pub(crate) fn over_claim(
        claim: Arc<File>,
        mode: AccessMode,
        len: u64,
        cache_bytes: u64,
    ) -> Self {
        Self::new(claim, mode, Backing::Claim { offset: 0 }, len, cache_bytes)
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn mode(&self) -> AccessMode {
        self.mode
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

    /// The image's leading bytes (up to `limit`), for bounded probes.
    pub fn prefix(&self, limit: usize) -> Result<Vec<u8>> {
        let take = (self.len).min(limit as u64) as usize;
        let mut bytes = vec![0u8; take];
        self.read_at(0, &mut bytes)?;
        Ok(bytes)
    }

    /// A [`MediumDevice`] over the same claim and the same backing this
    /// source reads.
    ///
    /// This is the bridge F43 turns on. The two planes a medium has — the
    /// raw bytes identification and the HDOS reader work over, and the
    /// presented disk the format adapters expose — are different layers
    /// (P13), but they are one artifact under one claim, and before this
    /// they were reached by opening the file twice. The claim is shared
    /// rather than reacquired, so the second plane costs no second claim
    /// and cannot conflict with the first.
    pub fn medium_device(&self, path: String) -> MediumDevice {
        let (backing, base) = match &self.backing {
            Backing::Claim { offset } => (Arc::clone(&self.claim), *offset),
            Backing::Spool { spool, offset } => (Arc::clone(spool), *offset),
        };
        MediumDevice::range(
            Arc::clone(&self.claim),
            backing,
            base,
            self.len,
            self.mode,
            path,
        )
    }

}

/// A read-only [`Device`] over an [`ImageSource`], for drivers that walk
/// the image (the session's qcow2 layer walk).
pub(crate) struct SourceDevice<'a>(pub &'a ImageSource);

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

/// The fully-resolved image source and provenance.
///
/// Both names are optional because a caller-opened handle need not have
/// one: a name is recovered from the handle for location alone, under an
/// identity check, and a nameless handle is served everywhere that does
/// not need a neighbourhood (`handle.rs`).
#[derive(Debug)]
pub(crate) struct ResolvedImage {
    pub source_path: Option<PathBuf>,
    pub image_path: Option<PathBuf>,
    pub source: ImageSource,
    pub archive_layers: Vec<ArchiveLayer>,
    /// Whose open the claim beneath this source is.
    pub claim: Claim,
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
pub(crate) fn resolve_entry(
    claim: Arc<File>,
    mode: AccessMode,
    claim_class: Claim,
    layer: ArchiveLayer,
    entry: EntrySource,
    cache_bytes: u64,
) -> ResolvedImage {
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
    ResolvedImage {
        source_path,
        image_path: Some(image_path),
        source: ImageSource::new(claim, mode, backing, len, cache_bytes),
        archive_layers: vec![layer],
        // The child rides the archive's claim, so it is the archive's
        // class the entry inherits rather than one of its own.
        claim: claim_class,
    }
}

/// Resolves the caller's own opened file to a streamed image source
/// under **their** claim (P7 as amended).
///
/// Nothing is opened here and no lock is taken: the handle arrived
/// claimed, the library asks it the one question it is entitled to ask —
/// may it write? — and recovers a name from it for location alone.
pub(crate) fn resolve_handle(file: File, cache_bytes: u64) -> Result<ResolvedImage> {
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
    Ok(ResolvedImage {
        source_path: name.clone(),
        image_path: name,
        source: ImageSource::new(
            Arc::new(file),
            mode,
            Backing::Claim { offset: 0 },
            len,
            cache_bytes,
        ),
        archive_layers: Vec::new(),
        claim: Claim::CallerOpened,
    })
}

/// Resolves `path` to a streamed image source under the caller's
/// declared intent (P7) and declared cache bound (P27).
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
pub(crate) fn resolve_image(
    path: &Path,
    intent: AccessIntent,
    cache_bytes: u64,
) -> Result<ResolvedImage> {
    let file = open_declared(path, intent)?;
    let len = file
        .metadata()
        .map_err(|error| Error::io(format!("failed to stat '{}': {error}", path.display())))?
        .len();
    Ok(ResolvedImage {
        source_path: Some(path.to_path_buf()),
        image_path: Some(path.to_path_buf()),
        source: ImageSource::new(
            Arc::new(file),
            intent.mode(),
            Backing::Claim { offset: 0 },
            len,
            cache_bytes,
        ),
        archive_layers: Vec::new(),
        claim: Claim::LibraryOpened,
    })
}
