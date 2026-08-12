// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The archive medium and the catalog seam beneath it.
//!
//! **An archive is a medium** (P14): independent recorded state with no
//! physical article behind it, held by no drive, loaded into a virtual
//! slot like every other medium into its own device. Its native vantage
//! is a **namespace** rather than a space — there is no meaningful
//! "sector 5 of a zip", its byte extent being the encoding (P13) — so
//! what a caller reaches through it is names, and the artifact's own
//! bytes stay readable beside them as evidence.
//!
//! Each archive grammar sits behind its own catalog adapter —
//! [`crate::zip::ZipCatalog`] owns ZIP,
//! [`crate::sevenzip::SevenZipCatalog`] owns 7z — and is the P12 adapter
//! that recognizes the artifact and loads its state. A catalog does
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

use crate::assurance::{Assurance, AssuranceOutcome, ByteRange};
use crate::device::{AccessIntent, AccessMode, Claim, open_locked, read_exact_at};
use crate::error::{Error, ErrorCategory, Result};
use crate::filesystem::{Catalog, Entry, EntryFact, EntryKind};
use crate::handle;
use crate::media_profile::{ARCHIVE, MediaProfile};
use crate::session::{Identification, Layer, LayerKind, LayerLayout, SizeInformation};
use crate::sevenzip::SEVENZIP_ADAPTER;
use crate::source::{self, ArchiveLayer, ImageSource, ResolvedImage};
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
///
/// It is crate-private: an archive's entries reach a caller as the
/// namespace its medium bears (P19), in the same [`Entry`] every other
/// filesystem answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArchiveEntry {
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
    /// session storage — session-backed. Several entries obtained
    /// together may share one spool, each at its own offset.
    Spooled {
        spool: Arc<File>,
        offset: u64,
        length: u64,
    },
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

    /// The bytes of several entries, obtained in one pass over the
    /// archive's coded stream.
    ///
    /// A logical artifact spread over many members — a capture set is
    /// one disk per stream per head per step position — asks for them
    /// together so a grammar whose members share one coded stream
    /// decodes it once rather than once per member. The default is the
    /// honest one for a grammar where each entry stands alone; a solid
    /// archive overrides it, because there the difference is the whole
    /// cost of the operation.
    fn entry_group(&self, indices: &[usize]) -> Result<Vec<EntrySource>> {
        indices
            .iter()
            .map(|&index| self.entry_source(index))
            .collect()
    }
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

    /// The adapter a declared grammar names.
    fn adapter_by_id(&self, id: &str) -> Option<&'a dyn ArchiveFormatAdapter> {
        self.adapters
            .iter()
            .copied()
            .find(|adapter| adapter.descriptor().id == id)
    }

    /// Opens the caller's own opened archive as the grammar they
    /// **declared**, under their claim.
    ///
    /// The declaration replaces the extension the path journey reads a
    /// grammar off: nothing here looks at a name, and the grammar itself
    /// is what checks the artifact — a 7z declared `zip` fails to parse
    /// as one and is refused by that grammar, naming what it found.
    fn load(&self, file: File, grammar: &str, named: &str) -> Result<ClaimedArchive> {
        let adapter = self
            .adapter_by_id(grammar)
            .expect("a declared grammar is one this catalog holds");
        let mode = handle::afforded_access(&file);
        let len = file
            .metadata()
            .map_err(|error| {
                Error::io(format!("cannot read the size of {named}: {error}"))
            })?
            .len();
        let file = Arc::new(file);
        let catalog: Arc<dyn ArchiveCatalog> = Arc::from(adapter.open(Arc::clone(&file), len)?);
        Ok(ClaimedArchive {
            file,
            mode,
            catalog,
        })
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
        let catalog: Arc<dyn ArchiveCatalog> = Arc::from(adapter.open(Arc::clone(&file), len)?);
        Ok(ClaimedArchive {
            file,
            mode,
            catalog,
        })
    }
}

/// Whether an enrolled grammar claims `path` — the recognition that
/// makes an artifact an archive medium rather than a block image.
///
/// It is the extension the grammar answers to, which is what the catalog
/// has always claimed by: a ZIP's own signature sits behind whatever
/// stub precedes it, so a grammar that recognized by leading bytes would
/// refuse self-extracting archives this one reads.
pub(crate) fn is_archive(path: &Path) -> bool {
    archive_catalogs().adapter_for(path).is_some()
}

