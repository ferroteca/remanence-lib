// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The block device seam: the byte-addressed surface the device stack
//! works over, the P7 claims — declared intent for the device stack, the
//! discovery ladder for identification sessions — and the host-write
//! capture a durable commit stages into before the recovery journal is
//! armed (P9). The P2 commit-point buffer itself is the session cache
//! (`cache.rs`), and the capture is another instance of it, so a
//! commit's transient staging is bounded too (P27).

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::Arc;

use crate::cache::SessionCache;
use crate::error::{Error, Result};

/// The caller's declared intent when opening a disk (P7): the session's
/// mode is declared at open, never discovered by fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessIntent {
    /// Read the disk. The open takes no write access for itself, denies
    /// writes to every other process, and keeps admitting other readers.
    Read,
    /// Read and write the disk. The claim excludes every other reader
    /// and writer for the session's whole life; an open that cannot
    /// secure it fails at the open, never by silent fallback.
    Write,
}

impl AccessIntent {
    /// The mode a disk opened with this intent reports — an echo of the
    /// declaration.
    pub(crate) fn mode(self) -> AccessMode {
        match self {
            Self::Read => AccessMode::ReadOnly,
            Self::Write => AccessMode::ReadWrite,
        }
    }
}

/// Whose open a medium's P7 claim is.
///
/// In-force P7 makes denying writes to every other process mandatory
/// **where the library opens**, and leaves the claim to the caller where
/// the caller opened. Which of the two a medium holds is a fact about the
/// session rather than about the artifact, so it travels on the medium's
/// [`Assurance`](crate::Assurance) beside the access it established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// The library opened the artifact and holds P7's denial itself —
    /// the discovery path, and every artifact reached by name.
    LibraryOpened,
    /// The caller opened the artifact and handed the handle over. What
    /// that handle affords is the whole of what this session has: the
    /// library checked it for one thing, honours it exactly, and takes no
    /// lock of its own.
    CallerOpened,
}

impl Claim {
    /// The stable cross-language spelling of this claim's class.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LibraryOpened => "library-opened",
            Self::CallerOpened => "caller-opened",
        }
    }
}

impl std::fmt::Display for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A session's access mode. On the device stack this echoes the declared
/// [`AccessIntent`]; on an identification session it reports what the
/// P7 ladder obtained; on a caller-opened medium it reports what that
/// caller's handle affords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// Write permission for us. For a disk session the claim is
    /// exclusive — no other reader or writer for its whole life; for an
    /// identification session writes are denied to others and readers
    /// stay admitted.
    ReadWrite,
    /// Read-only for us, writes denied to every other process, other
    /// readers admitted.
    ReadOnly,
}

/// A byte-addressed block device.
pub(crate) trait Device {
    fn len(&self) -> u64;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()>;
    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
}

/// Opens `path` under the identification session's P7 ladder: read/write
/// with writes denied to others (preferred); read-only with writes still
/// denied to others when our own write permission cannot be had; fail
/// fast when deny-write cannot be obtained at all. The device stack does
/// not ladder — it opens per the caller's declared intent
/// ([`open_declared`]).
pub(crate) fn open_locked(path: &Path) -> Result<(File, AccessMode)> {
    match open_claimed(path, true) {
        Ok(file) => Ok((file, AccessMode::ReadWrite)),
        Err(first) => match open_claimed(path, false) {
            Ok(file) => Ok((file, AccessMode::ReadOnly)),
            Err(_) if is_sharing_conflict(&first) => Err(Error::locked(format!(
                "cannot lock '{}': another process holds write access",
                path.display()
            ))),
            Err(second) => Err(Error::io(format!(
                "failed to open '{}': {second}",
                path.display()
            ))),
        },
    }
}

/// Opens `path` per the caller's declared intent (P7): a `Write` open
/// takes read/write access and excludes every other reader and writer
/// for its whole life; a `Read` open takes read access only, denies
/// writes to every other process, and keeps admitting other readers.
/// Either open fails immediately, naming the reason, when its claim
/// cannot be secured — never by falling back to a weaker mode.
pub(crate) fn open_declared(path: &Path, intent: AccessIntent) -> Result<File> {
    open_exact(path, intent).map_err(|error| {
        let verb = match intent {
            AccessIntent::Read => "reading",
            AccessIntent::Write => "writing",
        };
        if is_sharing_conflict(&error) {
            let holder = match intent {
                AccessIntent::Read => "another process holds write access",
                AccessIntent::Write => "another process has the file open",
            };
            Error::locked(format!(
                "cannot claim '{}' for {verb}: {holder}",
                path.display()
            ))
        } else {
            Error::io(format!(
                "cannot open '{}' for {verb}: {error}",
                path.display()
            ))
        }
    })
}

