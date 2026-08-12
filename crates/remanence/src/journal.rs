// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The durable undo journal beneath the commit point (P9): before a
//! commit's first byte reaches the image, the bytes it will overwrite
//! are made durable in a sidecar journal, so an interruption at any
//! point leaves state the next open reconciles — back to wholly the old
//! image, or already wholly the committed new one — before the disk is
//! exposed. The journal is private transient state: its path is derived,
//! not user-owned, there is no cleanup verb, and it is gone again the
//! moment a commit completes or the next open reconciles it. Recording
//! and reconciling both stream through a bounded buffer (P27):
//! a journal the size of the write set never sits in memory.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::device::{AccessIntent, Capture, Device, MediumDevice, read_exact_at};
use crate::error::{Error, ErrorCategory, Result};

const MAGIC: [u8; 8] = *b"RMNUNDO1";
/// Magic, original length, new length, record count.
const HEADER_LEN: usize = 32;
/// The trailing FNV-1a seal over everything before it. A journal whose
/// seal does not verify was torn mid-write, which proves the commit
/// never crossed the durability boundary — the image was never touched.
const SEAL_LEN: usize = 8;
/// The bounded buffer journal streams move through (P27).
const CHUNK: usize = 64 * 1024;

const FNV_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// The recovery sidecar's path, derived from the image's own — private
/// transient state, not a user-owned path.
pub(crate) fn sidecar_path(image: &Path) -> PathBuf {
    let mut spelled = image.as_os_str().to_os_string();
    spelled.push(".remanence-recovery");
    PathBuf::from(spelled)
}

fn fnv1a_extend(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn le64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("8 bytes"))
}

fn contradiction(sidecar: &Path, reason: &str) -> Error {
    Error::io(format!(
        "the recovery journal '{}' is sealed but self-contradictory ({reason}); \
         refusing to reconcile from it",
        sidecar.display()
    ))
}

fn sidecar_read_error(sidecar: &Path, error: std::io::Error) -> Error {
    Error::io(format!(
        "cannot read the recovery journal '{}': {error}",
        sidecar.display()
    ))
}

/// Writes the sidecar incrementally, hashing everything written so the
/// seal can close the stream without the journal ever sitting in memory.
struct SealedWriter {
    file: File,
    hash: u64,
}

impl SealedWriter {
    fn create(sidecar: &Path) -> std::io::Result<Self> {
        Ok(Self {
            file: File::create(sidecar)?,
            hash: FNV_SEED,
        })
    }

    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.hash = fnv1a_extend(self.hash, bytes);
        self.file.write_all(bytes)
    }

    /// Appends the seal and makes the whole sidecar durable — the
    /// durability boundary (P9).
    fn seal_and_sync(mut self, sidecar: &Path) -> std::io::Result<()> {
        let seal = self.hash;
        self.file.write_all(&seal.to_le_bytes())?;
        self.file.sync_all()?;
        drop(self.file);
        sync_parent(sidecar);
        Ok(())
    }
}

/// Records the undo journal for one commit and makes it durable: for
/// every captured host extent, the image's current bytes there (growth
/// past the image's present end needs no bytes — truncation restores
/// it). Original bytes stream from the image into the sidecar through a
/// bounded buffer; nothing holds the write set in memory.
pub(crate) fn record(sidecar: &Path, device: &mut MediumDevice, capture: &Capture) -> Result<()> {
    let original_len = device.len();
    let record_error = |error: std::io::Error| {
        Error::io(format!(
            "cannot record the commit's recovery journal '{}': {error}",
            sidecar.display()
        ))
    };

    let mut count: u64 = 0;
    capture.for_each_dirty(&mut |offset, _| {
        if offset < original_len {
            count += 1;
        }
        Ok(())
    })?;

    let mut writer = SealedWriter::create(sidecar).map_err(record_error)?;
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(&MAGIC);
    header.extend_from_slice(&original_len.to_le_bytes());
    header.extend_from_slice(&capture.len().to_le_bytes());
    header.extend_from_slice(&count.to_le_bytes());
    writer.write(&header).map_err(record_error)?;

    let mut original = Vec::new();
    capture.for_each_dirty(&mut |offset, data| {
        if offset >= original_len {
            return Ok(());
        }
        let take = (original_len - offset).min(data.len() as u64) as usize;
        original.resize(take, 0);
        device.read_at(offset, &mut original)?;
        writer.write(&offset.to_le_bytes()).map_err(record_error)?;
        writer
            .write(&(take as u64).to_le_bytes())
            .map_err(record_error)?;
        writer.write(&original).map_err(record_error)?;
        Ok(())
    })?;

    writer.seal_and_sync(sidecar).map_err(record_error)
}

/// An armed journal's header facts, verified against the seal.
struct ArmedHeader {
    original_len: u64,
    new_len: u64,
    count: u64,
    body_len: u64,
}