pub(crate) fn archive_catalogs() -> ArchiveCatalogRegistry<'static> {
    ArchiveCatalogRegistry {
        adapters: &BUILT_IN_ARCHIVE_ADAPTERS,
    }
}

/// An opened catalog together with the claim it reads through.
///
/// The catalog is refcounted because two readers share it: the medium,
/// which answers for what the artifact is, and the namespace view over
/// it, which cannot borrow from the device it is reached through.
pub(crate) struct ClaimedArchive {
    pub file: Arc<File>,
    pub mode: AccessMode,
    pub catalog: Arc<dyn ArchiveCatalog>,
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

/// Opens the caller's own opened archive under a declared grammar.
pub(crate) fn load_archive(file: File, grammar: &str, named: &str) -> Result<ClaimedArchive> {
    archive_catalogs().load(file, grammar, named)
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


/// The archive medium: the artifact claimed, the catalog its grammar
/// reads, and the evidence plane beside them.
///
/// It is a medium in exactly P14's sense and answers the medium
/// questions — what it is, what claim it holds, what the open
/// established — while answering none of the space questions, because it
/// has no space. Those refuse by name on the device that holds it.
pub(crate) struct ArchiveMedium {
    /// The artifact as the caller named it, or as its handle was
    /// recovered — absent where the handle has no recoverable name.
    path: Option<String>,
    claimed: ClaimedArchive,
    /// The artifact's own bytes, under the same claim: readable as
    /// evidence (P2), which is plumbing rather than a vantage.
    source: ImageSource,
    /// The declared session cache bound (P27), carried into whatever is
    /// loaded out of this archive.
    cache_bytes: u64,
    assurance: Assurance,
}

impl std::fmt::Debug for ArchiveMedium {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchiveMedium")
            .field("path", &self.path)
            .field("format", &self.format_id())
            .field("entries", &self.claimed.catalog.entries().len())
            .finish()
    }
}

impl ArchiveMedium {
    /// Claims the archive at `path` (P7) and loads its state — the
    /// entries its grammar declares.
    ///
    /// **Read-only, as archives are.** A write would have to be encoded
    /// back into the grammar's own form and no adapter claims that, so a
    /// write intent is refused by name here rather than accepted and
    /// disappointed at the commit point (P7).
    pub(crate) fn open(path: &Path, intent: AccessIntent, cache_bytes: u64) -> Result<Self> {
        if intent == AccessIntent::Write {
            return Err(Error::read_only(format!(
                "'{}' is an archive medium, which this release reads and does \
                 not write: a write would have to be encoded back into the \
                 archive's own grammar, and no adapter claims that",
                path.display()
            )));
        }
        let claimed = open_archive(path)?;
        Ok(Self::over(
            Some(path.display().to_string()),
            claimed,
            Claim::LibraryOpened,
            cache_bytes,
        ))
    }

    /// Loads the caller's own opened archive as the grammar they
    /// **declared**, under their claim (P7 as amended).
    ///
    /// An archive is read and not written whichever way it was reached,
    /// so a handle affording writes is not an error here and buys
    /// nothing: the medium is read-only because no adapter encodes an
    /// archive back into its own grammar, which is a fact about this
    /// release rather than about the claim.
    pub(crate) fn load(file: File, grammar: &str, cache_bytes: u64) -> Result<Self> {
        let name = handle::recovered_name(&file).map(|name| name.display().to_string());
        let named = crate::media::named(name.as_deref());
        let claimed = load_archive(file, grammar, &named)?;
        Ok(Self::over(name, claimed, Claim::CallerOpened, cache_bytes))
    }

    /// The medium both journeys build: the claimed catalog, the evidence
    /// plane beside it, and the assurance the index established.
    fn over(
        path: Option<String>,
        claimed: ClaimedArchive,
        claim: Claim,
        cache_bytes: u64,
    ) -> Self {
        let len = claimed.catalog.archive_size();
        let source =
            ImageSource::over_claim(Arc::clone(&claimed.file), claimed.mode, len, cache_bytes);
        let assurance = Assurance {
            outcome: AssuranceOutcome::Verified,
            condition: None,
            evidence: vec![
                format!(
                    "read the {} index whole",
                    claimed.catalog.descriptor().name
                ),
                format!(
                    "the archive declares {} entries",
                    claimed.catalog.entries().len()
                ),
            ],
            readable: vec![ByteRange::new(0, len)],
            access: claimed.mode,
            claim,
            declared_bytes: None,
            observed_bytes: Some(len),
            first_unavailable_byte: None,
        };
        Self {
            path,
            claimed,
            source,
            cache_bytes,
            assurance,
        }
    }

    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The artifact as a refusal names it.
    pub(crate) fn named(&self) -> String {
        crate::media::named(self.path())
    }

