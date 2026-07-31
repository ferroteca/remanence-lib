// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The block device seam: the byte-addressed surface the disk stack
//! works over, the P7 lock ladder, and the commit-point overlay (P2).

use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::error::{Error, Result};

/// How a file was claimed under the P7 ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// Write permission for us, writes denied to every other process.
    ReadWrite,
    /// Read-only for us (the file or media denies us write permission),
    /// writes still denied to every other process.
    ReadOnly,
}

/// A byte-addressed block device.
pub(crate) trait Device {
    fn len(&self) -> u64;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()>;
    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
}

/// Opens `path` under the P7 ladder: read/write with writes denied to
/// others (preferred); read-only with writes still denied to others when
/// our own write permission cannot be had; fail fast when deny-write
/// cannot be obtained at all.
pub(crate) fn open_locked(path: &Path) -> Result<(File, AccessMode)> {
    match open_claimed(path, true) {
        Ok(file) => Ok((file, AccessMode::ReadWrite)),
        Err(first) => match open_claimed(path, false) {
            Ok(file) => Ok((file, AccessMode::ReadOnly)),
            Err(_) if is_sharing_conflict(&first) => Err(Error::io(format!(
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
    // cooperating readers, which P7 tolerates).
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

/// A raw image file opened where it lies, claimed per P7.
#[derive(Debug)]
pub(crate) struct FileDevice {
    file: File,
    len: u64,
    mode: AccessMode,
    path: String,
}

impl FileDevice {
    pub fn open(path: &Path) -> Result<Self> {
        let (file, mode) = open_locked(path)?;
        let len = file
            .metadata()
            .map_err(|error| {
                Error::io(format!("failed to stat '{}': {error}", path.display()))
            })?
            .len();
        Ok(Self { file, len, mode, path: path.display().to_string() })
    }

    pub fn mode(&self) -> AccessMode {
        self.mode
    }

    fn io_error(&self, action: &str, error: std::io::Error) -> Error {
        Error::io(format!("{action} '{}' failed: {error}", self.path))
    }
}

#[cfg(windows)]
fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
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
fn write_all_at(file: &File, offset: u64, data: &[u8]) -> std::io::Result<()> {
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
fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

#[cfg(not(windows))]
fn write_all_at(file: &File, offset: u64, data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(data, offset)
}

impl Device for FileDevice {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.len {
            return Err(Error::io(format!(
                "read past end of '{}' (offset {offset}, length {})",
                self.path,
                buf.len()
            )));
        }
        read_exact_at(&self.file, offset, buf)
            .map_err(|error| self.io_error("read from", error))
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if self.mode == AccessMode::ReadOnly {
            return Err(Error::io(format!(
                "'{}' is open read-only; write denied",
                self.path
            )));
        }
        write_all_at(&self.file, offset, data)
            .map_err(|error| self.io_error("write to", error))?;
        self.len = self.len.max(offset + data.len() as u64);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.file.sync_data().map_err(|error| self.io_error("flush", error))
    }
}

/// A read-only device over an in-memory byte buffer (used by
/// identification, which already holds the session bytes).
pub(crate) struct SliceDevice<'a> {
    bytes: &'a [u8],
}

impl<'a> SliceDevice<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}

impl Device for SliceDevice<'_> {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let start = usize::try_from(offset)
            .map_err(|_| Error::io("read offset out of range".to_owned()))?;
        let end = start
            .checked_add(buf.len())
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| Error::io("read past end of buffer".to_owned()))?;
        buf.copy_from_slice(&self.bytes[start..end]);
        Ok(())
    }

    fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(Error::io("in-memory device is read-only".to_owned()))
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

const OVERLAY_BLOCK: u64 = 4096;

/// The commit point (P2): writes land in an in-memory overlay and reach
/// the underlying device only on `commit`; `rollback` discards them all.
#[derive(Debug)]
pub(crate) struct Overlay {
    blocks: std::collections::BTreeMap<u64, Vec<u8>>,
}

impl Overlay {
    pub fn new() -> Self {
        Self { blocks: std::collections::BTreeMap::new() }
    }

    pub fn modified(&self) -> bool {
        !self.blocks.is_empty()
    }

    pub fn rollback(&mut self) {
        self.blocks.clear();
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

    /// Writes every buffered block through to `base` and clears the
    /// overlay. The base is flushed before the overlay is dropped, so an
    /// interruption can only lose uncommitted work, never committed work.
    pub fn commit(&mut self, base: &mut dyn Device) -> Result<()> {
        for (&offset, block) in &self.blocks {
            let take = if offset + OVERLAY_BLOCK > base.len() {
                // The device may legitimately end mid-block.
                (base.len().max(offset) - offset) as usize
            } else {
                OVERLAY_BLOCK as usize
            };
            if take > 0 {
                base.write_at(offset, &block[..take])?;
            }
        }
        base.flush()?;
        self.blocks.clear();
        Ok(())
    }
}
