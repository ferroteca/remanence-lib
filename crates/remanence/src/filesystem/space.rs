// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! [`StorageSpace`] — the one node the file verbs live on — and the
//! [`File`] view over one entry in it.
//!
//! **Two vantage traits on one object** (D26). A partition composes one
//! space, and what that space affords is trait presence rather than
//! prose: a FAT volume has both addressable I/O within its own extent
//! and namespace I/O over the files it names, a volume bearing no
//! filesystem has only the first, and a medium's own namespace only the
//! second. A namespace no device composed is this same node with its
//! device and its extent absent, not a second type carrying the same
//! verbs.

use crate::error::{Error, ErrorCategory, Result};
use crate::io::source::FileSource;
use crate::model::discovery::Discovery;
use crate::model::media::Medium;
use crate::model::report::{VolumeId, VolumeLabel};

use super::*;

pub(super) fn path_is_root(path: &str) -> bool {
    path.split(['/', '\\'])
        .all(|segment| segment.is_empty() || segment == ".")
}

/// Which namespace a [`StorageSpace`] is a view of.
#[derive(Debug)]
enum Namespace<'a> {
    /// One recognized on the partition's own extent (P17 → P18). Reads
    /// and writes project through that extent into the active layer.
    Volume {
        offset: u64,
        kind: String,
        label: VolumeLabel,
        evidence: Vec<String>,
    },
    /// One a medium — or a layer no medium composed — bears directly,
    /// whose adapter materialized it. Read-only in this release.
    ///
    /// The catalog may borrow what it reads through, which is what lets
    /// a namespace be presented over a layer that is not a device at all
    /// — a flux family's presentation has no device to be reached
    /// through (P13), and the view still stops answering when what it
    /// reads from goes away.
    Medium {
        kind: &'static str,
        catalog: Box<dyn Catalog + 'a>,
    },
}

/// The addressable vantage's extent: where a space sits in the presented
/// content, how far it runs, and the identity the inspection report
/// issued for the volume composed over it where it composed one.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpaceExtent {
    pub(crate) start_bytes: u64,
    pub(crate) length_bytes: u64,
    pub(crate) volume: Option<VolumeId>,
}

/// What a partition hands this seam when it composes its space: the
/// namespace it verified, or the refusal the seam that looked stated.
pub(crate) enum SpaceNamespace<'a> {
    /// A filesystem recognized on the space's own extent.
    Fat {
        offset: u64,
        kind: String,
        label: VolumeLabel,
        evidence: Vec<String>,
    },
    /// One the content bears directly, already opened by the adapter
    /// that reads it.
    Catalog {
        kind: &'static str,
        catalog: Box<dyn Catalog + 'a>,
    },
    /// None — and the refusal is the one the seam that looked produced,
    /// never a coarser one of this seam's own: that refusal already
    /// carries the category and rule identity explaining it (P4, P10).
    Absent(Error),
}

/// Whether a space bears a namespace, and where the answer came from when
/// it does not.
#[derive(Debug)]
enum NamespaceState<'a> {
    Present(Namespace<'a>),
    Absent(Error),
}

/// The volume/filesystem node: **one object carrying two vantage
/// traits**.
///
/// *Volume* is the addressable vantage — reads and writes by position
/// within the extent this space names. *Filesystem* is the namespace
/// vantage — the file verbs, which live here and nowhere else. They are
/// two words for one node, and an object implements what it has: a FAT
/// volume both, swap and unformatted space the addressable one alone, an
/// archive's namespace the namespace one alone. That is the 0..1 carried
/// by the type rather than asserted beside it, and it is why no phantom
/// volume is invented for a namespace with no space beneath it (D26).
///
/// It is a **view over its provider's state, never an instance** (P23).
/// Every verb reads or writes the state beneath it, mutations project
/// into the active layer, and nothing here holds a second copy of a
/// listing that could go stale. The view stops answering when what it
/// reads through leaves, because it holds that borrow until it is
/// dropped — a device where one composed it, and whatever a namespace
/// presented over another layer reads through where none did.
///
/// **A namespace needs no device.** The flux family is reached through
/// its own types rather than through a device (P13), and a CBM DOS
/// directory read off a recording is still a namespace and still this
/// node: the same verbs, its medium and its extent simply absent. That
/// is what keeps the file verbs on one type instead of one per provider.
#[derive(Debug)]
pub struct StorageSpace<'a> {
    /// The medium the space reads through, where one composed it.
    ///
    /// A namespace presented over something that is not a medium has
    /// none — a flux recording's sector layer is reached through its own
    /// types rather than through a medium (P13) — and every verb that
    /// needs one refuses by name rather than assuming it is there.
    medium: Option<&'a mut Medium>,
    extent: Option<SpaceExtent>,
    namespace: NamespaceState<'a>,
}