    /// The archive file's own size in bytes — the artifact, not a
    /// presented disk, of which an archive has none.
    pub(crate) fn size_bytes(&self) -> u64 {
        self.claimed.catalog.archive_size()
    }

    pub(crate) fn mode(&self) -> AccessMode {
        self.claimed.mode
    }

    pub(crate) fn assurance(&self) -> &Assurance {
        &self.assurance
    }

    pub(crate) fn cache_bytes(&self) -> u64 {
        self.cache_bytes
    }

    /// The media type this medium is (P14) — the archive, whose one
    /// family fact is that its vantage is a namespace.
    pub(crate) fn media(&self) -> &'static MediaProfile {
        &ARCHIVE
    }

    pub(crate) fn format_id(&self) -> &'static str {
        self.claimed.catalog.descriptor().id
    }

    pub(crate) fn format_name(&self) -> &'static str {
        self.claimed.catalog.descriptor().name
    }

    /// Reads the artifact's own bytes — evidence, streamed through the
    /// session cache like every other read (P27).
    pub(crate) fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.source.read_at(offset, buf)
    }

    /// The nesting this artifact was reached through: the archive, and
    /// nothing else.
    ///
    /// There is no image layer because nothing was recognized inside — an
    /// entry is recognized when it is opened as an artifact of its own,
    /// which is a separate act carrying its own identification.
    pub(crate) fn identify(&self) -> Identification {
        let descriptor = self.claimed.catalog.descriptor();
        Identification {
            layers: vec![Layer {
                kind: LayerKind::Archive,
                id: descriptor.id.to_owned(),
                name: descriptor.name.to_owned(),
                confidence: 100,
                known: true,
                size: SizeInformation {
                    current_bytes: Some(self.size_bytes()),
                    expected_bytes: None,
                },
                // No entry was traversed to reach this medium: the
                // archive *is* the medium, so there is no entry layout to
                // state.
                layout: LayerLayout::Unknown,
            }],
            modified: false,
            evidence: vec![format!(
                "read the {} index: {} entries",
                descriptor.name,
                self.claimed.catalog.entries().len()
            )],
        }
    }

    /// The namespace this medium bears, for the resolver above.
    ///
    /// An archive's content *is* its namespace, so this always answers —
    /// there is nothing to probe for and nothing to refuse.
    pub(crate) fn namespace(&self) -> (&'static str, Box<dyn Catalog>) {
        (
            self.format_id(),
            Box::new(ArchiveNamespace {
                catalog: Arc::clone(&self.claimed.catalog),
                file: Arc::clone(&self.claimed.file),
            }),
        )
    }

    /// Resolves one entry to a source a device can load, under this
    /// archive's own claim.
    pub(crate) fn resolve_entry(&self, name: &str) -> Result<ResolvedImage> {
        let catalog = self.claimed.catalog.as_ref();
        let descriptor = catalog.descriptor();
        let index = entry_index(catalog, name)?;
        let entry = &catalog.entries()[index];
        if entry.is_dir {
            return Err(Error::categorized_archive(
                ErrorCategory::IsDirectory,
                descriptor.id,
                format!("entry '{name}' is a directory"),
            ));
        }
        let layer = ArchiveLayer {
            id: descriptor.id.to_owned(),
            name: descriptor.name.to_owned(),
            path: self.path.as_deref().map(PathBuf::from),
            entry_name: entry.name.clone(),
            archive_size: Some(catalog.archive_size()),
            compressed_size: entry.compressed_size,
            uncompressed_size: Some(entry.uncompressed_size),
        };
        Ok(source::resolve_entry(
            Arc::clone(&self.claimed.file),
            self.claimed.mode,
            self.assurance.claim,
            layer,
            catalog.entry_source(index)?,
            self.cache_bytes,
        ))
    }
}