/// Creates `path` and claims it exclusively for this process (P7).
///
/// The file must not already exist: creation is the claim, and it is the
/// same call, so nothing can appear between deciding to write and owning
/// the path. An adapter producing a new artifact writes through this and
/// nothing else, so the artifact is never observable half-written by
/// anyone else while it is being built.
pub(crate) fn create_claimed(path: &Path) -> Result<File> {
    create_exclusive(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => Error::io(format!(
            "cannot create '{}': something is already there, and this library \
             never overwrites a destination it did not create",
            path.display()
        )),
        _ if is_sharing_conflict(&error) => Error::locked(format!(
            "cannot claim '{}': another process has the file open",
            path.display()
        )),
        _ => Error::io(format!("cannot create '{}': {error}", path.display())),
    })
}

/// Puts `bytes` at `path` as a whole new artifact, or leaves the
/// destination as it found it.
///
/// An existing destination is a named refusal rather than an overwrite,
/// and the bytes are on the medium before the name exists — built beside
/// the destination and renamed into place — so what `path` names is
/// either the whole artifact or nothing (P6, P7, P9).
pub(crate) fn place_new_artifact(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.try_exists().unwrap_or(false) {
        return Err(Error::io(format!(
            "cannot write '{}': something is already there, and a destination this \
             library did not create is never overwritten",
            path.display()
        )));
    }
    let staging = staging_path(path);
    let file = create_claimed(&staging)?;
    let built = write_all_at(&file, 0, bytes)
        .map_err(|error| Error::io(format!("cannot write '{}': {error}", staging.display())))
        .and_then(|()| {
            file.sync_all().map_err(|error| {
                Error::io(format!(
                    "cannot commit '{}' to storage: {error}",
                    staging.display()
                ))
            })
        });
    drop(file);
    if let Err(error) = built {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    std::fs::rename(&staging, path).map_err(|error| {
        let _ = std::fs::remove_file(&staging);
        Error::io(format!(
            "cannot put the written artifact in place at '{}': {error}",
            path.display()
        ))
    })
}

/// Where an artifact is built: beside its destination, so moving it into
/// place is a rename within one filesystem rather than a copy.
fn staging_path(destination: &Path) -> std::path::PathBuf {
    let name = destination.file_name().map_or_else(
        || "artifact".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    destination.with_file_name(format!(".{name}.part-{}-{nonce}", std::process::id()))
}

#[cfg(windows)]
fn create_exclusive(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // Share mode 0: while this file is being written, no other process
    // reads it or writes it.
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(path)
}

#[cfg(not(windows))]
fn create_exclusive(path: &Path) -> std::io::Result<File> {
    use std::os::fd::AsRawFd;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

fn is_sharing_conflict(error: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32) / ERROR_LOCK_VIOLATION (33) on Windows;
    // EWOULDBLOCK from a contended advisory lock elsewhere.
    match error.raw_os_error() {
        #[cfg(windows)]
        Some(code) => code == 32 || code == 33,
        #[cfg(not(windows))]
        Some(code) => code == 11 || code == 35,
        None => false,
    }
}

#[cfg(windows)]
fn open_claimed(path: &Path, writable: bool) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // Share mode FILE_SHARE_READ alone: other processes may read, any
    // other open for writing is refused by the kernel, and this open is
    // refused if a writer already holds the file.
    const FILE_SHARE_READ: u32 = 0x1;
    OpenOptions::new()
        .read(true)
        .write(writable)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn open_claimed(path: &Path, writable: bool) -> std::io::Result<File> {
    use std::os::fd::AsRawFd;
    // POSIX has no sharing modes; the exclusive advisory lock is the
    // deny-write claim, asserted as protocol (it also holds off
    // cooperating readers, which the identification session tolerates).
    let file = OpenOptions::new().read(true).write(writable).open(path)?;
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

#[cfg(windows)]
fn open_exact(path: &Path, intent: AccessIntent) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // Read intent shares reads: other readers stay admitted, any open
    // for writing is refused by the kernel, and this open is refused
    // while a writer holds the file. Write intent shares nothing — the
    // session admits no observers.
    const FILE_SHARE_READ: u32 = 0x1;
    let (writable, share_mode) = match intent {
        AccessIntent::Read => (false, FILE_SHARE_READ),
        AccessIntent::Write => (true, 0),
    };
    OpenOptions::new()
        .read(true)
        .write(writable)
        .share_mode(share_mode)
        .open(path)
}

#[cfg(not(windows))]
fn open_exact(path: &Path, intent: AccessIntent) -> std::io::Result<File> {
    use std::os::fd::AsRawFd;
    // POSIX has no sharing modes; the advisory lock is the claim,
    // asserted as protocol — shared for a read open (other readers
    // admitted, writers held off), exclusive for a writable session
    // (no observers).
    let writable = intent == AccessIntent::Write;
    let file = OpenOptions::new().read(true).write(writable).open(path)?;
    const LOCK_SH: i32 = 1;
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    let operation = if writable { LOCK_EX } else { LOCK_SH } | LOCK_NB;
    if unsafe { flock(file.as_raw_fd(), operation) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

/// One medium's bytes, claimed per the caller's declared intent (P7),
/// addressed as a range so the medium need not be a whole file.
///
/// A plain image is the whole of its claimed file: `backing` and `claim`
/// are the same handle and `base` is zero. An image stored inside an
/// archive is a range instead — read in place at an offset within the
/// claimed archive when it is stored uncompressed, or within the spool it
/// was decoded into when it is not (P27) — and then `claim` is the
/// archive, held for the medium's whole life, while `backing` is wherever
/// the bytes actually are. That distinction is the whole reason this is a
/// range device: the image adapters open through it, and before F43 they
/// took a whole claimed file, which is why an archived image could be
/// identified but never inspected.
#[derive(Debug)]
pub(crate) struct MediumDevice {
    /// The P7 claim held for the medium's life. Kept even when it is not
    /// the read backing, because dropping it would release the claim.
    _claim: Arc<File>,
    /// Where the bytes are, which is the claim itself unless the medium
    /// was decoded into private session storage.
    backing: Arc<File>,
    /// Where the medium starts inside `backing`.
    base: u64,
    len: u64,
    mode: AccessMode,
    path: String,
    capture: Option<Capture>,
}

/// A commit's staging area (P9): while a capture is active, every host
/// write buffers here — in memory within the bound, spilled to private
/// session storage beyond it (P27) — and the file itself is untouched,
/// so the whole set of host writes is known, and journaled, before the
/// first one lands.
#[derive(Debug)]
pub(crate) struct Capture {
    cache: SessionCache,
    /// The device length as the buffered writes would leave it.
    len: u64,
}

impl Capture {
    /// The device length once the captured writes are applied.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the capture holds no writes at all.
    pub fn is_clean(&self) -> bool {
        !self.cache.modified()
    }

    /// Streams every captured extent in offset order through a bounded
    /// buffer.
    pub fn for_each_dirty(
        &self,
        f: &mut dyn FnMut(u64, &[u8]) -> Result<()>,
    ) -> Result<()> {
        self.cache.for_each_dirty(f)
    }
}

impl MediumDevice {
    /// Claims `path` whole, per the caller's declared intent (P7).
    pub fn open(path: &Path, intent: AccessIntent) -> Result<Self> {
        let file = open_declared(path, intent)?;
        let len = file
            .metadata()
            .map_err(|error| {
                Error::io(format!("failed to stat '{}': {error}", path.display()))
            })?
            .len();
        let claim = Arc::new(file);
        Ok(Self {
            _claim: Arc::clone(&claim),
            backing: claim,
            base: 0,
            len,
            mode: intent.mode(),
            path: path.display().to_string(),
            capture: None,
        })
    }

    /// A medium occupying `len` bytes at `base` inside `backing`, under a
    /// claim already held on `claim`.
    ///
    /// The claim is not reacquired: the caller already holds it, and this
    /// shares it, which is the whole point — one medium is one claim (P7),
    /// however many planes read it.
    pub fn range(
        claim: Arc<File>,
        backing: Arc<File>,
        base: u64,
        len: u64,
        mode: AccessMode,
        path: String,
    ) -> Self {
        Self {
            _claim: claim,
            backing,
            base,
            len,
            mode,
            path,
            capture: None,
        }
    }

    /// Whether this medium is the whole of its claimed file, and so can
    /// be grown or truncated in place.
    fn is_whole_file(&self) -> bool {
        self.base == 0 && Arc::ptr_eq(&self._claim, &self.backing)
    }

    /// Starts capturing under the session's declared cache bound:
    /// until [`MediumDevice::take_capture`], writes buffer in the
    /// capture's cache — spilling to private session storage past the
    /// bound — reads compose over them, and the file is not touched.
    pub fn begin_capture(&mut self, cache_bytes: u64) {
        debug_assert!(self.capture.is_none(), "a capture is already active");
        self.capture = Some(Capture {
            cache: SessionCache::with_bytes(cache_bytes),
            len: self.len,
        });
    }

    /// Ends the capture, returning the staged writes and the length the
    /// device would have once they are applied. Dropping the result
    /// discards the staged writes; the file was never touched.
    pub fn take_capture(&mut self) -> Capture {
        self.capture.take().expect("a capture is active")
    }

    /// Writes a capture's extents through to the file — streamed in
    /// offset order, each clamped to the length the capture recorded —
    /// and flushes.
    pub fn apply(&mut self, capture: &Capture) -> Result<()> {
        debug_assert!(self.capture.is_none(), "cannot apply during a capture");
        let new_len = capture.len();
        capture.for_each_dirty(&mut |offset, data| {
            let take = (new_len.saturating_sub(offset)).min(data.len() as u64) as usize;
            if take > 0 {
                self.write_at(offset, &data[..take])?;
            }
            Ok(())
        })?;
        self.flush()
    }

    /// Truncates the file to `len` and flushes — reconciliation's final
    /// step after an undo journal's records are written back.
    pub fn truncate_and_sync(&mut self, len: u64) -> Result<()> {
        debug_assert!(self.capture.is_none(), "cannot truncate during a capture");
        debug_assert!(self.mode == AccessMode::ReadWrite, "truncation needs write access");
        debug_assert!(self.is_whole_file(), "only a whole-file medium is truncated");
        self.backing
            .set_len(len)
            .map_err(|error| self.io_error("truncate", error))?;
        self.backing
            .sync_all()
            .map_err(|error| self.io_error("flush", error))?;
        self.len = len;
        Ok(())
    }

    fn io_error(&self, action: &str, error: std::io::Error) -> Error {
        Error::io(format!("{action} '{}' failed: {error}", self.path))
    }
}

/// The real file beneath an active capture: it reports the capture's
/// grown length, while reads clamp to the file's true length and
/// zero-fill past it, because captured growth has no underlying bytes
/// yet.
struct RawFile<'a> {
    file: &'a File,
    /// Where the medium starts inside the file.
    base: u64,
    /// The captured (possibly grown) device length.
    reported: u64,
    /// The medium's true length, bounding what can actually be read.
    real: u64,
    path: &'a str,
}

impl Device for RawFile<'_> {
    fn len(&self) -> u64 {
        self.reported
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let take = if offset >= self.real {
            0
        } else {
            ((self.real - offset) as usize).min(buf.len())
        };
        if take > 0 {
            read_exact_at(self.file, self.base + offset, &mut buf[..take]).map_err(|error| {
                Error::io(format!("read from '{}' failed: {error}", self.path))
            })?;
        }
        buf[take..].fill(0);
        Ok(())
    }

    fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(Error::io(
            "the file beneath a capture is never written directly".to_owned(),
        ))
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < buf.len() {
        let read = file.seek_read(&mut buf[done..], offset + done as u64)?;
        if read == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }
        done += read;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn write_all_at(file: &File, offset: u64, data: &[u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < data.len() {
        let wrote = file.seek_write(&data[done..], offset + done as u64)?;
        if wrote == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WriteZero));
        }
        done += wrote;
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

#[cfg(not(windows))]
pub(crate) fn write_all_at(file: &File, offset: u64, data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(data, offset)
}

/// Where a decompressor pulls its coded bytes from, one at a time — the
/// mechanism both the DEFLATE and the LZMA decoders read through, so
/// neither owns a file-reading path of its own.
pub(crate) trait ByteSource {
    /// The next coded byte, or `None` at end of input (or on a source
    /// failure the concrete source reports separately).
    fn next_byte(&mut self) -> Option<u8>;
}

/// Coded bytes pulled from a byte range of a file through a bounded
/// chunk (P27): the compressed stream is never resident whole.
pub(crate) struct FileByteSource<'a> {
    file: &'a File,
    next: u64,
    end: u64,
    buf: Vec<u8>,
    buf_pos: usize,
    failed: bool,
}

impl<'a> FileByteSource<'a> {
    pub fn new(file: &'a File, offset: u64, length: u64) -> Self {
        Self {
            file,
            next: offset,
            end: offset + length,
            buf: Vec::new(),
            buf_pos: 0,
            failed: false,
        }
    }

    /// Whether a read of the underlying file failed — distinguishing an
    /// I/O failure from a stream that merely ran out.
    pub fn failed(&self) -> bool {
        self.failed
    }
}

impl ByteSource for FileByteSource<'_> {
    fn next_byte(&mut self) -> Option<u8> {
        if self.buf_pos == self.buf.len() {
            if self.failed || self.next == self.end {
                return None;
            }
            let take = (self.end - self.next).min(4096) as usize;
            self.buf.resize(take, 0);
            self.buf_pos = 0;
            if read_exact_at(self.file, self.next, &mut self.buf).is_err() {
                self.failed = true;
                self.buf.clear();
                return None;
            }
            self.next += take as u64;
        }
        let byte = self.buf[self.buf_pos];
        self.buf_pos += 1;
        Some(byte)
    }
}

/// Coded bytes pulled from memory, for streams a format already bounds.
pub(crate) struct SliceByteSource<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceByteSource<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl ByteSource for SliceByteSource<'_> {
    fn next_byte(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }
}