/// Streams the sidecar once, hashing the body against the seal. `None`
/// means short, unsealed, or torn: the durability boundary was never
/// crossed, so the image was never touched. A journal that seals but
/// contradicts itself is refused (P6), never guessed at.
fn verify_sealed(file: &File, len: u64, sidecar: &Path) -> Result<Option<ArmedHeader>> {
    if len < (HEADER_LEN + SEAL_LEN) as u64 {
        return Ok(None);
    }
    let body_len = len - SEAL_LEN as u64;
    let mut header = [0u8; HEADER_LEN];
    let mut hash = FNV_SEED;
    let mut buf = vec![0u8; CHUNK];
    let mut at = 0u64;
    while at < body_len {
        let take = (body_len - at).min(CHUNK as u64) as usize;
        read_exact_at(file, at, &mut buf[..take])
            .map_err(|error| sidecar_read_error(sidecar, error))?;
        if at == 0 {
            header.copy_from_slice(&buf[..HEADER_LEN]);
        }
        hash = fnv1a_extend(hash, &buf[..take]);
        at += take as u64;
    }
    let mut seal = [0u8; SEAL_LEN];
    read_exact_at(file, body_len, &mut seal).map_err(|error| sidecar_read_error(sidecar, error))?;
    if hash != u64::from_le_bytes(seal) || header[..8] != MAGIC {
        return Ok(None);
    }

    // Sealed: the journal was written whole, so nothing past this point
    // is a torn write and nothing gets the benefit of the doubt (P6).
    let original_len = le64(&header, 8);
    let new_len = le64(&header, 16);
    if new_len < original_len {
        return Err(contradiction(sidecar, "the image shrank"));
    }
    Ok(Some(ArmedHeader {
        original_len,
        new_len,
        count: le64(&header, 24),
        body_len,
    }))
}

/// Walks an armed journal's records through a bounded buffer:
/// validation only when `apply` is absent, or writing each record's
/// original bytes back to the device. Validation runs to completion
/// before the first byte is restored (P6).
fn walk_records(
    file: &File,
    header: &ArmedHeader,
    sidecar: &Path,
    mut apply: Option<&mut MediumDevice>,
) -> Result<()> {
    let mut buf = vec![0u8; CHUNK];
    let mut at = HEADER_LEN as u64;
    for _ in 0..header.count {
        if header.body_len - at < 16 {
            return Err(contradiction(sidecar, "a record overruns the seal"));
        }
        let mut record_header = [0u8; 16];
        read_exact_at(file, at, &mut record_header)
            .map_err(|error| sidecar_read_error(sidecar, error))?;
        let offset = le64(&record_header, 0);
        let length = le64(&record_header, 8);
        at += 16;
        if length > header.body_len - at {
            return Err(contradiction(sidecar, "a record overruns the seal"));
        }
        match offset.checked_add(length) {
            Some(end) if end <= header.original_len => {}
            _ => return Err(contradiction(sidecar, "a record lies past the old image")),
        }
        if let Some(device) = apply.as_deref_mut() {
            let mut done = 0u64;
            while done < length {
                let take = (length - done).min(CHUNK as u64) as usize;
                read_exact_at(file, at + done, &mut buf[..take])
                    .map_err(|error| sidecar_read_error(sidecar, error))?;
                device.write_at(offset + done, &buf[..take])?;
                done += take as u64;
            }
        }
        at += length;
    }
    if at != header.body_len {
        return Err(contradiction(sidecar, "trailing bytes after the records"));
    }
    Ok(())
}

/// Retires the journal once the commit is wholly applied: invalidate
/// first — an empty sidecar is not armed — then remove. A failure to
/// invalidate matters (the caller falls back to undoing the apply); a
/// failure to remove after a successful invalidation does not, because
/// an unarmed leftover is discarded by the next open.
pub(crate) fn retire(sidecar: &Path) -> std::io::Result<()> {
    let file = File::create(sidecar)?;
    file.sync_all()?;
    drop(file);
    let _ = std::fs::remove_file(sidecar);
    sync_parent(sidecar);
    Ok(())
}

/// Directory-entry durability: a journal created or removed must survive
/// power loss, which on POSIX needs the parent directory fsynced.
/// Best-effort — a failure here narrows durability to what the
/// filesystem provides on its own, never correctness.
#[cfg(not(windows))]
fn sync_parent(path: &Path) {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if let Ok(handle) = File::open(parent) {
        let _ = handle.sync_all();
    }
}

/// NTFS journals its own name-space metadata; there is no directory
/// fsync to perform.
#[cfg(windows)]
fn sync_parent(_path: &Path) {}

/// Reconciles an interrupted commit from its sidecar, under the claim
/// `device` already holds: an armed journal rolls the image back to
/// wholly the old state; a torn one proves the image was never touched.
/// Either way the sidecar is gone when this returns.
pub(crate) fn reconcile(sidecar: &Path, device: &mut MediumDevice, image: &Path) -> Result<()> {
    let file = match File::open(sidecar) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(sidecar_read_error(sidecar, error)),
    };
    let len = file
        .metadata()
        .map_err(|error| sidecar_read_error(sidecar, error))?
        .len();
    match verify_sealed(&file, len, sidecar)? {
        None => {
            drop(file);
            let _ = retire(sidecar);
            Ok(())
        }
        Some(header) => {
            let held = device.len();
            if held < header.original_len || held > header.new_len {
                return Err(Error::io(format!(
                    "the recovery journal '{}' does not match '{}': the journal \
                     covers an image of {} to {} bytes, but the file holds {held}; \
                     refusing to reconcile from it",
                    sidecar.display(),
                    image.display(),
                    header.original_len,
                    header.new_len
                )));
            }
            walk_records(&file, &header, sidecar, None)?;
            walk_records(&file, &header, sidecar, Some(device))?;
            drop(file);
            device.truncate_and_sync(header.original_len)?;
            let _ = retire(sidecar);
            Ok(())
        }
    }
}

