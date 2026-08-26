// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The namespace verbs over a block medium's volumes, and the two
//! guards every write passes first.
//!
//! Reads and writes are buffered until [`MediaState::commit`] like
//! every other write (P2). `require_writable` is P28's effective mode
//! at the point of use — a degraded session never regains authority it
//! did not open with — and `require_usable` is the harder stop: a
//! commit that failed partway and could not undo itself leaves caches
//! that no longer describe the file, so every verb refuses until a
//! fresh open reconciles it.

use crate::error::{Error, Result};
use crate::filesystem::fat::{FatEntry, FatVolume};
use crate::io::device::Device;

use super::state::MediaState;

impl MediaState {
    /// Lists a directory in the extent starting at `offset`
    /// ("" = root; "A/B" descends).
    pub(crate) fn entries(&mut self, offset: u64, path: &str) -> Result<Vec<FatEntry>> {
        let segments = Self::split_path(path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.entries(&mut composed, &segments)
    }

    /// Answers one path with its entry, or `None` where nothing exists
    /// at it — absence being an answer rather than a failure (U3).
    pub(crate) fn stat(&mut self, offset: u64, path: &str) -> Result<Option<FatEntry>> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.stat(&mut composed, &segments)
    }

    /// The degraded session's extraction gate (P28): an entry is answered
    /// whole or not at all, so its directory record and its complete
    /// cluster chain must lie inside the readable extent before a byte of
    /// it is served. A verified session has no gate to pass.
    ///
    /// This is what keeps a crossing file from being clipped, zero-filled,
    /// or served in the part that happens to be present — including
    /// through the ranged form, where the requested span alone might sit
    /// inside the extent while the file does not.
    fn require_whole(&mut self, offset: u64, segments: &[&str], path: &str) -> Result<()> {
        let Some(bound) = self.bound else {
            return Ok(());
        };
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        let end = fat.extent_end(&mut composed, segments)?;
        if end > bound.end {
            return Err(bound.withheld(&format!("'{path}'"), end));
        }
        Ok(())
    }

    /// Copies a file's bytes out of the extent starting at `offset`.
    pub(crate) fn read_file(&mut self, offset: u64, path: &str) -> Result<Vec<u8>> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        self.require_whole(offset, &segments, path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.read_file(&mut composed, &segments)
    }

    /// Reads part of a file — the streamed form (P27): exactly `buf`
    /// bytes at `offset`, which must lie within the file.
    pub(crate) fn read_file_at(
        &mut self,
        at: u64,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        self.require_whole(at, &segments, path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, at)?;
        fat.read_file_at(&mut composed, &segments, offset, buf)
    }

