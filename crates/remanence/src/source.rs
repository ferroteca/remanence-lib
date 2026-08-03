// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Resolves a user-supplied path — a raw image, or `archive[/entry]`
//! for any grammar the archive catalog claims — to a streamed image
//! source plus the archive layers unwrapped along the way. The source
//! file is opened under the P7 claim, and nothing is loaded whole
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

use crate::archive::{
    ArchiveCatalog, EntrySource, normalize_entry_name, open_archive, split_archive_path,
};
use crate::cache::{EXTENT, SessionCache};
use crate::device::{AccessMode, Device, FileRangeDevice, open_locked, read_exact_at};
use crate::error::{Error, ErrorCategory, Result};

/// How far the predictive reader runs ahead of a sequential access
/// pattern, in extents — part of the session's stated read-ahead (P27).
const PREFETCH_DEPTH: u64 = 8;

/// One archive wrapper that was unwrapped while resolving an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArchiveLayer {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
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
    /// Private session storage holding a decoded archive entry.
    /// Session-backed (P27) — served through the same cache.
    Spool(Arc<File>),
}

/// The predictive reader (P27): a worker that follows a sequential
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
            Backing::Spool(spool) => (Arc::clone(spool), 0),
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
            Backing::Spool(spool) => FileRangeDevice::new(spool, 0, self.len),
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

    /// Materializes the whole image only when its length is within
    /// `cap` — the P27 rule that a whole layer may be held only when its
    /// format bounds it beneath the working set. Anything larger is
    /// refused, never loaded.
    pub fn bytes_bounded(&self, cap: u64, what: &str) -> Result<Vec<u8>> {
        if self.len > cap {
            return Err(Error::invalid_image(
                what,
                format!(
                    "image is {} bytes; {what} images are bounded at {cap} bytes",
                    self.len
                ),
            ));
        }
        let mut bytes = vec![0u8; self.len as usize];
        self.read_at(0, &mut bytes)?;
        Ok(bytes)
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
#[derive(Debug)]
pub(crate) struct ResolvedImage {
    pub source_path: PathBuf,
    pub image_path: PathBuf,
    pub source: ImageSource,
    pub archive_layers: Vec<ArchiveLayer>,
}

/// Returns the name of the archive's only file entry, or an error when
/// the archive is empty or ambiguous.
fn only_file_entry_name(catalog: &dyn ArchiveCatalog, archive_path: &Path) -> Result<String> {
    let format = catalog.descriptor().id;
    let mut found: Option<&str> = None;
    for entry in catalog.entries() {
        if entry.is_dir {
            continue;
        }
        if found.is_some() {
            return Err(Error::archive(
                format,
                format!(
                    "'{}' contains multiple files; specify an entry with archive/path",
                    archive_path.display()
                ),
            ));
        }
        found = Some(&entry.name);
    }

    found
        .map(str::to_owned)
        .ok_or_else(|| Error::archive(format, "archive contains no files"))
}

/// Resolves one archive entry to a streamed image source, through the
/// catalog its grammar declares.
fn resolve_archive_entry(
    archive_path: PathBuf,
    entry_path: Option<PathBuf>,
    cache_bytes: u64,
) -> Result<ResolvedImage> {
    let claimed = open_archive(&archive_path)?;
    let catalog = claimed.catalog.as_ref();
    let descriptor = catalog.descriptor();

    let entry_name = match &entry_path {
        Some(entry_path) => normalize_entry_name(entry_path),
        None => only_file_entry_name(catalog, &archive_path)?,
    };
    let index = catalog
        .entries()
        .iter()
        .position(|entry| entry.name == entry_name)
        .ok_or_else(|| {
            Error::categorized_archive(
                ErrorCategory::NotFound,
                descriptor.id,
                format!("entry '{entry_name}' not found"),
            )
        })?;
    let entry = &catalog.entries()[index];
    if entry.is_dir {
        return Err(Error::categorized_archive(
            ErrorCategory::IsDirectory,
            descriptor.id,
            format!("entry '{entry_name}' is a directory"),
        ));
    }

    let layer = ArchiveLayer {
        id: descriptor.id.to_owned(),
        name: descriptor.name.to_owned(),
        path: archive_path.clone(),
        entry_name: entry_name.clone(),
        archive_size: Some(catalog.archive_size()),
        compressed_size: entry.compressed_size,
        uncompressed_size: Some(entry.uncompressed_size),
    };

    let (backing, len) = match catalog.entry_source(index)? {
        EntrySource::InPlace { offset, length } => (Backing::Claim { offset }, length),
        EntrySource::Spooled { spool, length } => (Backing::Spool(spool), length),
    };

    Ok(ResolvedImage {
        source_path: archive_path,
        image_path: PathBuf::from(entry_name),
        source: ImageSource::new(claimed.file, claimed.mode, backing, len, cache_bytes),
        archive_layers: vec![layer],
    })
}

/// Resolves `path` to a streamed image source under the session's
/// declared cache bound, consulting the archive catalog first.
pub(crate) fn resolve_image(path: &Path, cache_bytes: u64) -> Result<ResolvedImage> {
    if let Some((archive_path, entry_path)) = split_archive_path(path) {
        return resolve_archive_entry(archive_path, entry_path, cache_bytes);
    }

    let (file, mode) = open_locked(path)?;
    let len = file
        .metadata()
        .map_err(|error| Error::io(format!("failed to stat '{}': {error}", path.display())))?
        .len();
    Ok(ResolvedImage {
        source_path: path.to_path_buf(),
        image_path: path.to_path_buf(),
        source: ImageSource::new(
            Arc::new(file),
            mode,
            Backing::Claim { offset: 0 },
            len,
            cache_bytes,
        ),
        archive_layers: Vec::new(),
    })
}