impl<'a> StorageSpace<'a> {
    /// The space a partition composes over its medium — **the one node,
    /// carrying whichever vantages the partition has** (D26).
    ///
    /// Both vantage doors reach this, so which of them was opened
    /// changes nothing about what comes back: a partition with an extent
    /// and a namespace answers both ways, one with an extent alone is an
    /// ordinary volume (swap, boot code, unformatted space), and one with
    /// a namespace alone is a namespace with nothing addressed beneath
    /// it.
    pub(crate) fn compose(
        medium: &'a mut Medium,
        extent: Option<SpaceExtent>,
        namespace: SpaceNamespace<'a>,
    ) -> Self {
        Self {
            medium: Some(medium),
            extent,
            namespace: match namespace {
                SpaceNamespace::Fat {
                    offset,
                    kind,
                    label,
                    evidence,
                } => NamespaceState::Present(Namespace::Volume {
                    offset,
                    kind,
                    label,
                    evidence,
                }),
                SpaceNamespace::Catalog { kind, catalog } => {
                    NamespaceState::Present(Namespace::Medium { kind, catalog })
                }
                SpaceNamespace::Absent(absence) => NamespaceState::Absent(absence),
            },
        }
    }

    /// A namespace presented over something that is not a medium.
    ///
    /// The one node still carries the file verbs (P19); what it lacks is
    /// the addressable vantage, because nothing composed an extent for
    /// it to be a position within. The catalog borrows what it reads
    /// through, so the view stops answering when that goes away — the
    /// same rule a medium-backed space lives by.
    pub(crate) fn over_catalog(kind: &'static str, catalog: Box<dyn Catalog + 'a>) -> Self {
        Self {
            medium: None,
            extent: None,
            namespace: NamespaceState::Present(Namespace::Medium { kind, catalog }),
        }
    }

    /// The medium this space reads through, or the refusal naming why
    /// there is none.
    fn medium(&mut self) -> Result<&mut Medium> {
        match self.medium.as_deref_mut() {
            Some(medium) => Ok(medium),
            None => Err(refuse(
                SpaceRule::NotAddressable,
                "this namespace is presented over a layer no medium composed, so there \
                 is nothing beneath it to reach by position",
            )),
        }
    }

    // ------------------------------------------- the addressable vantage

    /// Whether this space has the addressable vantage — an extent to read
    /// and write by position.
    pub fn is_addressable(&self) -> bool {
        self.extent.is_some()
    }

    /// The identity the inspection report issued for the volume composed
    /// over this space's partition, or `None` where it composed none —
    /// a space with no addressed extent at all, and a partition the
    /// report states as declared without composing a volume from it.
    ///
    /// It is the same identity in the report and here, so an identity
    /// names the same volume wherever it is met (P21, U4).
    pub fn volume_id(&self) -> Option<VolumeId> {
        self.extent.and_then(|extent| extent.volume)
    }

    /// Where this space starts in the presented disk, where it is
    /// addressable.
    pub fn start_bytes(&self) -> Option<u64> {
        self.extent.map(|extent| extent.start_bytes)
    }

    /// How far this space runs, where it is addressable.
    pub fn length_bytes(&self) -> Option<u64> {
        self.extent.map(|extent| extent.length_bytes)
    }

