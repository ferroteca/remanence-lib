// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The durable undo journal beneath the commit point (P9): before a
//! commit's first byte reaches the image, the bytes it will overwrite
//! are made durable in a sidecar journal, so an interruption at any
//! point leaves state the next open reconciles — back to wholly the old
//! image, or already wholly the committed new one — before the disk is
//! exposed. The journal is private transient state: its path is derived,
//! not user-owned, there is no cleanup verb, and it is gone again the
//! moment a commit completes or the next open reconciles it.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::device::{AccessIntent, Device, FileDevice};
use crate::error::{Error, ErrorCategory, Result};

const MAGIC: [u8; 8] = *b"RMNUNDO1";
/// Magic, original length, new length, record count.
const HEADER_LEN: usize = 32;
/// The trailing FNV-1a seal over everything before it. A journal whose
/// seal does not verify was torn mid-write, which proves the commit
/// never crossed the durability boundary — the image was never touched.
const SEAL_LEN: usize = 8;

/// The recovery sidecar's path, derived from the image's own — private
/// transient state, not a user-owned path.
pub(crate) fn sidecar_path(image: &Path) -> PathBuf {
    let mut spelled = image.as_os_str().to_os_string();
    spelled.push(".remanence-recovery");
    PathBuf::from(spelled)
}

/// One undo record: the image's original bytes at `offset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UndoRecord {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// An armed journal. Reconciling means writing every record's bytes
/// back and truncating the image to `original_len` — idempotent, so an
/// interruption during recovery only means recovering again.
#[derive(Debug)]
pub(crate) struct UndoJournal {
    pub original_len: u64,
    pub new_len: u64,
    pub records: Vec<UndoRecord>,
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn le64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("8 bytes"))
}