impl Device for MediumDevice {
    fn len(&self) -> u64 {
        self.capture.as_ref().map_or(self.len, |capture| capture.len)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.len() {
            return Err(Error::io(format!(
                "read past end of '{}' (offset {offset}, length {})",
                self.path,
                buf.len()
            )));
        }
        if let Some(capture) = &mut self.capture {
            let mut raw = RawFile {
                file: &self.backing,
                base: self.base,
                reported: capture.len,
                real: self.len,
                path: &self.path,
            };
            return capture.cache.read_at(&mut raw, offset, buf);
        }
        read_exact_at(&self.backing, self.base + offset, buf)
            .map_err(|error| self.io_error("read from", error))
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if self.mode == AccessMode::ReadOnly {
            return Err(Error::read_only(format!(
                "'{}' is open read-only; write denied",
                self.path
            )));
        }
        if let Some(capture) = &mut self.capture {
            let mut raw = RawFile {
                file: &self.backing,
                base: self.base,
                reported: capture.len,
                real: self.len,
                path: &self.path,
            };
            capture.cache.write_at(&mut raw, offset, data)?;
            capture.len = capture.len.max(offset + data.len() as u64);
            return Ok(());
        }
        write_all_at(&self.backing, self.base + offset, data)
            .map_err(|error| self.io_error("write to", error))?;
        self.len = self.len.max(offset + data.len() as u64);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.capture.is_some() {
            // Nothing has reached the file; there is nothing to flush.
            return Ok(());
        }
        self.backing.sync_data().map_err(|error| self.io_error("flush", error))
    }
}