    /// Reads `buf` at `offset` **within this space**, not within the
    /// medium.
    ///
    /// This is the vantage that reaches what a namespace does not name: a
    /// boot record, allocation metadata, the extents a filesystem calls
    /// free, or the bytes behind a file just listed — without the caller
    /// computing offsets against the medium by hand. The bound is the
    /// space's own, so a read past its end is refused rather than
    /// wandering into whatever follows.
    pub fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let extent = self.addressable(offset, buf.len() as u64)?;
        self.medium()?
            .read_space_at(extent.start_bytes + offset, buf)
    }

    /// Writes `data` at `offset` within this space, buffered until
    /// [`Medium::commit`](crate::Medium::commit) like every other write
    /// (P2), landing in the active layer (P23).
    ///
    /// The bytes are taken as given: this vantage names positions, and
    /// nothing here reinterprets what a filesystem above may make of
    /// them.
    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        let extent = self.addressable(offset, data.len() as u64)?;
        self.medium()?
            .write_space_at(extent.start_bytes + offset, data)
    }

    /// The extent a positioned access runs in, or the refusal naming
    /// which of the two rules it broke.
    fn addressable(&self, offset: u64, length: u64) -> Result<SpaceExtent> {
        let Some(extent) = self.extent else {
            return Err(refuse(
                SpaceRule::NotAddressable,
                "this space is a namespace with no addressed extent beneath \
                 it, so there is no position to read or write by",
            ));
        };
        let end = offset.checked_add(length).ok_or_else(|| {
            refuse(
                SpaceRule::OutsideExtent,
                "the requested span overflows past the end of addressing",
            )
        })?;
        if end > extent.length_bytes {
            return Err(refuse(
                SpaceRule::OutsideExtent,
                format!(
                    "this space runs {} bytes and the request reaches {end}",
                    extent.length_bytes
                ),
            ));
        }
        Ok(extent)
    }

    // --------------------------------------------- the namespace vantage

    /// Whether this space has the namespace vantage — files to name.
    pub fn has_namespace(&self) -> bool {
        matches!(self.namespace, NamespaceState::Present(_))
    }

    /// The filesystem kind, in its stable cross-language spelling —
    /// `"FAT12"`, `"hdos"` — or the named absence of a namespace. It is
    /// data on the handle, never a type of its own.
    pub fn kind(&self) -> Result<&str> {
        Ok(match self.present()? {
            Namespace::Volume { kind, .. } => kind,
            Namespace::Medium { kind, .. } => kind,
        })
    }

    /// The label the recognizing filesystem read, answered whole — the
    /// name, which source decided it, and every source it consulted.
    ///
    /// A namespace whose format carries no such field at all answers
    /// `None`, which is a different fact from a field that is present
    /// and blank; the readings say which it was. The answer is the
    /// recognizing seam's own, read when the space was composed rather
    /// than looked up in a report beside it.
    pub fn label(&mut self) -> Result<Option<VolumeLabel>> {
        match &self.namespace {
            NamespaceState::Absent(absence) => Err(absence.clone()),
            NamespaceState::Present(Namespace::Medium { catalog, .. }) => Ok(catalog.label()),
            NamespaceState::Present(Namespace::Volume { label, .. }) => Ok(Some(label.clone())),
        }
    }

    /// What recognized the namespace this space bears (P4).
    ///
    /// A verdict without the observations that produced it is not an
    /// answer, so the claim that this is a CBM DOS disk, or a FAT12
    /// volume, comes back with what the recognizing seam read to say so.
    pub fn evidence(&mut self) -> Result<Vec<String>> {
        match &self.namespace {
            NamespaceState::Absent(absence) => Err(absence.clone()),
            NamespaceState::Present(Namespace::Medium { catalog, .. }) => Ok(catalog.evidence()),
            NamespaceState::Present(Namespace::Volume { evidence, .. }) => Ok(evidence.clone()),
        }
    }

    /// The namespace this space bears, or the absence the recognizing
    /// seam stated.
    fn present(&self) -> Result<&Namespace<'a>> {
        match &self.namespace {
            NamespaceState::Present(namespace) => Ok(namespace),
            NamespaceState::Absent(absence) => Err(absence.clone()),
        }
    }

    /// Lists a directory (`""` is the root; `"A/B"` descends).
    pub fn entries(&mut self, path: &str) -> Result<Vec<Entry>> {
        match &self.namespace {
            NamespaceState::Absent(absence) => Err(absence.clone()),
            NamespaceState::Present(Namespace::Volume { offset, .. }) => {
                let offset = *offset;
                Ok(self
                    .medium()?
                    .entries(offset, path)?
                    .iter()
                    .map(Entry::from_fat)
                    .collect())
            }
            NamespaceState::Present(Namespace::Medium { catalog, .. }) => catalog.entries(path),
        }
    }

    /// Answers one path with its entry, or `None` when nothing exists
    /// there — a missing leaf, a missing parent, or a parent that is a
    /// file alike. Absence is an answer, distinguished from failure to
    /// read the namespace (U3).
    pub fn stat(&mut self, path: &str) -> Result<Option<Entry>> {
        match &self.namespace {
            NamespaceState::Absent(absence) => Err(absence.clone()),
            NamespaceState::Present(Namespace::Volume { offset, .. }) => {
                let offset = *offset;
                Ok(self
                    .medium()?
                    .stat(offset, path)?
                    .as_ref()
                    .map(Entry::from_fat))
            }
            NamespaceState::Present(Namespace::Medium { catalog, .. }) => catalog.stat(path),
        }
    }

    /// The file at `path`, or a refusal naming what is there instead.
    ///
    /// This is where absence stops being an answer: `stat` asks whether
    /// something is there, and this asks for the file itself, so nothing
    /// and a directory are both refused by name.
    pub fn get_file(&mut self, path: &str) -> Result<File<'_>> {
        let entry = self.stat(path)?.ok_or_else(|| {
            Error::categorized_io(
                ErrorCategory::NotFound,
                format!("'{path}' names nothing in this filesystem"),
            )
        })?;
        if entry.kind == EntryKind::Directory {
            return Err(Error::categorized_io(
                ErrorCategory::IsDirectory,
                format!("'{path}' names a directory, which holds no bytes"),
            ));
        }
        // The presence was established by the `stat` above, which refuses
        // on an absent namespace before anything here can borrow one.
        let NamespaceState::Present(namespace) = &self.namespace else {
            unreachable!("stat answered, so a namespace is present")
        };
        Ok(File {
            medium: self.medium.as_deref_mut(),
            namespace,
            path: path.to_owned(),
            entry,
        })
    }

    /// Every file under `path` (`""` is the whole namespace), gathered
    /// as a load's sources — the collection shape of
    /// [`Session::load_media`](crate::Session::load_media).
    ///
    /// The sources are **free-standing**: each rides the claim of the
    /// medium it came from, so the walk that gathered them ends before
    /// the load begins and nothing is opened twice. A solid archive's
    /// coded stream decodes once for the whole gathering, not once per
    /// member (P27). This release gathers from an archive's namespace
    /// alone — a volume-backed filesystem's files are read through the
    /// filesystem that names them.
    pub fn files(&mut self, path: &str) -> Result<Vec<FileSource>> {
        match &self.namespace {
            NamespaceState::Absent(absence) => Err(absence.clone()),
            NamespaceState::Present(Namespace::Volume { kind, .. }) => Err(refuse(
                SpaceRule::NotAddressable,
                format!(
                    "this release gathers a load's collection from an archive's \
                     namespace, and this is a {kind} volume: read its files \
                     through this filesystem"
                ),
            )),
            NamespaceState::Present(Namespace::Medium { .. }) => {
                self.medium()?.entry_group_sources(path)
            }
        }
    }

    /// Copies a file's bytes out — the whole-value convenience beside
    /// [`File::read_at`].
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>> {
        match &self.namespace {
            NamespaceState::Absent(absence) => Err(absence.clone()),
            NamespaceState::Present(Namespace::Volume { offset, .. }) => {
                let offset = *offset;
                self.medium()?.read_file(offset, path)
            }
            NamespaceState::Present(Namespace::Medium { catalog, .. }) => catalog.read_file(path),
        }
    }

    /// Sets a file's size, creating it when absent: kept bytes preserved
    /// in place, a grown region reads as zeros. Buffered until commit.
    pub fn resize_file(&mut self, path: &str, size: u64) -> Result<()> {
        let offset = self.writable()?;
        self.medium()?.resize_file(offset, path, size)
    }

    /// Writes a file, an existing one overwritten — shorter or longer,
    /// its old clusters released and reclaimed — and an existing
    /// directory refused. Buffered until
    /// [`Medium::commit`](crate::Medium::commit).
    pub fn write_file(&mut self, path: &str, contents: &[u8]) -> Result<()> {
        let offset = self.writable()?;
        self.medium()?.write_file(offset, path, contents)
    }

    /// Ensures a directory exists: missing parents are created, and a
    /// path that already leads to one — the root included — succeeds
    /// unchanged. Buffered until commit.
    pub fn make_directory(&mut self, path: &str) -> Result<()> {
        let offset = self.writable()?;
        self.medium()?.make_directory(offset, path)
    }

    /// Where in the presented content a write projects, or the refusal
    /// naming a namespace this release reads and does not write.
    fn writable(&self) -> Result<u64> {
        match self.present()? {
            Namespace::Volume { offset, .. } => Ok(*offset),
            Namespace::Medium { kind, .. } => Err(refuse(
                SpaceRule::NotWritable,
                format!("this release reads the {kind} namespace and does not write it"),
            )),
        }
    }
}

