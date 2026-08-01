// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The block device seam: the byte-addressed surface the disk stack
//! works over, the P7 claims — declared intent for the disk stack, the
//! discovery ladder for identification sessions — and the host-write
//! capture a durable commit stages into before the recovery journal is
//! armed (P9). The P2 commit-point buffer itself is the session cache
//! (`cache.rs`).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::error::{Error, Result};
use crate::journal::UndoRecord;

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

/// A session's access mode. On the disk stack this echoes the declared
/// [`AccessIntent`]; on an identification session it reports what the
/// P7 ladder obtained.
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
/// fast when deny-write cannot be obtained at all. The disk stack does
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

/// A raw image file opened where it lies, claimed per the caller's
/// declared intent (P7).
#[derive(Debug)]
pub(crate) struct FileDevice {
    file: File,
    len: u64,
    mode: AccessMode,
    path: String,
    capture: Option<Capture>,
}

/// A commit's staging area (P9): while a capture is active, every write
/// buffers here and the file itself is untouched, so the whole set of
/// host writes is known — and journaled — before the first one lands.
#[derive(Debug)]
struct Capture {
    overlay: Overlay,
    /// The device length as the buffered writes would leave it.
    len: u64,
}

impl FileDevice {
    pub fn open(path: &Path, intent: AccessIntent) -> Result<Self> {
        let file = open_declared(path, intent)?;
        let len = file
            .metadata()
            .map_err(|error| {
                Error::io(format!("failed to stat '{}': {error}", path.display()))
            })?
            .len();
        Ok(Self {
            file,
            len,
            mode: intent.mode(),
            path: path.display().to_string(),
            capture: None,
        })
    }

    pub fn mode(&self) -> AccessMode {
        self.mode
    }

    /// Starts capturing: until [`FileDevice::take_capture`], writes
    /// buffer in memory, reads compose over them, and the file is not
    /// touched.
    pub fn begin_capture(&mut self) {
        debug_assert!(self.capture.is_none(), "a capture is already active");
        self.capture = Some(Capture { overlay: Overlay::new(), len: self.len });
    }

    /// Ends the capture, returning the buffered blocks and the length
    /// the device would have once they are applied. Dropping the result
    /// discards the staged writes; the file was never touched.
    pub fn take_capture(&mut self) -> (BTreeMap<u64, Vec<u8>>, u64) {
        let capture = self.capture.take().expect("a capture is active");
        (capture.overlay.blocks, capture.len)
    }

    /// Writes captured blocks through to the file — each clamped to
    /// `new_len`, the length the capture recorded — and flushes.
    pub fn apply(&mut self, blocks: &BTreeMap<u64, Vec<u8>>, new_len: u64) -> Result<()> {
        debug_assert!(self.capture.is_none(), "cannot apply during a capture");
        for (&offset, block) in blocks {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            if take > 0 {
                self.write_at(offset, &block[..take])?;
            }
        }
        self.flush()
    }

    /// Restores the image to wholly the old state an undo journal
    /// records: every record's original bytes written back, the file
    /// truncated to its original length, and the result flushed.
    /// Idempotent, so an interruption partway means restoring again.
    pub fn restore(&mut self, records: &[UndoRecord], original_len: u64) -> Result<()> {
        debug_assert!(self.capture.is_none(), "cannot restore during a capture");
        debug_assert!(self.mode == AccessMode::ReadWrite, "restore needs write access");
        for record in records {
            write_all_at(&self.file, record.offset, &record.bytes)
                .map_err(|error| self.io_error("write to", error))?;
        }
        self.file
            .set_len(original_len)
            .map_err(|error| self.io_error("truncate", error))?;
        self.file
            .sync_all()
            .map_err(|error| self.io_error("flush", error))?;
        self.len = original_len;
        Ok(())
    }

    fn io_error(&self, action: &str, error: std::io::Error) -> Error {
        Error::io(format!("{action} '{}' failed: {error}", self.path))
    }
}

/// The real file beneath an active capture: reads clamp to the file's
/// true length and zero-fill past it, because captured growth has no
/// underlying bytes yet.
struct RawFile<'a> {
    file: &'a File,
    len: u64,
    path: &'a str,
}