/// The index of the entry named `name`, or the refusal naming what was
/// asked for.
fn entry_index(catalog: &dyn ArchiveCatalog, name: &str) -> Result<usize> {
    catalog
        .entries()
        .iter()
        .position(|entry| entry.name == name)
        .ok_or_else(|| {
            Error::categorized_archive(
                ErrorCategory::NotFound,
                catalog.descriptor().id,
                format!("entry '{name}' not found"),
            )
        })
}

/// The archive's namespace, presented through the seam every
/// medium-borne namespace presents through.
///
/// **Directories are the grammar's own hierarchy, not manufactured
/// names.** An entry called `disks/boot.h8d` says there is a `disks`, and
/// listing it reads what the archive recorded rather than inventing a
/// pseudo-file (P19). A grammar that also records the directory itself is
/// answered from that record.
struct ArchiveNamespace {
    catalog: Arc<dyn ArchiveCatalog>,
    /// The claimed artifact, for the positioned reads a stored entry
    /// resolves to.
    file: Arc<File>,
}

impl std::fmt::Debug for ArchiveNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchiveNamespace")
            .field("format", &self.catalog.descriptor().id)
            .field("entries", &self.catalog.entries().len())
            .finish()
    }
}

impl ArchiveNamespace {
    /// The entry name a caller's path means, in the `/` form every
    /// grammar reports in.
    fn normalize(path: &str) -> String {
        path.split(['/', '\\'])
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .collect::<Vec<_>>()
            .join("/")
    }

    fn entry(&self, name: &str) -> Option<&ArchiveEntry> {
        self.catalog
            .entries()
            .iter()
            .find(|entry| entry.name.trim_end_matches('/') == name)
    }

    /// Where one entry's bytes are, and how many.
    fn located(&self, name: &str) -> Result<(EntrySource, u64)> {
        let index = entry_index(self.catalog.as_ref(), name)?;
        let entry = &self.catalog.entries()[index];
        if entry.is_dir {
            return Err(Error::categorized_archive(
                ErrorCategory::IsDirectory,
                self.catalog.descriptor().id,
                format!("entry '{name}' is a directory"),
            ));
        }
        let length = entry.uncompressed_size;
        Ok((self.catalog.entry_source(index)?, length))
    }

    /// The entry a directory listing reports one name as.
    fn named(name: &str, entry: &ArchiveEntry) -> Entry {
        Entry {
            name: name.to_owned(),
            kind: if entry.is_dir {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            size_bytes: entry.uncompressed_size,
            declared: entry_facts(entry),
        }
    }

    /// A directory the grammar records no entry of its own for, put there
    /// by the names of what it holds.
    fn implied_directory(name: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            kind: EntryKind::Directory,
            size_bytes: 0,
            declared: vec![EntryFact::new(
                "declared-by",
                "the names of the entries within it",
            )],
        }
    }
}

impl Catalog for ArchiveNamespace {
    fn entries(&self, path: &str) -> Result<Vec<Entry>> {
        let prefix = Self::normalize(path);
        if !prefix.is_empty() && self.entry(&prefix).is_some_and(|entry| !entry.is_dir) {
            return Err(Error::categorized_archive(
                ErrorCategory::NotDirectory,
                self.catalog.descriptor().id,
                format!("'{prefix}' names a file, which holds no names"),
            ));
        }
        let head = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };

