// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The archive-catalog seam. Each archive grammar sits behind its own
//! catalog adapter — [`crate::zip::ZipCatalog`] owns ZIP,
//! [`crate::sevenzip::SevenZipCatalog`] owns 7z — and a catalog does
//! exactly two things: it reports the archive's entries in the
//! archive's own order, and it produces one entry's bytes as a bounded
//! source. It does not identify disk images, interpret media, or know
//! what any entry is for; that reading happens above it (P19).
//!
//! The catalog list is wiring. An adapter is reached by enrollment
//! alone — its descriptor names the extensions it answers to — so
//! adding a grammar changes its own module, its tests, and one entry in
//! [`BUILT_IN_ARCHIVE_ADAPTERS`], and nothing here branches on a format
//! identifier.
//!
//! Nothing is read whole (P27). The archive is claimed under P7 for the
//! catalog's lifetime and read by positioned reads: an entry stored
//! uncompressed resolves to a span of the archive file, read in place,
//! while a coded entry is decoded once through its decompressor's
//! window into private session storage.

use std::ffi::OsStr;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::device::{AccessMode, open_locked};
use crate::error::{Error, Result};
use crate::sevenzip::SEVENZIP_ADAPTER;
use crate::zip::ZIP_ADAPTER;

/// What an archive grammar is called, and what it answers to.
pub(crate) struct ArchiveFormatDescriptor {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) extensions: &'static [&'static str],
}

impl ArchiveFormatDescriptor {
    fn claims_extension(&self, extension: &OsStr) -> bool {
        self.extensions
            .iter()
            .any(|claimed| extension.eq_ignore_ascii_case(claimed))
    }
}

/// One entry an archive holds, as its catalog reports it.
///
/// Sizes are the archive's own claims about the entry, not measurements
/// of what was read. `compressed_size` is absent where the grammar
/// attributes none to a single entry — a member of a solid 7z folder is
/// compressed together with its neighbours, so no share of the packed
/// bytes is its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// The entry's path inside the archive, `/`-separated.
    pub name: String,
    pub is_dir: bool,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: u64,
}

/// Where one entry's bytes come from, bounded (P27).
pub(crate) enum EntrySource {
    /// The entry's bytes are a span of the claimed archive file and are
    /// read in place — source-backed, nothing copied.
    InPlace { offset: u64, length: u64 },
    /// The entry had to be decoded, and was produced once into private
    /// session storage — session-backed.
    Spooled { spool: Arc<File>, length: u64 },
}

/// One archive grammar's reader over a claimed file.
pub(crate) trait ArchiveCatalog: Send + Sync {
    fn descriptor(&self) -> &'static ArchiveFormatDescriptor;
    /// The archive file's own size in bytes.
    fn archive_size(&self) -> u64;
    /// Every entry, in the archive's own order.
    fn entries(&self) -> &[ArchiveEntry];
    /// The bytes of the entry at `index`, bounded.
    fn entry_source(&self, index: usize) -> Result<EntrySource>;
}

/// One enrolled archive grammar.
pub(crate) trait ArchiveFormatAdapter: Sync {
    fn descriptor(&self) -> &'static ArchiveFormatDescriptor;
    fn open(&self, file: Arc<File>, len: u64) -> Result<Box<dyn ArchiveCatalog>>;
}

static BUILT_IN_ARCHIVE_ADAPTERS: [&dyn ArchiveFormatAdapter; 2] =
    [&ZIP_ADAPTER, &SEVENZIP_ADAPTER];

/// The enrolled archive grammars, in the order they are consulted.
pub(crate) struct ArchiveCatalogRegistry<'a> {
    adapters: &'a [&'a dyn ArchiveFormatAdapter],
}

impl<'a> ArchiveCatalogRegistry<'a> {
    /// The adapter claiming `path`'s extension, if any.
    fn adapter_for(&self, path: &Path) -> Option<&'a dyn ArchiveFormatAdapter> {
        let extension = path.extension()?;
        self.adapters
            .iter()
            .copied()
            .find(|adapter| adapter.descriptor().claims_extension(extension))
    }

    /// Splits `path` at the first component an enrolled grammar claims,
    /// into `(archive_path, entry_path)`.
    fn split(&self, path: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
        let mut archive_path = PathBuf::new();
        let mut entry_path = PathBuf::new();
        let mut found = false;

        for component in path.components() {
            if found {
                if matches!(component, Component::CurDir) {
                    continue;
                }
                entry_path.push(component.as_os_str());
                continue;
            }
            archive_path.push(component.as_os_str());
            if self
                .adapter_for(Path::new(component.as_os_str()))
                .is_some()
            {
                found = true;
            }
        }

        if !found {
            return None;
        }
        let entry = (!entry_path.as_os_str().is_empty()).then_some(entry_path);
        Some((archive_path, entry))
    }

    /// Claims `path` (P7) and opens the catalog its grammar declares.
    fn open(&self, path: &Path) -> Result<ClaimedArchive> {
        let adapter = self.adapter_for(path).ok_or_else(|| {
            Error::unsupported(format!(
                "'{}' names no archive format this library reads",
                path.display()
            ))
        })?;
        let (file, mode) = open_locked(path)?;
        let len = file
            .metadata()
            .map_err(|error| Error::io(format!("failed to stat '{}': {error}", path.display())))?
            .len();
        let file = Arc::new(file);
        let catalog = adapter.open(Arc::clone(&file), len)?;
        Ok(ClaimedArchive {
            file,
            mode,
            catalog,
        })
    }
}