impl Device for RawFile<'_> {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let take = if offset >= self.len {
            0
        } else {
            ((self.len - offset) as usize).min(buf.len())
        };
        if take > 0 {
            read_exact_at(self.file, offset, &mut buf[..take]).map_err(|error| {
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

impl Device for FileDevice {
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
        if let Some(capture) = &self.capture {
            let mut raw = RawFile { file: &self.file, len: self.len, path: &self.path };
            return capture.overlay.read_at(&mut raw, offset, buf);
        }
        read_exact_at(&self.file, offset, buf)
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
            let mut raw = RawFile { file: &self.file, len: self.len, path: &self.path };
            capture.overlay.write_at(&mut raw, offset, data)?;
            capture.len = capture.len.max(offset + data.len() as u64);
            return Ok(());
        }
        write_all_at(&self.file, offset, data)
            .map_err(|error| self.io_error("write to", error))?;
        self.len = self.len.max(offset + data.len() as u64);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.capture.is_some() {
            // Nothing has reached the file; there is nothing to flush.
            return Ok(());
        }
        self.file.sync_data().map_err(|error| self.io_error("flush", error))
    }
}

/// A read-only device over a byte range of an already-claimed file:
/// positioned reads, no resident copy. The session cache streams
/// identification reads through this (pledged P27).
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

const OVERLAY_BLOCK: u64 = 4096;

/// The staging area a durable commit's capture buffers host writes in
/// (P9): while a capture is active every host write lands here, so the
/// complete write set is known — and journaled — before the first byte
/// of the file changes. The session-lifetime P2 buffer is the session
/// cache (`cache.rs`); this overlay lives only inside one commit.
#[derive(Debug)]
pub(crate) struct Overlay {
    blocks: BTreeMap<u64, Vec<u8>>,
}

impl Overlay {
    pub fn new() -> Self {
        Self { blocks: BTreeMap::new() }
    }

    /// Reads `buf` from `base` with overlay blocks patched in.
    pub fn read_at(
        &self,
        base: &mut dyn Device,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        base.read_at(offset, buf)?;
        let end = offset + buf.len() as u64;
        let first_block = offset / OVERLAY_BLOCK * OVERLAY_BLOCK;
        for (&block_offset, block) in self.blocks.range(first_block..end) {
            let from = block_offset.max(offset);
            let to = (block_offset + OVERLAY_BLOCK).min(end);
            if from >= to {
                continue;
            }
            let src = &block[(from - block_offset) as usize..(to - block_offset) as usize];
            buf[(from - offset) as usize..(to - offset) as usize].copy_from_slice(src);
        }
        Ok(())
    }

    /// Buffers a write; nothing reaches `base` until `commit`.
    pub fn write_at(
        &mut self,
        base: &mut dyn Device,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        let end = offset + data.len() as u64;
        let mut block_offset = offset / OVERLAY_BLOCK * OVERLAY_BLOCK;
        while block_offset < end {
            if !self.blocks.contains_key(&block_offset) {
                // Seed the block from the base so a partial write keeps
                // the surrounding bytes.
                let mut block = vec![0u8; OVERLAY_BLOCK as usize];
                let base_len = base.len();
                if block_offset < base_len {
                    let take = ((base_len - block_offset) as usize)
                        .min(OVERLAY_BLOCK as usize);
                    base.read_at(block_offset, &mut block[..take])?;
                }
                self.blocks.insert(block_offset, block);
            }
            let block = self.blocks.get_mut(&block_offset).expect("just inserted");
            let from = block_offset.max(offset);
            let to = (block_offset + OVERLAY_BLOCK).min(end);
            block[(from - block_offset) as usize..(to - block_offset) as usize]
                .copy_from_slice(&data[(from - offset) as usize..(to - offset) as usize]);
            block_offset += OVERLAY_BLOCK;
        }
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

        let mut device = FileDevice::open(&path, AccessIntent::Write).expect("opens");
        device.begin_capture();
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

        let (blocks, new_len) = device.take_capture();
        assert_eq!(new_len, 8292);
        assert_eq!(device.len(), 8192, "discarding the capture restores the length");

        device.apply(&blocks, new_len).expect("applies");
        drop(device);
        let bytes = std::fs::read(&path).expect("reads back");
        assert_eq!(bytes.len(), 8292);
        assert_eq!(&bytes[4090..4098], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&bytes[8192..], &[9u8; 100]);
        std::fs::remove_file(&path).ok();
    }
}