/// One file, borrowed from the filesystem that names it.
///
/// It is never an instance: the bytes stay where they are and this
/// offers the two ways of reaching them — [`read_at`](Self::read_at),
/// the bounded streamed form, and [`bytes`](Self::bytes), the
/// whole-value convenience beside it (P27).
#[derive(Debug)]
pub struct File<'a> {
    /// The medium the file's bytes are reached through, where one
    /// composed the space. A namespace presented over a layer no medium
    /// composed has none, and the verbs that need one refuse by name.
    medium: Option<&'a mut Medium>,
    namespace: &'a Namespace<'a>,
    path: String,
    entry: Entry,
}

impl File<'_> {
    /// The medium this file is reached through, or the refusal naming
    /// why there is none.
    fn medium(&mut self) -> Result<&mut Medium> {
        match self.medium.as_deref_mut() {
            Some(medium) => Ok(medium),
            None => Err(refuse(
                SpaceRule::NotAddressable,
                "this file is named by a namespace presented over a layer no medium \
                 composed, so there is nothing beneath it to reach by position",
            )),
        }
    }

    /// The path this file was reached by.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The name as the filesystem stores it, which is not always the
    /// spelling the caller asked by.
    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// What the filesystem claims this file's size is.
    pub fn size_bytes(&self) -> u64 {
        self.entry.size_bytes
    }

    /// This file's entry, declared facts included.
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// The whole file, copied out.
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        match self.namespace {
            Namespace::Volume { offset, .. } => {
                let (offset, path) = (*offset, self.path.clone());
                self.medium()?.read_file(offset, &path)
            }
            Namespace::Medium { catalog, .. } => catalog.read_file(&self.path),
        }
    }

    /// Exactly `buf.len()` bytes at `offset`, which must lie within the
    /// file.
    pub fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        match self.namespace {
            Namespace::Volume { offset: at, .. } => {
                let (at, path) = (*at, self.path.clone());
                self.medium()?.read_file_at(at, &path, offset, buf)
            }
            Namespace::Medium { catalog, .. } => catalog.read_file_at(&self.path, offset, buf),
        }
    }

    /// Opens this file as an artifact of its own, answering with the
    /// [`Discovery`] a device loads it from (P12, P25).
    ///
    /// **Recursion is the same journey again.** An entry recognized as
    /// an image is not read through the namespace that names it: it is
    /// loaded into a device of its own, the host's archive and the disk
    /// it holds being two media rather than one. The claim is the
    /// one the archive already holds, so nothing is re-opened and no
    /// window exists between naming the entry and loading it.
    ///
    /// This release mints a discovery from an **archive entry**; a file
    /// on a volume-backed filesystem is refused by name, its bytes being
    /// reached through the filesystem that names it.
    pub fn discover(&mut self) -> Result<Discovery> {
        match self.namespace {
            Namespace::Volume { kind, .. } => Err(refuse(
                SpaceRule::NotAddressable,
                format!(
                    "this release opens an archive entry as an artifact of its \
                     own, and '{}' is a file on a {kind} volume: read its bytes \
                     through this filesystem",
                    self.path
                ),
            )),
            Namespace::Medium { .. } => {
                let path = self.path.clone();
                self.medium()?.discover_entry(&path)
            }
        }
    }

    /// This file taken as a load's source — the single-`File` source
    /// shape of [`Session::load_media`](crate::Session::load_media).
    ///
    /// The source is **free-standing**: it rides the claim of the
    /// medium it came from, so the walk that named it ends before the
    /// load begins and nothing is opened twice. This release takes a
    /// load's source from an archive's namespace alone — a file on a
    /// volume-backed filesystem is read through the filesystem that
    /// names it.
    pub fn source(&mut self) -> Result<FileSource> {
        match self.namespace {
            Namespace::Volume { kind, .. } => Err(refuse(
                SpaceRule::NotAddressable,
                format!(
                    "this release takes a load's source from an archive's \
                     namespace, and '{}' is a file on a {kind} volume: read \
                     its bytes through this filesystem",
                    self.path
                ),
            )),
            Namespace::Medium { .. } => {
                let path = self.path.clone();
                self.medium()?.entry_source(&path)
            }
        }
    }

    /// Writes `data` at `offset` in place — the streamed form beside
    /// [`StorageSpace::write_file`]: the span must lie within the file's
    /// current size, and [`StorageSpace::resize_file`] is what changes it.
    /// Buffered until commit.
    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        match self.namespace {
            Namespace::Volume { offset: at, .. } => {
                let (at, path) = (*at, self.path.clone());
                self.medium()?.write_file_at(at, &path, offset, data)
            }
            Namespace::Medium { kind, .. } => Err(refuse(
                SpaceRule::NotWritable,
                format!("this release reads the {kind} namespace and does not write it"),
            )),
        }
    }
}