/// A read-only device over a byte range of an already-claimed file:
/// positioned reads, no resident copy. The session cache streams
/// identification reads through this (P27).
pub(crate) struct FileRangeDevice<'a> {
    file: &'a File,
    base: u64,
    len: u64,
}

impl<'a> FileRangeDevice<'a> {
    pub fn new(file: &'a File, base: u64, len: u64) -> Self {
        Self { file, base, len }
    }
}

impl Device for FileRangeDevice<'_> {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.len {
            return Err(Error::io(format!(
                "read past end of image (offset {offset}, length {})",
                buf.len()
            )));
        }
        read_exact_at(self.file, self.base + offset, buf)
            .map_err(|error| Error::io(format!("read from claimed source failed: {error}")))
    }

    fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(Error::read_only(
            "a claimed identification source is read-only".to_owned(),
        ))
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_buffers_host_writes_until_applied() {
        let path = std::env::temp_dir().join(format!(
            "remanence-capture-{}.img",
            std::process::id()
        ));
        std::fs::write(&path, vec![0xAAu8; 8192]).expect("image writes");

        let mut device = MediumDevice::open(&path, AccessIntent::Write).expect("opens");
        device.begin_capture(crate::cache::DEFAULT_CACHE_BYTES);
        device.write_at(4090, &[1, 2, 3, 4, 5, 6, 7, 8]).expect("buffers");
        device.write_at(8192, &[9; 100]).expect("buffers growth");
        assert_eq!(device.len(), 8292, "the capture reports the grown length");

        // Reads compose the buffered writes over the untouched file.
        let mut back = [0u8; 12];
        device.read_at(4088, &mut back).expect("reads");
        assert_eq!(back, [0xAA, 0xAA, 1, 2, 3, 4, 5, 6, 7, 8, 0xAA, 0xAA]);
        assert_eq!(
            std::fs::metadata(&path).expect("stat").len(),
            8192,
            "the file is untouched while the capture is active"
        );

        let capture = device.take_capture();
        assert_eq!(capture.len(), 8292);
        assert!(!capture.is_clean());
        assert_eq!(device.len(), 8192, "discarding the capture restores the length");

        device.apply(&capture).expect("applies");
        drop(device);
        let bytes = std::fs::read(&path).expect("reads back");
        assert_eq!(bytes.len(), 8292);
        assert_eq!(&bytes[4090..4098], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&bytes[8192..], &[9u8; 100]);
        std::fs::remove_file(&path).ok();
    }
}