fn serialize(journal: &UndoJournal) -> Vec<u8> {
    let payload: usize = journal
        .records
        .iter()
        .map(|record| 16 + record.bytes.len())
        .sum();
    let mut out = Vec::with_capacity(HEADER_LEN + payload + SEAL_LEN);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&journal.original_len.to_le_bytes());
    out.extend_from_slice(&journal.new_len.to_le_bytes());
    out.extend_from_slice(&(journal.records.len() as u64).to_le_bytes());
    for record in &journal.records {
        out.extend_from_slice(&record.offset.to_le_bytes());
        out.extend_from_slice(&(record.bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(&record.bytes);
    }
    let seal = fnv1a(&out);
    out.extend_from_slice(&seal.to_le_bytes());
    out
}

#[derive(Debug)]
enum Parsed {
    /// The seal verifies: the interrupted commit may have reached the
    /// image, and the journal governs what the image rolls back to.
    Armed(UndoJournal),
    /// Short, unsealed, or torn: the durability boundary was never
    /// crossed, so the image was never touched.
    NotArmed,
}

fn contradiction(sidecar: &Path, reason: &str) -> Error {
    Error::io(format!(
        "the recovery journal '{}' is sealed but self-contradictory ({reason}); \
         refusing to reconcile from it",
        sidecar.display()
    ))
}

fn parse(bytes: &[u8], sidecar: &Path) -> Result<Parsed> {
    if bytes.len() < HEADER_LEN + SEAL_LEN || bytes[..8] != MAGIC {
        return Ok(Parsed::NotArmed);
    }
    let (body, seal) = bytes.split_at(bytes.len() - SEAL_LEN);
    if fnv1a(body) != u64::from_le_bytes(seal.try_into().expect("8 bytes")) {
        return Ok(Parsed::NotArmed);
    }

    // Sealed: the journal was written whole, so nothing past this point
    // is a torn write and nothing gets the benefit of the doubt (P6).
    let original_len = le64(body, 8);
    let new_len = le64(body, 16);
    if new_len < original_len {
        return Err(contradiction(sidecar, "the image shrank"));
    }
    let count = le64(body, 24);
    let body_len = body.len() as u64;
    let mut records = Vec::new();
    let mut at = HEADER_LEN as u64;
    for _ in 0..count {
        if body_len - at < 16 {
            return Err(contradiction(sidecar, "a record overruns the seal"));
        }
        let offset = le64(body, at as usize);
        let length = le64(body, at as usize + 8);
        at += 16;
        if length > body_len - at {
            return Err(contradiction(sidecar, "a record overruns the seal"));
        }
        if offset + length > original_len {
            return Err(contradiction(sidecar, "a record lies past the old image"));
        }
        records.push(UndoRecord {
            offset,
            bytes: body[at as usize..(at + length) as usize].to_vec(),
        });
        at += length;
    }
    if at != body_len {
        return Err(contradiction(sidecar, "trailing bytes after the records"));
    }
    Ok(Parsed::Armed(UndoJournal { original_len, new_len, records }))
}

/// Records the undo journal for one commit and makes it durable: for
/// every staged host block, the image's current bytes there (growth past
/// the image's present end needs no bytes — truncation restores it).
/// Returns the journal, kept in memory for an in-process undo should the
/// apply fail short of completion.
pub(crate) fn record(
    sidecar: &Path,
    device: &mut FileDevice,
    blocks: &BTreeMap<u64, Vec<u8>>,
    new_len: u64,
) -> Result<UndoJournal> {
    let original_len = device.len();
    let mut records = Vec::new();
    for (&offset, block) in blocks {
        if offset >= original_len {
            continue;
        }
        let take = (original_len - offset).min(block.len() as u64) as usize;
        let mut bytes = vec![0u8; take];
        device.read_at(offset, &mut bytes)?;
        records.push(UndoRecord { offset, bytes });
    }
    let journal = UndoJournal { original_len, new_len, records };
    write_sidecar(sidecar, &serialize(&journal)).map_err(|error| {
        Error::io(format!(
            "cannot record the commit's recovery journal '{}': {error}",
            sidecar.display()
        ))
    })?;
    Ok(journal)
}

fn write_sidecar(sidecar: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(sidecar)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
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

/// Reconciles an interrupted commit from its sidecar, under the claim
/// `device` already holds: an armed journal rolls the image back to
/// wholly the old state; a torn one proves the image was never touched.
/// Either way the sidecar is gone when this returns.
pub(crate) fn reconcile(
    sidecar: &Path,
    device: &mut FileDevice,
    image: &Path,
) -> Result<()> {
    let bytes = match std::fs::read(sidecar) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(Error::io(format!(
                "cannot read the recovery journal '{}': {error}",
                sidecar.display()
            )));
        }
    };
    match parse(&bytes, sidecar)? {
        Parsed::NotArmed => {
            let _ = retire(sidecar);
            Ok(())
        }
        Parsed::Armed(journal) => {
            let held = device.len();
            if held < journal.original_len || held > journal.new_len {
                return Err(Error::io(format!(
                    "the recovery journal '{}' does not match '{}': the journal \
                     covers an image of {} to {} bytes, but the file holds {held}; \
                     refusing to reconcile from it",
                    sidecar.display(),
                    image.display(),
                    journal.original_len,
                    journal.new_len
                )));
            }
            device.restore(&journal.records, journal.original_len)?;
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
    match FileDevice::open(image, AccessIntent::Write) {
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

    fn sample() -> UndoJournal {
        UndoJournal {
            original_len: 100,
            new_len: 4096,
            records: vec![
                UndoRecord { offset: 0, bytes: vec![1, 2, 3, 4] },
                UndoRecord { offset: 96, bytes: vec![9, 9, 9, 9] },
            ],
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
    fn a_sealed_journal_round_trips() {
        let bytes = serialize(&sample());
        let Parsed::Armed(back) = parse(&bytes, Path::new("j")).expect("parses") else {
            panic!("a sealed journal is armed");
        };
        assert_eq!(back.original_len, 100);
        assert_eq!(back.new_len, 4096);
        assert_eq!(back.records, sample().records);
    }

    #[test]
    fn a_torn_journal_is_not_armed() {
        let bytes = serialize(&sample());
        // Truncated anywhere, or flipped anywhere: never armed.
        for cut in [0, 7, HEADER_LEN, bytes.len() - SEAL_LEN, bytes.len() - 1] {
            let Parsed::NotArmed = parse(&bytes[..cut], Path::new("j")).expect("parses")
            else {
                panic!("a truncated journal must not be armed (cut at {cut})");
            };
        }
        for flip in [0, 9, HEADER_LEN + 3, bytes.len() - 4] {
            let mut torn = bytes.clone();
            torn[flip] ^= 0x40;
            let Parsed::NotArmed = parse(&torn, Path::new("j")).expect("parses") else {
                panic!("a corrupted journal must not be armed (flip at {flip})");
            };
        }
    }

    #[test]
    fn a_sealed_contradiction_is_refused_not_guessed_at() {
        // A structurally impossible journal wearing a valid seal: one
        // record promised, none present.
        let mut body = Vec::new();
        body.extend_from_slice(&MAGIC);
        body.extend_from_slice(&100u64.to_le_bytes());
        body.extend_from_slice(&4096u64.to_le_bytes());
        body.extend_from_slice(&1u64.to_le_bytes());
        let seal = fnv1a(&body);
        body.extend_from_slice(&seal.to_le_bytes());

        let error = parse(&body, Path::new("j")).expect_err("refuses");
        assert!(error.to_string().contains("self-contradictory"));
    }
}