/// Reconciles any interrupted commit on `image` before it is opened or
/// composed into a chain. With no sidecar present this is a metadata
/// check and nothing more. Reconciling writes, so it takes a moment of
/// exclusive access first; losing that claim to another opener that has
/// already reconciled is benign, while losing it with the sidecar still
/// present is an immediate, named failure (P7 — never a hidden wait).
pub(crate) fn reconcile_at(image: &Path) -> Result<()> {
    let sidecar = sidecar_path(image);
    if !sidecar.exists() {
        return Ok(());
    }
    match MediumDevice::open(image, AccessIntent::Write) {
        Ok(mut exclusive) => reconcile(&sidecar, &mut exclusive, image),
        Err(error) => {
            if !sidecar.exists() {
                // Another opener held the claim and finished the
                // reconciliation while we asked.
                return Ok(());
            }
            let message = format!(
                "'{}' carries an interrupted commit that must be reconciled \
                 before the disk is exposed, and reconciling needs write \
                 access: {error}",
                image.display()
            );
            Err(if error.category() == ErrorCategory::Locked {
                Error::locked(message)
            } else {
                Error::io(message)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::write_all_at;

    /// Builds sealed journal bytes the way the streaming writer would —
    /// small, in memory, for exercising the streaming verifier.
    fn serialize(original_len: u64, new_len: u64, records: &[(u64, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&original_len.to_le_bytes());
        out.extend_from_slice(&new_len.to_le_bytes());
        out.extend_from_slice(&(records.len() as u64).to_le_bytes());
        for (offset, bytes) in records {
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(bytes);
        }
        let seal = fnv1a_extend(FNV_SEED, &out);
        out.extend_from_slice(&seal.to_le_bytes());
        out
    }

    fn sample() -> Vec<u8> {
        serialize(100, 4096, &[(0, vec![1, 2, 3, 4]), (96, vec![9, 9, 9, 9])])
    }

    /// Runs the streaming verifier and record walk over raw bytes:
    /// `Ok(true)` armed and structurally whole, `Ok(false)` not armed.
    fn classify(bytes: &[u8]) -> Result<bool> {
        let file = crate::cache::session_storage_file().expect("session storage");
        write_all_at(&file, 0, bytes).expect("journal bytes write");
        match verify_sealed(&file, bytes.len() as u64, Path::new("j"))? {
            None => Ok(false),
            Some(header) => {
                walk_records(&file, &header, Path::new("j"), None)?;
                Ok(true)
            }
        }
    }

    #[test]
    fn the_sidecar_path_derives_from_the_image_path() {
        assert_eq!(
            sidecar_path(Path::new("dir/disk.qcow2")),
            Path::new("dir/disk.qcow2.remanence-recovery")
        );
    }

    #[test]
    fn a_sealed_journal_verifies_and_walks() {
        assert!(
            classify(&sample()).expect("classifies"),
            "sealed means armed"
        );
    }

    #[test]
    fn a_torn_journal_is_not_armed() {
        let bytes = sample();
        // Truncated anywhere, or flipped anywhere: never armed.
        for cut in [0, 7, HEADER_LEN, bytes.len() - SEAL_LEN, bytes.len() - 1] {
            assert!(
                !classify(&bytes[..cut]).expect("classifies"),
                "a truncated journal must not be armed (cut at {cut})"
            );
        }
        for flip in [0, 9, HEADER_LEN + 3, bytes.len() - 4] {
            let mut torn = bytes.clone();
            torn[flip] ^= 0x40;
            assert!(
                !classify(&torn).expect("classifies"),
                "a corrupted journal must not be armed (flip at {flip})"
            );
        }
    }

    #[test]
    fn a_sealed_contradiction_is_refused_not_guessed_at() {
        // A structurally impossible journal wearing a valid seal: one
        // record promised, none present.
        let bytes = serialize(100, 4096, &[]).split_at(HEADER_LEN).0.to_vec();
        let mut body = bytes;
        body[24..32].copy_from_slice(&1u64.to_le_bytes());
        let seal = fnv1a_extend(FNV_SEED, &body);
        body.extend_from_slice(&seal.to_le_bytes());

        let error = classify(&body).expect_err("refuses");
        assert!(error.to_string().contains("self-contradictory"));
    }

    #[test]
    fn a_record_past_the_old_image_is_refused() {
        let bytes = serialize(100, 4096, &[(98, vec![7, 7, 7, 7])]);
        let error = classify(&bytes).expect_err("refuses");
        assert!(error.to_string().contains("past the old image"));
    }
}
