// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The durable commit point (P2, P9).
//!
//! A recovery journal is armed beneath the write-through, so an
//! interruption at any point leaves state the next open reconciles —
//! wholly the old image or wholly the committed new one — before the
//! disk is exposed. Interruption never invents a third state.
//!
//! `crash_test_process_at` is how that is proved rather than asserted:
//! under test the process vanishes at a named boundary, bypassing
//! destructors, so the parent observes exactly what a lost process
//! leaves on disk.

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::io::journal;

use super::state::MediaState;

#[cfg(test)]
fn crash_test_process_at(boundary: &str) {
    if std::env::var_os("REMANENCE_CRASH_TEST_BOUNDARY").as_deref()
        == Some(std::ffi::OsStr::new(boundary))
    {
        // Deliberately bypass destructors: the parent test must observe
        // exactly what a vanished process leaves on disk.
        std::process::exit(86);
    }
}

impl MediaState {
    /// The commit point (P2), staged and journalled so that it is also
    /// durable (P9): the write-through runs against a capture while the
    /// file is untouched, the bytes it will overwrite reach the recovery
    /// journal before the first of them changes, and the journal retires
    /// only once the apply is through. The three phases below are that
    /// sequence, and each one's failure path is what makes an
    /// interruption reconcilable.
    pub(crate) fn commit(&mut self) -> Result<()> {
        self.require_usable()?;
        self.require_writable()?;
        if !self.cache.modified() {
            return self.virtual_disk.device_mut().flush();
        }
        // The durable commit journals beside the artifact (P9), so it
        // needs to know where the artifact sits. A handle whose name
        // could not be recovered refuses here rather than committing
        // without the journal that makes an interruption reconcilable.
        let (journal_path, image_path) = match (&self.journal_path, &self.path) {
            (Some(journal_path), Some(path)) => (journal_path.clone(), PathBuf::from(path)),
            _ => {
                return Err(Error::unsupported(
                    "this medium's source handle has no recoverable name, and a \
                     durable commit lands its recovery journal beside the \
                     artifact: commit through a handle whose file this host can \
                     name, or keep the changes in the session"
                        .to_owned(),
                ));
            }
        };

        // Stage: the write-through runs against a capture of the host
        // file — itself a bounded cache spilling to session storage
        // (P27) — so the complete set of host writes is known while the
        // file is still untouched. A refusal discards the staging —
        // the driver's caches put back alongside it — and keeps the
        // buffered state for the caller.
        let cache_snapshot = self.virtual_disk.cache_snapshot();
        let cache_bytes = self.cache_bytes;
        // Consuming the altered set joins the offloads in flight first.
        self.cache.join_offloads();
        self.virtual_disk.host_mut().begin_capture(cache_bytes);
        let staged = self.cache.write_through(self.virtual_disk.device_mut());
        let capture = self.virtual_disk.host_mut().take_capture();
        if let Err(error) = staged {
            self.virtual_disk.restore_cache(cache_snapshot);
            return Err(error);
        }
        if capture.is_clean() {
            self.cache.mark_committed();
            return self.virtual_disk.device_mut().flush();
        }

        // The durability boundary (P9): the bytes the apply will
        // overwrite are durable in the recovery journal — streamed
        // there, never held whole — before the first of them changes.
        if let Err(error) = journal::record(&journal_path, self.virtual_disk.host_mut(), &capture) {
            let _ = journal::retire(&journal_path);
            self.virtual_disk.restore_cache(cache_snapshot);
            return Err(error);
        }
        #[cfg(test)]
        crash_test_process_at("journal-armed");

        // Apply, then retire the journal. Should either fail, the
        // in-process undo reconciles from the armed journal, putting
        // the image back to wholly the old state; should even that
        // fail, the journal remains for the next open to reconcile.
        let applied = self
            .virtual_disk
            .host_mut()
            .apply(&capture)
            .and_then(|()| {
                #[cfg(test)]
                crash_test_process_at("image-applied");
                journal::retire(&journal_path).map_err(|error| {
                    Error::io(format!(
                        "cannot retire the commit's recovery journal '{}': {error}",
                        journal_path.display()
                    ))
                })
            })
            .map(|()| {
                #[cfg(test)]
                crash_test_process_at("journal-retired");
            });
        if let Err(error) = applied {
            match journal::reconcile(&journal_path, self.virtual_disk.host_mut(), &image_path) {
                Ok(()) => {
                    self.virtual_disk.restore_cache(cache_snapshot);
                }
                Err(_) => {
                    self.failed = Some(format!(
                        "a commit on '{}' failed partway and could not be undone \
                         in this session; reopen the disk to reconcile it",
                        image_path.display()
                    ));
                }
            }
            return Err(error);
        }

        self.cache.mark_committed();
        Ok(())
    }