        // One pass over the index, in the archive's own order: a name
        // directly under `head` is an entry of this directory, and a
        // deeper one contributes the directory it sits in.
        let mut names: Vec<Entry> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for entry in self.catalog.entries() {
            let name = entry.name.trim_end_matches('/');
            let Some(rest) = name.strip_prefix(head.as_str()) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            let (reported, item) = match rest.split_once('/') {
                None => (rest, Self::named(rest, entry)),
                Some((directory, _)) => (directory, Self::implied_directory(directory)),
            };
            if seen.iter().any(|taken| taken == reported) {
                continue;
            }
            seen.push(reported.to_owned());
            names.push(item);
        }
        Ok(names)
    }

    fn stat(&self, path: &str) -> Result<Option<Entry>> {
        let name = Self::normalize(path);
        if name.is_empty() {
            return Ok(None);
        }
        let leaf = name.rsplit('/').next().unwrap_or(&name).to_owned();
        if let Some(entry) = self.entry(&name) {
            return Ok(Some(Self::named(&leaf, entry)));
        }
        let head = format!("{name}/");
        let implied = self
            .catalog
            .entries()
            .iter()
            .any(|entry| entry.name.starts_with(head.as_str()));
        Ok(implied.then(|| Self::implied_directory(&leaf)))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let name = Self::normalize(path);
        let (source, length) = self.located(&name)?;
        if length > WHOLE_ENTRY_BOUND {
            return Err(Error::categorized_archive(
                ErrorCategory::Unsupported,
                self.catalog.descriptor().id,
                format!(
                    "'{name}' holds {length} bytes and copying an entry whole is \
                     bounded to {WHOLE_ENTRY_BOUND}; read it in parts, or load \
                     it into a device of its own"
                ),
            ));
        }
        let mut bytes = vec![0u8; length as usize];
        read_entry(&self.file, &source, 0, &mut bytes)?;
        Ok(bytes)
    }

    fn read_file_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<()> {
        let name = Self::normalize(path);
        let (source, length) = self.located(&name)?;
        let within = offset
            .checked_add(buf.len() as u64)
            .is_some_and(|end| end <= length);
        if !within {
            return Err(Error::categorized_archive(
                ErrorCategory::NotFound,
                self.catalog.descriptor().id,
                format!(
                    "'{name}' holds {length} bytes; the requested span at \
                     {offset} runs past it"
                ),
            ));
        }
        read_entry(&self.file, &source, offset, buf)
    }
}

/// The largest entry [`Catalog::read_file`] copies out whole (P27). The
/// streamed form has no such bound: it reads the span asked for.
const WHOLE_ENTRY_BOUND: u64 = 64 * 1024 * 1024;

/// Reads within one entry, wherever its bytes ended up.
///
/// A stored entry is a span of the claimed archive and a coded one a span
/// of private session storage; both answer a positioned read directly, so
/// nothing is materialized to serve part of an entry.
fn read_entry(file: &Arc<File>, source: &EntrySource, offset: u64, buf: &mut [u8]) -> Result<()> {
    let (backing, base) = match source {
        EntrySource::InPlace { offset: base, .. } => (file, *base),
        EntrySource::Spooled {
            spool,
            offset: base,
            ..
        } => (spool, *base),
    };
    read_exact_at(backing, base + offset, buf)
        .map_err(|error| Error::io(format!("failed to read an archive entry: {error}")))
}

/// What an entry declares beyond the node's own fields, in the grammar's
/// own spelling — the two-outcome rule at the node's surface.
fn entry_facts(entry: &ArchiveEntry) -> Vec<EntryFact> {
    let mut facts = vec![EntryFact::new(
        "uncompressed-size",
        entry.uncompressed_size.to_string(),
    )];
    if let Some(compressed) = entry.compressed_size {
        facts.push(EntryFact::new("compressed-size", compressed.to_string()));
    }
    facts
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
        let error = match archive_catalogs().open(Path::new("nowhere.rar")) {
            Ok(_) => panic!("rar is outside the claim"),
            Err(error) => error,
        };
        assert_eq!(error.category(), ErrorCategory::Unsupported);
        assert!(
            error.to_string().contains("names no archive format"),
            "{error}"
        );
        assert!(!is_archive(Path::new("nowhere.rar")));
        assert!(is_archive(Path::new("disks.zip")), "an enrolled grammar claims its own");
    }

    #[test]
    fn a_write_intent_on_an_archive_is_refused_by_name() {
        // Read-only, as archives are: a write would have to be encoded
        // back into the grammar's own form, and no adapter claims that.
        let error = ArchiveMedium::open(
            Path::new("nowhere.zip"),
            AccessIntent::Write,
            crate::DEFAULT_CACHE_BYTES,
        )
        .expect_err("a write intent is refused");
        assert_eq!(error.category(), ErrorCategory::ReadOnly);
        assert!(error.to_string().contains("reads and does not write"), "{error}");
    }

    #[test]
    fn entry_names_normalize_to_forward_slashes() {
        assert_eq!(
            normalize_entry_name(Path::new("inner/./deeper/disk.img")),
            "inner/deeper/disk.img"
        );
    }
}