pub(crate) fn archive_catalogs() -> ArchiveCatalogRegistry<'static> {
    ArchiveCatalogRegistry {
        adapters: &BUILT_IN_ARCHIVE_ADAPTERS,
    }
}

/// An opened catalog together with the claim it reads through.
pub(crate) struct ClaimedArchive {
    pub file: Arc<File>,
    pub mode: AccessMode,
    pub catalog: Box<dyn ArchiveCatalog>,
}

/// Splits a path at the first archive component into
/// `(archive_path, optional entry_path)`.
pub(crate) fn split_archive_path(path: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    archive_catalogs().split(path)
}

/// Claims and opens the archive at `path`.
pub(crate) fn open_archive(path: &Path) -> Result<ClaimedArchive> {
    archive_catalogs().open(path)
}

/// Joins the normal components of an entry path with `/`.
pub(crate) fn normalize_entry_name(path: &Path) -> String {
    let mut result = String::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(&component.to_string_lossy());
    }
    result
}

/// An archive's entries, read through the claim this listing holds.
///
/// Opening claims the archive file (P7) — writes denied to every other
/// process — and holds that claim until the listing is dropped. Only
/// the archive's own index is read: entry data is never touched, so
/// listing a hundred-gigabyte archive costs its directory and nothing
/// more (P27).
pub struct Archive {
    path: PathBuf,
    claimed: ClaimedArchive,
}

impl std::fmt::Debug for Archive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Archive")
            .field("path", &self.path)
            .field("format", &self.format_id())
            .field("entries", &self.entries().len())
            .finish()
    }
}

impl Archive {
    /// Opens the archive at `path`. A path naming no archive format
    /// this library reads is refused by name, never guessed at.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        Ok(Self {
            path: path.to_path_buf(),
            claimed: open_archive(path)?,
        })
    }

    /// The path the archive was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The archive format's stable identifier, e.g. `"zip"` or `"7z"`.
    pub fn format_id(&self) -> &'static str {
        self.claimed.catalog.descriptor().id
    }

    /// The archive format's human-readable name.
    pub fn format_name(&self) -> &'static str {
        self.claimed.catalog.descriptor().name
    }

    /// Which P7 mode the open obtained on the archive file.
    pub fn access_mode(&self) -> AccessMode {
        self.claimed.mode
    }

    /// The archive file's own size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.claimed.catalog.archive_size()
    }

    /// Every entry the archive holds, in the archive's own order.
    pub fn entries(&self) -> &[ArchiveEntry] {
        self.claimed.catalog.entries()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    static TEST_DESCRIPTOR: ArchiveFormatDescriptor = ArchiveFormatDescriptor {
        id: "test",
        name: "Test archive",
        extensions: &["test"],
    };

    struct TestAdapter;

    impl ArchiveFormatAdapter for TestAdapter {
        fn descriptor(&self) -> &'static ArchiveFormatDescriptor {
            &TEST_DESCRIPTOR
        }

        fn open(&self, _file: Arc<File>, _len: u64) -> Result<Box<dyn ArchiveCatalog>> {
            Err(Error::archive("test", "test adapter was selected"))
        }
    }

    static TEST_ADAPTER: TestAdapter = TestAdapter;

    fn test_registry() -> ArchiveCatalogRegistry<'static> {
        static ADAPTERS: [&dyn ArchiveFormatAdapter; 1] = [&TEST_ADAPTER];
        ArchiveCatalogRegistry {
            adapters: &ADAPTERS,
        }
    }

    #[test]
    fn an_archive_grammar_is_reached_by_enrollment_alone() {
        let (archive, entry) = test_registry()
            .split(Path::new("sample.test/inner/disk.img"))
            .expect("the enrolled extension splits the path");
        assert_eq!(archive, Path::new("sample.test"));
        assert_eq!(entry.as_deref(), Some(Path::new("inner/disk.img")));
    }

    #[test]
    fn a_path_no_grammar_claims_does_not_split() {
        assert!(test_registry().split(Path::new("disk.img")).is_none());
        assert!(split_archive_path(Path::new("disk.img")).is_none());
    }

    #[test]
    fn the_built_in_grammars_claim_their_extensions() {
        let (archive, entry) = split_archive_path(Path::new("captures.7z/track00.raw"))
            .expect("7z splits the path");
        assert_eq!(archive, Path::new("captures.7z"));
        assert_eq!(entry.as_deref(), Some(Path::new("track00.raw")));

        let (archive, entry) =
            split_archive_path(Path::new("Disks.ZIP")).expect("the match ignores case");
        assert_eq!(archive, Path::new("Disks.ZIP"));
        assert_eq!(entry, None);
    }

    #[test]
    fn an_unclaimed_extension_is_refused_by_name() {
        let error = Archive::open("nowhere.rar").expect_err("rar is outside the claim");
        assert_eq!(error.category(), ErrorCategory::Unsupported);
        assert!(
            error.to_string().contains("names no archive format"),
            "{error}"
        );
    }

    #[test]
    fn entry_names_normalize_to_forward_slashes() {
        assert_eq!(
            normalize_entry_name(Path::new("inner/./deeper/disk.img")),
            "inner/deeper/disk.img"
        );
    }
}