    /// Sets a file's size, creating it when absent: kept bytes preserved
    /// in place, a grown region reading as zeros. Buffered until commit.
    pub(crate) fn resize_file(&mut self, offset: u64, path: &str, size: u64) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.resize_file(&mut composed, &segments, size)
    }

    /// Writes part of a file in place — the streamed form (P27): the
    /// span must lie within the file's current size. Buffered until
    /// commit.
    pub(crate) fn write_file_at(
        &mut self,
        at: u64,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, at)?;
        fat.write_file_at(&mut composed, &segments, offset, data)
    }

    /// Reads at an absolute offset in the presented disk, for a space that
    /// resolved the position against its own extent.
    ///
    /// It reads through the session cache rather than off the source, so
    /// a caller sees the state its own buffered writes produced — the
    /// same truth every other verb in the session reads (P2, P27).
    pub(crate) fn read_space_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if let Some(bound) = &self.bound {
            bound.check(offset, buf.len() as u64)?;
        }
        let mut composed = self.composed();
        composed.read_at(offset, buf)
    }

    /// Writes at an absolute offset in the presented disk, for a space
    /// that resolved the position against its own extent. Buffered until
    /// commit (P2), landing in the active layer (P23).
    pub(crate) fn write_space_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.require_writable()?;
        let mut composed = self.composed();
        composed.write_at(offset, data)
    }

    /// Writes a file into the extent starting at `offset`, an
    /// existing one overwritten and an existing directory refused.
    /// Buffered until [`MediaState::commit`].
    pub(crate) fn write_file(&mut self, offset: u64, path: &str, contents: &[u8]) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        if segments.is_empty() {
            return Err(Error::io("a file path is required".to_owned()));
        }
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.write_file(&mut composed, &segments, contents)
    }

    /// The label of the volume in the extent starting at `offset`,
    /// answered whole by the FAT seam that owns the policy.
    pub(crate) fn volume_label(&mut self, offset: u64) -> Result<crate::VolumeLabel> {
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.label(&mut composed)
    }

    /// Sets or removes the label of the volume in the extent starting at
    /// `offset` — the root directory's volume-ID entry, exactly what
    /// DOS's own `LABEL` writes. Buffered until commit.
    pub(crate) fn set_label(&mut self, offset: u64, label: Option<&str>) -> Result<()> {
        self.require_writable()?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.set_label(&mut composed, label)
    }

    /// Ensures a directory exists, missing parents created and an
    /// existing directory succeeding unchanged. Buffered until commit.
    pub(crate) fn make_directory(&mut self, offset: u64, path: &str) -> Result<()> {
        self.require_writable()?;
        let segments = Self::split_path(path)?;
        let mut composed = self.composed();
        let fat = FatVolume::open(&mut composed, offset)?;
        fat.make_directory(&mut composed, &segments)
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;
    use crate::io::device::AccessIntent;

    #[test]
    fn the_label_verbs_round_trip_on_fat16_and_survive_the_commit() {
        let path = temp_image("label-verbs");
        std::fs::write(&path, fat16_volume_bytes()).expect("image writes");
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);

        // The fixture's own label answers, from the entry it wrote.
        let label = disk.volume_label(volume).expect("answers");
        assert_eq!(label.name.as_deref(), Some("REMANENCE"));

        disk.set_label(volume, Some("RELABELLED"))
            .expect("relabels");
        disk.commit().expect("commits");
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Write).expect("reopens");
        let label = reopened.volume_label(volume).expect("answers");
        assert_eq!(
            label.name.as_deref(),
            Some("RELABELLED"),
            "the relabel survives the commit"
        );

        // Removed — and this volume's boot record states no label field
        // at all, so the answer is no label from no source.
        reopened.set_label(volume, None).expect("removes");
        let label = reopened.volume_label(volume).expect("answers");
        assert_eq!(label.name, None);
        assert_eq!(label.answered_by, None);
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn streamed_file_verbs_round_trip_beside_the_whole_file_forms() {
        let path = temp_image("streamed-verbs");
        std::fs::write(&path, fat16_volume_bytes()).expect("image writes");
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);

        // Streamed replace: size the file, then write it in chunks.
        let contents = new_content();
        disk.resize_file(volume, "BIG.BIN", contents.len() as u64)
            .expect("sizes");
        for (n, chunk) in contents.chunks(10_000).enumerate() {
            disk.write_file_at(volume, "BIG.BIN", (n * 10_000) as u64, chunk)
                .expect("writes a chunk");
        }
        assert_eq!(
            disk.read_file(volume, "BIG.BIN").expect("whole read"),
            contents,
            "the streamed write equals a whole-file write"
        );

        // Streamed read: ranged reads reassemble the whole.
        let mut ranged = vec![0u8; contents.len()];
        for start in (0..contents.len()).step_by(7_777) {
            let end = (start + 7_777).min(contents.len());
            disk.read_file_at(volume, "BIG.BIN", start as u64, &mut ranged[start..end])
                .expect("reads a range");
        }
        assert_eq!(ranged, contents);

        // Shrink keeps the prefix; growth reads as zeros, never stale bytes.
        disk.resize_file(volume, "BIG.BIN", 100).expect("shrinks");
        disk.resize_file(volume, "BIG.BIN", 20_000)
            .expect("regrows");
        let back = disk.read_file(volume, "BIG.BIN").expect("reads");
        assert_eq!(&back[..100], &contents[..100], "the kept prefix survives");
        assert!(back[100..].iter().all(|&byte| byte == 0), "growth is zeros");

        // The bounds are refusals, not clamps.
        let mut probe = [0u8; 8];
        assert!(
            disk.read_file_at(volume, "BIG.BIN", 19_996, &mut probe)
                .is_err(),
            "a read past the size is refused"
        );
        assert!(
            disk.write_file_at(volume, "BIG.BIN", 19_996, &probe)
                .is_err(),
            "a write past the size is refused"
        );

        // Everything above survives the commit.
        disk.commit().expect("commits");
        drop(disk);
        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        let back = reopened.read_file(volume, "BIG.BIN").expect("reads");
        assert_eq!(back.len(), 20_000);
        assert_eq!(&back[..100], &contents[..100]);
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }
}