    /// Discards everything buffered; the image is untouched. Unaltered
    /// cached extents stay resident — they still mirror the image.
    pub(crate) fn rollback(&mut self) {
        self.cache.discard_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;
    use crate::io::device::{AccessIntent, Device};

    /// The subprocess half of the crash harness. It is ignored during an
    /// ordinary test walk and selected explicitly by the parent below.
    /// `commit` terminates this process at the requested boundary; if
    /// it returns, the harness was not attached to that boundary.
    #[test]
    #[ignore]
    fn crash_commit_child() {
        let path = std::env::var_os(CRASH_IMAGE).expect("the parent supplies an image path");
        let mut disk = MediaState::open(std::path::PathBuf::from(path), AccessIntent::Write)
            .expect("child opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &new_content())
            .expect("child overwrites");
        disk.commit().expect("child commits");
        panic!("the requested crash boundary was not reached");
    }

    #[test]
    fn crash_harness_covers_every_commit_boundary_and_image_shape() {
        let boundaries = [
            ("journal-armed", false),
            ("image-applied", false),
            ("journal-retired", true),
        ];
        for (boundary, expect_new) in boundaries {
            for shape in ["raw", "qcow2", "vdi", "chain", "vdi-chain"] {
                let stem = format!("remanence-crash-{shape}-{boundary}-{}", std::process::id());
                let (path, backing, directory) = match shape {
                    "raw" => {
                        let path = std::env::temp_dir().join(format!("{stem}.img"));
                        build_committed_raw(&path);
                        (path, None, None)
                    }
                    "qcow2" => {
                        let path = std::env::temp_dir().join(format!("{stem}.qcow2"));
                        build_committed_qcow2(&path);
                        (path, None, None)
                    }
                    "vdi" => {
                        let path = std::env::temp_dir().join(format!("{stem}.vdi"));
                        build_committed_vdi(&path);
                        (path, None, None)
                    }
                    "chain" => {
                        let directory = std::env::temp_dir().join(stem);
                        let (path, backing) = build_committed_chain(&directory);
                        (path, Some(backing), Some(directory))
                    }
                    "vdi-chain" => {
                        let directory = std::env::temp_dir().join(stem);
                        let (path, backing) = build_committed_vdi_chain(&directory);
                        (path, Some(backing), Some(directory))
                    }
                    _ => unreachable!(),
                };
                let backing_before = backing
                    .as_ref()
                    .map(|path| std::fs::read(path).expect("backing reads"));

                run_crashing_commit(&path, boundary);

                let mut reopened = MediaState::open(&path, AccessIntent::Read)
                    .unwrap_or_else(|error| panic!("{shape}/{boundary} reopens: {error}"));
                let volume = only_extent(&mut reopened);
                let content = reopened
                    .read_file(volume, "OLD.BIN")
                    .unwrap_or_else(|error| panic!("{shape}/{boundary} reads: {error}"));
                assert_eq!(
                    content,
                    if expect_new {
                        new_content()
                    } else {
                        old_content()
                    },
                    "{shape}/{boundary} reconciles to a whole state"
                );
                assert!(
                    !crate::io::journal::sidecar_path(&path).exists(),
                    "{shape}/{boundary} leaves no recovery artifact after reopen"
                );
                drop(reopened);

                if let (Some(backing), Some(before)) = (&backing, backing_before) {
                    assert_eq!(
                        std::fs::read(backing).expect("backing reads"),
                        before,
                        "{shape}/{boundary} never modifies the backing file"
                    );
                }
                std::fs::remove_file(&path).ok();
                if let Some(backing) = backing {
                    std::fs::remove_file(backing).ok();
                }
                if let Some(directory) = directory {
                    std::fs::remove_dir(directory).ok();
                }
            }
        }
    }

    #[test]
    fn a_tiny_declared_cache_bound_still_commits_correctly() {
        let path = temp_image("tiny-bound");
        build_committed_raw(&path);

        // A one-extent working set: reads, uncommitted writes, and the
        // commit's capture all evict and spill constantly (P27), and
        // the result is byte-identical to an unbounded run.
        let mut disk = MediaState::open_with_cache(&path, AccessIntent::Write, 1).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        assert!(disk.is_modified());
        disk.commit().expect("commits");
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            new_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_commit_retires_its_recovery_journal() {
        let path = temp_image("retires");
        build_committed_raw(&path);

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");

        let volume = only_extent(&mut disk);
        disk.write_file(volume, "NEW.BIN", &new_content())
            .expect("writes");
        disk.commit().expect("commits");
        assert!(
            !crate::io::journal::sidecar_path(&path).exists(),
            "a completed commit leaves no recovery sidecar behind"
        );
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened.read_file(volume, "NEW.BIN").expect("reads"),
            new_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_torn_journal_proves_the_image_was_never_touched() {
        let path = temp_image("torn");
        build_committed_raw(&path);

        // A crash before the durability boundary leaves a torn sidecar
        // and an untouched image; the next open discards the one and
        // exposes the other unchanged — through the read-intent route,
        // which must trade its claim for the reconciliation.
        let sidecar = crate::io::journal::sidecar_path(&path);
        std::fs::write(&sidecar, b"torn mid-write, never sealed").expect("sidecar writes");

        let mut reopened =
            MediaState::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!sidecar.exists(), "the torn sidecar is discarded");
        let volume = only_extent(&mut reopened);
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interruption_mid_apply_reconciles_to_the_old_image() {
        let path = temp_image("mid-apply");
        build_committed_raw(&path);

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");

        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        assert!(blocks.len() >= 2, "the staged write set spans blocks");

        // The crash: half the staged writes land, the rest never do,
        // and the process vanishes without retiring the journal.
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        let mut reopened =
            MediaState::open(&path, AccessIntent::Write).expect("reconciles and opens");
        assert!(!crate::io::journal::sidecar_path(&path).exists());
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            old_content(),
            "the image reconciles to wholly the old state"
        );

        // The reconciled disk is fully usable: the same overwrite
        // commits durably this time.
        reopened
            .write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        reopened.commit().expect("commits");
        drop(reopened);
        let mut committed = MediaState::open(&path, AccessIntent::Read).expect("opens");
        assert_eq!(
            committed.read_file(volume, "OLD.BIN").expect("reads"),
            new_content()
        );
        drop(committed);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interruption_before_retirement_reconciles_to_the_old_image() {
        let path = temp_image("unretired");
        build_committed_raw(&path);

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");

        let volume = only_extent(&mut disk);
        disk.write_file(volume, "NEW.BIN", &new_content())
            .expect("writes");
        let (blocks, new_len) = stage_and_arm(&mut disk);

        // The crash: the apply completes, but the journal is never
        // retired — the commit never returned, so the armed journal
        // governs and the next open rolls the image back.
        for &(offset, ref block) in &blocks {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        let mut reopened =
            MediaState::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!crate::io::journal::sidecar_path(&path).exists());
        assert_eq!(
            reopened.stat(volume, "NEW.BIN").expect("stats"),
            None,
            "the interrupted commit's file never existed"
        );
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_interrupted_qcow2_commit_reconciles_before_the_disk_is_exposed() {
        let path = std::env::temp_dir().join(format!(
            "remanence-durable-qcow2-{}.qcow2",
            std::process::id()
        ));
        build_fat16_qcow2(&path);

        // The wholly-old state: one committed file inside the qcow2.
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &old_content())
            .expect("writes");
        disk.commit().expect("commits");
        assert!(!crate::io::journal::sidecar_path(&path).exists());
        drop(disk);

        // An interrupted commit: cluster allocations and metadata
        // updates land partially, then the process vanishes.
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("opens");
        disk.write_file(volume, "NEW.BIN", &new_content())
            .expect("writes");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        assert!(blocks.len() >= 2, "the staged write set spans blocks");
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        // The next open reconciles to wholly the old image: metadata
        // consistent, the old file intact, the interrupted one absent.
        let mut reopened =
            MediaState::open(&path, AccessIntent::Read).expect("reconciles and opens");
        assert!(!crate::io::journal::sidecar_path(&path).exists());
        let volume = only_extent(&mut reopened);
        assert_eq!(reopened.stat(volume, "NEW.BIN").expect("stats"), None);
        assert_eq!(
            reopened.read_file(volume, "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_backing_member_reconciles_before_the_chain_composes() {
        let base = std::env::temp_dir().join(format!(
            "remanence-durable-chain-base-{}.qcow2",
            std::process::id()
        ));
        let top = std::env::temp_dir().join(format!(
            "remanence-durable-chain-top-{}.qcow2",
            std::process::id()
        ));
        let virtual_size = build_fat16_qcow2(&base);
        let mut disk = MediaState::open(&base, AccessIntent::Write).expect("opens");
        let volume = only_extent(&mut disk);
        disk.write_file(volume, "OLD.BIN", &old_content())
            .expect("writes");
        disk.commit().expect("commits");
        drop(disk);

        // An interrupted commit on the base, left behind before it
        // became a backing file.
        let mut disk = MediaState::open(&base, AccessIntent::Write).expect("opens");
        disk.write_file(volume, "OLD.BIN", &new_content())
            .expect("overwrites");
        let (blocks, new_len) = stage_and_arm(&mut disk);
        for &(offset, ref block) in blocks.iter().take(blocks.len() / 2) {
            let take = (new_len.saturating_sub(offset)).min(block.len() as u64) as usize;
            disk.virtual_disk
                .host_mut()
                .write_at(offset, &block[..take])
                .expect("applies");
        }
        drop(disk);

        // A fresh top image naming the base as its backing file.
        let mut image = empty_qcow2_bytes(virtual_size);
        let name = base.to_str().expect("utf-8 temp path").as_bytes();
        image[0x200..0x200 + name.len()].copy_from_slice(name);
        image[8..16].copy_from_slice(&0x200u64.to_be_bytes());
        image[16..20].copy_from_slice(&(name.len() as u32).to_be_bytes());
        std::fs::write(&top, image).expect("top writes");

        // Composing the chain reconciles the base first (P9): its
        // sidecar is gone and wholly the old bytes show through.
        let mut chained =
            MediaState::open(&top, AccessIntent::Read).expect("reconciles and composes");
        assert!(!crate::io::journal::sidecar_path(&base).exists());
        assert_eq!(
            chained.read_file(volume, "OLD.BIN").expect("reads"),
            old_content()
        );
        drop(chained);
        std::fs::remove_file(&top).ok();
        std::fs::remove_file(&base).ok();
    }
}
